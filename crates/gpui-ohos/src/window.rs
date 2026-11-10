//! 鸿蒙窗口：`OHNativeWindow` + `WgpuRenderer` + `PlatformWindow`。
//!
//! 一个 ability 只有一个原生窗口；[`OhosWindow`] 在第一次 `attach_surface` 时
//! 创建，之后跨 surface 重建存活（renderer 与字体图集都不丢）。
//! [`OhosPlatformWindow`] 是交给 GPUI 的 `Box<dyn PlatformWindow>` 包装。

use crate::{
    keyboard::{self, ModifierState},
    surface::{InsetsPx, NativeWindow, RawOhosWindow, sanitize_size},
};
use anyhow::Context as _;
use futures::channel::oneshot;
use gpui::{
    self, AtlasKey, AtlasTile, Bounds, Capslock, DevicePixels, DispatchEventResult, GpuSpecs,
    Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow,
    Point, PromptButton, PromptLevel, RequestFrameOptions, ScrollDelta, ScrollWheelEvent, Size,
    TouchEvent, TouchId, TouchPhase, WindowBackgroundAppearance, WindowBounds, WindowControlArea,
    WindowInsets, WindowVisibility, point, px, size,
};
use gpui_wgpu::{GpuContext, WgpuRenderer, WgpuSurfaceConfig, wgpu};
use parking_lot::Mutex;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use serde::Deserialize;
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

// ── 输入载荷（NAPI 解析后、已统一换算成物理像素） ────────────────────────────

/// `touch(eventJson).phase`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TouchPhaseName {
    Down,
    Up,
    Move,
    Cancel,
}

impl TouchPhaseName {
    fn to_gpui(self) -> TouchPhase {
        match self {
            TouchPhaseName::Down => TouchPhase::Started,
            TouchPhaseName::Up => TouchPhase::Ended,
            TouchPhaseName::Move => TouchPhase::Moved,
            TouchPhaseName::Cancel => TouchPhase::Cancelled,
        }
    }
}

/// 一次触摸。`x`/`y` 已是**物理像素**（NAPI 入口统一做过 `vp * density`）。
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct TouchInput {
    pub id: i32,
    pub phase: TouchPhaseName,
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub force: Option<f32>,
}

/// `mouse(eventJson).action`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseActionName {
    Press,
    Release,
    Move,
    Cancel,
    Scroll,
    Hover,
}

/// `mouse(eventJson).button`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButtonName {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButtonName {
    fn to_gpui(self) -> MouseButton {
        match self {
            MouseButtonName::Left => MouseButton::Left,
            MouseButtonName::Right => MouseButton::Right,
            MouseButtonName::Middle => MouseButton::Middle,
            MouseButtonName::Back => {
                MouseButton::Navigate(gpui::NavigationDirection::Back)
            }
            MouseButtonName::Forward => {
                MouseButton::Navigate(gpui::NavigationDirection::Forward)
            }
        }
    }
}

/// 鼠标/触控板事件。坐标为物理像素。
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MouseInput {
    pub action: MouseActionName,
    #[serde(default)]
    pub button: Option<MouseButtonName>,
    pub x: f32,
    pub y: f32,
    /// `1 = shift, 2 = ctrl, 4 = alt, 8 = meta`（与 ArkTS `MouseEvent.button` 约定一致）。
    #[serde(default)]
    pub modifiers: u32,
    #[serde(default)]
    pub delta_x: f32,
    #[serde(default)]
    pub delta_y: f32,
    /// 仅 `action: "hover"` 使用：`true` 指针进入，`false` 离开。
    #[serde(default)]
    pub is_hover: Option<bool>,
}

/// 按键事件。`modifiers` 缺省时用平台自己跟踪的修饰键状态。
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInput {
    pub code: i32,
    pub action: i32,
    #[serde(default)]
    pub modifiers: Option<u32>,
    #[serde(default)]
    pub unicode: Option<u32>,
    #[serde(default)]
    pub caps_lock: Option<bool>,
}

// ── 状态 ─────────────────────────────────────────────────────────────────────

struct WindowState {
    native_window: Option<NativeWindow>,
    gpu_context: GpuContext,
    renderer: Option<WgpuRenderer>,
    width: u32,
    height: u32,
    scale_factor: f32,

    safe_area: InsetsPx,
    keyboard: InsetsPx,
    geometry_dirty: bool,

    appearance: gpui::WindowAppearance,
    is_active: bool,
    hovered: bool,
    transparent: bool,
    pressed_button: Option<MouseButton>,
    mouse_position: Point<Pixels>,
    modifiers: ModifierState,
    back_enabled: bool,

    request_frame_callback: Option<Box<dyn FnMut(RequestFrameOptions) + Send>>,
    input_callback: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>>,
    resize_callback: Option<Box<dyn FnMut(Size<Pixels>, f32) + Send>>,
    insets_callback: Option<Box<dyn FnMut(WindowInsets) + Send>>,
    active_status_callback: Option<Box<dyn FnMut(bool) + Send>>,
    visibility_callback: Option<Box<dyn FnMut(WindowVisibility) + Send>>,
    hover_status_callback: Option<Box<dyn FnMut(bool) + Send>>,
    appearance_changed_callback: Option<Box<dyn FnMut() + Send>>,
    close_callback: Option<Box<dyn FnOnce() + Send>>,
    back_handler: Option<Box<dyn FnMut() + Send>>,
}

