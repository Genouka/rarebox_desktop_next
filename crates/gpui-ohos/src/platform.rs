//! `gpui::Platform` 的鸿蒙实现。
//!
//! 只做平台级的事情：显示器、字体、剪贴板、文件选择、生命周期与菜单占位。
//! 真正的窗口/输入/渲染在 [`crate::window`]，宿主驱动在 [`crate::host`]。
//!
//! # `Platform::run` 不阻塞
//!
//! GPUI 的桌面平台 `run` 会阻塞到退出；鸿蒙的渲染循环由 [`crate::host`] 自己
//! 驱动（`Application::run_embedded`），因此这里的 `run` 只调用 launch 回调
//! 并立即返回。这与 Android 的 host 模式完全一致（地图 R2/R5）。

use crate::{
    display::{DisplayList, OhosDisplay},
    dispatcher::OhosDispatcher,
    fonts,
    host::AppDirs,
    window::{OhosPlatformWindow, OhosWindow, WindowList},
    OhosBackend,
};
use anyhow::Result;
use futures::channel::oneshot;
use gpui::{
    Action, ActivityGuard, AnyWindowHandle, AppLifecyclePhase, BackgroundExecutor, ClipboardItem,
    CursorStyle, ForegroundExecutor, GestureTuning, Keymap, Menu, MenuItem, PathPromptOptions,
    Platform, PlatformDisplay, PlatformGestures, PlatformKeyboardLayout, PlatformKeyboardMapper,
    PlatformTextSystem, PlatformWindow, ScrollPhysics, Task, ThermalState, WindowAppearance,
    WindowParams,
};
use gpui_wgpu::{CosmicTextSystem, GpuContext};
use parking_lot::Mutex;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// 平台状态。只在渲染线程访问；`Arc` 是为了让 [`SharedPlatform`] 与 host 共享。
struct OhosPlatformState {
    dispatcher: Arc<OhosDispatcher>,
    gpu_context: GpuContext,
    windows: WindowList,
    displays: DisplayList,
    text_system: Arc<CosmicTextSystem>,

    clipboard: Option<String>,
    credentials: HashMap<(String, String), Vec<u8>>,

    finish_launching: Option<Box<dyn FnOnce() + Send>>,
    quit_callback: Option<Box<dyn FnMut() -> bool + Send>>,
    reopen_callback: Option<Box<dyn FnMut() + Send>>,
    open_urls_callback: Option<Box<dyn FnMut(Vec<String>) + Send>>,
    keyboard_layout_callback: Option<Box<dyn FnMut() + Send>>,
    lifecycle_callback: Option<Box<dyn FnMut(AppLifecyclePhase) + Send>>,
    memory_warning_callback: Option<Box<dyn FnMut() + Send>>,
    thermal_callback: Option<Box<dyn FnMut() + Send>>,

    is_active: bool,
    density: f32,
    dirs: AppDirs,
    backend: OhosBackend,
}

// SAFETY: `GpuContext = Rc<RefCell<Option<WgpuContext>>>` 是 `!Send`，但整个
// `OhosPlatformState` 只在渲染线程（dispatcher 主线程）上被访问；`Arc` 只是为了
// 让 `SharedPlatform` 与 host 共享同一份状态。这与 Android 后端的做法一致。
unsafe impl Send for OhosPlatformState {}

/// GPUI 的鸿蒙平台对象。
pub struct OhosPlatform {
    state: Mutex<OhosPlatformState>,
    should_quit: AtomicBool,
}

