//! XComponent 回调：surface 生命周期 + 触摸/鼠标/按键。
//!
//! `OH_NativeXComponent*` 在 Node-API 模块初始化时从 `exports` 的
//! `__NATIVE_XCOMPONENT_OBJ__` 属性 `napi_unwrap` 得到（见 [`crate::napi`]）。
//! 指针由系统持有，只用于注册回调；**不保存任何 surface 指针**（风险 R10）：
//! surface 指针在回调里立即交给 [`crate::host::surface_attached`]，销毁时经
//! [`crate::host::detach_surface`] 阻塞到渲染线程释放。
//!
//! 所有回调都跑在 ArkTS/UI 线程：它们只读 C 结构、换算单位、投递命令，
//! 绝不触碰 GPUI（风险 R9）。

use crate::{
    host,
    keyboard,
    surface::{InsetsPx, NativeWindow},
    window::{KeyInput, MouseActionName, MouseButtonName, MouseInput, TouchInput, TouchPhaseName},
};
use std::{
    ffi::{c_char, c_void},
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};
use parking_lot::Mutex;

// ── C 结构（ace/xcomponent/native_interface_xcomponent.h） ───────────────────

type SurfaceCallback = unsafe extern "C" fn(*mut c_void, *mut c_void);
type TouchCallback = unsafe extern "C" fn(*mut c_void, *mut c_void);
type MouseCallback = unsafe extern "C" fn(*mut c_void, *mut c_void);
type HoverCallback = unsafe extern "C" fn(*mut c_void, bool);
type FocusCallback = unsafe extern "C" fn(*mut c_void, *mut c_void);
type KeyCallbackWithResult = unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool;

#[repr(C)]
struct XComponentCallback {
    on_surface_created: Option<SurfaceCallback>,
    on_surface_changed: Option<SurfaceCallback>,
    on_surface_destroyed: Option<SurfaceCallback>,
    dispatch_touch_event: Option<TouchCallback>,
}

#[repr(C)]
struct MouseEventCallback {
    dispatch_mouse_event: Option<MouseCallback>,
    dispatch_hover_event: Option<HoverCallback>,
}

/// 触摸事件（`OH_NativeXComponent_TouchEvent`）。
#[repr(C)]
#[derive(Clone, Copy)]
struct NativeTouchPoint {
    id: i32,
    screen_x: f32,
    screen_y: f32,
    x: f32,
    y: f32,
    event_type: i32,
    size: f64,
    force: f32,
    time_stamp: i64,
    is_pressed: bool,
}

/// 最多 10 个触点（`OH_NATIVE_XCOMPONENT_MAX_TOUCH_POINTS_NUMBER`）。
const MAX_TOUCH_POINTS: usize = 10;

#[repr(C)]
struct NativeTouchEvent {
    id: i32,
    screen_x: f32,
    screen_y: f32,
    x: f32,
    y: f32,
    event_type: i32,
    size: f64,
    force: f32,
    device_id: i64,
    time_stamp: i64,
    touch_points: [NativeTouchPoint; MAX_TOUCH_POINTS],
    num_points: u32,
}

/// 鼠标事件（`OH_NativeXComponent_MouseEvent`，坐标单位 vp）。
#[repr(C)]
struct NativeMouseEvent {
    x: f32,
    y: f32,
    screen_x: f32,
    screen_y: f32,
    timestamp: i64,
    action: i32,
    button: i32,
}

unsafe extern "C" {
    fn OH_NativeXComponent_RegisterCallback(
        component: *mut c_void,
        callback: *mut XComponentCallback,
    ) -> i32;
    fn OH_NativeXComponent_RegisterMouseEventCallback(
        component: *mut c_void,
        callback: *mut MouseEventCallback,
    ) -> i32;
    fn OH_NativeXComponent_RegisterKeyEventCallbackWithResult(
        component: *mut c_void,
        callback: Option<KeyCallbackWithResult>,
    ) -> i32;
    fn OH_NativeXComponent_RegisterKeyEventCallback(
        component: *mut c_void,
        callback: Option<SurfaceCallback>,
    ) -> i32;
    fn OH_NativeXComponent_RegisterFocusEventCallback(
        component: *mut c_void,
        callback: Option<FocusCallback>,
    ) -> i32;
    fn OH_NativeXComponent_RegisterBlurEventCallback(
        component: *mut c_void,
        callback: Option<FocusCallback>,
    ) -> i32;
    fn OH_NativeXComponent_GetTouchEvent(
        component: *mut c_void,
        window: *const c_void,
        touch_event: *mut NativeTouchEvent,
    ) -> i32;
    fn OH_NativeXComponent_GetMouseEvent(
        component: *mut c_void,
        window: *const c_void,
        mouse_event: *mut NativeMouseEvent,
    ) -> i32;
    fn OH_NativeXComponent_GetKeyEvent(
        component: *mut c_void,
        key_event: *mut *mut c_void,
    ) -> i32;
    fn OH_NativeXComponent_GetKeyEventCode(key_event: *mut c_void, code: *mut i32) -> i32;
    fn OH_NativeXComponent_GetKeyEventAction(key_event: *mut c_void, action: *mut i32) -> i32;
    fn OH_NativeXComponent_GetKeyEventCapsLockState(
        key_event: *mut c_void,
        is_caps_lock_on: *mut bool,
    ) -> i32;
    fn OH_NativeXComponent_SetNeedSoftKeyboard(component: *mut c_void, need_soft_keyboard: bool)
        -> i32;
}

