//! 远程控制：内置 scrcpy-server 的启动、实时画面与触控注入。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::engine::{Core, Ctx, Message};
use crate::event::{Event, VideoFrame};
use crate::scrcpy::ScrcpySession;
use crate::state::{RemotePhase, ToastKind};

pub const KEYCODE_BACK: u32 = 4;
pub const KEYCODE_HOME: u32 = 3;
pub const KEYCODE_APP_SWITCH: u32 = 187;
pub const KEYCODE_POWER: u32 = 26;

impl Core {
    pub(crate) fn start_remote(&mut self, cx: &mut Ctx) {
        if self.state.remote_phase == RemotePhase::Starting || self.remote.is_some() {
            return;
        }
        let (adb, serial) = match self.require_device() {
            Ok(pair) => pair,
            Err(err) => {
                self.state.remote_phase = RemotePhase::Failed;
                self.state.remote_error = Some(format!("{err:#}"));
                cx.toast(ToastKind::Error, format!("无法启动远程控制：{err:#}"));
                return;
            }
        };

        let adb_path = adb.program().to_path_buf();
        self.state.remote_generation = self.state.remote_generation.wrapping_add(1);
        let generation = self.state.remote_generation;
        self.state.remote_phase = RemotePhase::Starting;
        self.state.remote_error = None;
        self.state.remote_size = (0, 0);

        self.run_task(
            cx,
            "正在启动 scrcpy 远程控制…",
            move || ScrcpySession::start(adb_path, serial, 1024),
            move |core, result, cx| {
                if core.state.remote_generation != generation {
                    if let Ok(mut session) = result {
                        std::thread::spawn(move || session.stop());
                    }
                    return;
                }
                match result {
                    Ok(session) => {
                        let session = Arc::new(Mutex::new(session));
                        core.state.remote_size = {
                            let guard = session.lock().unwrap_or_else(|p| p.into_inner());
                            (guard.width(), guard.height())
                        };
                        core.remote = Some(session.clone());
                        core.state.remote_phase = RemotePhase::Running;
                        core.state.remote_error = None;
                        core.spawn_remote_poller(session, generation, cx.done.clone());
                        cx.toast(ToastKind::Success, "远程控制已连接");
                    }
                    Err(err) => {
                        core.state.remote_phase = RemotePhase::Failed;
                        core.state.remote_error = Some(format!("{err:#}"));
                        cx.toast(ToastKind::Error, format!("启动远程控制失败：{err:#}"));
                    }
                }
            },
        );
    }

    pub(crate) fn stop_remote(&mut self, cx: &mut Ctx) {
        self.state.remote_generation = self.state.remote_generation.wrapping_add(1);
        if let Some(stop) = self.remote_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(session) = self.remote.take() {
            std::thread::spawn(move || {
                if let Ok(mut session) = session.lock() {
                    session.stop();
                }
            });
        }
        self.state.remote_phase = RemotePhase::Idle;
        self.state.remote_error = None;
        cx.notify();
    }

