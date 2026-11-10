//! ADB 连接：TCP 连接、CNXN/AUTH 握手、流的多路复用。
//!
//! 握手结束后会启动一个读取线程：它按包头里的 `local_id`（`arg1`）把
//! `OKAY` / `WRTE` / `CLSE` 分发给对应流，并在收到 `WRTE` 时立即回 `OKAY`
//! 做流控确认。发送路径由持锁的写半连接串行化，保证不同流的包头不会交错。

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use crate::auth::{AdbKey, default_identity};
use crate::error::{Error, Result};
use crate::proto::{
    A_VERSION_MIN, A_VERSION_SKIP_CHECKSUM, AUTH_RSAPUBLICKEY, AUTH_SIGNATURE, AUTH_TOKEN, Command,
    HEADER_SIZE, MAX_PAYLOAD, MAX_PAYLOAD_V1, MessageHeader, TOKEN_SIZE, checksum, service_payload,
};
use crate::stream::AdbStream;

/// 鉴权过程的可选观察者，用于界面上提示“请在手机上确认”。
pub type AuthObserver = Arc<dyn Fn(AuthEvent) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthEvent {
    /// 设备发来 `AUTH TOKEN`。
    TokenReceived,
    /// 已用本地私钥签名并回给设备。
    SignatureSent,
    /// 设备再次索要 token，说明刚才的签名不被接受。
    SignatureRejected,
    /// 已发送公钥，接下来要等用户在设备上点“允许”。
    PublicKeySent,
}

/// 建立连接的可调参数。
#[derive(Clone)]
pub struct ConnectOptions {
    /// CNXN 里通告的最大负载，会被设备的上限收紧。
    pub max_payload: u32,
    /// CNXN payload 里的 `features=` 列表。
    pub features: Vec<String>,
    /// 公钥文件里 `user@host` 部分。
    pub identity: String,
    /// 建连 + 握手总超时。
    pub handshake_timeout: Duration,
    /// 打开一个服务（等待 OKAY）的超时。
    pub open_timeout: Duration,
    /// 等待设备 `OKAY` 流控确认的超时。
    pub write_timeout: Duration,
    pub auth_observer: Option<AuthObserver>,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            max_payload: MAX_PAYLOAD,
            // 只声明真正用得上的能力：我们打开的是不带 v2 前缀的 `shell:`/`exec:`。
            features: vec!["cmd".to_string()],
            identity: default_identity(),
            handshake_timeout: Duration::from_secs(15),
            open_timeout: Duration::from_secs(15),
            write_timeout: Duration::from_secs(30),
            auth_observer: None,
        }
    }
}

impl ConnectOptions {
    /// `CNXN` 的 payload，例如 `host::features=cmd`。
    pub fn host_banner(&self) -> String {
        if self.features.is_empty() {
            "host::".to_string()
        } else {
            format!("host::features={}", self.features.join(","))
        }
    }
}

impl fmt::Debug for ConnectOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectOptions")
            .field("max_payload", &self.max_payload)
            .field("features", &self.features)
            .field("identity", &self.identity)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("open_timeout", &self.open_timeout)
            .field("write_timeout", &self.write_timeout)
            .field("auth_observer", &self.auth_observer.is_some())
            .finish()
    }
}

/// 连接级失败原因，会广播给所有已打开的流。
#[derive(Clone, Debug)]
pub(crate) enum Failure {
    Protocol(String),
    Checksum { expected: u32, actual: u32 },
    Closed(String),
}

impl Failure {
    pub(crate) fn into_error(self) -> Error {
        match self {
            Failure::Protocol(message) => Error::Protocol(message),
            Failure::Checksum { expected, actual } => Error::Checksum { expected, actual },
            Failure::Closed(message) => Error::ConnectionClosed(message),
        }
    }