impl WindowState {
    fn logical_size(&self) -> Size<Pixels> {
        crate::logical_size_from_device(self.width, self.height, self.scale_factor)
    }

    fn effective_insets(&self) -> InsetsPx {
        self.safe_area.max(self.keyboard)
    }

    fn window_insets(&self) -> WindowInsets {
        WindowInsets {
            safe_area: self.safe_area.to_edges(self.scale_factor),
            ime: self.keyboard.to_edges(self.scale_factor),
        }
    }
}

/// 鸿蒙原生窗口（渲染器 + 回调槽位）。
pub struct OhosWindow {
    state: Mutex<WindowState>,
    id: std::sync::atomic::AtomicU64,
    active: AtomicBool,
    force_render_once: AtomicBool,
    /// 活跃触摸的 native id → GPUI `TouchId`。
    active_touches: Mutex<HashMap<i32, TouchId>>,
    next_touch_id: Mutex<u64>,
}

impl OhosWindow {
    /// 创建窗口并立刻建立 wgpu surface。
    pub fn new(
        native_window: NativeWindow,
        gpu_context: GpuContext,
        width_px: u32,
        height_px: u32,
        scale_factor: f32,
    ) -> anyhow::Result<Arc<Self>> {
        let (width, height) = sanitize_size(width_px, height_px);
        let scale_factor = sane_scale(scale_factor);
        let raw = RawOhosWindow::new(native_window);
        let renderer = create_renderer(&raw, Rc::clone(&gpu_context), width, height, false)
            .context("failed to create gpui_wgpu renderer on the XComponent surface")?;

        log::info!("OhosWindow::new — {width}×{height}px scale={scale_factor:.2}");
        let state = WindowState {
            native_window: Some(native_window),
            gpu_context,
            renderer: Some(renderer),
            width,
            height,
            scale_factor,
            safe_area: InsetsPx::default(),
            keyboard: InsetsPx::default(),
            geometry_dirty: true,
            appearance: gpui::WindowAppearance::Dark,
            is_active: true,
            hovered: false,
            transparent: false,
            pressed_button: None,
            mouse_position: Point::default(),
            modifiers: ModifierState::new(),
            back_enabled: false,
            request_frame_callback: None,
            input_callback: None,
            resize_callback: None,
            insets_callback: None,
            active_status_callback: None,
            visibility_callback: None,
            hover_status_callback: None,
            appearance_changed_callback: None,
            close_callback: None,
            back_handler: None,
        };

        Ok(Arc::new(Self {
            state: Mutex::new(state),
            id: std::sync::atomic::AtomicU64::new(native_window.as_ptr() as u64),
            active: AtomicBool::new(true),
            force_render_once: AtomicBool::new(true),
            active_touches: Mutex::new(HashMap::new()),
            next_touch_id: Mutex::new(0),
        }))
    }

    pub fn id(&self) -> u64 {
        self.id.load(Ordering::Relaxed)
    }

    /// 当前原始指针（用于 surface 去重）。
    pub fn native_window_ptr(&self) -> Option<*mut std::ffi::c_void> {
        self.state
            .lock()
            .native_window
            .map(|window| window.as_ptr())
    }

    /// 是否已绑定 surface。GPUI 的 `draw` 在未绑定时静默丢弃。
    pub fn has_surface(&self) -> bool {
        let state = self.state.lock();
        state.native_window.is_some() && state.renderer.is_some()
    }

    /// 循环是否应该请求一帧：有 surface 且窗口处于活跃状态。
    pub fn can_draw(&self) -> bool {
        let state = self.state.lock();
        state.is_active && state.native_window.is_some() && state.renderer.is_some()
    }

    /// 把新的 `OHNativeWindow*` 接到已有 renderer 上。
    ///
    /// 走 `replace_surface` 而不是 `destroy` + `recover`：前者保留 device/queue/
    /// 字体图集（GPUI 缓存的 `AtlasTextureId` 继续有效），并且只在 GL 后端使用
    /// （EGL 允许同一 native window 上先后存在两个 EGLSurface；Vulkan 的
    /// `ERROR_NATIVE_WINDOW_IN_USE_KHR` 不适用，因为本后端从不启用 Vulkan）。
    pub fn init_surface(
        &self,
        native_window: NativeWindow,
        width_px: u32,
        height_px: u32,
        scale_factor: f32,
    ) -> anyhow::Result<()> {
        let (width, height) = sanitize_size(width_px, height_px);
        let scale_factor = sane_scale(scale_factor);
        let raw = RawOhosWindow::new(native_window);

        let mut state = self.state.lock();
        let transparent = state.transparent;
        let instance = {
            let gpu_context = state.gpu_context.borrow();
            gpu_context.as_ref().map(|context| context.instance.clone())
        };
        let config = WgpuSurfaceConfig {
            size: size(DevicePixels(width as i32), DevicePixels(height as i32)),
            transparent,
            // GL 后端只保证 Fifo（vsync）；显式写出来避免 Mailbox 失败日志（地图 §3.2 D）。
            preferred_present_mode: Some(wgpu::PresentMode::Fifo),
        };

        if let Some(renderer) = state.renderer.as_mut() {
            match instance {
                Some(instance) => renderer
                    .replace_surface(&raw, config, &instance)
                    .context("replace_surface failed")?,
                None => {
                    // 唯一可能：公开 API 的上下文被外部清掉了。退回完整重建。
                    renderer.destroy();
                    renderer.recover(&raw).context("recover failed")?;
                }
            }
            renderer.update_drawable_size(size(
                DevicePixels(width as i32),
                DevicePixels(height as i32),
            ));
        } else {
            state.renderer = Some(create_renderer(
                &raw,
                Rc::clone(&state.gpu_context),
                width,
                height,
                transparent,
            )?);
        }

        let size_changed = state.width != width || state.height != height;
        let scale_changed = (state.scale_factor - scale_factor).abs() > f32::EPSILON;
        state.native_window = Some(native_window);
        state.width = width;
        state.height = height;
        state.scale_factor = scale_factor;
        state.is_active = true;
        state.geometry_dirty = true;
        self.active.store(true, Ordering::Relaxed);
        self.force_render_once.store(true, Ordering::Relaxed);
        self.id
            .store(native_window.as_ptr() as u64, Ordering::Relaxed);
        drop(state);

        if size_changed || scale_changed {
            self.notify_resize();
            self.notify_insets();
        }
        // 新 swapchain 是空的：让下一帧全量重绘，并请求这一帧。
        crate::frame_source::resume();
        Ok(())
    }