// ── 模块级状态 ───────────────────────────────────────────────────────────────

/// 注册了回调的 `OH_NativeXComponent*`。
static COMPONENT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

// 回调结构体必须与 `.so` 同寿命：系统会长期持有其指针。
static CALLBACKS: Mutex<Option<Box<XComponentCallback>>> = Mutex::new(None);
static MOUSE_CALLBACKS: Mutex<Option<Box<MouseEventCallback>>> = Mutex::new(None);

/// 触摸事件的 action 值（`OH_NativeXComponent_TouchEventType`）。
const TOUCH_DOWN: i32 = 0;
const TOUCH_UP: i32 = 1;
const TOUCH_MOVE: i32 = 2;
const TOUCH_CANCEL: i32 = 3;

/// 鼠标 action 值（`OH_NativeXComponent_MouseEventAction`）。
const MOUSE_PRESS: i32 = 1;
const MOUSE_RELEASE: i32 = 2;
const MOUSE_MOVE: i32 = 3;
const MOUSE_CANCEL: i32 = 4;

/// 安装回调。`component` 来自 `napi_unwrap`；重复调用是 no-op。
pub fn install(component: *mut c_void) {
    if component.is_null() {
        log::warn!("XComponent: module init did not provide __NATIVE_XCOMPONENT_OBJ__");
        return;
    }
    if !COMPONENT.swap(component, Ordering::SeqCst).is_null() {
        return;
    }

    let mut callbacks = Box::new(XComponentCallback {
        on_surface_created: Some(on_surface_created),
        on_surface_changed: Some(on_surface_changed),
        on_surface_destroyed: Some(on_surface_destroyed),
        dispatch_touch_event: Some(on_touch),
    });
    // SAFETY: 结构体被 Box 固定住（存进静态 Mutex），指针在进程内有效。
    let status = unsafe { OH_NativeXComponent_RegisterCallback(component, &mut *callbacks) };
    if status != 0 {
        log::error!("OH_NativeXComponent_RegisterCallback failed: {status}");
    }

    let mut mouse = Box::new(MouseEventCallback {
        dispatch_mouse_event: Some(on_mouse),
        dispatch_hover_event: Some(on_hover),
    });
    // SAFETY: 同上。
    let status = unsafe { OH_NativeXComponent_RegisterMouseEventCallback(component, &mut *mouse) };
    if status != 0 {
        log::warn!("OH_NativeXComponent_RegisterMouseEventCallback failed: {status}");
    }

    // SAFETY: 回调签名与头文件一致；WithResult 是 API 14+，失败时退回旧入口。
    let status =
        unsafe { OH_NativeXComponent_RegisterKeyEventCallbackWithResult(component, Some(on_key)) };
    if status != 0 {
        log::info!(
            "RegisterKeyEventCallbackWithResult unavailable ({status}); using the legacy key callback"
        );
        // SAFETY: 旧入口只接受 void 回调；我们仍能拿到按键，只是不能“消费”它。
        unsafe { OH_NativeXComponent_RegisterKeyEventCallback(component, Some(on_key_legacy)) };
    }

    // SAFETY: 焦点回调没有特殊约束。
    unsafe {
        OH_NativeXComponent_RegisterFocusEventCallback(component, Some(on_focus));
        OH_NativeXComponent_RegisterBlurEventCallback(component, Some(on_blur));
    }

    *CALLBACKS.lock() = Some(callbacks);
    *MOUSE_CALLBACKS.lock() = Some(mouse);
    log::info!("XComponent callbacks registered (component={component:p})");
}

/// 当前 XComponent 句柄（`attach_surface` 需要它取尺寸）。
pub fn current_component() -> *mut c_void {
    COMPONENT.load(Ordering::SeqCst)
}

