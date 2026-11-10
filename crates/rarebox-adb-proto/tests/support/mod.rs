//! 进程内假 adbd：独立于被测 crate 的协议解析实现，用于端到端验证。
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rarebox_adb_proto::auth::decode_android_public_key;
use rsa::RsaPublicKey;
use rsa::pkcs1v15::Pkcs1v15Sign;
use sha1::{Digest, Sha1};

pub const C_SYNC: u32 = 0x434e_5953;
pub const C_CNXN: u32 = 0x4e58_4e43;
pub const C_OPEN: u32 = 0x4e45_504f;
pub const C_OKAY: u32 = 0x5941_4b4f;
pub const C_CLSE: u32 = 0x4553_4c43;
pub const C_WRTE: u32 = 0x4554_5257;
pub const C_AUTH: u32 = 0x4854_5541;

pub const AUTH_SIGNATURE: u32 = 2;
pub const AUTH_RSAPUBLICKEY: u32 = 3;

pub const HEADER: usize = 24;
pub const MAX_PAYLOAD: u32 = 1024 * 1024;

/// 与 AOSP 一致的 payload 校验和：逐字节累加，允许回绕。
pub fn checksum(data: &[u8]) -> u32 {
    data.iter()
        .fold(0u32, |sum, byte| sum.wrapping_add(u32::from(*byte)))
}

#[derive(Debug, Clone)]
pub struct Packet {
    pub command: u32,
    pub arg0: u32,
    pub arg1: u32,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn label(&self) -> &'static str {
        match self.command {
            C_CNXN => "CNXN",
            C_OPEN => "OPEN",
            C_OKAY => "OKAY",
            C_CLSE => "CLSE",
            C_WRTE => "WRTE",
            C_AUTH => "AUTH",
            _ => "UNKNOWN",
        }
    }
}

#[derive(Debug, Default)]
pub struct Records {
    pub cnxn_banner: String,
    pub cnxn_version: u32,
    pub cnxn_maxdata: u32,
    pub signature_attempts: u32,
    pub signature_verified: bool,
    pub public_key: Option<String>,
    pub opens: Vec<String>,
    /// host 对假 adbd 发出的 `WRTE` 回的 `OKAY` 数量。
    pub okay_from_host: usize,
    /// 假 adbd 收到并确认的 host → device `WRTE` 数量。
    pub wrte_from_host: usize,
    /// 假 adbd 发往 host 的 `WRTE` 数量。
    pub wrte_sent: usize,
    pub checksum_errors: usize,
}

#[derive(Clone)]
pub enum AuthMode {
    /// 设备不需要鉴权，直接发 CNXN。
    None,
    /// 设备已保存该公钥，host 的签名必须通过验签。
    KnownKey(RsaPublicKey),
    /// 首次配对：拒绝签名 → host 发公钥 → 验签通过后发 CNXN。
    FirstPairing,
    /// 拒绝签名、收到公钥后持续索要 token（模拟设备上的授权对话框）。
    PendingConfirmation,
    /// 收到公钥后直接 CLSE 拒绝。
    RejectAll,
}

pub struct FakeConfig {
    pub version: u32,
    pub maxdata: u32,
    pub auth: AuthMode,
    pub banner: String,
}

impl Default for FakeConfig {
    fn default() -> Self {
        Self {
            version: 0x0100_0000,
            maxdata: 4096,
            auth: AuthMode::None,
            banner: "device::ro.product.name=fake_product;ro.product.model=Fake Phone;\
                     ro.product.device=fake_device;ro.serialno=fake-serial;features=shell_v2,cmd"
                .to_string(),
        }
    }
}

pub struct FakeAdbd {
    pub addr: SocketAddr,
    pub records: Arc<Mutex<Records>>,
    stop: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<TcpStream>>>,
    handle: Option<JoinHandle<()>>,
}

impl FakeAdbd {
    pub fn spawn<F>(config: FakeConfig, handler: F) -> Self
    where
        F: FnOnce(&mut Session) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake adbd");
        let addr = listener.local_addr().expect("local addr");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");

        let stop = Arc::new(AtomicBool::new(false));
        let socket = Arc::new(Mutex::new(None));
        let records = Arc::new(Mutex::new(Records::default()));

