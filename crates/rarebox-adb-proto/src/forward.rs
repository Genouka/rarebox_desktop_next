//! 本地 TCP ↔ 设备 `localabstract:` 转发桥，等价于 `adb forward tcp:N localabstract:name`。
//!
//! 监听线程接受本地连接，每个连接打开一条 ADB 流，然后两个方向各用一个线程搬运字节。
//! [`ForwardHandle::shutdown`] 会关闭监听、断开已有 TCP 连接并关闭对应 ADB 流。

use std::fmt;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::conn::Connection;
use crate::error::{Error, Result};
use crate::stream::AdbStream;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 监听 `listen`，把每个本地 TCP 连接桥接到设备的 `localabstract:<local>`。
///
/// 如果 `local` 已经带服务前缀（含 `:`），则原样使用，
/// 例如 `forward_tcp(conn, addr, "localabstract:scrcpy")`。
pub fn forward_tcp(conn: &Connection, listen: SocketAddr, local: &str) -> Result<ForwardHandle> {
    let service = if local.contains(':') {
        local.to_string()
    } else {
        format!("localabstract:{local}")
    };
    let listener = TcpListener::bind(listen)?;
    let local_addr = listener.local_addr()?;
    listener.set_nonblocking(true)?;

    let stop = Arc::new(AtomicBool::new(false));
    let connections: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
    let streams: Arc<Mutex<Vec<Arc<AdbStream>>>> = Arc::new(Mutex::new(Vec::new()));

    let accept_thread = {
        let stop = stop.clone();
        let connections = connections.clone();
        let streams = streams.clone();
        let conn = conn.clone();
        let service = service.clone();
        thread::Builder::new()
            .name("rarebox-adb-forward".to_string())
            .spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((tcp, _)) => {
                            let _ = tcp.set_nonblocking(false);
                            let _ = tcp.set_nodelay(true);
                            if let Ok(clone) = tcp.try_clone() {
                                lock(&connections).push(clone);
                            }
                            spawn_bridge(
                                conn.clone(),
                                service.clone(),
                                tcp,
                                streams.clone(),
                                stop.clone(),
                            );
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            })
            .map_err(Error::Io)?
    };

    Ok(ForwardHandle {
        local_addr,
        stop,
        connections,
        streams,
        accept_thread: Some(accept_thread),
    })
}

/// 转发桥句柄；drop 或 [`shutdown`](Self::shutdown) 都会停止全部连接。
pub struct ForwardHandle {
    local_addr: SocketAddr,
    stop: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<TcpStream>>>,
    streams: Arc<Mutex<Vec<Arc<AdbStream>>>>,
    accept_thread: Option<JoinHandle<()>>,
}

impl ForwardHandle {
    /// 实际监听的地址（`listen` 端口为 0 时这里是系统分配的端口）。
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// 释放监听端口、断开已有连接并关闭对应 ADB 流。
    pub fn shutdown(mut self) {
        self.stop_all();
    }

    fn stop_all(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for tcp in lock(&self.connections).drain(..) {
            let _ = tcp.shutdown(Shutdown::Both);
        }
        for stream in lock(&self.streams).drain(..) {
            let _ = stream.close();
        }
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ForwardHandle {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn spawn_bridge(
    conn: Connection,
    service: String,
    tcp: TcpStream,
    streams: Arc<Mutex<Vec<Arc<AdbStream>>>>,
    stop: Arc<AtomicBool>,
) {
    thread::Builder::new()
        .name("rarebox-adb-bridge".to_string())
        .spawn(move || {
            let stream = match conn.open_service(&service) {
                Ok(stream) => Arc::new(stream),
                Err(_) => {
                    let _ = tcp.shutdown(Shutdown::Both);
                    return;
                }
            };
            let local_id = stream.local_id();
            lock(&streams).push(stream.clone());

            let mut reader = match tcp.try_clone() {
                Ok(reader) => reader,
                Err(_) => {
                    let _ = stream.close();
                    return;
                }
            };
            let mut writer = tcp;

            let read_handle = {
                let stream = stream.clone();
                let stop = stop.clone();
                thread::spawn(move || {
                    let mut buffer = [0u8; 16 * 1024];
                    while !stop.load(Ordering::SeqCst) {
                        match reader.read(&mut buffer) {
                            Ok(0) => {
                                let _ = stream.close();
                                break;
                            }
                            Ok(read) => {
                                if stream.write_all(&buffer[..read]).is_err() {
                                    break;
                                }
                            }
                            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(_) => {
                                let _ = stream.close();
                                break;
                            }
                        }
                    }
                    let _ = reader.shutdown(Shutdown::Both);
                })
            };

            let mut buffer = [0u8; 16 * 1024];
            loop {
                match stream.read_some(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if writer.write_all(&buffer[..read]).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = writer.shutdown(Shutdown::Both);
            let _ = read_handle.join();
            let _ = stream.close();
            lock(&streams).retain(|item| item.local_id() != local_id);
        })
        .ok();
}

impl fmt::Debug for ForwardHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForwardHandle")
            .field("local_addr", &self.local_addr)
            .finish()
    }
}