impl OhosPlatform {
    /// 构造平台并装载系统字体。调用线程必须是渲染线程。
    pub fn new(
        dispatcher: Arc<OhosDispatcher>,
        density: f32,
        width_px: u32,
        height_px: u32,
        dirs: AppDirs,
    ) -> Arc<Self> {
        crate::init_logger();
        let density = if density.is_finite() && density > 0.0 {
            density
        } else {
            1.0
        };

        // `new_without_system_fonts`：cosmic-text 的 FontSystem::new() 在 OHOS 上
        // 扫描不到字体目录（地图 §5.3 第 1 条），字体由 `fonts` 模块显式装载。
        let text_system = Arc::new(CosmicTextSystem::new_without_system_fonts(fonts::UI_FONT));
        let report = fonts::load_system_fonts(
            &*text_system,
            Path::new(fonts::SYSTEM_FONT_DIR),
        );
        log::info!(
            "OhosPlatform: loaded {} system fonts ({} missing, {} failed)",
            report.loaded.len(),
            report.missing.len(),
            report.failed.len()
        );

        let display = OhosDisplay::new(width_px.max(1), height_px.max(1), density, 1);
        log::info!("OhosPlatform::new — {display}");

        Arc::new(Self {
            state: Mutex::new(OhosPlatformState {
                dispatcher,
                gpu_context: Rc::new(RefCell::new(None)),
                windows: WindowList::default(),
                displays: DisplayList::single(display),
                text_system,
                clipboard: None,
                credentials: HashMap::new(),
                finish_launching: None,
                quit_callback: None,
                reopen_callback: None,
                open_urls_callback: None,
                keyboard_layout_callback: None,
                lifecycle_callback: None,
                memory_warning_callback: None,
                thermal_callback: None,
                is_active: true,
                density,
                dirs,
                backend: OhosBackend::Gles,
            }),
            should_quit: AtomicBool::new(false),
        })
    }

    // ── 宿主查询 ─────────────────────────────────────────────────────────────

    pub fn dispatcher(&self) -> Arc<OhosDispatcher> {
        Arc::clone(&self.state.lock().dispatcher)
    }

    pub fn gpu_context(&self) -> GpuContext {
        Rc::clone(&self.state.lock().gpu_context)
    }

    pub fn text_system(&self) -> Arc<CosmicTextSystem> {
        Arc::clone(&self.state.lock().text_system)
    }

    pub fn density(&self) -> f32 {
        self.state.lock().density
    }

    pub fn dirs(&self) -> AppDirs {
        self.state.lock().dirs.clone()
    }

    pub fn backend(&self) -> OhosBackend {
        self.state.lock().backend
    }

    pub fn is_active(&self) -> bool {
        self.state.lock().is_active
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit.load(Ordering::Relaxed)
    }

    /// 请求退出（渲染循环下一轮检查）。
    pub fn request_quit(&self) {
        let mut callback = self.state.lock().quit_callback.take();
        let can_quit = callback.as_mut().is_none_or(|callback| callback());
        if let Some(callback) = callback {
            self.state.lock().quit_callback = Some(callback);
        }
        if can_quit {
            self.should_quit.store(true, Ordering::SeqCst);
            self.state.lock().dispatcher.wake();
        }
    }

    // ── 窗口/显示器 ──────────────────────────────────────────────────────────

    /// 用新 surface 创建主窗口。
    pub fn open_window(
        &self,
        native_window: crate::surface::NativeWindow,
        width_px: u32,
        height_px: u32,
        scale_factor: f32,
    ) -> Result<Arc<OhosWindow>> {
        let mut state = self.state.lock();
        let window = OhosWindow::new(
            native_window,
            Rc::clone(&state.gpu_context),
            width_px,
            height_px,
            scale_factor,
        )?;
        state.windows.push(Arc::clone(&window));
        log::info!(
            "OhosPlatform::open_window — id={:#x} {}×{}px",
            window.id(),
            width_px,
            height_px
        );
        Ok(window)
    }

    pub fn primary_window(&self) -> Option<Arc<OhosWindow>> {
        self.state.lock().windows.primary().cloned()
    }

    pub fn window_count(&self) -> usize {
        self.state.lock().windows.len()
    }