        let handle = {
            let stop = stop.clone();
            let socket = socket.clone();
            let records = records.clone();
            thread::Builder::new()
                .name("fake-adbd".to_string())
                .spawn(move || {
                    let mut accepted = None;
                    while !stop.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                accepted = Some(stream);
                                break;
                            }
                            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(5));
                            }
                            Err(_) => break,
                        }
                    }
                    let Some(stream) = accepted else {
                        return;
                    };
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_nodelay(true);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                    *socket.lock().unwrap() = stream.try_clone().ok();

                    let mut sock = FakeSocket { stream };
                    match handshake(&mut sock, &config, &records, &stop) {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(err) => {
                            eprintln!("fake adbd handshake error: {err}");
                            return;
                        }
                    }

                    let mut session = Session {
                        sock,
                        maxdata: config.maxdata,
                        next_remote: 1,
                        records,
                        stop,
                        buffers: HashMap::new(),
                        eof: HashSet::new(),
                    };
                    handler(&mut session);
                })
                .expect("spawn fake adbd thread")
        };

        Self {
            addr,
            records,
            stop,
            socket,
            handle: Some(handle),
        }
    }

    pub fn opens(&self) -> Vec<String> {
        self.records.lock().unwrap().opens.clone()
    }
}

impl Drop for FakeAdbd {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(socket) = self.socket.lock().unwrap().as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        if let Some(handle) = self.handle.take()
            && let Err(payload) = handle.join()
            && !thread::panicking()
        {
            panic!("fake adbd thread panicked: {}", panic_text(&payload));
        }
    }
}

fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "non-string panic".to_string()
    }
}

struct FakeSocket {
    stream: TcpStream,
}

impl FakeSocket {
    fn read_packet(&mut self) -> io::Result<Option<Packet>> {
        let mut raw = [0u8; HEADER];
        loop {
            match self.stream.read(&mut raw[..1]) {
                Ok(0) => return Ok(None),
                Ok(_) => break,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            }
        }
        self.stream.read_exact(&mut raw[1..])?;

        let le32 = |at: usize| u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
        let command = le32(0);
        let magic = le32(20);
        if magic != command ^ 0xffff_ffff {
            return Err(invalid(format!(
                "host sent bad magic: command={command:#010x} magic={magic:#010x}"
            )));
        }
        let length = le32(12) as usize;
        if length > MAX_PAYLOAD as usize {
            return Err(invalid(format!("host payload too large: {length}")));
        }
        let mut payload = vec![0u8; length];
        if length > 0 {
            self.stream.read_exact(&mut payload)?;
        }
        let declared = le32(16);
        if declared != checksum(&payload) {
            return Err(invalid(format!(
                "host sent bad checksum: declared {declared:#010x}, computed {:#010x}",
                checksum(&payload)
            )));
        }
        Ok(Some(Packet {
            command,
            arg0: le32(4),
            arg1: le32(8),
            payload,
        }))
    }

    fn write_packet(
        &mut self,
        command: u32,
        arg0: u32,
        arg1: u32,
        payload: &[u8],
    ) -> io::Result<()> {
        self.write_packet_with_checksum(command, arg0, arg1, payload, checksum(payload))
    }

