//! 纯 Rust 实现的 ADB host 侧协议（TCP 传输），不依赖、也不执行外部 `adb` 可执行文件。
//!
//! 面向鸿蒙等不允许启动外部进程的平台：直接与 adbd（或端口映射后的 adbd）通信。
//!
//! 已实现：
//!
//! * `CNXN` / `AUTH`（RSA-2048、PKCS#1 v1.5 + SHA-1、Android 公钥格式、等待用户确认）握手；
//! * `OPEN` / `OKAY` / `WRTE` / `CLSE` 多路复用流与流控；
//! * `shell:` / `exec:` / `sync:` / `localabstract:` / `reboot:` 服务；
//! * sync v1 的 `SEND` / `RECV` / `STAT` / `LIST`；
//! * 本地 TCP → `localabstract:` 的转发桥。
//!
//! ```no_run
//! use rarebox_adb_proto::{AdbClient, AdbKey, ConnectOptions};
//!
//! # fn main() -> rarebox_adb_proto::Result<()> {
//! let key = AdbKey::generate()?;
//! let client = AdbClient::connect("127.0.0.1:5555", &key, &ConnectOptions::default())?;
//! let model = client.device_info().model().unwrap_or("unknown");
//! println!("connected to {model}");
//! let output = client.exec_out("getprop ro.build.version.release")?;
//! # Ok(())
//! # }
//! ```

#![deny(unsafe_code)]

pub mod auth;
pub mod client;
pub mod conn;
pub mod error;
pub mod forward;
pub mod proto;
pub mod stream;
pub mod sync;

pub use auth::AdbKey;
pub use client::{AdbClient, DeviceInfo, InstallOptions, shell_quote};
pub use conn::{AuthEvent, AuthObserver, ConnectOptions, Connection};
pub use error::{Error, Result};
pub use forward::{ForwardHandle, forward_tcp};
pub use proto::{
    A_VERSION_MIN, A_VERSION_SKIP_CHECKSUM, AUTH_RSAPUBLICKEY, AUTH_SIGNATURE, AUTH_TOKEN, Command,
    HEADER_SIZE, MAX_PAYLOAD, MAX_PAYLOAD_V1, MessageHeader, TOKEN_SIZE, checksum, service_payload,
};
pub use stream::AdbStream;
pub use sync::{DirEntry, FileStat, SYNC_DATA_MAX, SyncSession};