    /// 释放 renderer 对 surface 的引用，但保留 renderer/图集（跨前后台）。
    pub fn term_surface(&self) {
        let mut state = self.state.lock();
        if let Some(renderer) = state.renderer.as_mut() {
            // unconfigure 之后不再 present；图集与 device 都保留。
            renderer.unconfigure_surface();
        }
        state.native_window = None;
        state.is_active = false;
        drop(state);
        self.active.store(false, Ordering::Relaxed);
        self.set_active_callbacks(false);
    }

    /// surface 尺寸变化（同一个原生窗口）。
    pub fn resize_surface(&self, width_px: u32, height_px: u32, scale_factor: f32) {
        let (width, height) = sanitize_size(width_px, height_px);
        let scale_factor = sane_scale(scale_factor);
        {
            let mut state = self.state.lock();
            if state.width == width
                && state.height == height
                && (state.scale_factor - scale_factor).abs() <= f32::EPSILON
            {
                return;
            }
            state.width = width;
            state.height = height;
            state.scale_factor = scale_factor;
            state.geometry_dirty = true;
            if let Some(renderer) = state.renderer.as_mut() {
                renderer
                    .update_drawable_size(size(DevicePixels(width as i32), DevicePixels(height as i32)));
            }
        }
        self.notify_resize();
        self.notify_insets();
        self.force_render_once.store(true, Ordering::Relaxed);
        crate::frame_source::schedule_frame();
    }

    /// 把当前尺寸/缩放回报给 GPUI。
    fn notify_resize(&self) {
        let (logical, scale) = {
            let state = self.state.lock();
            (state.logical_size(), state.scale_factor)
        };
        let callback = {
            let mut state = self.state.lock();
            state.resize_callback.take()
        };
        if let Some(mut callback) = callback {
            callback(logical, scale);
            let mut state = self.state.lock();
            if state.resize_callback.is_none() {
                state.resize_callback = Some(callback);
            }
        }
    }

    fn notify_insets(&self) {
        let insets = self.state.lock().window_insets();
        let callback = {
            let mut state = self.state.lock();
            state.insets_callback.take()
        };
        if let Some(mut callback) = callback {
            callback(insets.clone());
            let mut state = self.state.lock();
            if state.insets_callback.is_none() {
                state.insets_callback = Some(callback);
            }
        }
    }

    /// 更新安全区（px）。
    pub fn set_safe_area(&self, insets: InsetsPx) {
        let changed = {
            let mut state = self.state.lock();
            if state.safe_area == insets {
                false
            } else {
                state.safe_area = insets;
                state.geometry_dirty = true;
                true
            }
        };
        if changed {
            self.notify_insets();
            crate::frame_source::schedule_frame();
        }
    }

    /// 更新软键盘遮挡高度（px，0 表示收起）。
    pub fn set_keyboard_height(&self, height_px: i32) {
        let height_px = height_px.max(0);
        let changed = {
            let mut state = self.state.lock();
            let insets = InsetsPx {
                bottom: height_px,
                ..InsetsPx::default()
            };
            if state.keyboard == insets {
                false
            } else {
                state.keyboard = insets;
                state.geometry_dirty = true;
                true
            }
        };
        if changed {
            self.notify_insets();
            crate::frame_source::schedule_frame();
        }
    }

    /// 帧请求：取出回调执行，再放回（回调内部会调用 `draw`，需要同一把锁）。
    pub fn request_frame(&self) {
        let callback = {
            let mut state = self.state.lock();
            state.request_frame_callback.take()
        };
        if let Some(mut callback) = callback {
            let text_dirty = crate::take_text_input_dirty();
            let surface_new = self.force_render_once.swap(false, Ordering::AcqRel);
            callback(RequestFrameOptions {
                require_presentation: false,
                force_render: text_dirty || surface_new,
            });
            let mut state = self.state.lock();
            if state.request_frame_callback.is_none() {
                state.request_frame_callback = Some(callback);
            }
        }
    }