    fn write_packet_with_checksum(
        &mut self,
        command: u32,
        arg0: u32,
        arg1: u32,
        payload: &[u8],
        declared_checksum: u32,
    ) -> io::Result<()> {
        let mut raw = Vec::with_capacity(HEADER + payload.len());
        for value in [
            command,
            arg0,
            arg1,
            payload.len() as u32,
            declared_checksum,
            command ^ 0xffff_ffff,
        ] {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw.extend_from_slice(payload);
        self.stream.write_all(&raw)
    }
}

/// `Ok(true)` = 握手完成，`Ok(false)` = 连接已结束（无需再跑 handler）。
fn handshake(
    sock: &mut FakeSocket,
    config: &FakeConfig,
    records: &Arc<Mutex<Records>>,
    stop: &Arc<AtomicBool>,
) -> io::Result<bool> {
    let Some(cnxn) = sock.read_packet()? else {
        return Ok(false);
    };
    if cnxn.command != C_CNXN {
        return Err(invalid(format!("expected CNXN, got {}", cnxn.label())));
    }
    if cnxn.arg0 != 0x0100_0000 {
        return Err(invalid(format!(
            "unexpected host version {:#010x}",
            cnxn.arg0
        )));
    }
    let banner = String::from_utf8_lossy(&cnxn.payload).to_string();
    if !banner.starts_with("host::features=") {
        return Err(invalid(format!("unexpected host banner {banner:?}")));
    }
    {
        let mut records = records.lock().unwrap();
        records.cnxn_banner = banner;
        records.cnxn_version = cnxn.arg0;
        records.cnxn_maxdata = cnxn.arg1;
    }

    match &config.auth {
        AuthMode::None => {}
        AuthMode::KnownKey(public_key) => {
            let token = send_token(sock)?;
            let auth = expect_auth(sock)?;
            if auth.arg0 != AUTH_SIGNATURE {
                return Err(invalid("expected AUTH_SIGNATURE"));
            }
            verify_signature(public_key, &token, &auth.payload)?;
            records.lock().unwrap().signature_verified = true;
        }
        AuthMode::FirstPairing | AuthMode::PendingConfirmation | AuthMode::RejectAll => {
            let token = send_token(sock)?;
            let signature = expect_auth(sock)?;
            if signature.arg0 != AUTH_SIGNATURE {
                return Err(invalid("expected AUTH_SIGNATURE"));
            }
            records.lock().unwrap().signature_attempts += 1;

            // 拒绝签名：再发一个 token。
            let _second = send_token(sock)?;
            let public_key_packet = expect_auth(sock)?;
            if public_key_packet.arg0 != AUTH_RSAPUBLICKEY {
                return Err(invalid(format!(
                    "expected AUTH_RSAPUBLICKEY, got arg0={}",
                    public_key_packet.arg0
                )));
            }
            let text = trim_nul(&public_key_packet.payload);
            let public_key = decode_android_public_key(&text)
                .map_err(|err| invalid(format!("bad Android public key: {err}")))?;
            verify_signature(&public_key, &token, &signature.payload)?;
            {
                let mut records = records.lock().unwrap();
                records.signature_verified = true;
                records.public_key = Some(text);
            }

            match config.auth {
                AuthMode::RejectAll => {
                    sock.write_packet(C_CLSE, 0, 0, b"device offline: unauthorized")?;
                    return Ok(false);
                }
                AuthMode::PendingConfirmation => {
                    for _ in 0..40 {
                        if stop.load(Ordering::SeqCst) {
                            return Ok(false);
                        }
                        if sock.write_packet(C_AUTH, 1, 0, &[0x5a; 20]).is_err() {
                            return Ok(false);
                        }
                        thread::sleep(Duration::from_millis(25));
                    }
                    return Ok(false);
                }
                _ => {}
            }
        }
    }

    sock.write_packet(
        C_CNXN,
        config.version,
        config.maxdata,
        config.banner.as_bytes(),
    )?;
    Ok(true)
}

fn send_token(sock: &mut FakeSocket) -> io::Result<Vec<u8>> {
    let token = [0xa5u8; 20];
    sock.write_packet(C_AUTH, 1, 0, &token)?;
    Ok(token.to_vec())
}

fn expect_auth(sock: &mut FakeSocket) -> io::Result<Packet> {
    let packet = sock
        .read_packet()?
        .ok_or_else(|| invalid("expected AUTH, got EOF"))?;
    if packet.command != C_AUTH {
        return Err(invalid(format!("expected AUTH, got {}", packet.label())));
    }
    Ok(packet)
}

fn verify_signature(public_key: &RsaPublicKey, token: &[u8], signature: &[u8]) -> io::Result<()> {
    let digest = Sha1::digest(token);
    public_key
        .verify(Pkcs1v15Sign::new::<Sha1>(), &digest, signature)
        .map_err(|err| invalid(format!("signature rejected: {err}")))
}

fn trim_nul(payload: &[u8]) -> String {
    let end = payload
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(payload.len());
    String::from_utf8_lossy(&payload[..end]).to_string()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn is_disconnect(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::NotConnected
    )
}

#[derive(Debug, Clone)]
pub struct OpenRequest {
    pub local_id: u32,
    pub remote_id: u32,
    pub service: String,
}

pub struct Session {
    sock: FakeSocket,
    pub maxdata: u32,
    next_remote: u32,
    pub records: Arc<Mutex<Records>>,
    stop: Arc<AtomicBool>,
    buffers: HashMap<u32, VecDeque<u8>>,
    eof: HashSet<u32>,
}

impl Session {
    /// 读取并确认一个 host → device 的 `WRTE` 数据包（一般不需要直接调用）。
    fn pump(&mut self) -> io::Result<bool> {
        let packet = match self.sock.read_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => return Ok(false),
            Err(err) if is_disconnect(&err) => return Ok(false),
            Err(err) => return Err(err),
        };
        match packet.command {
            C_WRTE => {
                self.sock
                    .write_packet(C_OKAY, packet.arg0, packet.arg1, &[])?;
                self.records.lock().unwrap().wrte_from_host += 1;
                self.buffers
                    .entry(packet.arg1)
                    .or_default()
                    .extend(packet.payload);
                Ok(true)
            }
            C_CLSE => {
                self.eof.insert(packet.arg1);
                self.buffers.entry(packet.arg1).or_default();
                Ok(true)
            }
            C_OKAY => {
                self.records.lock().unwrap().okay_from_host += 1;
                Ok(true)
            }
            other => Err(invalid(format!(
                "unexpected command {other:#010x} from host"
            ))),
        }
    }

