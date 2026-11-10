//! 鸿蒙任务调度器。
//!
//! 两个队列，与 `gpui_linux` / Android 后端同形：
//!
//! * **主线程（前景）队列** —— 渲染线程。GPUI 的 `App` 只在这条线程上被访问，
//!   所以“主线程”就是 [`OhosDispatcher::new`] 被调用的线程（渲染线程自己构造
//!   平台，风险 R3）。
//! * **后台线程池** —— `std::thread` 固定池，数量取 `available_parallelism()`。
//!
//! 与 Android 的 `ALooper + pipe` 不同，OHOS 的 native 侧没有 looper：渲染线程
//! 是普通 Rust 线程。这里用 **条件变量 + 代际计数** 做唤醒，而不是 `pipe(2)`：
//!
//! * 生产者：入队后 [`OhosDispatcher::wake`]，在唤醒锁下递增代数并 `notify_all`；
//! * 消费者：[`OhosDispatcher::wait_for_work`] 先拿唤醒锁，再检查队列，
//!   只有队列为空才进入 `wait_for`。
//!
//! 二者共用同一把锁，因此不存在“检查完队列、还没开始等就错过通知”的窗口。
//! 帧源（vsync 回调线程）、NAPI 命令队列、延迟任务都走同一个 `wake`。
//!
//! 队列本身、延迟队列与线程池的语义都是平台无关的，宿主 `cargo test` 直接覆盖。

use gpui::{PlatformDispatcher, Priority, RunnableVariant};
use parking_lot::{Condvar, Mutex};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

/// 一条待执行任务。`RunnableVariant` 在这里被擦成闭包，便于宿主单测不依赖
/// `async_task`；`PlatformDispatcher` 的入口只是把它包一层 `runnable.run()`。
pub type Task = Box<dyn FnOnce() + Send + 'static>;

/// 延迟队列中的一项。
struct DelayedTask {
    due: Instant,
    task: Task,
}

struct Inner {
    main_thread: ThreadId,
    main: Mutex<VecDeque<Task>>,
    delayed: Mutex<Vec<DelayedTask>>,
    /// 唤醒代际 + 条件变量。见模块文档。
    wake: Mutex<u64>,
    wake_cvar: Condvar,
    pool: ThreadPool,
    shutdown: AtomicBool,
}

impl Inner {
    fn has_queued_work(&self) -> bool {
        if !self.main.lock().is_empty() {
            return true;
        }
        let now = Instant::now();
        self.delayed
            .lock()
            .first()
            .is_some_and(|task| task.due <= now)
    }
}

/// GPUI 的 `PlatformDispatcher` 实现。
///
/// `Arc<OhosDispatcher>` 同时被当作 `BackgroundExecutor` 与 `ForegroundExecutor`
/// 的数据源；`gpui::BackgroundExecutor::new` 接受 `Arc<dyn PlatformDispatcher>`。
pub struct OhosDispatcher {
    inner: Arc<Inner>,
}

impl OhosDispatcher {
    /// 在调用线程上创建调度器。调用线程即“主线程”。
    pub fn new() -> Arc<Self> {
        Self::with_pool_threads(
            thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .max(2),
        )
    }