/// 让 XComponent 成为（或不再成为）软键盘目标。
pub fn set_soft_keyboard_needed(needed: bool) {
    let component = current_component();
    if component.is_null() {
        return;
    }
    // SAFETY: 句柄在模块寿命内有效。
    let status = unsafe { OH_NativeXComponent_SetNeedSoftKeyboard(component, needed) };
    if status != 0 {
        log::debug!("OH_NativeXComponent_SetNeedSoftKeyboard({needed}) failed: {status}");
    }
}

// ── surface ──────────────────────────────────────────────────────────────────

/// 由组件 + 窗口指针得到物理像素尺寸；拿不到时用 native window 的缓冲几何。
fn surface_size(window: *mut c_void) -> (u32, u32) {
    let density = host::surface_density();
    let component = current_component();
    if let Some(native_window) = NativeWindow::new(window) {
        if let Some((width_vp, height_vp)) = crate::surface::component_size_vp(component, native_window)
        {
            return crate::surface::surface_size_from_vp(width_vp, height_vp, density);
        }
        if let Some(size) = native_window.buffer_geometry() {
            return size;
        }
    }
    (1, 1)
}

unsafe extern "C" fn on_surface_created(component: *mut c_void, window: *mut c_void) {
    let _ = component;
    let Some(native_window) = NativeWindow::new(window) else {
        log::error!("OnSurfaceCreated with a null OHNativeWindow");
        return;
    };
    let (width, height) = surface_size(window);
    let density = host::surface_density();
    log::info!("XComponent surface created: {width}×{height}px @ {density:.2}");
    host::surface_attached(native_window, width, height, density);
}

unsafe extern "C" fn on_surface_changed(component: *mut c_void, window: *mut c_void) {
    let _ = component;
    let Some(native_window) = NativeWindow::new(window) else {
        return;
    };
    let (width, height) = surface_size(window);
    let density = host::surface_density();
    // 同一个 surface 的尺寸变化：先按 attach 走后端去重（host 会比较指针）。
    host::surface_attached(native_window, width, height, density);
}

unsafe extern "C" fn on_surface_destroyed(_component: *mut c_void, _window: *mut c_void) {
    log::info!("XComponent surface destroyed");
    // 阻塞到渲染线程放弃 renderer 对 surface 的引用（R10）。
    host::detach_surface();
}

// ── 触摸 ─────────────────────────────────────────────────────────────────────

unsafe extern "C" fn on_touch(component: *mut c_void, window: *mut c_void) {
    let mut event: NativeTouchEvent = unsafe { std::mem::zeroed() };
    // SAFETY: component/window 是回调参数，结构体由我们初始化。
    let status = unsafe { OH_NativeXComponent_GetTouchEvent(component, window, &mut event) };
    if status != 0 {
        log::debug!("OH_NativeXComponent_GetTouchEvent failed: {status}");
        return;
    }

    let density = host::surface_density();
    let count = (event.num_points as usize).min(MAX_TOUCH_POINTS);
    if count == 0 {
        // 旧设备可能只填顶层字段。
        let phase = touch_phase(event.event_type);
        if let Some(phase) = phase {
            host::touch(TouchInput {
                id: event.id,
                phase,
                x: crate::vp_to_px(event.x, density),
                y: crate::vp_to_px(event.y, density),
                force: Some(event.force).filter(|force| *force > 0.0),
            });
        }
        return;
    }

    for point in &event.touch_points[..count] {
        let Some(phase) = touch_phase(point.event_type) else {
            continue;
        };
        host::touch(TouchInput {
            id: point.id,
            phase,
            x: crate::vp_to_px(point.x, density),
            y: crate::vp_to_px(point.y, density),
            force: Some(point.force).filter(|force| *force > 0.0),
        });
    }
}

fn touch_phase(event_type: i32) -> Option<TouchPhaseName> {
    match event_type {
        TOUCH_DOWN => Some(TouchPhaseName::Down),
        TOUCH_UP => Some(TouchPhaseName::Up),
        TOUCH_MOVE => Some(TouchPhaseName::Move),
        TOUCH_CANCEL => Some(TouchPhaseName::Cancel),
        _ => None,
    }
}

// ── 鼠标 ─────────────────────────────────────────────────────────────────────

