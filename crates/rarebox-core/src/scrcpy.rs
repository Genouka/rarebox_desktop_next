//! 远程控制后端：启动官方 scrcpy-server，在应用内解码 H.264 画面并通过控制通道注入触控。
//!
//! 采集与编码由 Genymobile/scrcpy 的 server 完成；本模块只负责启动会话、
//! 读取 scrcpy 视频协议、解码为 BGRA 帧，以及发送控制消息。

use std::io::Read;
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use fadb_scrcpy::decoder::VideoDecoder;
use fadb_scrcpy::protocol::{CodecMetadata, H264_CODEC_ID, PacketHeader};
use scrcpy_protocol::ControlSender;

use crate::config;

/// scrcpy server 在设备上的固定位置。
const SERVER_REMOTE_PATH: &str = "/data/local/tmp/scrcpy-server.jar";
/// 单帧最大负载，防止异常数据导致超大分配。
const MAX_PACKET_SIZE: usize = 16 * 1024 * 1024;

/// 一帧已解码并转换为 BGRA 的画面。
pub struct RemoteFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    /// 解码完成时间，用于诊断显示链路延迟。
    pub decoded_at: Instant,
}

/// 后台读取线程发回的事件。
pub enum SessionEvent {
    /// 视频流结束或读取失败。
    Ended(String),
}

