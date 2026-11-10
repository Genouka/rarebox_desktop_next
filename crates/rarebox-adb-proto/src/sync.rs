//! ADB sync 协议（v1）：`SEND` / `RECV` / `STAT` / `LIST`。
//!
//! 一帧固定 8 字节：4 字节 ASCII id + 4 字节小端 `length`。
//! 数据要么跟在帧后（`DATA`、路径、`FAIL` 原因），要么直接编码在 `length`
//! 里（`DONE` 的 length 就是文件的 mtime）。所有整数均为小端。

use std::fmt;
use std::io::{Read, Write};

use crate::conn::Connection;
use crate::error::{Error, Result};
use crate::stream::AdbStream;

/// 单个 `DATA` 块上限。
pub const SYNC_DATA_MAX: usize = 64 * 1024;

const ID_SEND: [u8; 4] = *b"SEND";
const ID_RECV: [u8; 4] = *b"RECV";
const ID_STAT: [u8; 4] = *b"STAT";
const ID_LIST: [u8; 4] = *b"LIST";
const ID_DENT: [u8; 4] = *b"DENT";
const ID_DATA: [u8; 4] = *b"DATA";
const ID_DONE: [u8; 4] = *b"DONE";
const ID_OKAY: [u8; 4] = *b"OKAY";
const ID_FAIL: [u8; 4] = *b"FAIL";

/// `STAT` 的结果（v1 只有模式、大小、mtime）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub mode: u32,
    pub size: u32,
    pub mtime: u32,
}

impl FileStat {
    pub fn is_dir(&self) -> bool {
        self.mode & 0o170_000 == 0o040_000
    }

    pub fn is_file(&self) -> bool {
        self.mode & 0o170_000 == 0o100_000
    }

    pub fn is_symlink(&self) -> bool {
        self.mode & 0o170_000 == 0o120_000
    }

    /// 权限位（含 setuid/setgid/sticky）。
    pub fn permissions(&self) -> u32 {
        self.mode & 0o7777
    }
}

/// `LIST` 返回的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub mode: u32,
    pub size: u32,
    pub mtime: u32,
}

impl DirEntry {
    pub fn is_dir(&self) -> bool {
        self.mode & 0o170_000 == 0o040_000
    }

    pub fn is_file(&self) -> bool {
        self.mode & 0o170_000 == 0o100_000
    }
}

/// 一条 `sync:` 会话。所有操作串行执行；用完 drop 即可关闭。
pub struct SyncSession {
    stream: AdbStream,
}

impl SyncSession {
    /// 在已有连接上打开 `sync:` 服务。
    pub fn open(conn: &Connection) -> Result<Self> {
        Ok(Self {
            stream: conn.open_service("sync:")?,
        })
    }

    /// 底层流（一般不需要直接使用）。
    pub fn stream(&self) -> &AdbStream {
        &self.stream
    }

    /// `SEND`：把内存中的字节写到设备文件，保留 `mode` 与 `mtime`。
    pub fn push(&mut self, remote: &str, mode: u32, mtime: u32, data: &[u8]) -> Result<()> {
        self.push_reader(remote, mode, mtime, &mut &data[..])
            .map(|_| ())
    }

