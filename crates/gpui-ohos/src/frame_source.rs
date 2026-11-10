//! 帧节拍：按需求合并、每 vsync 一帧。
//!
//! 节拍器 [`FramePacer`] 是平台无关的（从 `vendor/gpui-mobile/src/frame_pacer.rs`
//! 移植并补充了 vsync 丢失恢复），宿主单测直接覆盖它的数学；OHOS 相关部分
//! 只是 `OH_NativeVSync` 的线程胶水。
//!
//! # 为什么需要它
//!
//! GPUI 通过 `PlatformWindow::frame_waker` / `schedule_frame` 表达“我想要下一帧”。
//! 如果没有节拍器，渲染循环要么每轮都画（空转烧 CPU），要么按固定 sleep 画
//! （与屏幕刷新错拍，产生抖动）。节拍器把任意多次需求折叠成一个 vsync 回调，
//! 并在回调到达时正好放行一帧。
//!
//! # vsync 线程亲和性（风险 R1）
//!
//! `OH_NativeVSync` 的回调由创建它的线程的 EventRunner 派发。本后端按需求在
//! **渲染线程**创建 vsync 并 `RequestFrame`；如果回调永远不来（头文件没有承诺
//! 自我创建的线程一定收到回调），[`FramePacer::recover_stalled`] 会在超过
//! `2 × interval` 后把“已请求”当成丢失，直接放行一帧，从而退化为时钟节拍。
//! 这保证最坏情况是掉到 60Hz 时钟，而不是界面冻结。
//!
//! # 首次周期（风险 R15）
//!
//! `OH_NativeVSync_GetPeriod` 在收到第一次回调前无效（头文件明确要求先有一次
//! frame 回调）。因此在第一次回调前用 [`REFRESH_INTERVAL`]（16.667ms）兜底，
//! 收到回调后读到正数周期才替换。

use std::{
    cell::Cell,
    time::{Duration, Instant},
};

/// 60Hz 兜底刷新周期（`frame_source.rs` 在 Android 后端同样用这个值）。
pub const REFRESH_INTERVAL: Duration = Duration::from_micros(16_667);

/// 一次 vsync 请求。调用方必须在回调里调用 [`FramePacer::on_vsync`]。
pub type PostVsync = Box<dyn Fn()>;

/// 需求驱动的帧节拍器。
///
/// 只在渲染线程使用（内部是 `Cell`）。
pub struct FramePacer {
    /// `None` 表示没有 vsync 源，按墙钟节拍。
    post_vsync: Option<PostVsync>,
    interval: Cell<Duration>,
    /// 已请求、尚未回调。
    posted: Cell<bool>,
    /// 回调已到，帧尚未被取走。
    due: Cell<bool>,
    /// 最近一次请求 vsync 的时刻（用于检测回调丢失）。
    posted_at: Cell<Option<Instant>>,
    /// 时钟回退：需求等待到这个时刻。
    fallback_due_at: Cell<Option<Instant>>,
    /// 上一帧被取走的时刻，用于把两帧至少隔开一个 interval。
    last_frame_at: Cell<Option<Instant>>,
}

impl FramePacer {
    /// 由 vsync 驱动。`interval` 在拿到真实刷新周期前用 [`REFRESH_INTERVAL`]。
    pub fn with_vsync(post_vsync: PostVsync, interval: Duration) -> Self {
        Self {
            post_vsync: Some(post_vsync),
            interval: Cell::new(interval),
            posted: Cell::new(false),
            due: Cell::new(false),
            posted_at: Cell::new(None),
            fallback_due_at: Cell::new(None),
            last_frame_at: Cell::new(None),
        }
    }

    /// 没有 vsync 源时按墙钟把帧隔开 `interval`。
    pub fn with_clock(interval: Duration) -> Self {
        Self {
            post_vsync: None,
            interval: Cell::new(interval),
            posted: Cell::new(false),
            due: Cell::new(false),
            posted_at: Cell::new(None),
            fallback_due_at: Cell::new(None),
            last_frame_at: Cell::new(None),
        }
    }

