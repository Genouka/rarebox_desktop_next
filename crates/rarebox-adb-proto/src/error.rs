//! 统一错误模型。
//!
//! 四类错误严格区分：
//!
//! * [`Error::Io`] —— 本地 socket / 文件系统错误；
//! * [`Error::Protocol`] / [`Error::Checksum`] —— 对端违反 ADB 线格式；
//! * [`Error::DeviceClosed`] —— 设备用 `CLSE` 拒绝服务，携带设备原始原因；
//! * [`Error::AuthFailed`] / [`Error::AuthPending`] —— RSA 鉴权失败 / 等待用户在设备上确认。

use std::io;

/// crate 统一结果类型；错误类型可通过第二个类型参数替换。
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O 错误：{0}")]
    Io(#[from] io::Error),

    #[error("协议错误：{0}")]
    Protocol(String),

    #[error(
        "校验和错误：按收到的 payload 计算应为 {expected:#010x}，数据包声明的是 {actual:#010x}"
    )]
    Checksum { expected: u32, actual: u32 },

    /// 设备用 `CLSE` 关闭了流，payload 是设备给出的原始原因。
    #[error("设备拒绝 {service}：{reason}")]
    DeviceClosed { service: String, reason: String },

    /// 通过 shell / sync 服务返回的设备端失败（`FAIL` 帧、`pm` 输出等）。
    #[error("设备端 {operation} 失败：{reason}")]
    DeviceError { operation: String, reason: String },

    #[error("鉴权失败：{reason}")]
    AuthFailed { reason: String },

    /// 已发送公钥，但设备仍反复索要 token —— 通常表示设备上正显示“是否允许 USB 调试”对话框。
    #[error("等待用户在设备上确认调试授权（公钥已发送，设备尚未接受）")]
    AuthPending,

    #[error("等待超时：{0}")]
    Timeout(String),

    #[error("连接已关闭：{0}")]
    ConnectionClosed(String),

    #[error("不支持：{0}")]
    Unsupported(String),
}

impl Error {
    /// 是否属于“连接已经不可再用”的错误。
    pub fn is_disconnect(&self) -> bool {
        matches!(
            self,
            Error::Io(_) | Error::ConnectionClosed(_) | Error::Checksum { .. } | Error::Protocol(_)
        )
    }
}
