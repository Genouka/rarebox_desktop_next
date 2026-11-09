//! 无界面引擎：状态、命令队列与事件通道。
//!
//! 所有会被多个前端共享的操作都实现为 `Core` 上的方法。`Engine` 是给宿主
//! （桌面 UI、JNI、C ABI）的句柄：
//!
//! - `Engine::command` 投递命令，立即返回；
//! - `Engine::events` 订阅状态变更、提示、文件选择等事件；
//! - `Engine::state` 读取最近一次发布的可克隆状态快照。
//!
//! 命令在单一后台线程串行执行，副作用（ADB / hdc / 网络 / 文件）在额外线程
//! 里完成，完成后回到串行线程提交状态，因此宿主的渲染线程永远不会被 IO 阻塞。

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use async_channel::{Receiver, Sender};

use crate::command::Command;
use crate::event::{DirectoryPurpose, Event, PickPurpose};
use crate::scrcpy::ScrcpySession;
use crate::signing::FlowEvent;
use crate::state::{ConfirmKind, ConfirmRequest, CoreState, ToastKind};

/// 远程会话的停止标记。
pub(crate) type StopFlag = Arc<AtomicBool>;

/// 串行线程接收的内部消息。
pub(crate) enum Message {
    Command(Command),
    /// 后台任务完成后的状态提交。
    Done(Box<dyn FnOnce(&mut Core, &mut Ctx) + Send>),
    /// 签名流水线的进度事件与最终结果。
    Signing(SigningMessage),
}

pub(crate) enum SigningMessage {
    Event {
        generation: u64,
        event: FlowEvent,
    },
    Done {
        generation: u64,
        service: crate::signing::SigningService,
        result: anyhow::Result<crate::signing::FlowOutcome>,
    },
}

/// 命令执行上下文。所有方法都只投递消息 / 事件，不直接触碰宿主 UI。
pub struct Ctx {
    pub(crate) events: Sender<Event>,
    pub(crate) done: Sender<Message>,
}

impl Ctx {
    /// 状态变更由串行线程在命令结束后统一发布，这里保留空的兼容入口。
    pub fn notify(&mut self) {}

    pub fn emit(&self, event: Event) {
        let _ = self.events.try_send(event);
    }

    pub fn toast(&self, kind: ToastKind, message: impl Into<String>) {
        self.emit(Event::Toast {
            kind,
            message: message.into(),
        });
    }

    pub fn confirm(&self, request: ConfirmRequest) {
        self.emit(Event::Confirm(request));
    }

    pub fn confirm_kind(
        &self,
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
        kind: ConfirmKind,
    ) {
        self.confirm(ConfirmRequest {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            kind,
        });
    }

    pub fn pick_files(&self, purpose: PickPurpose) {
        self.emit(Event::PickFiles(purpose));
    }

    pub fn pick_directory(&self, purpose: DirectoryPurpose) {
        self.emit(Event::PickDirectory(purpose));
    }

    pub fn open_url(&self, url: impl Into<String>) {
        self.emit(Event::OpenUrl(url.into()));
    }

    pub fn quit(&self) {
        self.emit(Event::Quit);
    }
}

/// 串行线程持有的引擎主体。业务方法在 `logic` 下按页面分模块实现。
pub struct Core {
    pub state: CoreState,
    pub(crate) remote: Option<Arc<std::sync::Mutex<ScrcpySession>>>,
    pub(crate) remote_stop: Option<StopFlag>,
    pub(crate) latest: Arc<RwLock<Arc<CoreState>>>,
    pub(crate) events: Sender<Event>,
}

impl Core {
    /// 后台任务：在工作线程执行阻塞操作，完成后回到串行线程提交状态。
    pub fn run_task<T, F, D>(&mut self, cx: &mut Ctx, label: impl Into<String>, work: F, done: D)
    where
        T: Send + 'static,
        F: FnOnce() -> anyhow::Result<T> + Send + 'static,
        D: FnOnce(&mut Self, anyhow::Result<T>, &mut Ctx) + Send + 'static,
    {
        self.state.activity = Some(label.into());
        let done_tx = cx.done.clone();
        std::thread::Builder::new()
            .name("rarebox-job".into())
            .spawn(move || {
                let result = work();
                let _ = done_tx.send_blocking(Message::Done(Box::new(move |core, cx| {
                    core.state.activity = None;
                    done(core, result, cx);
                })));
            })
            .expect("spawn rarebox job");
        cx.notify();
    }

    pub fn toast(&mut self, cx: &mut Ctx, kind: ToastKind, message: impl Into<String>) {
        cx.toast(kind, message);
    }

    pub fn save_config(&mut self) {
        let config = self.state.config.clone();
        std::thread::spawn(move || config.save());
    }

    fn publish(&self) {
        let snapshot = Arc::new(self.state.clone());
        if let Ok(mut latest) = self.latest.write() {
            *latest = snapshot;
        }
        let _ = self.events.try_send(Event::Changed);
    }
}

/// 宿主持有的引擎句柄。克隆后可在任意线程投递命令。
#[derive(Clone)]
pub struct Engine {
    tx: Sender<Message>,
    events: Receiver<Event>,
    latest: Arc<RwLock<Arc<CoreState>>>,
}

impl Engine {
    /// 创建引擎并在后台开始探测设备。返回后即可投递命令。
    pub fn new(config: crate::config::Config) -> Self {
        let (event_tx, event_rx) = async_channel::unbounded();
        let (tx, rx) = async_channel::unbounded();

        let state = CoreState {
            adb_input: config.adb_path.clone(),
            hdc_input: config.hdc_path.clone(),
            screenshot_input: config.screenshot_dir.clone().unwrap_or_default(),
            config,
            ..CoreState::default()
        };

        let latest = Arc::new(RwLock::new(Arc::new(state.clone())));
        let core = Core {
            state,
            remote: None,
            remote_stop: None,
            latest: latest.clone(),
            events: event_tx.clone(),
        };

        let actor_tx = tx.clone();
        std::thread::Builder::new()
            .name("rarebox-core".into())
            .spawn(move || {
                let mut core = core;
                let mut ctx = Ctx {
                    events: event_tx,
                    done: actor_tx,
                };
                core.bootstrap(&mut ctx);
                core.publish();
                while let Ok(message) = rx.recv_blocking() {
                    match message {
                        Message::Command(command) => core.dispatch(command, &mut ctx),
                        Message::Done(done) => done(&mut core, &mut ctx),
                        Message::Signing(message) => {
                            core.apply_signing_message(message, &mut ctx)
                        }
                    }
                    core.publish();
                }
            })
            .expect("spawn rarebox core");

        Self {
            tx,
            events: event_rx,
            latest,
        }
    }

    /// 投递命令；引擎已关闭时返回错误。
    pub fn command(&self, command: Command) -> anyhow::Result<()> {
        self.tx
            .try_send(Message::Command(command))
            .map_err(|_| anyhow::anyhow!("engine stopped"))
    }

    /// 订阅事件。同一引擎建议只保留一个消费者。
    pub fn events(&self) -> Receiver<Event> {
        self.events.clone()
    }

    /// 最近一次发布的状态快照。
    pub fn state(&self) -> Arc<CoreState> {
        self.latest
            .read()
            .map(|state| state.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }
}