    /// `SEND`：从任意 `Read` 流式写入，返回写入字节数。
    pub fn push_reader(
        &mut self,
        remote: &str,
        mode: u32,
        mtime: u32,
        reader: &mut impl Read,
    ) -> Result<u64> {
        // v1 的 SEND 请求把 mode 以十进制拼在路径后面："/path,420"。
        let spec = format!("{remote},{mode}");
        self.write_frame(&ID_SEND, spec.len() as u32, spec.as_bytes())?;

        let mut written = 0u64;
        let mut buffer = vec![0u8; SYNC_DATA_MAX];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            self.write_frame(&ID_DATA, read as u32, &buffer[..read])?;
            written += read as u64;
        }
        // DONE 的 length 字段就是 mtime。
        self.write_frame(&ID_DONE, mtime, &[])?;
        self.read_acknowledgement("push", remote)?;
        Ok(written)
    }

    /// `RECV`：读取整个设备文件。
    pub fn pull(&mut self, remote: &str) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        self.pull_to(remote, &mut out)?;
        Ok(out)
    }

    /// `RECV`：把设备文件流式写入 `writer`，返回字节数。
    pub fn pull_to(&mut self, remote: &str, writer: &mut impl Write) -> Result<u64> {
        self.write_frame(&ID_RECV, remote.len() as u32, remote.as_bytes())?;

        let mut written = 0u64;
        loop {
            let (id, length) = self.read_frame()?;
            if id == ID_DATA {
                if length as usize > SYNC_DATA_MAX {
                    return Err(Error::Protocol(format!(
                        "sync DATA 块 {length} 字节，超过上限 {SYNC_DATA_MAX}"
                    )));
                }
                let chunk = self.stream.read_exact_bytes(length as usize)?;
                writer.write_all(&chunk)?;
                written += u64::from(length);
            } else if id == ID_DONE {
                return Ok(written);
            } else if id == ID_FAIL {
                let reason = self.read_failure(length)?;
                return Err(Error::DeviceError {
                    operation: format!("pull {remote}"),
                    reason,
                });
            } else {
                return Err(Error::Protocol(format!(
                    "sync pull 收到意外响应 {}",
                    id_name(&id)
                )));
            }
        }
    }

    /// `STAT`：读取设备文件元数据。
    pub fn stat(&mut self, remote: &str) -> Result<FileStat> {
        self.write_frame(&ID_STAT, remote.len() as u32, remote.as_bytes())?;
        let raw = self.stream.read_exact_bytes(16)?;
        let id = [raw[0], raw[1], raw[2], raw[3]];
        if id != ID_STAT {
            return Err(Error::Protocol(format!(
                "sync STAT 响应 id 是 {}",
                id_name(&id)
            )));
        }
        let mode = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        let size = u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]);
        let mtime = u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]);
        if mode == 0 && size == 0 && mtime == 0 {
            // v1 用全 0 表示失败，没有 errno 可读。
            return Err(Error::DeviceError {
                operation: "stat".to_string(),
                reason: format!("设备无法 stat {remote}（v1 协议未提供 errno）"),
            });
        }
        Ok(FileStat { mode, size, mtime })
    }

    /// `LIST`：列出目录项。
    pub fn list(&mut self, remote: &str) -> Result<Vec<DirEntry>> {
        self.write_frame(&ID_LIST, remote.len() as u32, remote.as_bytes())?;
        let mut entries = Vec::new();
        loop {
            let raw = self.stream.read_exact_bytes(20)?;
            let id = [raw[0], raw[1], raw[2], raw[3]];
            if id == ID_DONE {
                return Ok(entries);
            }
            if id != ID_DENT {
                return Err(Error::Protocol(format!(
                    "sync LIST 收到意外响应 {}",
                    id_name(&id)
                )));
            }
            let mode = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
            let size = u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]);
            let mtime = u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]);
            let namelen = u32::from_le_bytes([raw[16], raw[17], raw[18], raw[19]]) as usize;
            if namelen > 255 {
                return Err(Error::Protocol(format!(
                    "sync DENT 文件名长度 {namelen} 超过 255"
                )));
            }
            let name_bytes = self.stream.read_exact_bytes(namelen)?;
            let name = String::from_utf8_lossy(&name_bytes).to_string();
            if name.contains('/') || name.contains('\\') {
                return Err(Error::Protocol(format!("sync DENT 文件名非法：{name:?}")));
            }
            entries.push(DirEntry {
                name,
                mode,
                size,
                mtime,
            });
        }
    }

    fn write_frame(&mut self, id: &[u8; 4], value: u32, data: &[u8]) -> Result<()> {
        let mut frame = Vec::with_capacity(8 + data.len());
        frame.extend_from_slice(id);
        frame.extend_from_slice(&value.to_le_bytes());
        frame.extend_from_slice(data);
        self.stream.write_all(&frame)
    }

    fn read_frame(&self) -> Result<([u8; 4], u32)> {
        let raw = self.stream.read_exact_bytes(8)?;
        let id = [raw[0], raw[1], raw[2], raw[3]];
        let length = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        Ok((id, length))
    }

    /// 读取 push 的最终应答：`OKAY` 或 `FAIL`。
    fn read_acknowledgement(&mut self, operation: &str, remote: &str) -> Result<()> {
        let (id, length) = self.read_frame()?;
        if id == ID_OKAY {
            return Ok(());
        }
        if id == ID_FAIL {
            let reason = self.read_failure(length)?;
            return Err(Error::DeviceError {
                operation: format!("{operation} {remote}"),
                reason,
            });
        }
        Err(Error::Protocol(format!(
            "sync {operation} 收到意外应答 {}",
            id_name(&id)
        )))
    }

    fn read_failure(&self, length: u32) -> Result<String> {
        if length as usize > SYNC_DATA_MAX {
            return Err(Error::Protocol(format!("sync FAIL 原因长度 {length} 异常")));
        }
        let raw = self.stream.read_exact_bytes(length as usize)?;
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }
}

fn id_name(id: &[u8]) -> String {
    String::from_utf8_lossy(id).into_owned()
}

impl fmt::Debug for SyncSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SyncSession")
            .field("service", &self.stream.service())
            .finish()
    }
}