    fn message(&self) -> String {
        self.clone().into_error().to_string()
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        match error {
            Error::Checksum { expected, actual } => Failure::Checksum { expected, actual },
            Error::Protocol(message) => Failure::Protocol(message),
            Error::Io(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                Failure::Closed(format!("设备在数据包中途断开：{err}"))
            }
            other => Failure::Closed(other.to_string()),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn wait<'a, T>(condvar: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    condvar
        .wait(guard)
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn wait_timeout<'a, T>(
    condvar: &Condvar,
    guard: MutexGuard<'a, T>,
    timeout: Duration,
) -> MutexGuard<'a, T> {
    condvar
        .wait_timeout(guard, timeout)
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .0
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// 一条已建立的 ADB 连接；可以克隆后在多线程间共享。
#[derive(Clone)]
pub struct Connection {
    inner: Arc<ConnInner>,
}

struct ConnInner {
    write: Mutex<TcpStream>,
    socket: TcpStream,
    peer_addr: Option<SocketAddr>,
    local_addr: Option<SocketAddr>,
    streams: Mutex<HashMap<u32, Arc<StreamSlot>>>,
    next_local_id: AtomicU32,
    max_payload: u32,
    peer_version: u32,
    banner: String,
    write_timeout: Duration,
    open_timeout: Duration,
    closed: AtomicBool,
    failure: Mutex<Option<Failure>>,
}

impl Drop for ConnInner {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

/// 单个流在读取线程与用户线程之间共享的状态。
pub(crate) struct StreamSlot {
    state: Mutex<SlotState>,
    condvar: Condvar,
}

struct SlotState {
    events: VecDeque<StreamEvent>,
    pending_acks: VecDeque<usize>,
    outstanding: usize,
    remote_id: u32,
    closed: bool,
    close_reason: Option<String>,
    failure: Option<Failure>,
}

pub(crate) enum StreamEvent {
    Data(Vec<u8>),
    Close(Option<String>),
    Failure(Failure),
}

impl StreamSlot {
    fn new() -> Self {
        Self {
            state: Mutex::new(SlotState {
                events: VecDeque::new(),
                pending_acks: VecDeque::new(),
                outstanding: 0,
                remote_id: 0,
                closed: false,
                close_reason: None,
                failure: None,
            }),
            condvar: Condvar::new(),
        }
    }

    pub(crate) fn remote_id(&self) -> u32 {
        lock(&self.state).remote_id
    }

    pub(crate) fn close_reason(&self) -> Option<String> {
        lock(&self.state).close_reason.clone()
    }

    pub(crate) fn push(&self, event: StreamEvent) {
        let mut state = lock(&self.state);
        match &event {
            StreamEvent::Close(reason) => {
                state.closed = true;
                state.close_reason = reason.clone();
            }
            StreamEvent::Failure(failure) => {
                state.failure = Some(failure.clone());
            }
            StreamEvent::Data(_) => {}
        }
        state.events.push_back(event);
        self.condvar.notify_all();
    }

    fn on_okay(&self, remote_id: u32) {
        let mut state = lock(&self.state);
        if state.remote_id == 0 {
            // OPEN 的回应：arg0 是设备侧的 remote_id。
            state.remote_id = remote_id;
            self.condvar.notify_all();
            return;
        }
        if let Some(size) = state.pending_acks.pop_front() {
            state.outstanding = state.outstanding.saturating_sub(size);
            self.condvar.notify_all();
        }
    }

    fn on_remote_id(&self, remote_id: u32) {
        let mut state = lock(&self.state);
        if state.remote_id == 0 && remote_id != 0 {
            state.remote_id = remote_id;
            self.condvar.notify_all();
        }
    }

    fn wait_open(&self, timeout: Duration, service: &str) -> Result<u32> {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.state);
        loop {
            if state.remote_id != 0 {
                return Ok(state.remote_id);
            }
            if let Some(failure) = state.failure.clone() {
                return Err(failure.into_error());
            }
            if state.closed {
                let reason = state
                    .close_reason
                    .clone()
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or_else(|| "设备关闭了流，但没有给出原因".to_string());
                return Err(Error::DeviceClosed {
                    service: service.to_string(),
                    reason,
                });
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout(format!("等待设备接受 {service} 超时")));
            }
            state = wait_timeout(&self.condvar, state, deadline - now);
        }
    }

    /// 为一次 `WRTE` 申请流控窗口；未收到足够 `OKAY` 时阻塞。
    pub(crate) fn reserve(&self, len: usize, window: usize, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.state);
        loop {
            if let Some(failure) = state.failure.clone() {
                return Err(failure.into_error());
            }
            if state.closed {
                return Err(Error::ConnectionClosed("数据流已关闭".to_string()));
            }
            if state.outstanding == 0 || state.outstanding + len <= window {
                state.pending_acks.push_back(len);
                state.outstanding += len;
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout("等待设备流控 OKAY 超时".to_string()));
            }
            state = wait_timeout(&self.condvar, state, deadline - now);
        }
    }

    /// 取出一个事件；连接/流都尚未结束时阻塞等待。
    pub(crate) fn take_event(&self) -> Option<StreamEvent> {
        let mut state = lock(&self.state);
        loop {
            if let Some(event) = state.events.pop_front() {
                return Some(event);
            }
            if state.closed || state.failure.is_some() {
                return None;
            }
            state = wait(&self.condvar, state);
        }
    }

    /// 标记关闭；返回 true 表示这次调用真正完成了关闭动作。
    pub(crate) fn mark_closed(&self) -> bool {
        let mut state = lock(&self.state);
        if state.closed {
            return false;
        }
        state.closed = true;
        true
    }
}