    /// 绘制一帧。渲染器暂时移出锁，避免 `get_current_texture` 阻塞其它访问。
    pub fn draw(&self, scene: &gpui::Scene) {
        if scene_is_empty(scene) {
            log::trace!("OhosWindow::draw — skipping empty scene");
            return;
        }
        let mut renderer = {
            let mut state = self.state.lock();
            match state.renderer.take() {
                Some(renderer) => renderer,
                None => return,
            }
        };
        renderer.draw(scene);
        let mut state = self.state.lock();
        if state.renderer.is_none() {
            state.renderer = Some(renderer);
        }
    }

    // ── 输入 ─────────────────────────────────────────────────────────────────

    fn deliver_input(&self, event: PlatformInput) {
        let callback = {
            let mut state = self.state.lock();
            state.input_callback.take()
        };
        if let Some(mut callback) = callback {
            let _ = callback(event);
            let mut state = self.state.lock();
            if state.input_callback.is_none() {
                state.input_callback = Some(callback);
            }
        } else {
            log::debug!("input dropped: GPUI has not registered on_input yet");
        }
    }

    pub fn handle_touch(&self, input: TouchInput) {
        let phase = input.phase.to_gpui();
        let touch_id = match phase {
            TouchPhase::Started => {
                let mut next = self.next_touch_id.lock();
                let id = TouchId(*next);
                *next = next.checked_add(1).expect("touch id exhausted");
                drop(next);
                self.active_touches.lock().insert(input.id, id);
                id
            }
            _ => match self.active_touches.lock().get(&input.id).copied() {
                Some(id) => id,
                None => {
                    log::debug!("touch phase {phase:?} for unknown pointer {}", input.id);
                    return;
                }
            },
        };

        let scale = self.scale_factor();
        let event = TouchEvent {
            id: touch_id,
            phase,
            position: point(
                px(crate::px_to_vp(input.x, scale)),
                px(crate::px_to_vp(input.y, scale)),
            ),
            predicted_position: None,
            force: input.force,
        };
        self.deliver_input(PlatformInput::Touch(event));

        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.active_touches.lock().remove(&input.id);
        }
    }

    pub fn handle_mouse(&self, input: MouseInput) {
        let scale = self.scale_factor();
        let position = point(
            px(crate::px_to_vp(input.x, scale)),
            px(crate::px_to_vp(input.y, scale)),
        );
        let modifiers = {
            let mut state = self.state.lock();
            // XComponent 的鼠标事件不携带修饰键（回调里传 0）；0 视为“未知”，
            // 保留键盘线程跟踪的状态，否则每次移动鼠标都会清掉 Shift/Ctrl。
            if input.modifiers != 0 {
                state.modifiers.set_from_mask(input.modifiers);
            }
            state.mouse_position = position;
            state.modifiers.modifiers()
        };

        match input.action {
            MouseActionName::Press => {
                self.set_hovered(true);
                let button = input.button.unwrap_or(MouseButtonName::Left).to_gpui();
                self.state.lock().pressed_button = Some(button);
                self.deliver_input(PlatformInput::MouseDown(MouseDownEvent {
                    button,
                    position,
                    modifiers,
                    click_count: 1,
                    first_mouse: true,
                }));
            }
            MouseActionName::Release => {
                let button = input.button.unwrap_or(MouseButtonName::Left).to_gpui();
                self.state.lock().pressed_button = None;
                self.deliver_input(PlatformInput::MouseUp(MouseUpEvent {
                    button,
                    position,
                    modifiers,
                    click_count: 1,
                }));
            }
            MouseActionName::Move => {
                self.set_hovered(true);
                let pressed_button = self.state.lock().pressed_button;
                self.deliver_input(PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button,
                    modifiers,
                }));
            }
            MouseActionName::Cancel => {
                if let Some(button) = self.state.lock().pressed_button.take() {
                    self.deliver_input(PlatformInput::MouseUp(MouseUpEvent {
                        button,
                        position,
                        modifiers,
                        click_count: 1,
                    }));
                }
            }
            MouseActionName::Scroll => {
                self.deliver_input(PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position,
                    delta: ScrollDelta::Pixels(point(
                        px(crate::px_to_vp(input.delta_x, scale)),
                        px(crate::px_to_vp(input.delta_y, scale)),
                    )),
                    modifiers,
                    touch_phase: TouchPhase::Moved,
                }));
            }
            MouseActionName::Hover => self.set_hovered(input.is_hover.unwrap_or(true)),
        }
    }

    pub fn set_hovered(&self, hovered: bool) {
        let changed = {
            let mut state = self.state.lock();
            if state.hovered == hovered {
                false
            } else {
                state.hovered = hovered;
                true
            }
        };
        if !changed {
            return;
        }
        let callback = {
            let mut state = self.state.lock();
            state.hover_status_callback.take()
        };
        if let Some(mut callback) = callback {
            callback(hovered);
            let mut state = self.state.lock();
            if state.hover_status_callback.is_none() {
                state.hover_status_callback = Some(callback);
            }
        }
    }

    pub fn handle_key(&self, input: KeyInput) {
        let down = input.action == keyboard::KEY_ACTION_DOWN;
        let (modifiers, capslock, was_modifier) = {
            let mut state = self.state.lock();
            let was_modifier = state.modifiers.apply_key(input.code, down);
            // 0 表示宿主不知道（XComponent 键回调），保留自己跟踪的状态。
            if let Some(mask) = input.modifiers
                && mask != 0
                && !was_modifier
            {
                state.modifiers.set_from_mask(mask);
            }
            if let Some(caps) = input.caps_lock {
                state.modifiers.caps_lock = caps;
            }
            (
                state.modifiers.modifiers(),
                Capslock {
                    on: state.modifiers.caps_lock,
                },
                was_modifier,
            )
        };

        if was_modifier {
            self.deliver_input(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                modifiers,
                capslock,
            }));
            return;
        }

        let unicode = input
            .unicode
            .filter(|value| *value != 0)
            .and_then(char::from_u32);
        let Some(keystroke) =
            keyboard::key_event_to_keystroke(input.code, modifiers, capslock.on, unicode)
        else {
            return;
        };

        if down {
            self.deliver_input(PlatformInput::KeyDown(gpui::KeyDownEvent {
                keystroke,
                is_held: false,
                prefer_character_input: unicode.is_some(),
            }));
        } else {
            self.deliver_input(PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }));
        }
    }

    // ── 状态更新 ─────────────────────────────────────────────────────────────

    /// 更新活跃状态并触发回调。
    pub fn set_active(&self, active: bool) {
        let previous = self.active.swap(active, Ordering::AcqRel);
        {
            let mut state = self.state.lock();
            state.is_active = active;
        }
        if previous == active {
            return;
        }
        self.set_active_callbacks(active);
        if active {
            // 后台期间请求的 vsync 可能永远不回调，重新请求。
            crate::frame_source::resume();
        }
    }

    fn set_active_callbacks(&self, active: bool) {
        let active_callback = {
            let mut state = self.state.lock();
            state.active_status_callback.take()
        };
        if let Some(mut callback) = active_callback {
            callback(active);
            let mut state = self.state.lock();
            if state.active_status_callback.is_none() {
                state.active_status_callback = Some(callback);
            }
        }
        let visibility_callback = {
            let mut state = self.state.lock();
            state.visibility_callback.take()
        };
        if let Some(mut callback) = visibility_callback {
            callback(if active {
                WindowVisibility::Visible
            } else {
                WindowVisibility::Hidden
            });
            let mut state = self.state.lock();
            if state.visibility_callback.is_none() {
                state.visibility_callback = Some(callback);
            }
        }
    }

    /// 系统返回手势。返回 `true` 表示已被 GPUI 消费。
    pub fn handle_back(&self) -> bool {
        let (enabled, handler) = {
            let mut state = self.state.lock();
            (state.back_enabled, state.back_handler.take())
        };
        if let Some(mut handler) = handler {
            if enabled {
                handler();
            }
            let mut state = self.state.lock();
            if state.back_handler.is_none() {
                state.back_handler = Some(handler);
            }
            if enabled {
                return true;
            }
        }
        crate::host::emit("backUnhandled", "");
        false
    }

    pub fn scale_factor(&self) -> f32 {
        self.state.lock().scale_factor
    }

    pub fn set_appearance(&self, appearance: gpui::WindowAppearance) {
        let changed = {
            let mut state = self.state.lock();
            if state.appearance == appearance {
                false
            } else {
                state.appearance = appearance;
                true
            }
        };
        if !changed {
            return;
        }
        let callback = {
            let mut state = self.state.lock();
            state.appearance_changed_callback.take()
        };
        if let Some(mut callback) = callback {
            callback();
            let mut state = self.state.lock();
            if state.appearance_changed_callback.is_none() {
                state.appearance_changed_callback = Some(callback);
            }
        }
    }

    fn set_input_callback(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        self.state.lock().input_callback = Some(callback);
    }

    fn take_geometry_dirty(&self) -> bool {
        std::mem::take(&mut self.state.lock().geometry_dirty)
    }

    fn sprite_atlas(&self) -> Option<Arc<dyn PlatformAtlas>> {
        let state = self.state.lock();
        let renderer = state.renderer.as_ref()?;
        let atlas: Arc<dyn PlatformAtlas> = renderer.sprite_atlas().clone();
        Some(atlas)
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.state
            .lock()
            .renderer
            .as_ref()
            .map(|renderer| renderer.gpu_specs())
    }

    fn supports_subpixel_aa(&self) -> bool {
        self.state
            .lock()
            .renderer
            .as_ref()
            .is_some_and(|renderer| renderer.supports_dual_source_blending())
    }
}