    /// 更新主显示器尺寸/密度（surface 或 windowSizeChange 驱动）。
    pub fn set_display_size(&self, width_px: u32, height_px: u32, density: f32) {
        let mut state = self.state.lock();
        let density = if density.is_finite() && density > 0.0 {
            density
        } else {
            state.density
        };
        state.density = density;
        if state
            .displays
            .update_primary(width_px, height_px, density)
        {
            log::debug!(
                "display updated: {}×{}px @ {density:.2}",
                width_px,
                height_px
            );
        }
    }

    // ── 生命周期 ─────────────────────────────────────────────────────────────

    /// ArkTS `lifecycle(state)` → GPUI 的 [`AppLifecyclePhase`]。
    pub fn handle_lifecycle(&self, phase: AppLifecyclePhase) {
        {
            let mut state = self.state.lock();
            state.is_active = matches!(
                phase,
                AppLifecyclePhase::Active | AppLifecyclePhase::Foreground
            );
        }
        if let Some(window) = self.primary_window() {
            window.set_active(matches!(
                phase,
                AppLifecyclePhase::Active | AppLifecyclePhase::Foreground
            ));
        }

        let callback = {
            let mut state = self.state.lock();
            state.lifecycle_callback.take()
        };
        if let Some(mut callback) = callback {
            callback(phase);
            let mut state = self.state.lock();
            if state.lifecycle_callback.is_none() {
                state.lifecycle_callback = Some(callback);
            }
        }
        log::info!("lifecycle: {phase:?}");
    }

    /// 内存压力（ArkTS `on('memoryLevel', ...)`）→ GPUI 回调。
    pub fn handle_memory_warning(&self) {
        let callback = {
            let mut state = self.state.lock();
            state.memory_warning_callback.take()
        };
        if let Some(mut callback) = callback {
            callback();
            let mut state = self.state.lock();
            if state.memory_warning_callback.is_none() {
                state.memory_warning_callback = Some(callback);
            }
        }
    }

    /// 交付 URL（`openUrl` 事件的回程）。
    pub fn deliver_open_urls(&self, urls: Vec<String>) {
        let callback = {
            let mut state = self.state.lock();
            state.open_urls_callback.take()
        };
        if let Some(mut callback) = callback {
            callback(urls);
            let mut state = self.state.lock();
            if state.open_urls_callback.is_none() {
                state.open_urls_callback = Some(callback);
            }
        }
    }

    pub fn deliver_reopen(&self) {
        let callback = {
            let mut state = self.state.lock();
            state.reopen_callback.take()
        };
        if let Some(mut callback) = callback {
            callback();
            let mut state = self.state.lock();
            if state.reopen_callback.is_none() {
                state.reopen_callback = Some(callback);
            }
        }
    }

    /// 每轮渲染循环调用：派发到期后台任务。
    pub fn tick(&self) {
        let dispatcher = self.state.lock().dispatcher.clone();
        dispatcher.tick();
    }

    pub fn next_delayed_due(&self) -> Option<std::time::Instant> {
        self.state.lock().dispatcher.next_delayed_due()
    }

    /// 剪贴板写入时同步一份影子副本（宿主可经 `setClipboard` 回灌系统内容）。
    pub fn set_clipboard_text(&self, text: Option<String>) {
        self.state.lock().clipboard = text;
    }
}

/// Android 后端的同名常量：`OverScroller` 的摩擦曲线，逻辑像素按 160dpi 配对。
struct OhosGestures;

impl PlatformGestures for OhosGestures {
    fn tuning(&self) -> GestureTuning {
        GestureTuning {
            scroll_physics: ScrollPhysics::android(),
            ..GestureTuning::default()
        }
    }
}

// ── impl Platform ────────────────────────────────────────────────────────────

impl Platform for OhosPlatform {
    fn gestures(&self) -> Option<Rc<dyn PlatformGestures>> {
        Some(Rc::new(OhosGestures))
    }

