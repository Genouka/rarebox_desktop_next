//! ADB 消息层与 USB 之上的多路复用。
//!
//! 这里实现的是 ADB 的“主机”一侧：CNXN/AUTH 握手、OPEN/OKAY/WRTE/CLSE 流复用，
//! 以及 `sync:` 服务里的 push / pull / list。浏览器版不依赖本机 adb server，
//! WebUSB 直接和设备里的 adbd 通信。
//!
//! 协议参考：<https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/protocol.txt>

#![allow(async_fn_in_trait)]

use std::collections::{HashMap, VecDeque};

use anyhow::{Context as _, Result, bail};

use crate::auth::AdbKey;

pub const A_CNXN: u32 = 0x4e58_4e43;
pub const A_AUTH: u32 = 0x4854_5541;
pub const A_OPEN: u32 = 0x4e45_504f;
pub const A_OKAY: u32 = 0x5941_4b4f;
pub const A_CLSE: u32 = 0x4553_4c43;
pub const A_WRTE: u32 = 0x4554_5257;
pub const A_STLS: u32 = 0x534c_5453;

pub const A_VERSION: u32 = 0x0100_0001;
pub const AUTH_TOKEN: u32 = 1;
pub const AUTH_SIGNATURE: u32 = 2;
pub const AUTH_RSAPUBLICKEY: u32 = 3;

const HEADER_SIZE: usize = 24;
/// 单条 ADB 消息允许的最大 payload。设备会按 CNXN 里的值分片。
const MAX_PAYLOAD: usize = 16 * 1024;
/// 单条命令最多收集的输出，避免异常命令把浏览器内存吃光。
const MAX_COMMAND_OUTPUT: usize = 32 * 1024 * 1024;
/// sync 服务的最大单文件大小（浏览器内存限制）。
const MAX_SYNC_FILE: usize = 256 * 1024 * 1024;
/// sync DATA 帧的块大小。
const SYNC_CHUNK: usize = 64 * 1024;

/// WebUSB 与测试用的字节流。
///
/// 读操作应至少返回 1 字节；返回 0 视为连接关闭。
pub trait Link {
    async fn write_all(&mut self, data: &[u8]) -> Result<()>;
    async fn read_some(&mut self, buffer: &mut [u8]) -> Result<usize>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub command: u32,
    pub arg0: u32,
    pub arg1: u32,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        let length = self.payload.len() as u32;
        let checksum = self
            .payload
            .iter()
            .fold(0u32, |sum, byte| sum.wrapping_add(u32::from(*byte)));
        let magic = self.command ^ 0xffff_ffff;

        let mut out = Vec::with_capacity(HEADER_SIZE + self.payload.len());
        for value in [self.command, self.arg0, self.arg1, length, checksum, magic] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.payload);
        out
    }
}

/// 增量解析 ADB 消息。WebUSB 的 transferIn 不保证一条消息一次到达。
pub struct MessageParser {
    buffer: Vec<u8>,
    max_payload: usize,
}