fn sane_scale(scale_factor: f32) -> f32 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

fn scene_is_empty(scene: &gpui::Scene) -> bool {
    scene.shadows.is_empty()
        && scene.quads.is_empty()
        && scene.paths.is_empty()
        && scene.underlines.is_empty()
        && scene.monochrome_sprites.is_empty()
        && scene.subpixel_sprites.is_empty()
        && scene.polychrome_sprites.is_empty()
        && scene.surfaces.is_empty()
}

/// 在共享 `GpuContext` 里建立 **GL-only** 的 wgpu 实例（地图 §3.2 / 风险 R4）。
///
/// `gpui_wgpu::WgpuContext::instance` 固定 `Backends::VULKAN | GFX::GL`；Vulkan
/// 在 OHOS 上没有 `Rwh::OhosNdk` 分支，会在每个 surface 上失败并刷日志。这里
/// 先自己建一个 `Backends::GL` 的实例与上下文放进共享槽位，
/// `WgpuRenderer::new` 随后复用同一实例，于是整条链路只有 EGL/GLES。
fn ensure_gl_context(gpu_context: &GpuContext, raw: &RawOhosWindow) -> anyhow::Result<()> {
    if gpu_context.borrow().is_some() {
        return Ok(());
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::GL,
        ..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(raw.clone()))
    });
    let window_handle = raw
        .window_handle()
        .map_err(|error| anyhow::anyhow!("OHNativeWindow handle unavailable: {error}"))?;
    // SAFETY: `raw` 保证句柄在 renderer 存活期间有效（R10 的区间由 host 管理）。
    let surface = unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: None,
                raw_window_handle: window_handle.as_raw(),
            })
            .map_err(|error| anyhow::anyhow!("create_surface_unsafe failed: {error}"))?
    };
    // 这个 surface 只用来探测 adapter/device 是否与窗口兼容；`WgpuContext` 不保存它。
    let context = gpui_wgpu::WgpuContext::new(instance, &surface, None)
        .map_err(|error| anyhow::anyhow!("GL adapter selection failed: {error:#}"))?;
    drop(surface);
    *gpu_context.borrow_mut() = Some(context);
    Ok(())
}