    /// 指定后台线程数（测试用）。
    pub fn with_pool_threads(pool_threads: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Inner {
                main_thread: thread::current().id(),
                main: Mutex::new(VecDeque::new()),
                delayed: Mutex::new(Vec::new()),
                wake: Mutex::new(0),
                wake_cvar: Condvar::new(),
                pool: ThreadPool::new(pool_threads.max(1)),
                shutdown: AtomicBool::new(false),
            }),
        })
    }

    /// 调用线程是否是主（渲染）线程。
    pub fn is_main_thread(&self) -> bool {
        thread::current().id() == self.inner.main_thread
    }

    /// 把闭包排到主线程队列。
    pub fn dispatch_on_main_thread(&self, task: impl FnOnce() + Send + 'static) {
        if self.inner.shutdown.load(Ordering::Relaxed) {
            return;
        }
        self.inner.main.lock().push_back(Box::new(task));
        self.wake();
    }

    /// 把闭包排到后台线程池。
    pub fn dispatch_to_pool(&self, task: impl FnOnce() + Send + 'static) {
        if self.inner.shutdown.load(Ordering::Relaxed) {
            return;
        }
        self.inner.pool.dispatch(Box::new(task));
    }

    /// `delay` 之后在后台线程池执行。
    pub fn dispatch_after(&self, delay: Duration, task: impl FnOnce() + Send + 'static) {
        if self.inner.shutdown.load(Ordering::Relaxed) {
            return;
        }
        let due = Instant::now() + delay;
        let mut delayed = self.inner.delayed.lock();
        delayed.push(DelayedTask {
            due,
            task: Box::new(task),
        });
        delayed.sort_by_key(|task| task.due);
        drop(delayed);
        self.wake();
    }

    /// 最早到期的延迟任务时间；渲染循环据此缩短等待。
    pub fn next_delayed_due(&self) -> Option<Instant> {
        self.inner.delayed.lock().first().map(|task| task.due)
    }

    /// 把已到期的延迟任务投递到线程池。渲染循环每轮调用一次。
    pub fn tick(&self) {
        self.tick_at(Instant::now());
    }

    fn tick_at(&self, now: Instant) {
        let mut due = Vec::new();
        {
            let mut delayed = self.inner.delayed.lock();
            while delayed.first().is_some_and(|task| task.due <= now) {
                due.push(delayed.remove(0));
            }
        }
        for DelayedTask { task, .. } in due {
            self.inner.pool.dispatch(task);
        }
    }

    /// 同步执行主线程队列里的全部任务。只在主线程调用才有意义。
    pub fn flush_main_thread_tasks(&self) {
        loop {
            let task = self.inner.main.lock().pop_front();
            match task {
                Some(task) => task(),
                None => break,
            }
        }
    }

    /// 唤醒主线程（vsync 回调、NAPI 命令、延迟任务入队都调用它）。
    pub fn wake(&self) {
        let mut generation = self.inner.wake.lock();
        *generation = generation.wrapping_add(1);
        self.inner.wake_cvar.notify_all();
    }

    /// 阻塞当前（主）线程直到有工作或超时。
    ///
    /// 调用前应先跑完主线程任务与 `tick()`；本函数只负责“睡到下一次有事做”。
    /// 超时时间由调用方根据帧节拍与延迟任务算出。
    pub fn wait_for_work(&self, timeout: Duration) {
        let mut generation = self.inner.wake.lock();
        // 队列里有活就直接返回，不去睡（否则会丢一个 tick 的延迟）。
        if self.inner.has_queued_work() || self.inner.shutdown.load(Ordering::Relaxed) {
            return;
        }
        let _ = self.inner.wake_cvar.wait_for(&mut generation, timeout);
    }

    /// 停止接受新任务；正在等待的 `wait_for_work` 立即返回。
    pub fn shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        self.wake();
    }

    /// 是否已请求关闭。
    pub fn is_shutdown(&self) -> bool {
        self.inner.shutdown.load(Ordering::Relaxed)
    }
}

// ── ThreadPool ───────────────────────────────────────────────────────────────

/// 固定大小线程池。用 `std::sync::mpsc` + `std::sync::Mutex<Receiver>`：
/// 只有一个工作线程会在 `recv` 上阻塞，其余线程等锁；任务执行时不持锁。
struct ThreadPool {
    sender: std::sync::mpsc::Sender<Task>,
}

impl ThreadPool {
    fn new(threads: usize) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel::<Task>();
        let receiver = Arc::new(std::sync::Mutex::new(receiver));
        for index in 0..threads {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("gpui-ohos-bg-{index}"))
                .spawn(move || {
                    loop {
                        let task = {
                            let guard = receiver.lock().expect("bg receiver poisoned");
                            guard.recv()
                        };
                        match task {
                            Ok(task) => task(),
                            Err(_) => break,
                        }
                    }
                })
                .expect("failed to spawn gpui-ohos background thread");
        }
        Self { sender }
    }

    fn dispatch(&self, task: Task) {
        // 池已关闭时发送失败，静默丢弃（与 Android 后端一致）。
        let _ = self.sender.send(task);
    }
}

// ── PlatformDispatcher ───────────────────────────────────────────────────────

impl PlatformDispatcher for OhosDispatcher {
    fn is_main_thread(&self) -> bool {
        OhosDispatcher::is_main_thread(self)
    }

    fn dispatch(&self, runnable: RunnableVariant, priority: Priority) {
        let task: Task = Box::new(move || {
            runnable.run();
        });
        match priority {
            // 实时音频任务不能和普通后台任务抢线程池。
            Priority::RealtimeAudio => self.spawn_realtime(task),
            _ => self.dispatch_to_pool(move || task()),
        }
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, _priority: Priority) {
        self.dispatch_on_main_thread(move || {
            runnable.run();
        });
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        if self.inner.shutdown.load(Ordering::Relaxed) {
            return;
        }
        let due = Instant::now() + duration;
        let mut delayed = self.inner.delayed.lock();
        delayed.push(DelayedTask {
            due,
            task: Box::new(move || {
                runnable.run();
            }),
        });
        delayed.sort_by_key(|task| task.due);
        drop(delayed);
        self.wake();
    }

    fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
        let _ = thread::Builder::new()
            .name("gpui-ohos-rt".into())
            .spawn(f)
            .map_err(|error| log::error!("failed to spawn realtime thread: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn main_thread_detection_matches_constructor_thread() {
        let dispatcher = OhosDispatcher::with_pool_threads(1);
        assert!(dispatcher.is_main_thread());
        let other = Arc::clone(&dispatcher);
        let seen = Arc::new(std::sync::Mutex::new(None));
        let seen_clone = Arc::clone(&seen);
        thread::spawn(move || {
            *seen_clone.lock().unwrap() = Some(other.is_main_thread());
        })
        .join()
        .unwrap();
        assert_eq!(*seen.lock().unwrap(), Some(false));
    }

    #[test]
    fn main_thread_tasks_run_in_fifo_order_on_flush() {
        let dispatcher = OhosDispatcher::with_pool_threads(1);
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        for index in 0..8 {
            let order = Arc::clone(&order);
            dispatcher.dispatch_on_main_thread(move || order.lock().unwrap().push(index));
        }
        assert!(order.lock().unwrap().is_empty(), "must not run inline");
        dispatcher.flush_main_thread_tasks();
        assert_eq!(*order.lock().unwrap(), (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn pool_tasks_run_off_the_calling_thread() {
        let dispatcher = OhosDispatcher::with_pool_threads(2);
        let (sender, receiver) = mpsc::channel();
        let caller = thread::current().id();
        dispatcher.dispatch_to_pool(move || {
            let _ = sender.send(thread::current().id() != caller);
        });
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "pool task must run on a background thread"
        );
    }

    #[test]
    fn dispatch_on_main_thread_from_another_thread_wakes_the_waiter() {
        let dispatcher = OhosDispatcher::with_pool_threads(1);
        let waiter = Arc::clone(&dispatcher);
        thread::spawn(move || waiter.wait_for_work(Duration::from_secs(5)));

        // 给等待者一点时间进入 wait（即便没进入也不会丢事件：入队发生在
        // wake 之前，而 wait 会在锁下重新检查队列）。
        thread::sleep(Duration::from_millis(20));
        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = Arc::clone(&ran);
        dispatcher.dispatch_on_main_thread(move || {
            ran_clone.store(true, Ordering::SeqCst);
        });
        dispatcher.flush_main_thread_tasks();
        assert!(ran.load(Ordering::SeqCst));
    }

    #[test]
    fn wait_times_out_without_work() {
        let dispatcher = OhosDispatcher::with_pool_threads(1);
        let start = Instant::now();
        dispatcher.wait_for_work(Duration::from_millis(30));
        assert!(start.elapsed() >= Duration::from_millis(25));
    }

    #[test]
    fn delayed_tasks_are_sorted_by_due_time() {
        let dispatcher = OhosDispatcher::with_pool_threads(2);
        dispatcher.dispatch_after(Duration::from_millis(200), || {});
        dispatcher.dispatch_after(Duration::from_millis(100), || {});
        let first = dispatcher.next_delayed_due().expect("a delayed task");
        assert!(first <= Instant::now() + Duration::from_millis(200));
        dispatcher.dispatch_after(Duration::from_millis(0), || {});
        let earliest = dispatcher.next_delayed_due().unwrap();
        assert!(earliest <= first);
    }

    #[test]
    fn tick_runs_due_tasks_and_defers_future_ones() {
        let dispatcher = OhosDispatcher::with_pool_threads(2);
        let (sender, receiver) = mpsc::channel();
        dispatcher.dispatch_after(Duration::from_millis(0), move || {
            let _ = sender.send("due");
        });
        let (future_sender, future_receiver) = mpsc::channel();
        dispatcher.dispatch_after(Duration::from_secs(3600), move || {
            let _ = future_sender.send("not yet");
        });

        dispatcher.tick_at(Instant::now() + Duration::from_millis(1));
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok("due"));
        assert!(future_receiver.try_recv().is_err());
        assert!(dispatcher.next_delayed_due().is_some());
    }

    #[test]
    fn shutdown_stops_accepting_tasks() {
        let dispatcher = OhosDispatcher::with_pool_threads(1);
        dispatcher.shutdown();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = Arc::clone(&ran);
        dispatcher.dispatch_on_main_thread(move || ran_clone.store(true, Ordering::SeqCst));
        dispatcher.flush_main_thread_tasks();
        assert!(!ran.load(Ordering::SeqCst));
        assert!(dispatcher.is_shutdown());
    }
}