    pub fn next_packet(&mut self) -> io::Result<Option<Packet>> {
        match self.sock.read_packet() {
            Ok(packet) => Ok(packet),
            Err(err) if is_disconnect(&err) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// 接受下一个 `OPEN`，自动回 `OKAY` 并分配 `remote_id`。
    pub fn accept_open(&mut self) -> io::Result<Option<OpenRequest>> {
        loop {
            let Some(packet) = self.next_packet()? else {
                return Ok(None);
            };
            match packet.command {
                C_OPEN => {
                    let service = trim_nul(&packet.payload);
                    let remote_id = self.next_remote;
                    self.next_remote += 1;
                    self.sock
                        .write_packet(C_OKAY, remote_id, packet.arg0, &[])?;
                    self.records.lock().unwrap().opens.push(service.clone());
                    return Ok(Some(OpenRequest {
                        local_id: packet.arg0,
                        remote_id,
                        service,
                    }));
                }
                C_CLSE | C_OKAY => continue,
                C_WRTE => {
                    // 已关闭流上残留的数据：确认后丢弃。
                    self.sock
                        .write_packet(C_OKAY, packet.arg0, packet.arg1, &[])?;
                    continue;
                }
                other => {
                    return Err(invalid(format!("expected OPEN, got {other:#010x}")));
                }
            }
        }
    }

    pub fn send_raw(
        &mut self,
        command: u32,
        arg0: u32,
        arg1: u32,
        payload: &[u8],
    ) -> io::Result<()> {
        self.sock.write_packet(command, arg0, arg1, payload)
    }

    pub fn send_raw_with_checksum(
        &mut self,
        command: u32,
        arg0: u32,
        arg1: u32,
        payload: &[u8],
        declared_checksum: u32,
    ) -> io::Result<()> {
        self.sock
            .write_packet_with_checksum(command, arg0, arg1, payload, declared_checksum)
    }

    /// 按 `maxdata` 分片发送 `WRTE`。
    pub fn send_wrte(&mut self, local_id: u32, remote_id: u32, data: &[u8]) -> io::Result<()> {
        let chunk = (self.maxdata as usize).clamp(1, 64 * 1024);
        for piece in data.chunks(chunk) {
            self.sock.write_packet(C_WRTE, remote_id, local_id, piece)?;
            self.records.lock().unwrap().wrte_sent += 1;
        }
        Ok(())
    }

    pub fn send_close(&mut self, local_id: u32, remote_id: u32, reason: &str) -> io::Result<()> {
        self.sock
            .write_packet(C_CLSE, remote_id, local_id, reason.as_bytes())
    }

    pub fn send_status(
        &mut self,
        local_id: u32,
        remote_id: u32,
        id: &[u8; 4],
        message: &str,
    ) -> io::Result<()> {
        let mut frame = Vec::with_capacity(8 + message.len());
        frame.extend_from_slice(id);
        frame.extend_from_slice(&(message.len() as u32).to_le_bytes());
        frame.extend_from_slice(message.as_bytes());
        self.send_wrte(local_id, remote_id, &frame)
    }

    /// 读取该流上任意可用字节；`None` 表示流已关闭。
    pub fn read_some(&mut self, local_id: u32) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(buffer) = self.buffers.get_mut(&local_id)
                && !buffer.is_empty()
            {
                return Ok(Some(buffer.drain(..).collect()));
            }
            if self.eof.contains(&local_id) {
                return Ok(None);
            }
            if !self.pump()? {
                return Ok(None);
            }
        }
    }