unsafe extern "C" fn on_mouse(component: *mut c_void, window: *mut c_void) {
    let mut event: NativeMouseEvent = unsafe { std::mem::zeroed() };
    // SAFETY: 同上。
    let status = unsafe { OH_NativeXComponent_GetMouseEvent(component, window, &mut event) };
    if status != 0 {
        log::debug!("OH_NativeXComponent_GetMouseEvent failed: {status}");
        return;
    }
    let action = match event.action {
        MOUSE_PRESS => MouseActionName::Press,
        MOUSE_RELEASE => MouseActionName::Release,
        MOUSE_MOVE => MouseActionName::Move,
        MOUSE_CANCEL => MouseActionName::Cancel,
        _ => return,
    };
    let button = match event.button {
        0x01 => Some(MouseButtonName::Left),
        0x02 => Some(MouseButtonName::Right),
        0x04 => Some(MouseButtonName::Middle),
        0x08 => Some(MouseButtonName::Back),
        0x10 => Some(MouseButtonName::Forward),
        _ => None,
    };
    // 鼠标事件坐标单位是 vp（头文件注释），统一在入口换算成 px（R11）。
    let density = host::surface_density();
    host::mouse(MouseInput {
        action,
        button,
        x: crate::vp_to_px(event.x, density),
        y: crate::vp_to_px(event.y, density),
        modifiers: 0,
        delta_x: 0.0,
        delta_y: 0.0,
        is_hover: None,
    });
}

unsafe extern "C" fn on_hover(component: *mut c_void, is_hover: bool) {
    let _ = component;
    // 悬停状态直接同步给渲染线程的命令队列。
    crate::host::mouse(MouseInput {
        action: MouseActionName::Hover,
        button: None,
        x: 0.0,
        y: 0.0,
        modifiers: 0,
        delta_x: 0.0,
        delta_y: 0.0,
        is_hover: Some(is_hover),
    });
}

// ── 按键 ─────────────────────────────────────────────────────────────────────

unsafe extern "C" fn on_key(component: *mut c_void, _window: *mut c_void) -> bool {
    dispatch_key(component)
}

unsafe extern "C" fn on_key_legacy(component: *mut c_void, _window: *mut c_void) {
    dispatch_key(component);
}

fn dispatch_key(component: *mut c_void) -> bool {
    let mut key_event: *mut c_void = ptr::null_mut();
    // SAFETY: 事件指针由 ArkUI 持有，只在本次回调内使用。
    let status = unsafe { OH_NativeXComponent_GetKeyEvent(component, &mut key_event) };
    if status != 0 || key_event.is_null() {
        return false;
    }
    let mut code: i32 = keyboard::KEYCODE_UNKNOWN;
    let mut action: i32 = keyboard::KEY_ACTION_UNKNOWN;
    // SAFETY: key_event 非空。
    unsafe {
        OH_NativeXComponent_GetKeyEventCode(key_event, &mut code);
        OH_NativeXComponent_GetKeyEventAction(key_event, &mut action);
    }
    let mut caps_lock = false;
    // SAFETY: 同上；API 10+。
    let status = unsafe { OH_NativeXComponent_GetKeyEventCapsLockState(key_event, &mut caps_lock) };
    let caps_lock = (status == 0).then_some(caps_lock);

    // 头文件没有公开修饰键位掩码（`GetKeyEventModifierKeyStates` 为 API 20+ 且
    // 语义未文档化），所以 `modifiers: None` 让后端用自己跟踪的修饰键状态（R14）。
    host::key(KeyInput {
        code,
        action,
        modifiers: None,
        unicode: None,
        caps_lock,
    });
    // 已交给 GPUI；返回 true 阻止事件继续下发给 ArkUI 的默认处理（地图 §4.2）。
    true
}

// ── 焦点 ─────────────────────────────────────────────────────────────────────

unsafe extern "C" fn on_focus(_component: *mut c_void, _window: *mut c_void) {
    host::emit("xcomponentFocus", "");
}

unsafe extern "C" fn on_blur(_component: *mut c_void, _window: *mut c_void) {
    host::emit("xcomponentBlur", "");
}

/// 让 ArkTS 侧把避让区交给后端（`avoidAreaChange` 回调转调）。
pub fn push_avoid_area(top: i32, bottom: i32, left: i32, right: i32) {
    host::avoid_area(InsetsPx::new(top, right, bottom, left));
}

/// `char` 指针辅助：仅用于调试日志中的组件 id（当前未使用，保留 FFI 形状）。
#[allow(dead_code)]
fn component_id(component: *mut c_void) -> Option<String> {
    if component.is_null() {
        return None;
    }
    let mut buffer = [0 as c_char; 64];
    let mut size: u64 = buffer.len() as u64;
    unsafe extern "C" {
        fn OH_NativeXComponent_GetXComponentId(
            component: *mut c_void,
            id: *mut c_char,
            size: *mut u64,
        ) -> i32;
    }
    // SAFETY: 缓冲区长度与 size 一致。
    let status = unsafe { OH_NativeXComponent_GetXComponentId(component, buffer.as_mut_ptr(), &mut size) };
    if status != 0 {
        return None;
    }
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    String::from_utf8(bytes).ok()
}