impl MessageParser {
    pub fn new(max_payload: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_payload,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    pub fn next_message(&mut self) -> Result<Option<Message>> {
        if self.buffer.len() < HEADER_SIZE {
            return Ok(None);
        }

        let header = &self.buffer[..HEADER_SIZE];
        let command = read_u32(&header[0..4]);
        let arg0 = read_u32(&header[4..8]);
        let arg1 = read_u32(&header[8..12]);
        let length = read_u32(&header[12..16]) as usize;
        let checksum = read_u32(&header[16..20]);
        let magic = read_u32(&header[20..24]);

        if length > self.max_payload {
            bail!("ADB 消息过大：{length} 字节");
        }
        if self.buffer.len() < HEADER_SIZE + length {
            return Ok(None);
        }
        if magic != command ^ 0xffff_ffff {
            bail!("ADB 消息魔数不匹配（0x{magic:08x}）");
        }

        let payload = self.buffer[HEADER_SIZE..HEADER_SIZE + length].to_vec();
        let actual = payload
            .iter()
            .fold(0u32, |sum, byte| sum.wrapping_add(u32::from(*byte)));
        if actual != checksum {
            bail!("ADB 消息校验和错误：期望 0x{checksum:08x}，实际 0x{actual:08x}");
        }
        self.buffer.drain(..HEADER_SIZE + length);

        Ok(Some(Message {
            command,
            arg0,
            arg1,
            payload,
        }))
    }
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[derive(Default)]
struct Stream {
    remote: Option<u32>,
    incoming: VecDeque<u8>,
    closed: bool,
    close_sent: bool,
    pending_acks: u32,
}

pub struct Adb<T: Link> {
    link: T,
    parser: MessageParser,
    streams: HashMap<u32, Stream>,
    next_id: u32,
    read_buffer: Vec<u8>,
    /// CNXN 里设备返回的 `device::...` banner。
    pub banner: String,
    pub max_payload: usize,
}

impl<T: Link> Adb<T> {
    pub fn new(link: T) -> Self {
        Self {
            link,
            parser: MessageParser::new(MAX_PAYLOAD),
            streams: HashMap::new(),
            next_id: 1,
            read_buffer: vec![0u8; 16 * 1024],
            banner: String::new(),
            max_payload: 4096,
        }
    }

    pub fn link_mut(&mut self) -> &mut T {
        &mut self.link
    }

    /// 发送 CNXN 并完成 AUTH 握手，返回设备 banner。
    ///
    /// 声明 `shell_v2`：设备支持时 `shell()` 能拿到 stderr 与退出码，
    /// 不支持时回退到普通 `exec:`。
    pub async fn handshake(&mut self, key: &AdbKey) -> Result<String> {
        self.send(
            A_CNXN,
            A_VERSION,
            self.max_payload as u32,
            b"host::features=shell_v2",
        )
        .await?;

        let mut sent_signature = false;
        let mut sent_public_key = false;
        loop {
            let message = self.read_message().await?;
            match message.command {
                A_CNXN => {
                    self.banner = String::from_utf8_lossy(&message.payload)
                        .trim_end_matches('\0')
                        .to_string();
                    return Ok(self.banner.clone());
                }
                A_AUTH => match message.arg0 {
                    AUTH_TOKEN => {
                        if !sent_signature {
                            let signature = key.sign_token(&message.payload)?;
                            self.send(A_AUTH, AUTH_SIGNATURE, 0, &signature).await?;
                            sent_signature = true;
                        } else if !sent_public_key {
                            // 设备不信任现有密钥：发公钥，让用户在设备上点“允许”。
                            let mut payload = key.encoded().as_bytes().to_vec();
                            payload.push(0);
                            self.send(A_AUTH, AUTH_RSAPUBLICKEY, 0, &payload).await?;
                            sent_public_key = true;
                        }
                        // 公钥已发送后继续等待 CNXN；期间用户需要在设备上确认。
                    }
                    AUTH_SIGNATURE | AUTH_RSAPUBLICKEY => {
                        bail!("设备返回了异常认证消息")
                    }
                    other => bail!("未知 AUTH 类型 {other}"),
                },
                A_STLS => bail!("设备要求 TLS 会话；浏览器版 WebUSB 只支持普通 ADB 通道"),
                _ => {
                    // 握手阶段忽略其它消息（例如设备主动发起的流）。
                }
            }
        }
    }

    async fn send(&mut self, command: u32, arg0: u32, arg1: u32, payload: &[u8]) -> Result<()> {
        if payload.len() > self.max_payload {
            bail!(
                "ADB 消息超过协商上限（{} > {}）",
                payload.len(),
                self.max_payload
            );
        }
        let message = Message {
            command,
            arg0,
            arg1,
            payload: payload.to_vec(),
        };
        self.link.write_all(&message.encode()).await
    }

    async fn read_message(&mut self) -> Result<Message> {
        loop {
            if let Some(message) = self.parser.next_message()? {
                return Ok(message);
            }
            let read = self
                .link
                .read_some(&mut self.read_buffer)
                .await
                .context("读取 USB 数据失败")?;
            if read == 0 {
                bail!("USB 连接已断开");
            }
            self.parser.push(&self.read_buffer[..read]);
        }
    }
    /// 读取并处理一条消息。
    async fn pump_once(&mut self) -> Result<()> {
        let message = self.read_message().await?;
        self.dispatch(message).await
    }

    async fn dispatch(&mut self, message: Message) -> Result<()> {
        match message.command {
            A_OKAY => {
                let local = message.arg1;
                if let Some(stream) = self.streams.get_mut(&local) {
                    if stream.remote.is_none() {
                        stream.remote = Some(message.arg0);
                    }
                    stream.pending_acks = stream.pending_acks.saturating_sub(1);
                }
            }
            A_WRTE => {
                let local = message.arg1;
                let remote = message.arg0;
                let known = if let Some(stream) = self.streams.get_mut(&local) {
                    stream.incoming.extend(message.payload);
                    if stream.remote.is_none() {
                        stream.remote = Some(remote);
                    }
                    true
                } else {
                    false
                };
                if known {
                    self.send(A_OKAY, remote, local, &[]).await?;
                } else {
                    self.send(A_CLSE, remote, local, &[]).await?;
                }
            }
            A_CLSE => {
                let local = message.arg1;
                let mut reply = None;
                if let Some(stream) = self.streams.get_mut(&local) {
                    stream.closed = true;
                    if !stream.close_sent {
                        stream.close_sent = true;
                        reply = Some(stream.remote.unwrap_or(message.arg0));
                    }
                }
                if let Some(remote) = reply {
                    self.send(A_CLSE, remote, local, &[]).await?;
                }
            }
            A_CNXN | A_AUTH => {}
            _ => {}
        }
        Ok(())
    }

    fn allocate_id(&mut self) -> u32 {
        loop {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if !self.streams.contains_key(&id) {
                return id;
            }
        }
    }

    /// 打开一个 ADB 服务，例如 `shell:getprop`、`exec:screencap -p`、`sync:`。
    pub async fn open(&mut self, destination: &str) -> Result<u32> {
        let local = self.allocate_id();
        self.streams.insert(local, Stream::default());
        if let Err(err) = self.send(A_OPEN, local, 0, destination.as_bytes()).await {
            self.streams.remove(&local);
            return Err(err);
        }

        loop {
            let (ready, closed) = {
                let Some(stream) = self.streams.get(&local) else {
                    bail!("内部错误：流 {local} 丢失");
                };
                (stream.remote.is_some(), stream.closed)
            };
            if ready {
                return Ok(local);
            }
            if closed {
                self.streams.remove(&local);
                bail!("设备拒绝打开服务「{destination}」");
            }
            self.pump_once().await?;
        }
    }

    /// 向流写入数据；按 ADB 协商的 payload 上限分片，并等待设备逐片确认。
    pub async fn write_stream(&mut self, local: u32, data: &[u8]) -> Result<()> {
        let max = self.max_payload;
        for chunk in data.chunks(max) {
            let remote = {
                let Some(stream) = self.streams.get_mut(&local) else {
                    bail!("流 {local} 已关闭");
                };
                if stream.closed {
                    bail!("流 {local} 已关闭");
                }
                let Some(remote) = stream.remote else {
                    bail!("流 {local} 尚未建立");
                };
                stream.pending_acks += 1;
                remote
            };
            self.send(A_WRTE, remote, local, chunk).await?;

            loop {
                let (pending, closed) = {
                    let Some(stream) = self.streams.get(&local) else {
                        bail!("流 {local} 已关闭");
                    };
                    (stream.pending_acks, stream.closed)
                };
                if pending == 0 {
                    break;
                }
                if closed {
                    bail!("设备关闭了流 {local}");
                }
                self.pump_once().await?;
            }
        }
        Ok(())
    }

    /// 读取流的一段数据；`None` 表示设备已关闭且没有剩余数据。
    pub async fn read_stream(&mut self, local: u32, limit: usize) -> Result<Option<Vec<u8>>> {
        loop {
            let (chunk, closed) = {
                let Some(stream) = self.streams.get_mut(&local) else {
                    return Ok(None);
                };
                let take = stream.incoming.len().min(limit);
                let chunk: Vec<u8> = stream.incoming.drain(..take).collect();
                (chunk, stream.closed)
            };
            if !chunk.is_empty() {
                return Ok(Some(chunk));
            }
            if closed {
                return Ok(None);
            }
            self.pump_once().await?;
        }
    }

    /// 关闭并释放一条流。
    pub async fn close_stream(&mut self, local: u32) {
        let Some(mut stream) = self.streams.remove(&local) else {
            return;
        };
        if let Some(remote) = stream.remote {
            if !stream.close_sent {
                stream.close_sent = true;
                let _ = self.send(A_CLSE, remote, local, &[]).await;
            }
        }
    }

    /// 执行一条设备命令并收集完整输出（stdout/stderr 由 adbd 合并）。
    pub async fn exec(&mut self, command: &str) -> Result<Vec<u8>> {
        let local = self.open(&format!("exec:{command}")).await?;
        let result = self.collect_stream(local).await;
        self.close_stream(local).await;
        result
    }

    async fn collect_stream(&mut self, local: u32) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        loop {
            match self.read_stream(local, 64 * 1024).await? {
                Some(chunk) => {
                    output.extend_from_slice(&chunk);
                    if output.len() > MAX_COMMAND_OUTPUT {
                        bail!(
                            "命令输出超过 {} MiB，已中止",
                            MAX_COMMAND_OUTPUT / 1024 / 1024
                        );
                    }
                }
                None => break,
            }
        }
        Ok(output)
    }

    /// 设备是否支持 `shell_v2`（能拿到 stderr 与退出码）。
    pub fn supports_shell_v2(&self) -> bool {
        let banner = self.banner.strip_prefix("device::").unwrap_or(&self.banner);
        banner
            .split(';')
            .filter_map(|field| field.strip_prefix("features="))
            .flat_map(|features| features.split(','))
            .any(|feature| feature == "shell_v2")
    }

    /// 执行命令。设备支持 `shell_v2` 时使用 `shell,v2,raw:`，可区分 stdout/stderr
    /// 并拿到退出码；否则回退到 `exec:`（输出合并、无退出码）。
    pub async fn shell(&mut self, command: &str) -> Result<ShellOutput> {
        if !self.supports_shell_v2() {
            let bytes = self.exec(command).await?;
            return Ok(ShellOutput {
                stdout: String::from_utf8_lossy(&bytes).into_owned(),
                stderr: String::new(),
                exit_code: None,
            });
        }

        let local = self.open(&format!("shell,v2,raw:{command}")).await?;
        let result = self.collect_stream(local).await;
        self.close_stream(local).await;
        Ok(parse_shell_v2(&result?))
    }

    /// 执行一条文本命令，合并 stdout/stderr，并把 CRLF 归一为 LF。
    pub async fn shell_text(&mut self, command: &str) -> Result<String> {
        Ok(self.shell(command).await?.text())
    }

    // ── sync 服务 ────────────────────────────────────────────────────────

    async fn sync_request(&mut self, local: u32, command: &str, path: &str) -> Result<()> {
        if path.len() > u32::MAX as usize {
            bail!("路径过长");
        }
        let mut request = Vec::with_capacity(8 + path.len());
        request.extend_from_slice(command.as_bytes());
        request.extend_from_slice(&(path.len() as u32).to_le_bytes());
        request.extend_from_slice(path.as_bytes());
        self.write_stream(local, &request).await
    }

    async fn sync_read_exact(&mut self, local: u32, length: usize) -> Result<Vec<u8>> {
        let mut output = Vec::with_capacity(length);
        while output.len() < length {
            match self.read_stream(local, length - output.len()).await? {
                Some(chunk) => output.extend_from_slice(&chunk),
                None => bail!("sync 服务提前关闭"),
            }
        }
        Ok(output)
    }

    async fn sync_read_failure(&mut self, local: u32) -> Result<()> {
        let length = read_u32(&self.sync_read_exact(local, 4).await?) as usize;
        let message = self.sync_read_exact(local, length).await?;
        bail!("{}", String::from_utf8_lossy(&message).trim());
    }

    async fn sync_read_okay(&mut self, local: u32) -> Result<()> {
        let response = self.sync_read_exact(local, 4).await?;
        match response.as_slice() {
            b"OKAY" => Ok(()),
            b"FAIL" => self.sync_read_failure(local).await,
            other => bail!("未知 sync 响应：{:?}", String::from_utf8_lossy(other)),
        }
    }

    /// 推送到设备（`sync SEND`）。`mtime` 是设备上的修改时间（Unix 秒）。
    pub async fn push(&mut self, remote: &str, data: &[u8], mtime: u32) -> Result<()> {
        if data.len() > MAX_SYNC_FILE {
            bail!(
                "文件超过 {} MiB，浏览器版无法推送",
                MAX_SYNC_FILE / 1024 / 1024
            );
        }
        let local = self.open("sync:").await?;
        let result = async {
            self.sync_request(local, "SEND", remote).await?;
            for chunk in data.chunks(SYNC_CHUNK) {
                let mut frame = Vec::with_capacity(8 + chunk.len());
                frame.extend_from_slice(b"DATA");
                frame.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
                frame.extend_from_slice(chunk);
                self.write_stream(local, &frame).await?;
            }
            let mut done = Vec::with_capacity(8);
            done.extend_from_slice(b"DONE");
            done.extend_from_slice(&mtime.to_le_bytes());
            self.write_stream(local, &done).await?;
            self.sync_read_okay(local).await?;
            let _ = self.write_stream(local, b"QUIT").await;
            Ok(())
        }
        .await;
        self.close_stream(local).await;
        result
    }

    /// 从设备拉取文件（`sync RECV`）。
    pub async fn pull(&mut self, remote: &str) -> Result<Vec<u8>> {
        let local = self.open("sync:").await?;
        let result = async {
            self.sync_request(local, "RECV", remote).await?;
            let mut output = Vec::new();
            loop {
                let command = self.sync_read_exact(local, 4).await?;
                match command.as_slice() {
                    b"DATA" => {
                        let length = read_u32(&self.sync_read_exact(local, 4).await?) as usize;
                        if output.len() + length > MAX_SYNC_FILE {
                            bail!(
                                "文件超过 {} MiB，浏览器版无法下载",
                                MAX_SYNC_FILE / 1024 / 1024
                            );
                        }
                        output.extend(self.sync_read_exact(local, length).await?);
                    }
                    b"DONE" => {
                        let _ = self.sync_read_exact(local, 4).await?;
                        break;
                    }
                    b"FAIL" => self.sync_read_failure(local).await?,
                    other => bail!("未知 sync 响应：{:?}", String::from_utf8_lossy(other)),
                }
            }
            Ok(output)
        }
        .await;
        self.close_stream(local).await;
        result
    }

    /// 列出设备目录（`sync LIST` v1）。
    pub async fn list(&mut self, remote: &str) -> Result<Vec<RemoteEntry>> {
        let local = self.open("sync:").await?;
        let result = async {
            self.sync_request(local, "LIST", remote).await?;
            let mut entries = Vec::new();
            loop {
                let command = self.sync_read_exact(local, 4).await?;
                match command.as_slice() {
                    b"DENT" => {
                        let stat = self.sync_read_exact(local, 16).await?;
                        let mode = read_u32(&stat[0..4]);
                        let size = read_u32(&stat[4..8]);
                        let mtime = read_u32(&stat[8..12]);
                        let name_length = read_u32(&stat[12..16]) as usize;
                        let name_bytes = self.sync_read_exact(local, name_length).await?;
                        let name = String::from_utf8_lossy(&name_bytes)
                            .trim_end_matches('\0')
                            .to_string();
                        if name.is_empty() || name == "." || name == ".." {
                            continue;
                        }
                        entries.push(RemoteEntry {
                            name,
                            is_dir: mode & 0o170_000 == 0o040_000,
                            is_link: mode & 0o170_000 == 0o120_000,
                            size: u64::from(size),
                            mtime: u64::from(mtime),
                            mode,
                        });
                    }
                    b"DONE" => {
                        let _ = self.sync_read_exact(local, 4).await?;
                        break;
                    }
                    b"FAIL" => self.sync_read_failure(local).await?,
                    other => bail!("未知 sync 响应：{:?}", String::from_utf8_lossy(other)),
                }
            }
            entries.sort_by(|left, right| {
                right
                    .is_dir
                    .cmp(&left.is_dir)
                    .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            });
            Ok(entries)
        }
        .await;
        self.close_stream(local).await;
        result
    }
}

/// `shell_v2` 的一条命令结果。
#[derive(Clone, Debug, Default)]
pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<u8>,
}