fn create_renderer(
    raw: &RawOhosWindow,
    gpu_context: GpuContext,
    width: u32,
    height: u32,
    transparent: bool,
) -> anyhow::Result<WgpuRenderer> {
    ensure_gl_context(&gpu_context, raw)?;
    let config = WgpuSurfaceConfig {
        size: size(DevicePixels(width as i32), DevicePixels(height as i32)),
        transparent,
        preferred_present_mode: Some(wgpu::PresentMode::Fifo),
    };
    WgpuRenderer::new(gpu_context, raw, config, None)
}

// ── WindowList ───────────────────────────────────────────────────────────────

/// 存活窗口列表（鸿蒙上恒为一个，保留多窗口形状便于以后扩展）。
#[derive(Default)]
pub struct WindowList {
    windows: Vec<Arc<OhosWindow>>,
}

impl WindowList {
    pub fn push(&mut self, window: Arc<OhosWindow>) {
        self.windows.push(window);
    }

    pub fn primary(&self) -> Option<&Arc<OhosWindow>> {
        self.windows.first()
    }

    pub fn get(&self, id: u64) -> Option<&Arc<OhosWindow>> {
        self.windows.iter().find(|window| window.id() == id)
    }

    pub fn remove(&mut self, id: u64) -> Option<Arc<OhosWindow>> {
        let position = self.windows.iter().position(|window| window.id() == id)?;
        Some(self.windows.remove(position))
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}

// ── OhosPlatformWindow ───────────────────────────────────────────────────────

/// 交给 GPUI 的 `PlatformWindow` 包装。
pub struct OhosPlatformWindow {
    window: Arc<OhosWindow>,
    display: Option<Rc<dyn PlatformDisplay>>,
    input_handler: Rc<RefCell<Option<PlatformInputHandler>>>,
    /// IME 事件需要在没有输入法回调的时刻（帧请求）也能应用到 handler，
    /// 因此额外保存一份输入回调，与 `on_input` 注册的是同一个闭包。
    ime_input_callback:
        Arc<Mutex<Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>>>,
    title: String,
}

impl OhosPlatformWindow {
    pub fn new(window: Arc<OhosWindow>, display: Option<Rc<dyn PlatformDisplay>>) -> Self {
        Self {
            window,
            display,
            input_handler: Rc::new(RefCell::new(None)),
            ime_input_callback: Arc::new(Mutex::new(Box::new(|_| DispatchEventResult::default()))),
            title: String::new(),
        }
    }

    pub fn inner(&self) -> &Arc<OhosWindow> {
        &self.window
    }
}

impl HasWindowHandle for OhosPlatformWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let state = self.window.state.lock();
        let native_window = state
            .native_window
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let pointer = std::ptr::NonNull::new(native_window.as_ptr())
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = raw_window_handle::OhosNdkWindowHandle::new(pointer);
        // SAFETY: 句柄不含借用；surface 的有效期由 host 管理（R10）。
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(
                raw_window_handle::RawWindowHandle::OhosNdk(handle),
            )
        })
    }
}

impl HasDisplayHandle for OhosPlatformWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Ok(raw_window_handle::DisplayHandle::ohos())
    }
}