    fn background_executor(&self) -> BackgroundExecutor {
        let dispatcher: Arc<dyn gpui::PlatformDispatcher> = self.state.lock().dispatcher.clone();
        BackgroundExecutor::new(dispatcher)
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        let dispatcher: Arc<dyn gpui::PlatformDispatcher> = self.state.lock().dispatcher.clone();
        ForegroundExecutor::new(dispatcher)
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.state.lock().text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        // 渲染循环由 `host` 驱动；`run_embedded` 期望这里立即调用 launch 并返回。
        self.state.lock().finish_launching = Some(unsafe {
            // SAFETY: 调用发生在渲染线程（dispatcher 主线程）上，与 GPUI 的其余状态同线程。
            std::mem::transmute::<Box<dyn FnOnce()>, Box<dyn FnOnce() + Send>>(on_finish_launching)
        });
        let callback = self.state.lock().finish_launching.take();
        if let Some(callback) = callback {
            callback();
        }
    }

    fn quit(&self) {
        self.request_quit();
    }

    fn restart(&self, _binary_path: Option<PathBuf>, _arguments: Vec<std::ffi::OsString>) {
        log::warn!("OhosPlatform::restart — HarmonyOS cannot re-exec the app");
    }

    fn activate(&self, _ignoring_other_apps: bool) {
        self.state.lock().is_active = true;
    }

    fn hide(&self) {}