struct Handshake {
    version: u32,
    max_payload: u32,
    banner: String,
}

impl Connection {
    /// 连接 `addr`（通常是 `127.0.0.1:5555`，或由 USB 转发/端口映射得来）并完成握手。
    pub fn connect(
        addr: impl ToSocketAddrs,
        key: &AdbKey,
        options: &ConnectOptions,
    ) -> Result<Self> {
        let addresses: Vec<SocketAddr> = addr.to_socket_addrs()?.collect();
        if addresses.is_empty() {
            return Err(Error::Protocol("没有可用的 TCP 地址".to_string()));
        }

        let mut last_error = None;
        for address in addresses {
            let mut stream = match TcpStream::connect_timeout(&address, options.handshake_timeout) {
                Ok(stream) => stream,
                Err(err) => {
                    last_error = Some(Error::Io(err));
                    continue;
                }
            };
            let _ = stream.set_nodelay(true);
            stream.set_read_timeout(Some(options.handshake_timeout))?;

            let handshake = match handshake(&mut stream, key, options) {
                Ok(handshake) => handshake,
                Err(err) => {
                    last_error = Some(err);
                    continue;
                }
            };

            stream.set_read_timeout(None)?;
            let reader = stream.try_clone()?;
            let shutdown = stream.try_clone()?;
            let peer_addr = stream.peer_addr().ok();
            let local_addr = stream.local_addr().ok();
            let advertised = options.max_payload.clamp(MAX_PAYLOAD_V1, MAX_PAYLOAD);
            let max_payload = handshake.max_payload.min(advertised);

            let inner = Arc::new(ConnInner {
                write: Mutex::new(stream),
                socket: shutdown,
                peer_addr,
                local_addr,
                streams: Mutex::new(HashMap::new()),
                next_local_id: AtomicU32::new(1),
                max_payload,
                peer_version: handshake.version,
                banner: handshake.banner,
                write_timeout: options.write_timeout,
                open_timeout: options.open_timeout,
                closed: AtomicBool::new(false),
                failure: Mutex::new(None),
            });

            let strict_checksum = handshake.version < A_VERSION_SKIP_CHECKSUM;
            spawn_reader(Arc::downgrade(&inner), reader, max_payload, strict_checksum)?;
            return Ok(Self { inner });
        }

        Err(last_error.unwrap_or_else(|| Error::Protocol("无法连接到任何 TCP 地址".to_string())))
    }

    /// 设备在 CNXN 里发来的 banner。
    pub fn banner(&self) -> &str {
        &self.inner.banner
    }

    /// 设备通告的协议版本。
    pub fn peer_version(&self) -> u32 {
        self.inner.peer_version
    }

