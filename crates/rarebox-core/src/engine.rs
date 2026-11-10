//! 无界面引擎：状态、命令队列与事件通道。
//!
//! 所有会被多个前端共享的操作都实现为 `Core` 上的方法。`Engine` 是给宿主
//! （桌面 UI、JNI、C ABI）的句柄：
//!
//! - `Engine::command` 投递命令，立即返回；
//! - `Engine::events` 订阅状态变更、提示、文件选择等事件；
//! - `Engine::state` 读取最近一次发布的可克隆状态快照；
//! - `Engine::shutdown` 请求串行线程退出，`Engine::wait_closed` / `is_closed`
//!   观察退出（幂等，克隆句柄共享同一份关闭状态）。
//!
//! 命令在单一后台线程串行执行，副作用（ADB / hdc / 网络 / 文件）在额外线程
//! 里完成，完成后回到串行线程提交状态，因此宿主的渲染线程永远不会被 IO 阻塞。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock};

use async_channel::{Receiver, Sender};

use crate::command::Command;
use crate::event::{DirectoryPurpose, Event, PickPurpose};
use crate::scrcpy::ScrcpySession;
use crate::signing::FlowEvent;
use crate::state::{ConfirmKind, ConfirmRequest, CoreState, ToastKind};

/// 远程会话的停止标记。
pub(crate) type StopFlag = Arc<AtomicBool>;

/// 当前存活的核心线程数。
static LIVE_CORE_THREADS: AtomicUsize = AtomicUsize::new(0);

/// 当前存活的核心线程数量（`Engine::new` 启动的串行线程）。
///
/// 供绑定层（JNI / C ABI / Node-API）的泄漏回归测试断言“create / dispose 若干
/// 轮后线程数不增长”。核心线程退出时（包括 panic 展开）保证计数已回落。
pub fn live_core_threads() -> usize {
    LIVE_CORE_THREADS.load(Ordering::SeqCst)
}

/// 引擎生命周期：关闭请求闸门 + 核心线程退出信号。
///
/// 所有字段都通过 `&self` 操作，`Engine` 的克隆共享同一份状态，因此关闭语义
/// 对克隆后的句柄同样成立，并且天然幂等。
struct Lifecycle {
    /// `shutdown` 已调用；保证只投递一次关闭消息。
    requested: AtomicBool,
    /// 核心线程已退出（正常结束或 panic 展开）。
    closed: AtomicBool,
    wait: Mutex<()>,
    signal: Condvar,
}