    /// 更新刷新周期（R15：第一次 vsync 回调之后才拿到有效值）。
    ///
    /// 非有限/非正的值被忽略，避免 `poll_timeout` 返回零导致忙等。
    pub fn set_interval(&self, interval: Duration) {
        if !interval.is_zero() && interval < Duration::from_secs(1) {
            self.interval.set(interval);
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval.get()
    }

    /// 记录一次帧需求。
    ///
    /// * 已有请求/已有一帧待取/时钟已排期 → 折叠，不重复请求；
    /// * 距上一帧已超过一个 interval → 立即到期（循环落后时不再等 vsync，
    ///   否则输入会被多压一帧）；
    /// * 否则请求一次 vsync。
    pub fn schedule(&self, now: Instant) {
        if self.posted.get() || self.due.get() || self.fallback_due_at.get().is_some() {
            return;
        }
        let interval_elapsed = self
            .last_frame_at
            .get()
            .is_none_or(|last| now.duration_since(last) >= self.interval.get());
        match &self.post_vsync {
            Some(_) if interval_elapsed => self.due.set(true),
            Some(post) => {
                self.posted.set(true);
                self.posted_at.set(Some(now));
                post();
            }
            None => {
                let earliest = self
                    .last_frame_at
                    .get()
                    .map_or(now, |last| last + self.interval.get());
                self.fallback_due_at.set(Some(earliest.max(now)));
            }
        }
    }

    /// vsync 回调到达（由渲染线程在回调线程置位后调用）。
    pub fn on_vsync(&self) {
        self.posted.set(false);
        self.posted_at.set(None);
        self.due.set(true);
    }

    /// 取走到期的那一帧；返回 `true` 时渲染循环才真正画。
    pub fn take_frame(&self, now: Instant) -> bool {
        if self.due.replace(false) {
            self.last_frame_at.set(Some(now));
            return true;
        }
        match self.fallback_due_at.get() {
            Some(due_at) if due_at <= now => {
                self.fallback_due_at.set(None);
                self.last_frame_at.set(Some(now));
                true
            }
            _ => false,
        }
    }

    /// 是否还有未兑现的帧需求（诊断/测试用）。
    pub fn has_demand(&self) -> bool {
        self.posted.get() || self.due.get() || self.fallback_due_at.get().is_some()
    }

    /// 放弃一个可能永远不会触发的回调，并重新排期（surface 换绑/回前台时）。
    ///
    /// 迟到的旧回调会多标一帧 due，GPUI 在没有脏状态时会把它当成空帧。
    pub fn resume(&self, now: Instant) {
        self.posted.set(false);
        self.posted_at.set(None);
        self.schedule(now);
    }

    /// R1：已请求的 vsync 超过 `stall_after` 仍未回调，视为丢失。
    ///
    /// 返回 `true` 表示本次调用把一个挂起请求转成了到期帧（调用方应唤醒
    /// 渲染循环）。正常情况下 vsync 每 16.7ms 到达，这个分支只在创建 vsync
    /// 的线程收不到回调时生效。
    pub fn recover_stalled(&self, now: Instant, stall_after: Duration) -> bool {
        let Some(posted_at) = self.posted_at.get() else {
            return false;
        };
        if now.duration_since(posted_at) < stall_after {
            return false;
        }
        self.posted.set(false);
        self.posted_at.set(None);
        self.due.set(true);
        true
    }

    /// 循环可以睡多久：
    ///
    /// * 有到期帧 → 0；
    /// * 时钟回退 → 到期的剩余时间；
    /// * 纯 vsync → `None`（只有回调/其它唤醒能带来工作）。
    pub fn poll_timeout(&self, now: Instant) -> Option<Duration> {
        if self.due.get() {
            return Some(Duration::ZERO);
        }
        self.fallback_due_at
            .get()
            .map(|due_at| due_at.saturating_duration_since(now))
    }
}

// ── OHOS vsync 驱动 ──────────────────────────────────────────────────────────

#[cfg(target_env = "ohos")]
pub use vsync::{install, poll_timeout, resume, schedule_frame, take_frame, tick, uninstall};

#[cfg(target_env = "ohos")]
mod vsync {
    use super::{FramePacer, REFRESH_INTERVAL};
    use std::{
        cell::RefCell,
        ffi::{c_void, CString},
        ptr,
        sync::atomic::{AtomicBool, AtomicPtr, Ordering},
        time::{Duration, Instant},
    };

    /// 循环最长睡多久；漏掉一次唤醒最多损失这么多延迟。
    const IDLE_POLL_CAP: Duration = Duration::from_secs(1);

