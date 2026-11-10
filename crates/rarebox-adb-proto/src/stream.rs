//! 一条 ADB 流（`OPEN` 成功后的双向字节管道）。

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::conn::{Connection, Failure, StreamEvent, StreamSlot};
use crate::error::{Error, Result};
use crate::proto::Command;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `OPEN` 成功后返回的双向流；`Read` / `Write` 与直接方法都可用。
///
/// 读取是增量的：设备每发一个 `WRTE`，内容就进入本地队列，
/// 因此 shell 的 stdout、scrcpy 的视频流都可以边收边处理。
pub struct AdbStream {
    conn: Connection,
    slot: Arc<StreamSlot>,
    local_id: u32,
    service: String,
    read_state: Mutex<ReadState>,
}

struct ReadState {
    buffer: VecDeque<u8>,
    eof: bool,
    failure: Option<Failure>,
}

impl AdbStream {
    pub(crate) fn new(
        conn: Connection,
        slot: Arc<StreamSlot>,
        local_id: u32,
        service: String,
    ) -> Self {
        Self {
            conn,
            slot,
            local_id,
            service,
            read_state: Mutex::new(ReadState {
                buffer: VecDeque::new(),
                eof: false,
                failure: None,
            }),
        }
    }

    pub fn local_id(&self) -> u32 {
        self.local_id
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    /// 设备关闭流时给出的原因（`CLSE` payload）。
    pub fn close_reason(&self) -> Option<String> {
        self.slot.close_reason()
    }

    /// 读取任意可用字节；返回 `Ok(0)` 表示流已正常结束。
    pub fn read_some(&self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut state = lock(&self.read_state);
        loop {
            if !state.buffer.is_empty() {
                let len = buf.len().min(state.buffer.len());
                for (slot, byte) in buf.iter_mut().zip(state.buffer.drain(..len)) {
                    *slot = byte;
                }
                return Ok(len);
            }
            if let Some(failure) = state.failure.clone() {
                return Err(failure.into_error());
            }
            if state.eof {
                return Ok(0);
            }
            match self.slot.take_event() {
                Some(StreamEvent::Data(data)) => state.buffer.extend(data),
                Some(StreamEvent::Close(reason)) => {
                    state.eof = true;
                    drop(reason);
                }
                Some(StreamEvent::Failure(failure)) => state.failure = Some(failure),
                None => state.eof = true,
            }
        }
    }

    /// 精确读取 `len` 字节（sync 协议用）。
    pub fn read_exact_bytes(&self, len: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(len);
        let mut chunk = [0u8; 8192];
        while out.len() < len {
            let want = (len - out.len()).min(chunk.len());
            let read = self.read_some(&mut chunk[..want])?;
            if read == 0 {
                return Err(Error::ConnectionClosed(format!(
                    "数据流提前结束：期望 {len} 字节，只收到 {} 字节",
                    out.len()
                )));
            }
            out.extend_from_slice(&chunk[..read]);
        }
        Ok(out)
    }

    /// 读到流结束，返回全部内容。
    pub fn read_to_end_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let read = self.read_some(&mut chunk)?;
            if read == 0 {
                return Ok(out);
            }
            out.extend_from_slice(&chunk[..read]);
        }
    }

    /// 写入数据；按协商的 `maxdata` 分片，并等待设备 `OKAY` 流控。
    pub fn write_all(&self, data: &[u8]) -> Result<()> {
        let window = self.conn.max_payload() as usize;
        let remote_id = self.slot.remote_id();
        if remote_id == 0 {
            return Err(Error::Protocol("流尚未建立（缺少 remote_id）".to_string()));
        }

        let mut offset = 0;
        while offset < data.len() {
            let len = window.min(data.len() - offset);
            self.slot.reserve(len, window, self.conn.write_timeout())?;
            self.conn.send_packet(
                Command::WRTE,
                remote_id,
                self.local_id,
                &data[offset..offset + len],
            )?;
            offset += len;
        }
        Ok(())
    }

    /// 关闭流：发送 `CLSE`，并唤醒仍在读取的线程。
    pub fn close(&self) -> Result<()> {
        if !self.slot.mark_closed() {
            return Ok(());
        }
        self.conn.unregister(self.local_id);
        let remote_id = self.slot.remote_id();
        let result = if remote_id == 0 {
            Ok(())
        } else {
            self.conn
                .send_packet(Command::CLSE, remote_id, self.local_id, &[])
        };
        self.slot.push(StreamEvent::Close(None));
        result
    }
}

impl io::Read for AdbStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_some(buf).map_err(into_io_error)
    }
}

impl io::Write for AdbStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        AdbStream::write_all(self, buf).map_err(into_io_error)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for AdbStream {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl fmt::Debug for AdbStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdbStream")
            .field("service", &self.service)
            .field("local_id", &self.local_id)
            .finish()
    }
}

fn into_io_error(error: Error) -> io::Error {
    let kind = match &error {
        Error::Timeout(_) => io::ErrorKind::TimedOut,
        Error::ConnectionClosed(_) | Error::DeviceClosed { .. } => io::ErrorKind::ConnectionAborted,
        Error::Protocol(_) | Error::Checksum { .. } => io::ErrorKind::InvalidData,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error.to_string())
}