/// 一次正在运行的 scrcpy 会话。
pub struct ScrcpySession {
    adb: PathBuf,
    serial: String,
    port: u16,
    width: u16,
    height: u16,
    control: Option<ControlSender>,
    server: Option<Child>,
    video_shutdown: Option<TcpStream>,
    frames: Arc<Mutex<Option<RemoteFrame>>>,
    events: Receiver<SessionEvent>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl ScrcpySession {
    /// 连接设备、上传 server 并启动投屏。
    pub fn start(adb: PathBuf, serial: String, max_size: u32, bitrate: u32) -> Result<Self> {
        let scid = random_scid();
        let socket_name = format!("scrcpy_{scid:08x}");
        let local_jar = write_server_jar()?;
        debug_log(format_args!(
            "prepared server jar at {}",
            local_jar.display()
        ));

        let push = adb_command(&adb, &serial)
            .args([
                "push",
                local_jar.to_string_lossy().as_ref(),
                SERVER_REMOTE_PATH,
            ])
            .output()
            .context("无法执行 adb push 上传 scrcpy-server")?;
        if !push.status.success() {
            bail!(
                "上传 scrcpy-server 失败：{}",
                String::from_utf8_lossy(&push.stderr).trim()
            );
        }

        let forward = adb_command(&adb, &serial)
            .args(["forward", "tcp:0", &format!("localabstract:{socket_name}")])
            .output()
            .context("无法创建 scrcpy 端口转发")?;
        if !forward.status.success() {
            bail!(
                "创建 scrcpy 端口转发失败：{}",
                String::from_utf8_lossy(&forward.stderr).trim()
            );
        }
        let port: u16 = String::from_utf8_lossy(&forward.stdout)
            .trim()
            .parse()
            .context("adb forward 未返回有效端口")?;
        debug_log(format_args!("adb forward ready on tcp:{port}"));

        let server_args = format!(
            "CLASSPATH={SERVER_REMOTE_PATH} app_process / com.genymobile.scrcpy.Server {} \
             log_level=warn scid={scid:08x} tunnel_forward=true audio=false control=true \
             video_codec=h264 send_device_meta=false max_size={max_size} video_bit_rate={bitrate}",
            fadb_scrcpy::server::SCRCPY_VERSION
        );
        let mut server = adb_command(&adb, &serial);
        server
            .arg("shell")
            .args(server_args.split_whitespace())
            .stdin(Stdio::null())
            .stdout(if debug_enabled() {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .stderr(if debug_enabled() {
                Stdio::inherit()
            } else {
                Stdio::null()
            });
        let mut server = match server.spawn() {
            Ok(child) => child,
            Err(err) => {
                let _ = remove_forward(&adb, &serial, port);
                return Err(err).context("无法启动 scrcpy-server");
            }
        };
        debug_log(format_args!("scrcpy-server spawned as pid {}", server.id()));

        let started = Instant::now();
        let deadline = started + Duration::from_secs(20);
        let mut attempt = 0u32;
        let mut video = loop {
            attempt += 1;
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(mut stream) => {
                    let _ = stream.set_nodelay(true);
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(2500)));
                    match read_dummy(&mut stream) {
                        Ok(()) => {
                            debug_log(format_args!("video dummy received on attempt {attempt}"));
                            break stream;
                        }
                        Err(err) if Instant::now() < deadline => {
                            debug_log(format_args!("video attempt {attempt} not ready: {err:#}"));
                            thread::sleep(Duration::from_millis(80));
                        }
                        Err(err) => {
                            abort_start(&mut server, &adb, &serial, port);
                            return Err(err).context("读取 scrcpy 握手字节超时");
                        }
                    }
                }
                Err(_) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(120));
                }
                Err(err) => {
                    abort_start(&mut server, &adb, &serial, port);
                    return Err(err).context("连接 scrcpy 视频流超时");
                }
            }
        };

        // scrcpy-server 会依次接受视频和控制两个 socket，全部连接完成后才发送
        // 视频元数据；因此必须先连上控制通道，再读取编解码信息。
        let control = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(120));
                }
                Err(err) => {
                    abort_start(&mut server, &adb, &serial, port);
                    return Err(err).context("连接 scrcpy 控制通道超时");
                }
            }
        };
        let _ = control.set_nodelay(true);
        debug_log(format_args!("control channel connected"));

        let _ = video.set_read_timeout(Some(Duration::from_secs(5)));
        let metadata = match read_metadata(&mut video) {
            Ok(metadata) => metadata,
            Err(err) => {
                abort_start(&mut server, &adb, &serial, port);
                return Err(err).context("读取 scrcpy 视频元数据失败");
            }
        };
        debug_log(format_args!(
            "codec metadata: {}x{}",
            metadata.width, metadata.height
        ));
        let _ = video.set_read_timeout(None);
        let width = u16::try_from(metadata.width).unwrap_or(u16::MAX);
        let height = u16::try_from(metadata.height).unwrap_or(u16::MAX);
        let control = ControlSender::new(control, width, height);

        let frames = Arc::new(Mutex::new(None));
        let (event_tx, events) = sync_channel(4);
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = video.try_clone().ok();
        let reader = spawn_reader(video, stop.clone(), frames.clone(), event_tx);

        Ok(Self {
            adb,
            serial,
            port,
            width,
            height,
            control: Some(control),
            server: Some(server),
            video_shutdown: shutdown,
            frames,
            events,
            stop,
            reader,
        })
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// 取走当前可用的最新一帧；未取走的旧帧会被新帧直接覆盖以保持低延迟。
    pub fn take_frame(&self) -> Option<RemoteFrame> {
        self.frames.lock().ok()?.take()
    }

    /// 检查后台线程是否已结束，返回结束原因。
    pub fn poll_end(&self) -> Option<String> {
        match self.events.try_recv() {
            Ok(SessionEvent::Ended(reason)) => Some(reason),
            Err(_) => None,
        }
    }

    pub fn send_touch_down(&self, x: u32, y: u32) -> Result<()> {
        if let Some(control) = &self.control {
            control.send_touch_down(
                x.min(self.width.saturating_sub(1) as u32),
                y.min(self.height.saturating_sub(1) as u32),
            )?;
        }
        Ok(())
    }

    pub fn send_touch_move(&self, x: u32, y: u32) -> Result<()> {
        if let Some(control) = &self.control {
            control.send_touch_move(
                x.min(self.width.saturating_sub(1) as u32),
                y.min(self.height.saturating_sub(1) as u32),
            )?;
        }
        Ok(())
    }

    pub fn send_touch_up(&self, x: u32, y: u32) -> Result<()> {
        if let Some(control) = &self.control {
            control.send_touch_up(
                x.min(self.width.saturating_sub(1) as u32),
                y.min(self.height.saturating_sub(1) as u32),
            )?;
        }
        Ok(())
    }

    pub fn send_key(&self, keycode: u32) -> Result<()> {
        if let Some(control) = &self.control {
            control.send_key_press(keycode, 0)?;
        }
        Ok(())
    }

    pub fn send_text(&self, text: &str) -> Result<()> {
        if let Some(control) = &self.control {
            control.send_text(text)?;
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        if self.server.is_none()
            && self.reader.is_none()
            && self.control.is_none()
            && self.video_shutdown.is_none()
        {
            return;
        }
        self.stop.store(true, Ordering::Relaxed);
        if let Some(stream) = self.video_shutdown.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.control.take();
        if let Some(mut server) = self.server.take() {
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = remove_forward(&self.adb, &self.serial, self.port);
        let _ = adb_command(&self.adb, &self.serial)
            .args(["shell", "pkill", "-f", "scrcpy-server.jar"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }

    /// 视频尺寸变化（例如设备旋转）后，同步控制通道的坐标基准。
    pub fn update_screen_size(&self, width: u16, height: u16) {
        if let Some(control) = &self.control {
            control.update_screen_size(width, height);
        }
    }
}

impl Drop for ScrcpySession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn_reader(
    mut video: TcpStream,
    stop: Arc<AtomicBool>,
    latest_frame: Arc<Mutex<Option<RemoteFrame>>>,
    event_tx: SyncSender<SessionEvent>,
) -> Option<JoinHandle<()>> {
    thread::Builder::new()
        .name("rarebox-scrcpy-reader".into())
        .spawn(move || {
            let mut decoder = match VideoDecoder::new() {
                Ok(decoder) => decoder,
                Err(err) => {
                    let _ = event_tx.try_send(SessionEvent::Ended(format!(
                        "H.264 解码器初始化失败：{err}"
                    )));
                    return;
                }
            };
            let mut header = [0u8; 12];
            let mut reason = "scrcpy 视频流已结束".to_string();
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Err(err) = video.read_exact(&mut header) {
                    if !stop.load(Ordering::Relaxed) {
                        reason = format!("读取视频流失败：{err}");
                    }
                    break;
                }
                let Some(packet) = PacketHeader::parse(&header) else {
                    reason = "scrcpy 数据包头部无效".to_string();
                    break;
                };
                let size = packet.size as usize;
                if size == 0 || size > MAX_PACKET_SIZE {
                    reason = format!("scrcpy 数据包大小异常：{size}");
                    break;
                }
                let mut payload = vec![0u8; size];
                if let Err(err) = video.read_exact(&mut payload) {
                    if !stop.load(Ordering::Relaxed) {
                        reason = format!("读取视频帧失败：{err}");
                    }
                    break;
                }
                match decoder.decode(&payload) {
                    Ok(Some(frame)) => {
                        let mut bgra = frame.rgba;
                        for pixel in bgra.as_chunks_mut::<4>().0 {
                            pixel.swap(0, 2);
                        }
                        if let Ok(mut slot) = latest_frame.lock() {
                            *slot = Some(RemoteFrame {
                                width: frame.width as u32,
                                height: frame.height as u32,
                                bgra,
                                decoded_at: Instant::now(),
                            });
                        }
                    }
                    Ok(None) => {}
                    Err(err) => {
                        reason = format!("H.264 解码失败：{err}");
                        break;
                    }
                }
            }
            let _ = event_tx.try_send(SessionEvent::Ended(reason));
        })
        .ok()
}

fn debug_enabled() -> bool {
    std::env::var_os("RAREBOX_SCRCPY_DEBUG").is_some()
}

pub(crate) fn debug_log(args: std::fmt::Arguments<'_>) {
    if debug_enabled() {
        eprintln!("[scrcpy] {args}");
    }
}

/// 记录输入事件的墙钟时间，便于和外部自动化脚本对齐。
pub(crate) fn trace_input(label: &str) {
    if debug_enabled() {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        eprintln!("[input] {label} {millis}");
    }
}

fn read_dummy(stream: &mut TcpStream) -> Result<()> {
    let mut dummy = [0u8; 1];
    stream
        .read_exact(&mut dummy)
        .context("读取 scrcpy 握手字节失败")?;
    Ok(())
}

fn read_metadata(stream: &mut TcpStream) -> Result<CodecMetadata> {
    let mut bytes = [0u8; 12];
    stream
        .read_exact(&mut bytes)
        .context("读取 scrcpy 视频元数据失败")?;
    let metadata = CodecMetadata::parse(&bytes).context("scrcpy 视频元数据格式无效")?;
    if metadata.codec_id != H264_CODEC_ID {
        bail!("scrcpy 未提供 H.264 视频流");
    }
    Ok(metadata)
}

fn abort_start(server: &mut Child, adb: &Path, serial: &str, port: u16) {
    let _ = server.kill();
    let _ = server.wait();
    let _ = remove_forward(adb, serial, port);
    let _ = adb_command(adb, serial)
        .args(["shell", "pkill", "-f", "scrcpy-server.jar"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn write_server_jar() -> Result<PathBuf> {
    let dir = config::data_dir();
    std::fs::create_dir_all(&dir).context("无法创建应用数据目录")?;
    let path = dir.join(format!(
        "scrcpy-server-{}.jar",
        fadb_scrcpy::server::SCRCPY_VERSION
    ));
    let jar = fadb_scrcpy::server::SERVER_JAR;
    let needs_write = std::fs::metadata(&path)
        .map(|meta| meta.len() != jar.len() as u64)
        .unwrap_or(true);
    if needs_write {
        std::fs::write(&path, jar).context("无法写入内置 scrcpy-server")?;
    }
    Ok(path)
}

fn remove_forward(adb: &Path, serial: &str, port: u16) -> Result<()> {
    let output = adb_command(adb, serial)
        .args(["forward", "--remove", &format!("tcp:{port}")])
        .output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

fn adb_command(adb: &Path, serial: &str) -> Command {
    let mut command = Command::new(adb);
    command.arg("-s").arg(serial);
    crate::adb::prepare_command(&mut command);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

fn random_scid() -> u32 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    (nanos ^ pid.rotate_left(13)) & 0x7fff_ffff
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手动联调：设置 RAREBOX_ADB 与 RAREBOX_SERIAL 后运行
    /// `cargo test live_scrcpy_probe -- --ignored --nocapture`。
    #[test]
    #[ignore = "requires a connected Android device"]
    fn live_scrcpy_probe() {
        let adb = std::env::var_os("RAREBOX_ADB")
            .map(PathBuf::from)
            .expect("RAREBOX_ADB");
        let serial = std::env::var("RAREBOX_SERIAL").expect("RAREBOX_SERIAL");
        let mut session =
            ScrcpySession::start(adb, serial, 1024, 8_000_000).expect("start scrcpy");
        println!("session started: {}x{}", session.width(), session.height());
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Some(frame) = session.take_frame() {
                println!(
                    "frame: {}x{} ({} bytes)",
                    frame.width,
                    frame.height,
                    frame.bgra.len()
                );
                session.send_touch_down(10, 10).expect("touch down");
                session.send_touch_up(10, 10).expect("touch up");
                session.send_key(4).expect("back key");
                session.send_text("rarebox").expect("text injection");
                session.stop();
                return;
            }
            if let Some(reason) = session.poll_end() {
                panic!("session ended before first frame: {reason}");
            }
            thread::sleep(Duration::from_millis(50));
        }
        session.stop();
        panic!("no frame received within 15 seconds");
    }
}
