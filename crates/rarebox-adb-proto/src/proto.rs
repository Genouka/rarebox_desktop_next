//! ADB 线格式：协议常量、24 字节包头与 payload 校验和。
//!
//! 包头布局（全部小端）：
//!
//! | 偏移 | 字段            | 说明                          |
//! |------|-----------------|-------------------------------|
//! | 0    | `command`       | 4 字节 ASCII 命令（如 `CNXN`）|
//! | 4    | `arg0`          | 命令相关参数                  |
//! | 8    | `arg1`          | 命令相关参数                  |
//! | 12   | `data_length`   | payload 字节数                |
//! | 16   | `data_checksum` | payload 字节和（`A_VERSION_SKIP_CHECKSUM` 之后为 0） |
//! | 20   | `magic`         | `command ^ 0xffff_ffff`       |

use std::fmt;

use crate::error::{Error, Result};

/// 最小的 ADB 协议版本。
pub const A_VERSION_MIN: u32 = 0x0100_0000;
/// 从这个版本开始，payload 校验和字段填 0，接收方不再校验。
pub const A_VERSION_SKIP_CHECKSUM: u32 = 0x0100_0001;
/// 老 adbd 能接受的最大负载。
pub const MAX_PAYLOAD_V1: u32 = 4 * 1024;
/// 当前 ADB 的最大负载上限（1 MiB）。
pub const MAX_PAYLOAD: u32 = 1024 * 1024;
/// 包头长度。
pub const HEADER_SIZE: usize = 24;
/// `AUTH TOKEN` 的长度。
pub const TOKEN_SIZE: usize = 20;

/// `A_AUTH` 的 arg0：设备要求 host 对 token 签名。
pub const AUTH_TOKEN: u32 = 1;
/// `A_AUTH` 的 arg0：host 返回 PKCS#1 v1.5 + SHA-1 签名。
pub const AUTH_SIGNATURE: u32 = 2;
/// `A_AUTH` 的 arg0：host 返回 `base64(Android RSAPublicKey) + " user@host"`。
pub const AUTH_RSAPUBLICKEY: u32 = 3;

/// ADB 命令字。保留原始 u32，未知命令也能原样展示。
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Command(u32);

impl Command {
    pub const SYNC: Self = Self(0x434e_5953);
    pub const CNXN: Self = Self(0x4e58_4e43);
    pub const OPEN: Self = Self(0x4e45_504f);
    pub const OKAY: Self = Self(0x5941_4b4f);
    pub const CLSE: Self = Self(0x4553_4c43);
    pub const WRTE: Self = Self(0x4554_5257);
    pub const AUTH: Self = Self(0x4854_5541);
    pub const STLS: Self = Self(0x534c_5453);

    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn name(self) -> Option<&'static str> {
        match self.0 {
            0x434e_5953 => Some("SYNC"),
            0x4e58_4e43 => Some("CNXN"),
            0x4e45_504f => Some("OPEN"),
            0x5941_4b4f => Some("OKAY"),
            0x4553_4c43 => Some("CLSE"),
            0x4554_5257 => Some("WRTE"),
            0x4854_5541 => Some("AUTH"),
            0x534c_5453 => Some("STLS"),
            _ => None,
        }
    }
    pub const fn is_known(self) -> bool {
        self.name().is_some()
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => write!(f, "{name}({:#010x})", self.0),
            None => write!(f, "UNKNOWN({:#010x})", self.0),
        }
    }
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// ADB 数据包头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageHeader {
    pub command: Command,
    pub arg0: u32,
    pub arg1: u32,
    pub data_length: u32,
    pub data_checksum: u32,
    pub magic: u32,
}

impl MessageHeader {
    /// 创建一个 checksum 尚未填写的包头。
    pub fn new(command: Command, arg0: u32, arg1: u32, payload_len: usize) -> Self {
        Self {
            command,
            arg0,
            arg1,
            data_length: u32::try_from(payload_len).unwrap_or(u32::MAX),
            data_checksum: 0,
            magic: command.raw() ^ 0xffff_ffff,
        }
    }

    pub fn encode(&self) -> [u8; HEADER_SIZE] {
        let mut raw = [0u8; HEADER_SIZE];
        for (at, value) in [
            self.command.0,
            self.arg0,
            self.arg1,
            self.data_length,
            self.data_checksum,
            self.magic,
        ]
        .into_iter()
        .enumerate()
        {
            raw[at * 4..at * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        raw
    }

    pub fn decode(raw: &[u8; HEADER_SIZE]) -> Result<Self> {
        let le32 = |at: usize| u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
        let command = le32(0);
        let magic = le32(20);
        if !is_valid_magic(command, magic) {
            return Err(Error::Protocol(format!(
                "数据包 magic 校验失败：command={command:#010x} magic={magic:#010x}"
            )));
        }
        Ok(Self {
            command: Command::from_raw(command),
            arg0: le32(4),
            arg1: le32(8),
            data_length: le32(12),
            data_checksum: le32(16),
            magic,
        })
    }
}

/// AOSP 的口径：payload 各字节作为 u32 累加，允许溢出回绕。
pub fn checksum(payload: &[u8]) -> u32 {
    payload
        .iter()
        .fold(0u32, |sum, byte| sum.wrapping_add(u32::from(*byte)))
}

pub const fn is_valid_magic(command: u32, magic: u32) -> bool {
    magic == command ^ 0xffff_ffff
}

/// `OPEN` / 服务名使用的 NUL 结尾 payload。
pub fn service_payload(service: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(service.len() + 1);
    payload.extend_from_slice(service.as_bytes());
    payload.push(0);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let header = MessageHeader {
            command: Command::CNXN,
            arg0: A_VERSION_MIN,
            arg1: MAX_PAYLOAD,
            data_length: 3,
            data_checksum: 6,
            magic: Command::CNXN.raw() ^ 0xffff_ffff,
        };
        assert_eq!(
            MessageHeader::decode(&header.encode()).expect("decode"),
            header
        );
    }

    #[test]
    fn checksum_matches_aosp_definition() {
        assert_eq!(checksum(b"\x01\x02\x03\xff"), 0x105);
        assert_eq!(checksum(&[0xff; 32]), 0xff * 32);
    }

    #[test]
    fn command_names_match_ascii_bytes() {
        assert_eq!(
            Command::from_raw(u32::from_le_bytes(*b"CNXN")),
            Command::CNXN
        );
        assert_eq!(
            Command::from_raw(u32::from_le_bytes(*b"WRTE")),
            Command::WRTE
        );
        assert_eq!(Command::SYNC.name(), Some("SYNC"));
        assert_eq!(Command::from_raw(0xdead_beef).name(), None);
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut raw = MessageHeader::new(Command::OKAY, 1, 2, 0).encode();
        raw[20] ^= 0xff;
        assert!(matches!(
            MessageHeader::decode(&raw),
            Err(Error::Protocol(_))
        ));
    }
}