impl PlatformWindow for OhosPlatformWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        let size = self.window.state.lock().logical_size();
        Bounds {
            origin: point(px(0.0), px(0.0)),
            size,
        }
    }

    fn is_maximized(&self) -> bool {
        true
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Fullscreen(self.bounds())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.window.state.lock().logical_size()
    }

    /// 键盘/安全区遮住内容时，可视视口比 `content_size` 小。
    fn visual_viewport_bounds(&self) -> Bounds<Pixels> {
        let state = self.window.state.lock();
        let logical = state.logical_size();
        let insets = state.effective_insets();
        let scale = state.scale_factor;
        let left = crate::px_to_vp(insets.left as f32, scale);
        let top = crate::px_to_vp(insets.top as f32, scale);
        let right = crate::px_to_vp(insets.right as f32, scale);
        let bottom = crate::px_to_vp(insets.bottom as f32, scale);
        let width = (f32::from(logical.width) - left - right).max(0.0);
        let height = (f32::from(logical.height) - top - bottom).max(0.0);
        Bounds {
            origin: point(px(left), px(top)),
            size: size(px(width), px(height)),
        }
    }

    fn prepare_frame(&self) -> bool {
        self.window.take_geometry_dirty()
    }

    fn resize(&mut self, _size: Size<Pixels>) {
        // 窗口尺寸由系统决定（ArkUI 布局），应用侧 resize 是 no-op。
    }

    fn scale_factor(&self) -> f32 {
        self.window.scale_factor()
    }

    fn appearance(&self) -> gpui::WindowAppearance {
        self.window.state.lock().appearance
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.display.clone()
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.window.state.lock().mouse_position
    }

    fn modifiers(&self) -> Modifiers {
        self.window.state.lock().modifiers.modifiers()
    }

    fn capslock(&self) -> Capslock {
        Capslock {
            on: self.window.state.lock().modifiers.caps_lock,
        }
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        *self.input_handler.borrow_mut() = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.input_handler.borrow_mut().take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>> {
        // 鸿蒙的系统对话框（`promptAction.showDialog`）只能从 ArkTS 调用；
        // GPUI 的原生 prompt 走应用内 UI。返回 None 表示本平台不提供原生实现。
        None
    }

    fn activate(&self) {
        self.window.set_active(true);
    }

    fn is_active(&self) -> bool {
        self.window.active.load(Ordering::Relaxed)
    }

    fn visibility(&self) -> WindowVisibility {
        if self.is_active() {
            WindowVisibility::Visible
        } else {
            WindowVisibility::Hidden
        }
    }

    fn is_hovered(&self) -> bool {
        self.window.state.lock().hovered
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }

    fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
        log::debug!("OhosPlatformWindow::set_title({title})");
    }

    fn set_background_appearance(&self, background: WindowBackgroundAppearance) {
        let transparent = background == WindowBackgroundAppearance::Transparent;
        let mut state = self.window.state.lock();
        if state.transparent == transparent {
            return;
        }
        state.transparent = transparent;
        if let Some(renderer) = state.renderer.as_mut() {
            renderer.update_transparency(transparent);
        }
    }

    fn minimize(&self) {
        // 最小化由 ArkUI 的窗口按钮/手势完成；native 侧没有对应 API。
    }

    fn zoom(&self) {}

    fn toggle_fullscreen(&self) {}

    fn is_fullscreen(&self) -> bool {
        true
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        let input_handler = Rc::clone(&self.input_handler);
        let input_callback = Arc::clone(&self.ime_input_callback);
        let mut callback = callback;
        let wrapped: Box<dyn FnMut(RequestFrameOptions)> = Box::new(move |options| {
            // 先应用排队中的 IME 文本，再让 GPUI 布局这一帧。
            crate::text_input::drain(&input_handler, &mut **input_callback.lock());
            callback(options);
        });
        // 渲染线程唯一线程访问 GPUI；回调来自 `PlatformWindow`（!Send），
        // 存储到窗口时需要 Send。与 Android 后端同样的约束与理由。
        let send_callback: Box<dyn FnMut(RequestFrameOptions) + Send> =
            unsafe { std::mem::transmute(wrapped) };
        self.window.state.lock().request_frame_callback = Some(send_callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        let input_handler = Rc::clone(&self.input_handler);
        let mut callback = callback;
        let wrapped: Box<dyn FnMut(PlatformInput) -> DispatchEventResult> = Box::new(move |event| {
            if matches!(&event, PlatformInput::Touch(touch) if touch.phase == TouchPhase::Started) {
                // 在点击改变焦点前应用最后的 IME 更新。
                crate::text_input::drain(&input_handler, &mut callback);
                crate::text_input::finish_composition(&input_handler);
            }
            callback(event)
        });
        let send_callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send> =
            unsafe { std::mem::transmute(wrapped) };
        *self.ime_input_callback.lock() = send_callback;
        let shared = Arc::clone(&self.ime_input_callback);
        let dispatch: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send> =
            Box::new(move |event| shared.lock()(event));
        self.window.set_input_callback(dispatch);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        let callback: Box<dyn FnMut(bool) + Send> = unsafe { std::mem::transmute(callback) };
        self.window.state.lock().active_status_callback = Some(callback);
    }

    fn on_visibility_change(&self, callback: Box<dyn FnMut(WindowVisibility)>) {
        let callback: Box<dyn FnMut(WindowVisibility) + Send> =
            unsafe { std::mem::transmute(callback) };
        self.window.state.lock().visibility_callback = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        let callback: Box<dyn FnMut(bool) + Send> = unsafe { std::mem::transmute(callback) };
        self.window.state.lock().hover_status_callback = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        let callback: Box<dyn FnMut(Size<Pixels>, f32) + Send> =
            unsafe { std::mem::transmute(callback) };
        self.window.state.lock().resize_callback = Some(callback);
    }

    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}

    fn on_should_close(&self, _callback: Box<dyn FnMut() -> bool>) {}

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {}

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        let callback: Box<dyn FnOnce() + Send> = unsafe { std::mem::transmute(callback) };
        self.window.state.lock().close_callback = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        let callback: Box<dyn FnMut() + Send> = unsafe { std::mem::transmute(callback) };
        self.window.state.lock().appearance_changed_callback = Some(callback);
    }

    fn frame_waker(&self) -> Option<Rc<dyn Fn()>> {
        Some(Rc::new(crate::frame_source::schedule_frame))
    }

    fn schedule_frame(&self) {
        crate::frame_source::schedule_frame();
    }

    fn draw(&self, scene: &gpui::Scene) {
        log::trace!(
            "OhosPlatformWindow::draw — {} quads, {} shadows",
            scene.quads.len(),
            scene.shadows.len()
        );
        self.window.draw(scene);
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.window
            .sprite_atlas()
            .unwrap_or_else(|| Arc::new(FallbackAtlas::new()))
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        self.window.supports_subpixel_aa()
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.window.gpu_specs()
    }

    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        let scale = self.window.scale_factor();
        let x = crate::vp_to_px(f32::from(bounds.origin.x), scale);
        let y = crate::vp_to_px(f32::from(bounds.origin.y), scale);
        let width = crate::vp_to_px(f32::from(bounds.size.width), scale);
        let height = crate::vp_to_px(f32::from(bounds.size.height), scale);
        // ArkTS 侧用 `inputMethod.getController().updateCursor(CursorInfo)` 定位候选框。
        crate::host::emit(
            "imeCursor",
            &format!(
                "{{\"x\":{x:.2},\"y\":{y:.2},\"width\":{width:.2},\"height\":{height:.2}}}"
            ),
        );
    }

    fn insets(&self) -> WindowInsets {
        self.window.state.lock().window_insets()
    }

    fn on_insets_changed(&self, callback: Box<dyn FnMut(WindowInsets)>) {
        let callback: Box<dyn FnMut(WindowInsets) + Send> =
            unsafe { std::mem::transmute(callback) };
        self.window.state.lock().insets_callback = Some(callback);
    }

    fn set_back_handler(&self, callback: Box<dyn FnMut()>) {
        let callback: Box<dyn FnMut() + Send> = unsafe { std::mem::transmute(callback) };
        self.window.state.lock().back_handler = Some(callback);
    }

    fn set_back_enabled(&self, enabled: bool) {
        self.window.state.lock().back_enabled = enabled;
    }

    fn show_soft_keyboard(&self) {
        crate::napi::xcomponent::set_soft_keyboard_needed(true);
        crate::host::emit("showSoftKeyboard", "");
    }

    fn hide_soft_keyboard(&self) {
        crate::napi::xcomponent::set_soft_keyboard_needed(false);
        crate::host::emit("hideSoftKeyboard", "");
    }

    fn text_input_state_changed(&self, change: gpui::TextInputStateChange) {
        match change {
            gpui::TextInputStateChange::FocusGained => self.show_soft_keyboard(),
            gpui::TextInputStateChange::FocusLost => self.hide_soft_keyboard(),
            gpui::TextInputStateChange::SelectionChanged
            | gpui::TextInputStateChange::ContentChanged => {}
        }
    }
}