    /// 精确读取 `len` 字节。
    pub fn read_exact(&mut self, local_id: u32, len: usize) -> io::Result<Vec<u8>> {
        loop {
            let available = self.buffers.get(&local_id).map_or(0, VecDeque::len);
            if available >= len {
                return Ok(self
                    .buffers
                    .get_mut(&local_id)
                    .expect("buffer exists")
                    .drain(..len)
                    .collect());
            }
            if self.eof.contains(&local_id) {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream closed before enough bytes arrived",
                ));
            }
            if !self.pump()? {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed before enough bytes arrived",
                ));
            }
        }
    }

    /// sync 帧：8 字节头；流正常结束时返回 `None`。
    pub fn read_frame(&mut self, local_id: u32) -> io::Result<Option<([u8; 4], u32)>> {
        loop {
            let available = self.buffers.get(&local_id).map_or(0, VecDeque::len);
            if available >= 8 {
                let raw = self
                    .buffers
                    .get_mut(&local_id)
                    .expect("buffer exists")
                    .drain(..8)
                    .collect::<Vec<_>>();
                let id = [raw[0], raw[1], raw[2], raw[3]];
                let length = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
                return Ok(Some((id, length)));
            }
            if self.eof.contains(&local_id) {
                return Ok(None);
            }
            if !self.pump()? {
                return Ok(None);
            }
        }
    }

    /// 等 host 对假 adbd 发出的 `WRTE` 全部回 `OKAY`（验证流控）。
    pub fn expect_okays(&mut self, count: usize) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let start = self.records.lock().unwrap().okay_from_host;
        while self.records.lock().unwrap().okay_from_host < start + count {
            if Instant::now() > deadline {
                return Err(invalid(format!(
                    "timed out waiting for {count} OKAY packets"
                )));
            }
            if !self.pump()? {
                return Err(invalid("connection ended while waiting for OKAY"));
            }
        }
        Ok(())
    }

    pub fn abort(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.sock.stream.shutdown(Shutdown::Both);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FakeFile {
    pub data: Vec<u8>,
    pub mode: u32,
    pub mtime: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Upload {
    pub spec: String,
    pub mtime: u32,
}

#[derive(Debug, Default)]
pub struct FakeFs {
    pub files: BTreeMap<String, FakeFile>,
    pub dirs: BTreeSet<String>,
    pub uploads: Vec<Upload>,
}

impl FakeFs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: &str, data: Vec<u8>, mode: u32, mtime: u32) {
        self.files
            .insert(path.to_string(), FakeFile { data, mode, mtime });
    }

    pub fn enable_dir(&mut self, path: &str) {
        self.dirs.insert(path.to_string());
    }

    pub fn stat(&self, path: &str) -> Option<(u32, u32, u32)> {
        if let Some(file) = self.files.get(path) {
            return Some((file.mode, file.data.len() as u32, file.mtime));
        }
        if self.is_dir(path) {
            return Some((0o040_755, 4096, 0));
        }
        None
    }

    pub fn is_dir(&self, path: &str) -> bool {
        if self.dirs.contains(path) {
            return true;
        }
        let prefix = format!("{}/", path.trim_end_matches('/'));
        self.files.keys().any(|item| item.starts_with(&prefix))
    }

    pub fn list(&self, dir: &str) -> Vec<(String, u32, u32, u32)> {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        let mut children: BTreeMap<String, (u32, u32, u32)> = BTreeMap::new();
        for (path, file) in &self.files {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            let name = rest.split('/').next().unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            if rest.contains('/') {
                children
                    .entry(name.to_string())
                    .or_insert((0o040_755, 4096, 0));
            } else {
                children.insert(
                    name.to_string(),
                    (file.mode, file.data.len() as u32, file.mtime),
                );
            }
        }
        for path in &self.dirs {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            let name = rest.split('/').next().unwrap_or_default();
            if !name.is_empty() {
                children
                    .entry(name.to_string())
                    .or_insert((0o040_755, 4096, 0));
            }
        }
        children
            .into_iter()
            .map(|(name, (mode, size, mtime))| (name, mode, size, mtime))
            .collect()
    }
}