    /// vsync 回调丢失的判定阈值：两个刷新周期。
    const STALL_AFTER: Duration = Duration::from_micros(33_334);

    /// `OH_NativeVSync*`（渲染线程创建、进程生命周期内有效）。
    static NATIVE_VSYNC: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

    /// 任意线程提出了帧需求；渲染线程下一轮把它交给 pacer。
    static POST_REQUESTED: AtomicBool = AtomicBool::new(false);

    /// vsync 回调已触发；渲染线程下一轮把它交给 pacer。
    static FRAME_DUE: AtomicBool = AtomicBool::new(false);

    thread_local! {
        static PACER: RefCell<Option<FramePacer>> = const { RefCell::new(None) };
    }

    unsafe extern "C" {
        fn OH_NativeVSync_Create(name: *const std::ffi::c_char, length: u32) -> *mut c_void;
        fn OH_NativeVSync_Destroy(native_vsync: *mut c_void);
        fn OH_NativeVSync_RequestFrame(
            native_vsync: *mut c_void,
            callback: Option<unsafe extern "C" fn(i64, *mut c_void)>,
            data: *mut c_void,
        ) -> i32;
        fn OH_NativeVSync_GetPeriod(native_vsync: *mut c_void, period: *mut i64) -> i32;
    }

    /// 在渲染线程安装节拍器并创建 `OH_NativeVSync`（风险 R1/R15）。
    pub fn install() {
        let name = CString::new("gpui-ohos").expect("static string");
        // SAFETY: 名字指针在调用期间有效，返回的句柄由本线程持有到 uninstall。
        let handle = unsafe { OH_NativeVSync_Create(name.as_ptr(), name.as_bytes().len() as u32) };
        if handle.is_null() {
            log::error!("OH_NativeVSync_Create failed; falling back to a 60Hz clock");
            PACER.with(|slot| *slot.borrow_mut() = Some(FramePacer::with_clock(REFRESH_INTERVAL)));
            return;
        }
        NATIVE_VSYNC.store(handle, Ordering::SeqCst);
        PACER.with(|slot| {
            *slot.borrow_mut() = Some(FramePacer::with_vsync(
                Box::new(request_frame),
                REFRESH_INTERVAL,
            ))
        });
        log::info!("frame source installed (OH_NativeVSync={handle:p})");
    }

    /// 销毁 vsync（渲染线程退出时）。
    pub fn uninstall() {
        let handle = NATIVE_VSYNC.swap(ptr::null_mut(), Ordering::SeqCst);
        if !handle.is_null() {
            // SAFETY: 句柄由 install 创建，且此后再无 RequestFrame。
            unsafe { OH_NativeVSync_Destroy(handle) };
        }
        PACER.with(|slot| *slot.borrow_mut() = None);
    }

    /// `FramePacer` 的 post_vsync 回调：请求一次 vsync。
    fn request_frame() {
        let handle = NATIVE_VSYNC.load(Ordering::SeqCst);
        if handle.is_null() {
            // 没有 vsync：立即把回调当作已到达，让循环退回时钟节拍。
            FRAME_DUE.store(true, Ordering::SeqCst);
            crate::host::wake_render_thread();
            return;
        }
        // SAFETY: 句柄有效；回调只置位原子量并唤醒循环，不触碰 GPUI。
        let status = unsafe {
            OH_NativeVSync_RequestFrame(handle, Some(on_vsync_callback), ptr::null_mut())
        };
        if status != 0 {
            log::warn!("OH_NativeVSync_RequestFrame failed with {status}; using clock pacing");
            FRAME_DUE.store(true, Ordering::SeqCst);
            crate::host::wake_render_thread();
        }
    }

    unsafe extern "C" fn on_vsync_callback(_timestamp: i64, _data: *mut c_void) {
        FRAME_DUE.store(true, Ordering::SeqCst);
        crate::host::wake_render_thread();
    }

    /// 任意线程：请求下一帧。
    pub fn schedule_frame() {
        POST_REQUESTED.store(true, Ordering::SeqCst);
        crate::host::wake_render_thread();
    }

    /// surface 换绑/回前台：重新请求一次，旧回调可能永远不会到达。
    pub fn resume() {
        POST_REQUESTED.store(true, Ordering::SeqCst);
        crate::host::wake_render_thread();
    }