    fn hide_other_apps(&self) {}

    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        self.state
            .lock()
            .displays
            .all()
            .iter()
            .map(|display| Rc::new(display.clone()) as Rc<dyn PlatformDisplay>)
            .collect()
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.state
            .lock()
            .displays
            .primary()
            .map(|display| Rc::new(display.clone()) as Rc<dyn PlatformDisplay>)
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        // 平台层没有 GPUI 的 entity id；窗口栈由 App 自己维护。
        None
    }

    fn open_window(
        &self,
        _handle: AnyWindowHandle,
        _options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        // 原生窗口由 XComponent 的 surface 回调创建；这里只把已有窗口包装给 GPUI（R5）。
        let window = self.primary_window().ok_or_else(|| {
            anyhow::anyhow!(
                "OhosPlatform::open_window: no XComponent surface yet; \
                 cx.open_window must run after `gpui_ohos` emits \"ready\""
            )
        })?;
        let display = self
            .state
            .lock()
            .displays
            .primary()
            .map(|display| Rc::new(display.clone()) as Rc<dyn PlatformDisplay>);
        Ok(Box::new(OhosPlatformWindow::new(window, display)))
    }

    fn window_appearance(&self) -> WindowAppearance {
        // RareBox 是深色主题；ArkTS 侧若检测到系统浅色可在后续版本经 NAPI 推送。
        WindowAppearance::Dark
    }

    fn open_url(&self, url: &str) {
        crate::host::emit("openUrl", &serde_json::json!({ "url": url }).to_string());
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        let callback: Box<dyn FnMut(Vec<String>) + Send> = unsafe { std::mem::transmute(callback) };
        self.state.lock().open_urls_callback = Some(callback);
    }

    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        // URL scheme 在鸿蒙上由 `module.json5` 的 `skills` 静态声明，运行时无事可做。
        Task::ready(Ok(()))
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        crate::document_picker::prompt(options)
    }

    fn prompt_for_new_path(
        &self,
        _directory: &Path,
        _suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Ok(None));
        receiver
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }

    fn reveal_path(&self, path: &Path) {
        log::info!("OhosPlatform::reveal_path({}) — not supported", path.display());
    }

    fn open_with_system(&self, path: &Path) {
        crate::host::emit(
            "openWithSystem",
            &serde_json::json!({ "path": path.to_string_lossy() }).to_string(),
        );
    }

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        let callback: Box<dyn FnMut() -> bool + Send> = unsafe { std::mem::transmute(callback) };
        self.state.lock().quit_callback = Some(callback);
    }

    fn on_system_wake(&self, _callback: Box<dyn FnMut()>) {}

    fn on_system_sleep(&self, _callback: Box<dyn FnMut()>) {}

    fn on_app_lifecycle(&self, callback: Box<dyn FnMut(AppLifecyclePhase)>) {
        let callback: Box<dyn FnMut(AppLifecyclePhase) + Send> =
            unsafe { std::mem::transmute(callback) };
        self.state.lock().lifecycle_callback = Some(callback);
    }

    fn on_memory_warning(&self, callback: Box<dyn FnMut()>) {
        let callback: Box<dyn FnMut() + Send> = unsafe { std::mem::transmute(callback) };
        self.state.lock().memory_warning_callback = Some(callback);
    }

    fn hide_cursor_until_mouse_moves(&self) {
        // 光标由系统合成；native 侧无法隐藏。
    }

    fn is_cursor_visible(&self) -> bool {
        // 2in1 设备上系统指针始终可见；触屏上无意义但返回 true 不会造成绘制错误。
        true
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        let callback: Box<dyn FnMut() + Send> = unsafe { std::mem::transmute(callback) };
        self.state.lock().reopen_callback = Some(callback);
    }

    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {}

    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}

    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn Action)>) {}

    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}

    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {}

    fn thermal_state(&self) -> ThermalState {
        ThermalState::Nominal
    }

    fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) {
        // OHOS NDK 没有热状态订阅 API；ArkTS 的 `@ohos.thermal` 目前也不下发事件。
        // 保存回调，等宿主后续通过自定义 NAPI 推送（不留假事件）。
        let callback: Box<dyn FnMut() + Send> = unsafe { std::mem::transmute(callback) };
        self.state.lock().thermal_callback = Some(callback);
        log::debug!("thermal state callback stored (no OHOS native subscription available)");
    }

    fn prevent_idle_sleep(&self, reason: &str) -> Task<Result<ActivityGuard>> {
        // ArkTS 的 `window.setWindowKeepScreenOn(true)` 才能阻止息屏，native 侧没有 API。
        Task::ready(Err(anyhow::anyhow!(
            "idle sleep prevention for {reason:?} requires the ArkTS window API on HarmonyOS"
        )))
    }

    fn app_path(&self) -> Result<PathBuf> {
        Ok(self.dirs().files)
    }

    fn path_for_auxiliary_executable(&self, _name: &str) -> Result<PathBuf> {
        anyhow::bail!("auxiliary executables are not supported on HarmonyOS")
    }

    fn set_cursor_style(&self, _style: CursorStyle) {}

    fn should_auto_hide_scrollbars(&self) -> bool {
        true
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.state
            .lock()
            .clipboard
            .clone()
            .map(ClipboardItem::new_string)
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        let text = item.text().unwrap_or_default();
        self.state.lock().clipboard = Some(text.clone());
        // 宿主页面监听该事件并调用 `pasteboard.getSystemPasteboard().setData(...)`。
        crate::host::emit(
            "clipboardWrite",
            &serde_json::json!({ "text": text }).to_string(),
        );
    }

    // OHOS 的 target_os 仍是 linux，因此 `Platform` 把 X11/Wayland 的 primary
    // selection 方法列为必需；鸿蒙没有 primary selection，按普通剪贴板处理。
    fn read_from_primary(&self) -> Option<ClipboardItem> {
        self.read_from_clipboard()
    }

    fn write_to_primary(&self, item: ClipboardItem) {
        self.write_to_clipboard(item)
    }

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        self.state.lock().credentials.insert(
            (url.to_string(), username.to_string()),
            password.to_vec(),
        );
        Task::ready(Ok(()))
    }

    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        let state = self.state.lock();
        let found = state
            .credentials
            .iter()
            .find(|((service, _), _)| service == url)
            .map(|((_, username), password)| (username.clone(), password.clone()));
        Task::ready(Ok(found))
    }

    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        self.state
            .lock()
            .credentials
            .retain(|(service, _), _| service != url);
        Task::ready(Ok(()))
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(crate::keyboard::OhosKeyboardLayout::new("en-US"))
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(gpui::DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        let callback: Box<dyn FnMut() + Send> = unsafe { std::mem::transmute(callback) };
        self.state.lock().keyboard_layout_callback = Some(callback);
    }
}