// ── 回退图集 ─────────────────────────────────────────────────────────────────

/// renderer 尚未建立时的占位图集。
///
/// GPUI 在窗口创建早期就会查询 `sprite_atlas()`；返回 `None` 会 panic。
/// 这里只分配 tile id，不持有纹理数据——第一帧之后真实 `WgpuAtlas` 生效。
struct FallbackAtlas {
    state: Mutex<FallbackAtlasState>,
}

struct FallbackAtlasState {
    next_id: u32,
    tiles: HashMap<AtlasKey, AtlasTile>,
}

impl FallbackAtlas {
    fn new() -> Self {
        Self {
            state: Mutex::new(FallbackAtlasState {
                next_id: 1,
                tiles: HashMap::new(),
            }),
        }
    }
}

impl PlatformAtlas for FallbackAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: AtlasKey,
        build: &mut dyn FnMut() -> anyhow::Result<
            Option<(Size<DevicePixels>, std::borrow::Cow<'a, [u8]>)>,
        >,
    ) -> anyhow::Result<Option<AtlasTile>> {
        let mut state = self.state.lock();
        if let Some(tile) = state.tiles.get(&key) {
            return Ok(Some(tile.clone()));
        }
        let data = build()?;
        let Some((tile_size, _pixels)) = data else {
            return Ok(None);
        };
        let id = state.next_id;
        state.next_id += 1;
        let tile = AtlasTile {
            texture_id: gpui::AtlasTextureId {
                index: 0,
                kind: gpui::AtlasTextureKind::Monochrome,
            },
            tile_id: gpui::TileId(id),
            padding: 0,
            bounds: Bounds {
                origin: point(DevicePixels(0), DevicePixels(0)),
                size: tile_size,
            },
        };
        state.tiles.insert(key, tile.clone());
        Ok(Some(tile))
    }

    fn remove(&self, key: &AtlasKey) {
        self.state.lock().tiles.remove(key);
    }
}