impl ShellOutput {
    pub fn success(&self) -> bool {
        self.exit_code.map(|code| code == 0).unwrap_or(true)
    }

    /// stdout + stderr，CRLF 归一为 LF。
    pub fn text(&self) -> String {
        let stdout = self.stdout.replace("\r\n", "\n");
        let stderr = self.stderr.replace("\r\n", "\n");
        match (stdout.trim(), stderr.trim()) {
            ("", "") => String::new(),
            ("", stderr) => stderr.to_string(),
            (stdout, "") => stdout.to_string(),
            (stdout, stderr) => format!("{stdout}\n{stderr}"),
        }
    }
}

/// 解析 `shell_v2` 的帧：`id:u8, length:u32, payload`。
/// id 1 = stdout，2 = stderr，3 = 退出码。
fn parse_shell_v2(raw: &[u8]) -> ShellOutput {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_code = None;
    let mut offset = 0;
    while offset + 5 <= raw.len() {
        let id = raw[offset];
        let length = read_u32(&raw[offset + 1..offset + 5]) as usize;
        offset += 5;
        if offset + length > raw.len() {
            break;
        }
        let data = &raw[offset..offset + length];
        match id {
            1 => stdout.extend_from_slice(data),
            2 => stderr.extend_from_slice(data),
            3 => exit_code = data.first().copied(),
            _ => {}
        }
        offset += length;
    }
    ShellOutput {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_code,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    pub mtime: u64,
    pub mode: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_roundtrip() {
        let message = Message {
            command: A_OPEN,
            arg0: 7,
            arg1: 0,
            payload: b"shell:ls".to_vec(),
        };
        let encoded = message.encode();
        assert_eq!(encoded.len(), HEADER_SIZE + message.payload.len());

        let mut parser = MessageParser::new(MAX_PAYLOAD);
        parser.push(&encoded[..10]);
        assert_eq!(parser.next_message().unwrap(), None);
        parser.push(&encoded[10..]);
        assert_eq!(parser.next_message().unwrap(), Some(message));
        assert_eq!(parser.next_message().unwrap(), None);
    }

    #[test]
    fn parser_rejects_bad_checksum() {
        let mut encoded = Message {
            command: A_OKAY,
            arg0: 1,
            arg1: 2,
            payload: vec![1, 2, 3],
        }
        .encode();
        encoded[HEADER_SIZE] ^= 0xff;
        let mut parser = MessageParser::new(MAX_PAYLOAD);
        parser.push(&encoded);
        assert!(parser.next_message().is_err());
    }

    #[test]
    fn parser_handles_multiple_messages() {
        let first = Message {
            command: A_OKAY,
            arg0: 1,
            arg1: 2,
            payload: Vec::new(),
        };
        let second = Message {
            command: A_CLSE,
            arg0: 1,
            arg1: 2,
            payload: Vec::new(),
        };
        let mut bytes = first.encode();
        bytes.extend(second.encode());
        let mut parser = MessageParser::new(MAX_PAYLOAD);
        parser.push(&bytes);
        assert_eq!(parser.next_message().unwrap(), Some(first));
        assert_eq!(parser.next_message().unwrap(), Some(second));
    }

    struct MockLink {
        incoming: VecDeque<u8>,
        written: Vec<Message>,
    }

    impl MockLink {
        fn new(messages: &[Message]) -> Self {
            let mut incoming = VecDeque::new();
            for message in messages {
                incoming.extend(message.encode());
            }
            Self {
                incoming,
                written: Vec::new(),
            }
        }

        fn record(&mut self, data: &[u8]) {
            let mut parser = MessageParser::new(MAX_PAYLOAD);
            parser.push(data);
            while let Some(message) = parser.next_message().expect("host wrote a valid message") {
                self.written.push(message);
            }
        }
    }

    impl Link for MockLink {
        async fn write_all(&mut self, data: &[u8]) -> Result<()> {
            self.record(data);
            Ok(())
        }

        async fn read_some(&mut self, buffer: &mut [u8]) -> Result<usize> {
            let available = self.incoming.len().min(buffer.len());
            for index in 0..available {
                buffer[index] = self.incoming.pop_front().unwrap();
            }
            Ok(available)
        }
    }

    fn device_banner(features: &str) -> Vec<u8> {
        format!("device::ro.product.name=test;ro.product.model=Test Device;features={features}")
            .into_bytes()
    }

    #[test]
    fn handshake_signs_token_with_saved_key() {
        use rsa::pkcs1v15::{Signature, VerifyingKey};
        use rsa::signature::Verifier;

        let key = AdbKey::generate().expect("key");
        let token = [0x5au8; 20];
        let link = MockLink::new(&[
            Message {
                command: A_AUTH,
                arg0: AUTH_TOKEN,
                arg1: 0,
                payload: token.to_vec(),
            },
            Message {
                command: A_CNXN,
                arg0: A_VERSION,
                arg1: 4096,
                payload: device_banner("shell_v2,cmd"),
            },
        ]);
        let mut adb = Adb::new(link);
        let banner = futures::executor::block_on(adb.handshake(&key)).expect("handshake");

        assert!(banner.contains("ro.product.model=Test Device"));
        assert!(adb.supports_shell_v2());
        assert_eq!(adb.link_mut().written.len(), 2);
        assert_eq!(adb.link_mut().written[0].command, A_CNXN);
        let auth = &adb.link_mut().written[1];
        assert_eq!(auth.command, A_AUTH);
        assert_eq!(auth.arg0, AUTH_SIGNATURE);
        assert_eq!(auth.payload.len(), 256);

        let public = key.public_key();
        let verifier = VerifyingKey::<sha1::Sha1>::new(public);
        let signature = Signature::try_from(auth.payload.as_slice()).expect("signature");
        verifier
            .verify(&token, &signature)
            .expect("valid signature");
    }

    #[test]
    fn handshake_sends_public_key_when_token_repeats() {
        let key = AdbKey::generate().expect("key");
        let token = Message {
            command: A_AUTH,
            arg0: AUTH_TOKEN,
            arg1: 0,
            payload: vec![9u8; 20],
        };
        let link = MockLink::new(&[
            token.clone(),
            token,
            Message {
                command: A_CNXN,
                arg0: A_VERSION,
                arg1: 4096,
                payload: device_banner("shell_v2"),
            },
        ]);
        let mut adb = Adb::new(link);
        futures::executor::block_on(adb.handshake(&key)).expect("handshake");

        let written = &adb.link_mut().written;
        assert_eq!(written.len(), 3);
        assert_eq!(written[1].arg0, AUTH_SIGNATURE);
        assert_eq!(written[2].arg0, AUTH_RSAPUBLICKEY);
        let payload = &written[2].payload;
        assert_eq!(payload.last(), Some(&0u8));
        assert!(payload.starts_with(key.encoded().as_bytes()));
    }

    #[test]
    fn exec_opens_service_and_acks_payloads() {
        let link = MockLink::new(&[
            Message {
                command: A_OKAY,
                arg0: 100,
                arg1: 1,
                payload: Vec::new(),
            },
            Message {
                command: A_WRTE,
                arg0: 100,
                arg1: 1,
                payload: b"hello".to_vec(),
            },
            Message {
                command: A_CLSE,
                arg0: 100,
                arg1: 1,
                payload: Vec::new(),
            },
        ]);
        let mut adb = Adb::new(link);
        let output = futures::executor::block_on(adb.exec("echo hi")).expect("exec");
        assert_eq!(output, b"hello");

        let written = &adb.link_mut().written;
        assert_eq!(written[0].command, A_OPEN);
        assert_eq!(written[0].payload, b"exec:echo hi");
        assert!(written.iter().any(|message| message.command == A_OKAY));
        assert!(written.iter().any(|message| message.command == A_CLSE));
    }

    #[test]
    fn sync_pull_reads_data_frames() {
        let mut data_frame = Vec::new();
        data_frame.extend_from_slice(b"DATA");
        data_frame.extend_from_slice(&5u32.to_le_bytes());
        data_frame.extend_from_slice(b"hello");
        let mut done = Vec::new();
        done.extend_from_slice(b"DONE");
        done.extend_from_slice(&0u32.to_le_bytes());

        let link = MockLink::new(&[
            Message {
                command: A_OKAY,
                arg0: 100,
                arg1: 1,
                payload: Vec::new(),
            },
            // 设备对主机的 RECV 请求回 OKAY。
            Message {
                command: A_OKAY,
                arg0: 100,
                arg1: 1,
                payload: Vec::new(),
            },
            Message {
                command: A_WRTE,
                arg0: 100,
                arg1: 1,
                payload: data_frame,
            },
            Message {
                command: A_WRTE,
                arg0: 100,
                arg1: 1,
                payload: done,
            },
            Message {
                command: A_CLSE,
                arg0: 100,
                arg1: 1,
                payload: Vec::new(),
            },
        ]);
        let mut adb = Adb::new(link);
        let content = futures::executor::block_on(adb.pull("/data/test.bin")).expect("pull");
        assert_eq!(content, b"hello");

        let written = &adb.link_mut().written;
        assert_eq!(written[0].command, A_OPEN);
        assert_eq!(written[0].payload, b"sync:");
        let request = &written[1];
        assert_eq!(request.command, A_WRTE);
        assert!(request.payload.starts_with(b"RECV"));
        assert!(request.payload.ends_with(b"/data/test.bin"));
    }

    #[test]
    fn shell_v2_splits_streams_and_exit_code() {
        let mut stdout = vec![1u8];
        stdout.extend_from_slice(&4u32.to_le_bytes());
        stdout.extend_from_slice(b"out\n");
        let mut stderr = vec![2u8];
        stderr.extend_from_slice(&4u32.to_le_bytes());
        stderr.extend_from_slice(b"err\n");
        let exit = vec![3u8, 1, 0, 0, 0, 2];

        let link = MockLink::new(&[
            Message {
                command: A_OKAY,
                arg0: 100,
                arg1: 1,
                payload: Vec::new(),
            },
            Message {
                command: A_WRTE,
                arg0: 100,
                arg1: 1,
                payload: stdout,
            },
            Message {
                command: A_WRTE,
                arg0: 100,
                arg1: 1,
                payload: stderr,
            },
            Message {
                command: A_WRTE,
                arg0: 100,
                arg1: 1,
                payload: exit,
            },
            Message {
                command: A_CLSE,
                arg0: 100,
                arg1: 1,
                payload: Vec::new(),
            },
        ]);
        let mut adb = Adb::new(link);
        adb.banner = "device::features=shell_v2".to_string();
        let output = futures::executor::block_on(adb.shell("false")).expect("shell");
        assert_eq!(output.stdout, "out\n");
        assert_eq!(output.stderr, "err\n");
        assert_eq!(output.exit_code, Some(2));
        assert!(!output.success());

        let written = &adb.link_mut().written;
        assert_eq!(written[0].command, A_OPEN);
        assert_eq!(written[0].payload, b"shell,v2,raw:false");
    }
}