    pub(crate) fn restart_remote(&mut self, cx: &mut Ctx) {
        self.stop_remote(cx);
        let done = cx.done.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(450));
            let _ = done.send_blocking(Message::Done(Box::new(|core, cx| {
                core.start_remote(cx);
            })));
        });
    }

    fn spawn_remote_poller(
        &mut self,
        session: Arc<Mutex<ScrcpySession>>,
        generation: u64,
        done_tx: async_channel::Sender<Message>,
    ) {
        let stop = Arc::new(AtomicBool::new(false));
        self.remote_stop = Some(stop.clone());
        let events = self.events.clone();
        std::thread::Builder::new()
            .name("rarebox-scrcpy".into())
            .spawn(move || {
                let mut last_size = (0u16, 0u16);
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(8));
                    let (frame, ended, size) = {
                        let Ok(guard) = session.lock() else {
                            return;
                        };
                        let frame = guard.take_frame();
                        let ended = guard.poll_end();
                        let size = (guard.width(), guard.height());
                        (frame, ended, size)
                    };
                    if size != last_size && size.0 > 0 && size.1 > 0 {
                        last_size = size;
                        let _ = done_tx.send_blocking(Message::Done(Box::new(move |core, _cx| {
                            core.state.remote_size = size;
                        })));
                    }
                    if let Some(frame) = frame {
                        if let Some(video) =
                            build_video_frame(generation, frame.width, frame.height, frame.bgra)
                        {
                            let _ = events.try_send(Event::Video(video));
                        }
                    }
                    if let Some(reason) = ended {
                        let _ = events.try_send(Event::VideoEnded {
                            generation,
                            reason: reason.clone(),
                        });
                        let _ = done_tx.send_blocking(Message::Done(Box::new(move |core, cx| {
                            if core.state.remote_generation == generation {
                                core.remote_failure(reason, cx);
                            }
                        })));
                        return;
                    }
                }
            })
            .ok();
    }

    pub(crate) fn remote_failure(&mut self, reason: String, cx: &mut Ctx) {
        self.stop_remote(cx);
        self.state.remote_phase = RemotePhase::Failed;
        self.state.remote_error = Some(reason.clone());
        cx.toast(ToastKind::Error, format!("远程控制已断开：{reason}"));
    }

    pub(crate) fn remote_pointer_down(&mut self, x: u32, y: u32, cx: &mut Ctx) {
        self.remote_touch(RemoteTouch::Down, x, y, cx);
    }

    pub(crate) fn remote_pointer_move(&mut self, x: u32, y: u32, cx: &mut Ctx) {
        self.remote_touch(RemoteTouch::Move, x, y, cx);
    }

    pub(crate) fn remote_pointer_up(&mut self, x: u32, y: u32, cx: &mut Ctx) {
        self.remote_touch(RemoteTouch::Up, x, y, cx);
    }

    fn remote_touch(&mut self, kind: RemoteTouch, x: u32, y: u32, cx: &mut Ctx) {
        let Some(session) = self.remote.clone() else {
            return;
        };
        let result = session
            .lock()
            .map_err(|_| anyhow::anyhow!("远程会话已关闭"))
            .and_then(|session| match kind {
                RemoteTouch::Down => {
                    crate::scrcpy::trace_input("pointer_down");
                    session.send_touch_down(x, y)
                }
                RemoteTouch::Move => session.send_touch_move(x, y),
                RemoteTouch::Up => {
                    crate::scrcpy::trace_input("pointer_up");
                    session.send_touch_up(x, y)
                }
            });
        if let Err(err) = result {
            self.remote_failure(format!("发送触控失败：{err:#}"), cx);
        }
    }

    pub(crate) fn send_remote_key(&mut self, keycode: u32, cx: &mut Ctx) {
        let Some(session) = self.remote.clone() else {
            return;
        };
        crate::scrcpy::trace_input("key");
        let result = session
            .lock()
            .map_err(|_| anyhow::anyhow!("远程会话已关闭"))
            .and_then(|session| session.send_key(keycode));
        if let Err(err) = result {
            self.remote_failure(format!("发送按键失败：{err:#}"), cx);
        }
    }

    pub(crate) fn send_remote_text(&mut self, text: String, cx: &mut Ctx) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(session) = self.remote.clone() else {
            cx.toast(ToastKind::Info, "请先启动远程控制");
            return;
        };
        crate::scrcpy::trace_input("text");
        let result = session
            .lock()
            .map_err(|_| anyhow::anyhow!("远程会话已关闭"))
            .and_then(|session| session.send_text(&text));
        if let Err(err) = result {
            self.remote_failure(format!("发送文字失败：{err:#}"), cx);
        }
    }
}

enum RemoteTouch {
    Down,
    Move,
    Up,
}

fn build_video_frame(
    generation: u64,
    width: u32,
    height: u32,
    bgra: Vec<u8>,
) -> Option<VideoFrame> {
    if width == 0 || height == 0 {
        return None;
    }
    let expected = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if bgra.len() != expected {
        return None;
    }
    Some(VideoFrame {
        generation,
        width,
        height,
        bgra,
    })
}