/// 在一条 `sync:` 流上提供最小可用的文件服务。
pub fn serve_sync(
    session: &mut Session,
    open: &OpenRequest,
    fs: &Arc<Mutex<FakeFs>>,
) -> io::Result<()> {
    loop {
        let Some((id, length)) = session.read_frame(open.local_id)? else {
            return Ok(());
        };
        match &id {
            b"SEND" => {
                let spec =
                    String::from_utf8_lossy(&session.read_exact(open.local_id, length as usize)?)
                        .to_string();
                let mut data = Vec::new();
                let mtime;
                loop {
                    let Some((frame_id, frame_len)) = session.read_frame(open.local_id)? else {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "EOF in the middle of SEND",
                        ));
                    };
                    if &frame_id == b"DATA" {
                        data.extend(session.read_exact(open.local_id, frame_len as usize)?);
                    } else if &frame_id == b"DONE" {
                        mtime = frame_len;
                        break;
                    } else {
                        session.send_status(
                            open.local_id,
                            open.remote_id,
                            b"FAIL",
                            "invalid data message",
                        )?;
                        return Ok(());
                    }
                }
                if spec.contains("denied") {
                    session.send_status(
                        open.local_id,
                        open.remote_id,
                        b"FAIL",
                        "couldn't create file: Permission denied",
                    )?;
                    continue;
                }
                let Some((path, mode)) = spec.rsplit_once(',') else {
                    session.send_status(
                        open.local_id,
                        open.remote_id,
                        b"FAIL",
                        "missing , in ID_SEND_V1",
                    )?;
                    continue;
                };
                let mode: u32 = mode.trim().parse().unwrap_or(0o644);
                let path = path.to_string();
                {
                    let mut fs = fs.lock().unwrap();
                    fs.insert(&path, data, 0o100_000 | (mode & 0o777), mtime);
                    fs.uploads.push(Upload {
                        spec: spec.clone(),
                        mtime,
                    });
                }
                session.send_status(open.local_id, open.remote_id, b"OKAY", "")?;
            }
            b"RECV" => {
                let path =
                    String::from_utf8_lossy(&session.read_exact(open.local_id, length as usize)?)
                        .to_string();
                let file = fs.lock().unwrap().files.get(&path).cloned();
                match file {
                    Some(file) => {
                        for chunk in file.data.chunks(64 * 1024) {
                            let mut frame = Vec::with_capacity(8 + chunk.len());
                            frame.extend_from_slice(b"DATA");
                            frame.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
                            frame.extend_from_slice(chunk);
                            session.send_wrte(open.local_id, open.remote_id, &frame)?;
                        }
                        session.send_status(open.local_id, open.remote_id, b"DONE", "")?;
                    }
                    None => {
                        session.send_status(
                            open.local_id,
                            open.remote_id,
                            b"FAIL",
                            &format!("open failed: No such file or directory ({path})"),
                        )?;
                    }
                }
            }
            b"STAT" => {
                let path =
                    String::from_utf8_lossy(&session.read_exact(open.local_id, length as usize)?)
                        .to_string();
                let (mode, size, mtime) = fs.lock().unwrap().stat(&path).unwrap_or((0, 0, 0));
                let mut payload = Vec::with_capacity(16);
                payload.extend_from_slice(b"STAT");
                for value in [mode, size, mtime] {
                    payload.extend_from_slice(&value.to_le_bytes());
                }
                session.send_wrte(open.local_id, open.remote_id, &payload)?;
            }
            b"LIST" => {
                let path =
                    String::from_utf8_lossy(&session.read_exact(open.local_id, length as usize)?)
                        .to_string();
                let entries = fs.lock().unwrap().list(&path);
                for (name, mode, size, mtime) in entries {
                    let mut payload = Vec::with_capacity(20 + name.len());
                    payload.extend_from_slice(b"DENT");
                    for value in [mode, size, mtime, name.len() as u32] {
                        payload.extend_from_slice(&value.to_le_bytes());
                    }
                    payload.extend_from_slice(name.as_bytes());
                    session.send_wrte(open.local_id, open.remote_id, &payload)?;
                }
                let mut done = Vec::with_capacity(20);
                done.extend_from_slice(b"DONE");
                done.extend_from_slice(&[0u8; 16]);
                session.send_wrte(open.local_id, open.remote_id, &done)?;
            }
            other => {
                session.send_status(
                    open.local_id,
                    open.remote_id,
                    b"FAIL",
                    &format!("unknown command {}", String::from_utf8_lossy(other)),
                )?;
            }
        }
    }
}