// ── SharedPlatform ───────────────────────────────────────────────────────────

/// 把 `Arc<OhosPlatform>` 适配成 GPUI 需要的 `Rc<dyn Platform>`。
///
/// GPUI 的 `Application::with_platform` 要 `Rc`，而 host 需要在别的时刻
/// （attach_surface、lifecycle）继续引用同一平台，所以用 `Arc` 持有并在
/// `Rc` 外壳里委托。
pub struct SharedPlatform(pub Arc<OhosPlatform>);

impl SharedPlatform {
    pub fn new(inner: Arc<OhosPlatform>) -> Self {
        Self(inner)
    }

    pub fn into_rc(self) -> Rc<dyn Platform> {
        Rc::new(self)
    }
}

impl Platform for SharedPlatform {
    fn gestures(&self) -> Option<Rc<dyn PlatformGestures>> {
        <OhosPlatform as Platform>::gestures(&self.0)
    }
    fn background_executor(&self) -> BackgroundExecutor {
        <OhosPlatform as Platform>::background_executor(&self.0)
    }
    fn foreground_executor(&self) -> ForegroundExecutor {
        <OhosPlatform as Platform>::foreground_executor(&self.0)
    }
    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        <OhosPlatform as Platform>::text_system(&self.0)
    }
    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        <OhosPlatform as Platform>::run(&self.0, on_finish_launching)
    }
    fn quit(&self) {
        <OhosPlatform as Platform>::quit(&self.0)
    }
    fn restart(&self, binary_path: Option<PathBuf>, arguments: Vec<std::ffi::OsString>) {
        <OhosPlatform as Platform>::restart(&self.0, binary_path, arguments)
    }
    fn activate(&self, ignoring_other_apps: bool) {
        <OhosPlatform as Platform>::activate(&self.0, ignoring_other_apps)
    }
    fn hide(&self) {
        <OhosPlatform as Platform>::hide(&self.0)
    }
    fn hide_other_apps(&self) {
        <OhosPlatform as Platform>::hide_other_apps(&self.0)
    }
    fn unhide_other_apps(&self) {
        <OhosPlatform as Platform>::unhide_other_apps(&self.0)
    }
    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        <OhosPlatform as Platform>::displays(&self.0)
    }
    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        <OhosPlatform as Platform>::primary_display(&self.0)
    }
    fn active_window(&self) -> Option<AnyWindowHandle> {
        <OhosPlatform as Platform>::active_window(&self.0)
    }
    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        <OhosPlatform as Platform>::open_window(&self.0, handle, options)
    }
    fn window_appearance(&self) -> WindowAppearance {
        <OhosPlatform as Platform>::window_appearance(&self.0)
    }
    fn open_url(&self, url: &str) {
        <OhosPlatform as Platform>::open_url(&self.0, url)
    }
    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        <OhosPlatform as Platform>::on_open_urls(&self.0, callback)
    }
    fn register_url_scheme(&self, url: &str) -> Task<Result<()>> {
        <OhosPlatform as Platform>::register_url_scheme(&self.0, url)
    }
    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        <OhosPlatform as Platform>::prompt_for_paths(&self.0, options)
    }
    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        <OhosPlatform as Platform>::prompt_for_new_path(&self.0, directory, suggested_name)
    }
    fn can_select_mixed_files_and_dirs(&self) -> bool {
        <OhosPlatform as Platform>::can_select_mixed_files_and_dirs(&self.0)
    }
    fn reveal_path(&self, path: &Path) {
        <OhosPlatform as Platform>::reveal_path(&self.0, path)
    }
    fn open_with_system(&self, path: &Path) {
        <OhosPlatform as Platform>::open_with_system(&self.0, path)
    }
    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        <OhosPlatform as Platform>::on_quit(&self.0, callback)
    }
    fn on_system_wake(&self, callback: Box<dyn FnMut()>) {
        <OhosPlatform as Platform>::on_system_wake(&self.0, callback)
    }
    fn on_system_sleep(&self, callback: Box<dyn FnMut()>) {
        <OhosPlatform as Platform>::on_system_sleep(&self.0, callback)
    }
    fn on_app_lifecycle(&self, callback: Box<dyn FnMut(AppLifecyclePhase)>) {
        <OhosPlatform as Platform>::on_app_lifecycle(&self.0, callback)
    }
    fn on_memory_warning(&self, callback: Box<dyn FnMut()>) {
        <OhosPlatform as Platform>::on_memory_warning(&self.0, callback)
    }
    fn hide_cursor_until_mouse_moves(&self) {
        <OhosPlatform as Platform>::hide_cursor_until_mouse_moves(&self.0)
    }
    fn is_cursor_visible(&self) -> bool {
        <OhosPlatform as Platform>::is_cursor_visible(&self.0)
    }
    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        <OhosPlatform as Platform>::on_reopen(&self.0, callback)
    }
    fn set_menus(&self, menus: Vec<Menu>, keymap: &Keymap) {
        <OhosPlatform as Platform>::set_menus(&self.0, menus, keymap)
    }
    fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap) {
        <OhosPlatform as Platform>::set_dock_menu(&self.0, menu, keymap)
    }
    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        <OhosPlatform as Platform>::on_app_menu_action(&self.0, callback)
    }
    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        <OhosPlatform as Platform>::on_will_open_app_menu(&self.0, callback)
    }
    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        <OhosPlatform as Platform>::on_validate_app_menu_command(&self.0, callback)
    }
    fn thermal_state(&self) -> ThermalState {
        <OhosPlatform as Platform>::thermal_state(&self.0)
    }
    fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) {
        <OhosPlatform as Platform>::on_thermal_state_change(&self.0, callback)
    }
    fn prevent_idle_sleep(&self, reason: &str) -> Task<Result<ActivityGuard>> {
        <OhosPlatform as Platform>::prevent_idle_sleep(&self.0, reason)
    }
    fn app_path(&self) -> Result<PathBuf> {
        <OhosPlatform as Platform>::app_path(&self.0)
    }
    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        <OhosPlatform as Platform>::path_for_auxiliary_executable(&self.0, name)
    }
    fn set_cursor_style(&self, style: CursorStyle) {
        <OhosPlatform as Platform>::set_cursor_style(&self.0, style)
    }
    fn should_auto_hide_scrollbars(&self) -> bool {
        <OhosPlatform as Platform>::should_auto_hide_scrollbars(&self.0)
    }
    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        <OhosPlatform as Platform>::read_from_clipboard(&self.0)
    }
    fn write_to_clipboard(&self, item: ClipboardItem) {
        <OhosPlatform as Platform>::write_to_clipboard(&self.0, item)
    }
    fn read_from_primary(&self) -> Option<ClipboardItem> {
        <OhosPlatform as Platform>::read_from_primary(&self.0)
    }
    fn write_to_primary(&self, item: ClipboardItem) {
        <OhosPlatform as Platform>::write_to_primary(&self.0, item)
    }
    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        <OhosPlatform as Platform>::write_credentials(&self.0, url, username, password)
    }
    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        <OhosPlatform as Platform>::read_credentials(&self.0, url)
    }
    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        <OhosPlatform as Platform>::delete_credentials(&self.0, url)
    }
    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        <OhosPlatform as Platform>::keyboard_layout(&self.0)
    }
    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        <OhosPlatform as Platform>::keyboard_mapper(&self.0)
    }
    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        <OhosPlatform as Platform>::on_keyboard_layout_change(&self.0, callback)
    }
}