impl Lifecycle {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            wait: Mutex::new(()),
            signal: Condvar::new(),
        }
    }

    /// 首次请求关闭时返回 `true`。
    fn request(&self) -> bool {
        !self.requested.swap(true, Ordering::SeqCst)
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// 核心线程退出时调用。先递减存活计数再置位，保证 `wait_closed` 返回时
    /// 线程计数一定已经回落。
    fn mark_closed(&self) {
        LIVE_CORE_THREADS.fetch_sub(1, Ordering::SeqCst);
        {
            let _guard = lock(&self.wait);
            self.closed.store(true, Ordering::SeqCst);
        }
        self.signal.notify_all();
    }

    fn wait_closed(&self) {
        let mut guard = lock(&self.wait);
        while !self.closed.load(Ordering::SeqCst) {
            guard = match self.signal.wait(guard) {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// RAII：核心线程退出时递减存活计数并广播退出信号（panic 展开也会执行）。
struct CoreThreadExit(Arc<Lifecycle>);

impl Drop for CoreThreadExit {
    fn drop(&mut self) {
        self.0.mark_closed();
    }
}

/// 串行线程接收的内部消息。
pub(crate) enum Message {
    Command(Command),
    /// 后台任务完成后的状态提交。
    Done(Box<dyn FnOnce(&mut Core, &mut Ctx) + Send>),
    /// 签名流水线的进度事件与最终结果。
    Signing(SigningMessage),
    /// 关闭请求：让串行线程退出。`Command` 的 serde 面无此变体。
    Shutdown,
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
    lifecycle: Arc<Lifecycle>,
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

        let lifecycle = Arc::new(Lifecycle::new());
        let actor_tx = tx.clone();
        let thread_lifecycle = lifecycle.clone();
        LIVE_CORE_THREADS.fetch_add(1, Ordering::SeqCst);
        let spawned = std::thread::Builder::new()
            .name("rarebox-core".into())
            .spawn(move || {
                // 无论正常退出还是 panic 展开都会递减计数并广播退出信号。
                let _exit = CoreThreadExit(thread_lifecycle);
                let mut core = core;
                let mut ctx = Ctx {
                    events: event_tx,
                    done: actor_tx,
                };
                core.bootstrap(&mut ctx);
                core.publish();
                while let Ok(message) = rx.recv_blocking() {
                    match message {
                        Message::Shutdown => {
                            // 顺手停掉远程会话轮询线程，避免关闭后残留。
                            core.stop_remote(&mut ctx);
                            break;
                        }
                        Message::Command(command) => core.dispatch(command, &mut ctx),
                        Message::Done(done) => done(&mut core, &mut ctx),
                        Message::Signing(message) => core.apply_signing_message(message, &mut ctx),
                    }
                    core.publish();
                }
            });
        if spawned.is_err() {
            LIVE_CORE_THREADS.fetch_sub(1, Ordering::SeqCst);
        }
        spawned.expect("spawn rarebox core");

        Self {
            tx,
            events: event_rx,
            latest,
            lifecycle,
        }
    }

    /// 投递命令；引擎已关闭（或已请求关闭）时立即返回错误，不阻塞、不 panic。
    pub fn command(&self, command: Command) -> anyhow::Result<()> {
        if self.lifecycle.is_requested() {
            return Err(anyhow::anyhow!("engine stopped"));
        }
        self.tx
            .try_send(Message::Command(command))
            .map_err(|_| anyhow::anyhow!("engine stopped"))
    }

    /// 请求关闭核心线程；不阻塞，可重复调用（幂等）。
    ///
    /// 关闭消息排在已投递的消息之后，因此已排队的命令仍会执行完；请求一旦
    /// 发出，`command` 立即返回 "engine stopped"。核心线程退出由
    /// [`Engine::wait_closed`] 观察。
    pub fn shutdown(&self) {
        if self.lifecycle.request() {
            let _ = self.tx.try_send(Message::Shutdown);
        }
    }

    /// 等待核心线程退出；可重复调用。未调用过 [`Engine::shutdown`] 时永远等待。
    pub fn wait_closed(&self) {
        self.lifecycle.wait_closed();
    }

    /// [`Engine::shutdown`] + [`Engine::wait_closed`]：请求关闭并等待线程结束。
    pub fn shutdown_and_wait(&self) {
        self.shutdown();
        self.wait_closed();
    }

    /// 核心线程是否已经退出。
    pub fn is_closed(&self) -> bool {
        self.lifecycle.is_closed()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::config::Config;

    /// 生命周期测试会断言全局线程计数，必须串行执行。
    static LIFECYCLE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lifecycle_test_guard() -> MutexGuard<'static, ()> {
        lock(&LIFECYCLE_TEST_LOCK)
    }

    #[test]
    fn shutdown_terminates_core_thread_deterministically() {
        let _guard = lifecycle_test_guard();
        let baseline = live_core_threads();

        let engine = Engine::new(Config::default());
        assert!(!engine.is_closed());

        engine.shutdown();
        engine.wait_closed();
        // 重复等待 / 重复请求都安全。
        engine.wait_closed();
        engine.shutdown();

        assert!(engine.is_closed());
        assert_eq!(live_core_threads(), baseline);
    }

    #[test]
    fn repeated_create_and_shutdown_do_not_leak_threads() {
        let _guard = lifecycle_test_guard();
        let baseline = live_core_threads();

        for round in 0..8 {
            let engine = Engine::new(Config::default());
            assert_eq!(live_core_threads(), baseline + 1, "round {round}");
            engine.shutdown_and_wait();
            assert!(engine.is_closed(), "round {round}");
            assert_eq!(live_core_threads(), baseline, "round {round}");
        }
    }

    #[test]
    fn commands_after_shutdown_fail_without_blocking() {
        let _guard = lifecycle_test_guard();
        let baseline = live_core_threads();
        let engine = Engine::new(Config::default());
        engine.shutdown_and_wait();

        let err = engine
            .command(Command::RefreshDevices)
            .expect_err("closed engine must reject commands");
        assert!(err.to_string().contains("engine stopped"), "{err}");

        // 克隆句柄共享同一份生命周期状态。
        let clone = engine.clone();
        assert!(clone.command(Command::RefreshDevices).is_err());

        // 重复关闭仍然幂等，线程计数保持回落。
        engine.shutdown_and_wait();
        assert_eq!(live_core_threads(), baseline);
    }
}