    /// 渲染线程每轮调用：把跨线程请求翻译成 pacer 状态。
    pub fn tick() {
        let now = Instant::now();
        PACER.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(pacer) = slot.as_mut() else {
                return;
            };
            if FRAME_DUE.swap(false, Ordering::SeqCst) {
                // 第一次回调后周期才有效（R15）。
                let mut period_ns: i64 = 0;
                let handle = NATIVE_VSYNC.load(Ordering::SeqCst);
                if !handle.is_null() {
                    // SAFETY: 句柄有效；GetPeriod 只写 period_ns。
                    let status = unsafe { OH_NativeVSync_GetPeriod(handle, &mut period_ns) };
                    if status == 0 && period_ns > 0 {
                        pacer.set_interval(Duration::from_nanos(period_ns as u64));
                    }
                }
                pacer.on_vsync();
            }
            if POST_REQUESTED.swap(false, Ordering::SeqCst) {
                pacer.schedule(now);
            } else if pacer.recover_stalled(now, STALL_AFTER) {
                log::debug!("vsync callback missing; serving the frame from the clock");
            }
        });
    }

    /// 取一帧；只能在渲染线程、且窗口可绘制时调用。
    pub fn take_frame() -> bool {
        let now = Instant::now();
        PACER.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(|pacer| pacer.take_frame(now))
        })
    }

    /// 循环在条件变量上最多睡多久。
    pub fn poll_timeout(next_delayed_task: Option<Instant>, can_draw: bool) -> Duration {
        let now = Instant::now();
        let frame_timeout = if can_draw {
            PACER.with(|slot| slot.borrow().as_ref().and_then(|pacer| pacer.poll_timeout(now)))
        } else {
            // 不可绘制时到期帧保持 owed，但不能让循环空转。
            Some(IDLE_POLL_CAP)
        };
        let delayed_timeout = next_delayed_task.map(|due| due.saturating_duration_since(now));

        let timeout = match (frame_timeout, delayed_timeout) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => IDLE_POLL_CAP,
        };
        timeout.min(IDLE_POLL_CAP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    const INTERVAL: Duration = Duration::from_millis(16);

    fn vsync_pacer_after_frame(now: Instant) -> (FramePacer, Rc<Cell<u32>>) {
        let posts = Rc::new(Cell::new(0));
        let counter = Rc::clone(&posts);
        let pacer = FramePacer::with_vsync(
            Box::new(move || counter.set(counter.get() + 1)),
            INTERVAL,
        );
        pacer.schedule(now);
        assert!(pacer.take_frame(now), "第一帧立即服务");
        (pacer, posts)
    }

    #[test]
    fn first_frame_does_not_wait_for_a_vsync() {
        let posts = Rc::new(Cell::new(0));
        let counter = Rc::clone(&posts);
        let pacer =
            FramePacer::with_vsync(Box::new(move || counter.set(counter.get() + 1)), INTERVAL);
        let now = Instant::now();
        pacer.schedule(now);
        assert_eq!(posts.get(), 0);
        assert_eq!(pacer.poll_timeout(now), Some(Duration::ZERO));
        assert!(pacer.take_frame(now));
    }

    #[test]
    fn demand_posts_one_callback_until_it_fires() {
        let now = Instant::now();
        let (pacer, posts) = vsync_pacer_after_frame(now);
        let now = now + Duration::from_millis(1);
        pacer.schedule(now);
        pacer.schedule(now);
        pacer.schedule(now);
        assert_eq!(posts.get(), 1);
        assert!(pacer.has_demand());
        assert!(!pacer.take_frame(now), "vsync 之前不能画");
        assert_eq!(pacer.poll_timeout(now), None, "只有 vsync 能带来工作");
    }

    #[test]
    fn vsync_makes_exactly_one_frame_due() {
        let now = Instant::now();
        let (pacer, posts) = vsync_pacer_after_frame(now);
        let now = now + Duration::from_millis(1);
        pacer.schedule(now);
        pacer.on_vsync();
        assert_eq!(pacer.poll_timeout(now), Some(Duration::ZERO));
        assert!(pacer.take_frame(now));
        assert!(!pacer.take_frame(now));
        assert!(!pacer.has_demand());
        pacer.schedule(now + Duration::from_millis(1));
        assert_eq!(posts.get(), 2);
    }

    #[test]
    fn demand_while_a_frame_is_due_does_not_post() {
        let now = Instant::now();
        let (pacer, posts) = vsync_pacer_after_frame(now);
        let now = now + Duration::from_millis(1);
        pacer.schedule(now);
        pacer.on_vsync();
        pacer.schedule(now);
        assert_eq!(posts.get(), 1);
        assert!(pacer.take_frame(now));
        assert!(!pacer.take_frame(now));
    }

    #[test]
    fn a_frame_that_overran_the_interval_is_served_at_once() {
        let now = Instant::now();
        let (pacer, posts) = vsync_pacer_after_frame(now);
        let later = now + Duration::from_millis(40);
        pacer.schedule(later);
        assert_eq!(posts.get(), 0);
        assert_eq!(pacer.poll_timeout(later), Some(Duration::ZERO));
        assert!(pacer.take_frame(later));
        pacer.schedule(later + Duration::from_millis(5));
        assert_eq!(posts.get(), 1);
    }

    #[test]
    fn untaken_frame_stays_due_while_inactive() {
        let now = Instant::now();
        let (pacer, _posts) = vsync_pacer_after_frame(now);
        let now = now + Duration::from_millis(1);
        pacer.schedule(now);
        pacer.on_vsync();
        assert!(pacer.has_demand());
        assert!(pacer.take_frame(now + Duration::from_secs(5)));
    }

    #[test]
    fn resume_reposts_a_callback_that_may_never_fire() {
        let now = Instant::now();
        let (pacer, posts) = vsync_pacer_after_frame(now);
        let now = now + Duration::from_millis(1);
        pacer.schedule(now);
        assert_eq!(posts.get(), 1);
        pacer.resume(now);
        assert_eq!(posts.get(), 2);
        pacer.on_vsync();
        pacer.on_vsync();
        assert!(pacer.take_frame(now));
        assert!(!pacer.take_frame(now));
    }

    #[test]
    fn clock_fallback_spaces_frames_by_the_interval() {
        let pacer = FramePacer::with_clock(INTERVAL);
        let t0 = Instant::now();
        pacer.schedule(t0);
        assert_eq!(pacer.poll_timeout(t0), Some(Duration::ZERO));
        assert!(pacer.take_frame(t0));
        pacer.schedule(t0 + Duration::from_millis(1));
        assert_eq!(
            pacer.poll_timeout(t0 + Duration::from_millis(1)),
            Some(Duration::from_millis(15))
        );
        assert!(!pacer.take_frame(t0 + Duration::from_millis(10)));
        assert!(pacer.take_frame(t0 + INTERVAL));
        assert!(!pacer.has_demand());
    }

    #[test]
    fn clock_fallback_folds_demand_into_the_pending_frame() {
        let pacer = FramePacer::with_clock(INTERVAL);
        let t0 = Instant::now();
        pacer.schedule(t0);
        pacer.schedule(t0 + Duration::from_millis(5));
        assert!(pacer.take_frame(t0 + Duration::from_millis(5)));
        assert!(!pacer.take_frame(t0 + Duration::from_millis(5)));
    }

    #[test]
    fn interval_updates_replace_the_fallback() {
        let pacer = FramePacer::with_clock(INTERVAL);
        assert_eq!(pacer.interval(), INTERVAL);
        pacer.set_interval(Duration::from_micros(8_333));
        assert_eq!(pacer.interval(), Duration::from_micros(8_333));
        // 非法周期被忽略（R15 的 0 纳秒/异常值不能变成忙等）。
        pacer.set_interval(Duration::ZERO);
        assert_eq!(pacer.interval(), Duration::from_micros(8_333));
        pacer.set_interval(Duration::from_secs(5));
        assert_eq!(pacer.interval(), Duration::from_micros(8_333));
    }

    #[test]
    fn stalled_vsync_is_recovered_as_a_frame() {
        let now = Instant::now();
        let (pacer, _posts) = vsync_pacer_after_frame(now);
        let now = now + Duration::from_millis(1);
        pacer.schedule(now);
        assert!(pacer.has_demand());
        assert!(!pacer.recover_stalled(now + Duration::from_millis(10), INTERVAL));
        assert!(pacer.recover_stalled(now + Duration::from_millis(40), INTERVAL));
        assert!(pacer.take_frame(now + Duration::from_millis(40)));
        // 恢复后重新按 vsync/时钟排期。
        assert!(!pacer.has_demand());
    }
}
