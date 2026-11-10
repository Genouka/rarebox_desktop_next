//! ArkTS/UI 线程的 IME 事件队列，在渲染线程上应用。
//!
//! 与 Android 后端的 `text_input.rs` 同形：鸿蒙的系统 IME 通过
//! `@ohos.inputMethod` 的 `insertText` / `setPreviewText` / `finishTextPreview` /
//! `deleteLeft` / `deleteRight` 回调把编辑动作交给宿主页面，宿主再经 NAPI 的
//! `ime(actionJson)` 送进来。事件在 UI 线程入队，渲染线程在
//! `on_request_frame` 与触摸开始前 `drain` 到 [`PlatformInputHandler`]——
//! 这样 GPUI 的编辑模型（组词、选区、撤销）完全不需要知道鸿蒙的存在。
//!
//! 会话号（`session`）用来丢弃迟到的回调：焦点切换/IME attach 会开新会话，
//! 旧会话的事件在 `drain` 时被跳过。

use gpui::{
    DispatchEventResult, KeyDownEvent, Keystroke, Modifiers, PlatformInput, PlatformInputHandler,
};
use parking_lot::Mutex;
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

/// 组词（`setPreviewText`）→ `replace_and_mark_text_in_range`。
pub const IME_KIND_COMPOSE: i32 = 0;
/// 提交文本（`insertText` / `finishTextPreview`）→ `replace_text_in_range` + `unmark_text`。
pub const IME_KIND_COMMIT: i32 = 1;
/// 按 UTF-16 单元删除周围文本（`deleteLeft`/`deleteRight`）。
pub const IME_KIND_DELETE_SURROUNDING: i32 = 2;
/// 按 Unicode 码点删除（宿主以码点计数时使用）。
pub const IME_KIND_DELETE_IN_CODE_POINTS: i32 = 3;
/// 输入法关闭/完成（发送 Escape，让 GPUI 结束编辑状态）。
pub const IME_KIND_DONE: i32 = 4;

static SESSION: AtomicU64 = AtomicU64::new(0);
static COMPOSING: AtomicBool = AtomicBool::new(false);
static EVENTS: Mutex<VecDeque<ImeEvent>> = Mutex::new(VecDeque::new());

/// 一次 IME 编辑动作。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeEvent {
    /// 产生该事件的输入会话。
    pub session: u64,
    /// 见 `IME_KIND_*`。
    pub kind: i32,
    /// 提交/组词的文本。
    pub text: String,
    /// 组词选区起点（UTF-16）。
    pub start: usize,
    /// 组词选区终点（UTF-16）。
    pub end: usize,
}

impl ImeEvent {
    pub fn new(session: u64, kind: i32, text: impl Into<String>) -> Self {
        Self {
            session,
            kind,
            text: text.into(),
            start: 0,
            end: 0,
        }
    }

    pub fn with_selection(mut self, start: usize, end: usize) -> Self {
        self.start = start;
        self.end = end;
        self
    }
}

/// 开一个新 IME 会话并丢弃旧会话的排队事件。
pub fn new_session() -> u64 {
    let session = SESSION.fetch_add(1, Ordering::AcqRel) + 1;
    EVENTS.lock().clear();
    COMPOSING.store(false, Ordering::Release);
    session
}

/// 当前会话号。
pub fn current_session() -> u64 {
    SESSION.load(Ordering::Acquire)
}

/// 当前是否处于组词状态。
pub fn is_composing() -> bool {
    COMPOSING.load(Ordering::Acquire)
}

/// 入队一个事件。旧会话的事件被静默丢弃。
pub fn enqueue(event: ImeEvent) {
    if event.session != current_session() {
        return;
    }
    EVENTS.lock().push_back(event);
    crate::mark_text_input_dirty();
    notify_text_arrived();
}

/// 取出并应用全部排队事件。
///
/// 不跨 GPUI 更新持有 `RefCell` 借用：处理每个事件前先把 handler 取出，
/// 处理完再放回（GPUI 的更新可能替换 handler）。
pub fn drain(
    slot: &Rc<RefCell<Option<PlatformInputHandler>>>,
    input: &mut dyn FnMut(PlatformInput) -> DispatchEventResult,
) {
    let events: Vec<ImeEvent> = std::mem::take(&mut *EVENTS.lock()).into();
    for event in events {
        if event.session != current_session() {
            continue;
        }
        if event.kind == IME_KIND_DONE {
            finish_composition(slot);
            input(PlatformInput::KeyDown(KeyDownEvent {
                keystroke: Keystroke {
                    key: "escape".into(),
                    key_char: None,
                    modifiers: Modifiers::default(),
                },
                is_held: false,
                prefer_character_input: false,
            }));
            continue;
        }

        let Some(mut handler) = slot.borrow_mut().take() else {
            // 没有焦点输入框时 IME 不该发编辑事件；丢掉并留下诊断，
            // 而不是把它硬塞进键盘事件流（那会污染快捷键）。
            log::debug!(
                "dropping IME event kind={} ({} chars): no focused input handler",
                event.kind,
                event.text.chars().count()
            );
            continue;
        };

        match event.kind {
            IME_KIND_COMPOSE => {
                COMPOSING.store(true, Ordering::Release);
                handler.replace_and_mark_text_in_range(
                    None,
                    &event.text,
                    Some(event.start..event.end),
                );
            }
            IME_KIND_COMMIT => {
                COMPOSING.store(false, Ordering::Release);
                handler.replace_text_in_range(None, &event.text);
                handler.unmark_text();
            }
            IME_KIND_DELETE_SURROUNDING | IME_KIND_DELETE_IN_CODE_POINTS => {
                if let Some(selection) = handler.selected_text_range(false) {
                    let mut adjusted = None;
                    if let Some(text) = handler.text_for_range(0..usize::MAX, &mut adjusted) {
                        let range = deletion_range(
                            &text,
                            selection.range,
                            event.start,
                            event.end,
                            event.kind == IME_KIND_DELETE_IN_CODE_POINTS,
                        );
                        handler.replace_text_in_range(Some(range), "");
                    }
                }
            }
            other => {
                log::debug!("ignoring unknown IME event kind {other}");
            }
        }

        let mut current = slot.borrow_mut();
        if current.is_none() {
            *current = Some(handler);
        }
    }
}