    /// 协商后的最大 payload。
    pub fn max_payload(&self) -> u32 {
        self.inner.max_payload
    }

    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.inner.peer_addr
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.inner.local_addr
    }

    /// 连接因协议/IO 错误失败时的原因。
    pub fn protocol_error(&self) -> Option<String> {
        lock(&self.inner.failure).as_ref().map(Failure::message)
    }

    /// 打开一个 ADB 服务（payload 会自动补 NUL），例如 `shell:ls`、`sync:`。
    pub fn open_service(&self, service: &str) -> Result<AdbStream> {
        if service.is_empty() {
            return Err(Error::Protocol("服务名不能为空".to_string()));
        }
        if service.contains('\0') {
            return Err(Error::Protocol("服务名不能包含 NUL".to_string()));
        }

        let local_id = self.inner.alloc_local_id()?;
        let slot = Arc::new(StreamSlot::new());
        lock(&self.inner.streams).insert(local_id, slot.clone());

        let payload = service_payload(service);
        if let Err(err) = self.send_packet(Command::OPEN, local_id, 0, &payload) {
            lock(&self.inner.streams).remove(&local_id);
            return Err(err);
        }

        match slot.wait_open(self.inner.open_timeout, service) {
            Ok(_) => Ok(AdbStream::new(
                self.clone(),
                slot,
                local_id,
                service.to_string(),
            )),
            Err(err) => {
                lock(&self.inner.streams).remove(&local_id);
                Err(err)
            }
        }
    }

    /// 主动关闭整个连接：所有流收到 `ConnectionClosed`。
    pub fn close(&self) {
        self.inner
            .fail(Failure::Closed("连接已被主动关闭".to_string()));
    }

    pub(crate) fn send_packet(
        &self,
        command: Command,
        arg0: u32,
        arg1: u32,
        payload: &[u8],
    ) -> Result<()> {
        if self.inner.closed.load(Ordering::SeqCst) {
            let reason = lock(&self.inner.failure)
                .as_ref()
                .map(Failure::message)
                .unwrap_or_else(|| "连接已关闭".to_string());
            return Err(Error::ConnectionClosed(reason));
        }
        if payload.len() > MAX_PAYLOAD as usize {
            return Err(Error::Protocol(format!(
                "发送负载 {} 字节，超过协议上限 {MAX_PAYLOAD}",
                payload.len()
            )));
        }

        let mut header = MessageHeader::new(command, arg0, arg1, payload.len());
        header.data_checksum = checksum(payload);
        let mut buffer = Vec::with_capacity(HEADER_SIZE + payload.len());
        buffer.extend_from_slice(&header.encode());
        buffer.extend_from_slice(payload);

        let mut write = lock(&self.inner.write);
        if let Err(err) = write.write_all(&buffer) {
            drop(write);
            let failure = Failure::Closed(format!("写入设备失败：{err}"));
            self.inner.fail(failure);
            return Err(Error::Io(err));
        }
        Ok(())
    }

    pub(crate) fn unregister(&self, local_id: u32) {
        lock(&self.inner.streams).remove(&local_id);
    }

    pub(crate) fn write_timeout(&self) -> Duration {
        self.inner.write_timeout
    }
}

impl ConnInner {
    fn alloc_local_id(&self) -> Result<u32> {
        for _ in 0..u32::MAX {
            let id = self.next_local_id.fetch_add(1, Ordering::Relaxed);
            if id != 0 {
                return Ok(id);
            }
        }
        Err(Error::Protocol("本地流 ID 已耗尽".to_string()))
    }

    fn slot(&self, local_id: u32) -> Option<Arc<StreamSlot>> {
        lock(&self.streams).get(&local_id).cloned()
    }

    fn fail(&self, failure: Failure) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        *lock(&self.failure) = Some(failure.clone());
        let slots: Vec<Arc<StreamSlot>> = lock(&self.streams).values().cloned().collect();
        lock(&self.streams).clear();
        for slot in slots {
            slot.push(StreamEvent::Failure(failure.clone()));
        }
        let _ = self.socket.shutdown(Shutdown::Both);
    }

    fn dispatch(&self, header: MessageHeader, payload: Vec<u8>) -> Result<()> {
        match header.command {
            Command::OKAY => {
                if let Some(slot) = self.slot(header.arg1) {
                    slot.on_okay(header.arg0);
                }
                Ok(())
            }
            Command::WRTE => {
                let local_id = header.arg1;
                let remote_id = header.arg0;
                match self.slot(local_id) {
                    Some(slot) => {
                        slot.on_remote_id(remote_id);
                        slot.push(StreamEvent::Data(payload));
                        self.send_okay(remote_id, local_id)
                    }
                    None => {
                        // 流已不存在：告诉设备释放它，避免设备侧泄漏。
                        let _ = self.send(Command::CLSE, remote_id, local_id, &[]);
                        Ok(())
                    }
                }
            }
            Command::CLSE => {
                let slot = lock(&self.streams).remove(&header.arg1);
                if let Some(slot) = slot {
                    slot.push(StreamEvent::Close(decode_reason(&payload)));
                }
                Ok(())
            }
            other => Err(Error::Protocol(format!("收到未预期的 ADB 消息 {other}"))),
        }
    }

    fn send_okay(&self, remote_id: u32, local_id: u32) -> Result<()> {
        self.send(Command::OKAY, remote_id, local_id, &[])
    }

    fn send(&self, command: Command, arg0: u32, arg1: u32, payload: &[u8]) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::ConnectionClosed("连接已关闭".to_string()));
        }
        let mut header = MessageHeader::new(command, arg0, arg1, payload.len());
        header.data_checksum = checksum(payload);
        let mut buffer = Vec::with_capacity(HEADER_SIZE + payload.len());
        buffer.extend_from_slice(&header.encode());
        buffer.extend_from_slice(payload);

        let mut write = lock(&self.write);
        write.write_all(&buffer).map_err(Error::Io)
    }
}

fn spawn_reader(
    inner: Weak<ConnInner>,
    mut stream: TcpStream,
    max_payload: u32,
    strict_checksum: bool,
) -> Result<()> {
    thread::Builder::new()
        .name("rarebox-adb-reader".to_string())
        .spawn(move || {
            loop {
                let packet = read_packet(&mut stream, max_payload as usize, strict_checksum);
                // 读取期间不持有强引用：最后一个用户句柄释放时能立刻 Drop 并 shutdown。
                let Some(connection) = inner.upgrade() else {
                    break;
                };
                let outcome: std::result::Result<(), Failure> = match packet {
                    Ok(Some((header, payload))) => {
                        connection.dispatch(header, payload).map_err(Failure::from)
                    }
                    Ok(None) => Err(Failure::Closed("设备连接已断开（EOF）".to_string())),
                    Err(err) => Err(Failure::from(err)),
                };
                drop(connection);
                if let Err(failure) = outcome {
                    if let Some(connection) = inner.upgrade() {
                        connection.fail(failure);
                    }
                    break;
                }
            }
            let _ = stream.shutdown(Shutdown::Both);
        })
        .map(|_| ())
        .map_err(Error::Io)
}