/// 结束组词状态：取消 marked range，并让宿主页面重置输入法合成。
pub fn finish_composition(slot: &Rc<RefCell<Option<PlatformInputHandler>>>) {
    if !COMPOSING.swap(false, Ordering::AcqRel) {
        return;
    }
    if let Some(mut handler) = slot.borrow_mut().take() {
        handler.unmark_text();
        let mut current = slot.borrow_mut();
        if current.is_none() {
            *current = Some(handler);
        }
    }
    notify_reset_composition();
}

/// 删除范围。
///
/// 鸿蒙的 `deleteLeft/deleteRight` 以 **UTF-16 单元**计数（与 `InputMethodController`
/// 的回调参数一致，见地图 §4.2），但宿主也可以在 `ime(actionJson)` 里声明按码点
/// 计数（`codePoints: true`）。无论哪种，都把端点向外取整到字符边界，
/// 绝不把代理对切成两半。
pub fn deletion_range(
    text: &str,
    selection: std::ops::Range<usize>,
    before: usize,
    after: usize,
    code_points: bool,
) -> std::ops::Range<usize> {
    let mut boundaries = vec![0];
    for ch in text.chars() {
        boundaries.push(boundaries.last().unwrap() + ch.len_utf16());
    }
    let length = *boundaries.last().unwrap();
    if code_points {
        let start_index = boundaries.partition_point(|offset| *offset < selection.start);
        let end_index = boundaries.partition_point(|offset| *offset < selection.end);
        boundaries[start_index.saturating_sub(before)]
            ..boundaries[end_index.saturating_add(after).min(boundaries.len() - 1)]
    } else {
        let start = selection.start.saturating_sub(before);
        let end = selection.end.saturating_add(after).min(length);
        let start_index = boundaries
            .partition_point(|offset| *offset <= start)
            .saturating_sub(1);
        let end_index = boundaries.partition_point(|offset| *offset < end);
        boundaries[start_index]..boundaries[end_index]
    }
}

fn notify_text_arrived() {
    #[cfg(target_env = "ohos")]
    crate::frame_source::schedule_frame();
}

fn notify_reset_composition() {
    #[cfg(target_env = "ohos")]
    crate::host::emit("imeResetComposition", "");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(session: u64, kind: i32, text: &str) -> ImeEvent {
        ImeEvent::new(session, kind, text)
    }

    #[test]
    fn new_session_discards_previous_events() {
        let first = new_session();
        enqueue(event(first, IME_KIND_COMMIT, "old"));
        let second = new_session();
        assert!(second > first);
        enqueue(event(first, IME_KIND_COMMIT, "stale"));
        enqueue(event(second, IME_KIND_COMMIT, "fresh"));
        assert_eq!(EVENTS.lock().len(), 1);
        assert_eq!(EVENTS.lock().front().unwrap().text, "fresh");
        let _ = std::mem::take(&mut *EVENTS.lock());
        assert!(!is_composing());
    }

    #[test]
    fn session_numbers_are_monotonic_and_positive() {
        let a = new_session();
        let b = new_session();
        assert!(a >= 1 && b > a);
        assert_eq!(current_session(), b);
    }

    #[test]
    fn deletion_preserves_surrogate_pairs() {
        assert_eq!(deletion_range("你😀好", 3..3, 1, 0, false), 1..3);
        assert_eq!(deletion_range("你😀好", 1..1, 0, 1, false), 1..3);
        assert_eq!(deletion_range("你😀好", 3..3, 2, 0, true), 0..3);
        assert_eq!(deletion_range("你😀好", 1..1, 0, 2, true), 1..4);
        assert_eq!(deletion_range("你😀好", 0..0, 99, 99, true), 0..4);
    }

    #[test]
    fn deletion_clamps_to_document_bounds() {
        assert_eq!(deletion_range("abc", 3..3, 10, 10, false), 0..3);
        assert_eq!(deletion_range("", 0..0, 1, 1, false), 0..0);
    }

    #[test]
    fn composing_flag_tracks_kind_without_a_handler() {
        let session = new_session();
        // 没有 handler 的事件会被丢弃，但组词标志只在真正应用时改变；
        // 这里验证 finish_composition 在没有组词时是 no-op。
        enqueue(event(session, IME_KIND_COMPOSE, "ni"));
        assert!(!is_composing());
        finish_composition(&Rc::new(RefCell::new(None)));
        assert!(!is_composing());
        let _ = std::mem::take(&mut *EVENTS.lock());
    }
}