fn handshake(stream: &mut TcpStream, key: &AdbKey, options: &ConnectOptions) -> Result<Handshake> {
    let banner = options.host_banner();
    let advertised = options.max_payload.clamp(MAX_PAYLOAD_V1, MAX_PAYLOAD);
    write_packet(
        stream,
        Command::CNXN,
        A_VERSION_MIN,
        advertised,
        banner.as_bytes(),
    )?;

    let mut signed = false;
    let mut public_key_sent = false;
    let mut tokens = 0u32;

    loop {
        let packet = match read_packet(stream, MAX_PAYLOAD as usize, true) {
            Ok(Some(packet)) => packet,
            Ok(None) => {
                return Err(if public_key_sent {
                    Error::AuthPending
                } else {
                    Error::ConnectionClosed("设备在握手阶段断开了连接".to_string())
                });
            }
            Err(Error::Io(err)) if is_timeout(&err) => {
                return Err(if public_key_sent {
                    Error::AuthPending
                } else {
                    Error::Timeout("握手超时：设备没有回应 CNXN/AUTH".to_string())
                });
            }
            Err(err) => return Err(err),
        };

        let (header, payload) = packet;
        match header.command {
            Command::AUTH => match header.arg0 {
                AUTH_TOKEN => {
                    if payload.len() != TOKEN_SIZE {
                        return Err(Error::Protocol(format!(
                            "AUTH TOKEN 应为 {TOKEN_SIZE} 字节，实际 {}",
                            payload.len()
                        )));
                    }
                    tokens += 1;
                    if tokens > 64 {
                        return Err(Error::AuthFailed {
                            reason: "设备反复拒绝签名与公钥，已放弃".to_string(),
                        });
                    }
                    notify(&options.auth_observer, AuthEvent::TokenReceived);

                    if !signed {
                        let signature = key.sign_token(&payload)?;
                        write_packet(stream, Command::AUTH, AUTH_SIGNATURE, 0, &signature)?;
                        signed = true;
                        notify(&options.auth_observer, AuthEvent::SignatureSent);
                    } else {
                        // 设备再次索要 token，说明签名被拒：转交公钥，等待用户确认。
                        if !public_key_sent {
                            notify(&options.auth_observer, AuthEvent::SignatureRejected);
                        }
                        let public_key = key.user_public_key(&options.identity)?;
                        let mut payload = public_key.into_bytes();
                        payload.push(0);
                        write_packet(stream, Command::AUTH, AUTH_RSAPUBLICKEY, 0, &payload)?;
                        if !public_key_sent {
                            notify(&options.auth_observer, AuthEvent::PublicKeySent);
                        }
                        public_key_sent = true;
                    }
                }
                other => {
                    return Err(Error::Protocol(format!(
                        "设备发来未定义的 AUTH 类型 {other}"
                    )));
                }
            },
            Command::CNXN => {
                let max_payload = header.arg1.clamp(MAX_PAYLOAD_V1, MAX_PAYLOAD);
                let banner = String::from_utf8_lossy(&payload)
                    .trim_end_matches('\0')
                    .to_string();
                return Ok(Handshake {
                    version: header.arg0,
                    max_payload,
                    banner,
                });
            }
            Command::STLS => {
                return Err(Error::Unsupported(
                    "设备要求 ADB over TLS（STLS），当前实现只支持明文 ADB".to_string(),
                ));
            }
            Command::CLSE => {
                let reason = decode_reason(&payload)
                    .unwrap_or_else(|| "设备在握手阶段关闭了连接（未给出原因）".to_string());
                return Err(Error::AuthFailed { reason });
            }
            other => {
                return Err(Error::Protocol(format!("握手阶段收到意外消息 {other}")));
            }
        }
    }
}

fn notify(observer: &Option<AuthObserver>, event: AuthEvent) {
    if let Some(observer) = observer {
        observer(event);
    }
}

fn write_packet(
    stream: &mut TcpStream,
    command: Command,
    arg0: u32,
    arg1: u32,
    payload: &[u8],
) -> Result<()> {
    let mut header = MessageHeader::new(command, arg0, arg1, payload.len());
    header.data_checksum = checksum(payload);
    stream.write_all(&header.encode())?;
    if !payload.is_empty() {
        stream.write_all(payload)?;
    }
    Ok(())
}

fn read_packet(
    stream: &mut TcpStream,
    max_payload: usize,
    strict_checksum: bool,
) -> Result<Option<(MessageHeader, Vec<u8>)>> {
    let mut raw = [0u8; HEADER_SIZE];
    loop {
        match stream.read(&mut raw[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(Error::Io(err)),
        }
    }
    stream.read_exact(&mut raw[1..])?;

    let header = MessageHeader::decode(&raw)?;
    let len = header.data_length as usize;
    if len > max_payload {
        return Err(Error::Protocol(format!(
            "收到的数据包长度 {len} 超过协商上限 {max_payload}"
        )));
    }
    let mut payload = vec![0u8; len];
    if len > 0 {
        stream.read_exact(&mut payload)?;
    }
    if strict_checksum {
        let expected = checksum(&payload);
        if header.data_checksum != expected {
            return Err(Error::Checksum {
                expected,
                actual: header.data_checksum,
            });
        }
    }
    Ok(Some((header, payload)))
}

fn decode_reason(payload: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(payload);
    let text = text.trim_end_matches('\0').trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}
