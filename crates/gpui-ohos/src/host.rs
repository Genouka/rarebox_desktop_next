//! 宿主驱动入口：渲染线程 + 命令队列。
//!
//! ArkTS 侧的 Node-API 函数（见 [`crate::napi`]）只做三件事：解析 JSON、
//! 把命令塞进 [`COMMANDS`]、唤醒渲染线程。所有 GPUI 状态都只在渲染线程上访问。
//!
//! # 线程模型
//!
//! ```text
//! ArkTS/UI 线程                       渲染线程 "gpui-ohos-main"
//!   start(options) ─────────────────► 创建平台 / dispatcher / vsync
//!   attach_surface(id, density) ────► 首个 surface：建窗口 + run_embedded
//!   detach_surface() ─────── 阻塞 ──► term_surface() 完成后唤醒
//!   touch/mouse/key/ime ────────────► PlatformInput / IME 队列
//!   avoid_area/keyboard_height ─────► WindowInsets
//!   lifecycle(state) ───────────────► AppLifecyclePhase + set_active
//!   on_event(cb) ◄────────────────── ThreadsafeFunction（NonBlocking）
//! ```
//!
//! 与 Android 的 `android/host.rs` 相同：渲染线程是进程级、跨 surface 存活的，
//! `Platform::run` 立即返回（`Application::run_embedded` 形态），循环由本模块驱动。
//! 这样 Activity（鸿蒙上是 ability/window stage）重建时 GPUI 的视图树、
//! 字体图集与 `App` 都保留（风险 R2/R3）。
//!
//! # surface 生命周期（风险 R10）
//!
//! `OHNativeWindow*` 由系统持有，只在 `OnSurfaceCreated` 到 `OnSurfaceDestroyed`
//! 之间有效。本模块从不保存裸指针超过这个区间：
//!
//! * `surface_attached` 把指针交给渲染线程，渲染线程把它存进 `OhosWindow`；
//! * `detach_surface` **阻塞**到渲染线程执行完 `term_surface()`（放弃 renderer
//!   对 surface 的引用）才返回，宿主才可以把控制权还给 ArkUI。

use crate::{
    surface::{InsetsPx, NativeWindow},
    text_input::ImeEvent,
    window::{KeyInput, MouseInput, TouchInput},
};
use gpui::{App, AppContext, AppLifecyclePhase, Application, ApplicationHandle};
use parking_lot::{Condvar, Mutex};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

// ── 启动参数 ─────────────────────────────────────────────────────────────────

/// `gpui_ohos.start(optionsJson)` 的载荷。
///
/// 字段名与 ArkTS 侧一致（camelCase）。目录字段缺省时退回 `$HOME`，便于在
/// 预览器/单元环境里跑起来。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOptions {
    #[serde(default)]
    pub data_dir: Option<String>,
    #[serde(default)]
    pub cache_dir: Option<String>,
    #[serde(default)]
    pub files_dir: Option<String>,
    /// `display.getDefaultDisplaySync().densityPixels`；vp → px 的唯一来源。
    #[serde(default = "default_density")]
    pub density: f32,
    /// 系统字体缩放（ArkTS `configuration.fontScale`），仅用于日志/后续扩展。
    #[serde(default = "default_font_scale")]
    pub font_scale: f32,
    /// 启动时的窗口物理像素尺寸；surface 回调到达后以真实尺寸覆盖。
    #[serde(default)]
    pub width_px: u32,
    #[serde(default)]
    pub height_px: u32,
    /// `window.getWindowProperties().id`，仅用于 `OH_NativeVSync_Create_ForAssociatedWindow`
    /// 的后续接线；当前 vsync 用无窗口版本。
    #[serde(default)]
    pub window_id: u64,
}

fn default_density() -> f32 {
    1.0
}

fn default_font_scale() -> f32 {
    1.0
}

impl Default for StartOptions {
    fn default() -> Self {
        Self {
            data_dir: None,
            cache_dir: None,
            files_dir: None,
            density: default_density(),
            font_scale: default_font_scale(),
            width_px: 0,
            height_px: 0,
            window_id: 0,
        }
    }
}

impl StartOptions {
    /// 数据/缓存/文件目录；缺省落到 `$HOME` 或进程当前目录。
    pub fn dirs(&self) -> AppDirs {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let resolve = |value: &Option<String>, fallback: PathBuf| {
            value
                .as_deref()
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .unwrap_or(fallback)
        };
        AppDirs {
            data: resolve(&self.data_dir, home.join(".rarebox")),
            cache: resolve(&self.cache_dir, home.join(".cache/rarebox")),
            files: resolve(&self.files_dir, home.clone()),
        }
    }
}

/// 应用数据目录（`Platform::app_path` 也返回 `files`）。
#[derive(Clone, Debug)]
pub struct AppDirs {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub files: PathBuf,
}

// ── 全局状态 ─────────────────────────────────────────────────────────────────

type Launch = Box<dyn FnOnce(&mut App) + Send + 'static>;
type Configure = Box<dyn FnOnce(Application) -> Application + Send + 'static>;
type EventSink = Arc<dyn Fn(String) + Send + Sync>;

/// 从 ArkTS 线程投递的命令，渲染线程每轮 drain 一次。
enum Command {
    Attach {
        window: NativeWindow,
        width_px: u32,
        height_px: u32,
        density: f32,
    },
    Detach {
        ack: Ack,
    },
    Resize {
        width_px: u32,
        height_px: u32,
        density: f32,
    },
    Touch(TouchInput),
    Mouse(MouseInput),
    Key(KeyInput),
    Ime(ImeEvent),
    SafeArea(InsetsPx),
    KeyboardHeight(i32),
    Lifecycle(AppLifecyclePhase),
    Back,
    PickerResult {
        id: u64,
        paths: Vec<PathBuf>,
    },
    SetClipboard(Option<String>),
    MemoryWarning,
    OpenUrls(Vec<String>),
    Reopen,
    Quit,
}

/// 阻塞等待渲染线程完成的握手。
#[derive(Clone, Default)]
struct Ack(Arc<(Mutex<bool>, Condvar)>);

impl Ack {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(false), Condvar::new())))
    }

    fn complete(&self) {
        let (lock, cvar) = &*self.0;
        *lock.lock() = true;
        cvar.notify_all();
    }

    fn wait(&self, timeout: Duration) -> bool {
        let (lock, cvar) = &*self.0;
        let mut done = lock.lock();
        if *done {
            return true;
        }
        cvar.wait_for(&mut done, timeout);
        *done
    }
}

static COMMANDS: Mutex<Vec<Command>> = Mutex::new(Vec::new());
static OPTIONS: Mutex<Option<StartOptions>> = Mutex::new(None);
static LAUNCH: Mutex<Option<Launch>> = Mutex::new(None);
static CONFIGURE: Mutex<Option<Configure>> = Mutex::new(None);
static EVENT_SINK: Mutex<Option<EventSink>> = Mutex::new(None);
/// sink 尚未安装时缓存的事件（最多 [`PENDING_EVENT_CAP`] 条）。
///
/// ArkTS 通常在 `start()` 之后、首个 surface 之前调用 `on_event`；但顺序并不能
/// 保证，而 `"ready"` 丢一次就等于宿主永远不知道可以显示窗口（R5）。缓存少量
/// 事件并在安装 sink 时按序补发，代价只有几十字节。
static PENDING_EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
const PENDING_EVENT_CAP: usize = 64;
static DISPATCHER: Mutex<Option<Arc<crate::dispatcher::OhosDispatcher>>> = Mutex::new(None);
static PLATFORM: Mutex<Option<Arc<crate::platform::OhosPlatform>>> = Mutex::new(None);
static RENDER_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static STARTED: Mutex<bool> = Mutex::new(false);
static SHUTDOWN: Mutex<bool> = Mutex::new(false);
static SHUTDOWN_CVAR: Condvar = Condvar::new();
/// 当前已交给渲染线程的 `OHNativeWindow*`，仅用于去重（R10：不越过 detached 使用）。
static CURRENT_SURFACE: AtomicU64 = AtomicU64::new(0);
/// 最近一次 surface 的密度（`f32` bits），供 XComponent 回调换算 vp→px。
static SURFACE_DENSITY: AtomicU32 = AtomicU32::new(0);

// ── 对外入口 ─────────────────────────────────────────────────────────────────

/// 注册 GPUI 应用的启动闭包（在渲染线程、首个 surface 就绪后调用）。
///
/// 根 crate 的 `src/ohos.rs` 应在 NAPI `start` 之前调用本函数；没有注册时
/// [`default_launch`] 会打开一个空窗口，保证渲染链路可独立验证（地图 §8.5 第 2 步）。
pub fn set_launch_handler(launch: impl FnOnce(&mut App) + Send + 'static) {
    *LAUNCH.lock() = Some(Box::new(launch));
}

/// 注册 [`gpui::AssetSource`]（HAP 的 `rawfile/` 由宿主实现）。
pub fn set_asset_source(assets: impl gpui::AssetSource) {
    *CONFIGURE.lock() = Some(Box::new(move |application: Application| {
        application.with_assets(assets)
    }));
}

/// 注册 ArkTS 事件接收器。重复调用替换旧接收器。
///
/// 安装时会把 [`PENDING_EVENTS`] 里的缓存事件按序补发（都在调用者线程上执行，
/// 即 ArkTS 线程）。
pub fn set_event_sink(sink: Option<EventSink>) {
    let pending: Vec<String> = if sink.is_some() {
        std::mem::take(&mut *PENDING_EVENTS.lock())
    } else {
        Vec::new()
    };
    *EVENT_SINK.lock() = sink;
    for event in pending {
        deliver(&event);
    }
}

/// 向 ArkTS 发一个事件（`{"name": ..., "payload": ...}`）。
///
/// 任意线程可调用；接收器由 [`crate::napi::api::on_event`] 安装成
/// `ThreadsafeFunction`（NonBlocking），因此不会阻塞渲染线程。
pub fn emit(name: &str, payload_json: &str) {
    let payload = if payload_json.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(payload_json).unwrap_or(serde_json::Value::Null)
    };
    let envelope = serde_json::json!({ "name": name, "payload": payload }).to_string();
    if EVENT_SINK.lock().is_some() {
        deliver(&envelope);
        return;
    }
    let mut pending = PENDING_EVENTS.lock();
    if pending.len() >= PENDING_EVENT_CAP {
        pending.remove(0);
    }
    pending.push(envelope);
}

fn deliver(envelope: &str) {
    let sink = EVENT_SINK.lock().clone();
    if let Some(sink) = sink {
        sink(envelope.to_string());
    }
}

/// 启动渲染线程。重复调用只更新启动参数（幂等，见 Android `host::start`）。
pub fn start(options: StartOptions) {
    crate::init_logger();
    SURFACE_DENSITY.store(options.density.to_bits(), Ordering::SeqCst);
    *OPTIONS.lock() = Some(options);

    let mut started = STARTED.lock();
    if *started {
        log::info!("gpui_ohos.start: render thread already running");
        return;
    }
    *started = true;
    *SHUTDOWN.lock() = false;

    let handle = thread::Builder::new()
        .name("gpui-ohos-main".into())
        .spawn(render_thread)
        .expect("failed to spawn gpui-ohos-main");
    *RENDER_THREAD.lock() = Some(handle);
}

/// 当前渲染线程上的平台对象（NAPI 辅助函数用）。
pub fn current_platform() -> Option<Arc<crate::platform::OhosPlatform>> {
    PLATFORM.lock().clone()
}
/// 请求退出并等待渲染线程结束（最多 5 秒）。
pub fn shutdown() {
    if !is_started() {
        return;
    }
    post(Command::Quit);
    let mut done = SHUTDOWN.lock();
    if !*done {
        SHUTDOWN_CVAR.wait_for(&mut done, Duration::from_secs(5));
    }
    if !*done {
        log::warn!("gpui_ohos.shutdown: render thread did not stop within 5s");
    }
}

/// 渲染线程是否已启动。
pub fn is_started() -> bool {
    *STARTED.lock()
}

/// 当前密度（供 XComponent 回调把 vp 转成 px）。
pub fn surface_density() -> f32 {
    let bits = SURFACE_DENSITY.load(Ordering::SeqCst);
    let density = f32::from_bits(bits);
    if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    }
}

// ── surface ──────────────────────────────────────────────────────────────────

/// XComponent 的 `OnSurfaceCreated/OnSurfaceChanged` 回调把指针交到这里。
///
/// 尺寸由调用方（[`crate::napi::xcomponent`]）用
/// `OH_NativeXComponent_GetXComponentSize` 取到 vp 后换算成 px。
pub fn surface_attached(window: NativeWindow, width_px: u32, height_px: u32, density: f32) {
    let density = if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    };
    SURFACE_DENSITY.store(density.to_bits(), Ordering::SeqCst);
    post(Command::Attach {
        window,
        width_px,
        height_px,
        density,
    });
}

/// ArkTS `attach_surface(surfaceIdOrPointer, density)`。
///
/// * `surface_id_or_pointer == 0` → 使用 XComponent 回调保存的指针；
/// * 否则先按 surface id 用 `OH_NativeWindow_CreateNativeWindowFromSurfaceId` 解析，
///   失败则把它当作 `OHNativeWindow*` 指针（宿主在 C++ 层已 `unwrap` 时更方便）。
pub fn attach_surface(surface_id_or_pointer: f64, density: f32) -> anyhow::Result<()> {
    let value = if surface_id_or_pointer.is_finite() && surface_id_or_pointer > 0.0 {
        surface_id_or_pointer as u64
    } else {
        0
    };

    let window = if value == 0 {
        let pointer = CURRENT_SURFACE.load(Ordering::SeqCst) as *mut std::ffi::c_void;
        NativeWindow::new(pointer)
    } else {
        crate::surface::native_window_from_surface_id(value).or_else(|| {
            NativeWindow::new(value as *mut std::ffi::c_void)
        })
    }
    .ok_or_else(|| anyhow::anyhow!("attach_surface: no usable OHNativeWindow (id={value})"))?;

    // 尺寸优先取 native window 的缓冲几何（px）；拿不到时退回启动参数。
    let (width_px, height_px) = window
        .buffer_geometry()
        .or_else(|| {
            let options = OPTIONS.lock();
            let options = options.as_ref()?;
            (options.width_px > 0 && options.height_px > 0)
                .then_some((options.width_px, options.height_px))
        })
        .unwrap_or((1, 1));

    surface_attached(window, width_px, height_px, density);
    Ok(())
}

/// ArkTS `detach_surface()`：阻塞到渲染线程释放 surface（R10）。
pub fn detach_surface() {
    let ack = Ack::new();
    post(Command::Detach { ack: ack.clone() });
    if !ack.wait(Duration::from_secs(5)) {
        log::warn!("gpui_ohos.detach_surface: render thread did not release the surface in 5s");
    }
    CURRENT_SURFACE.store(0, Ordering::SeqCst);
}

/// ArkTS `resize(widthPx, heightPx)`。
pub fn resize(width_px: u32, height_px: u32, density: f32) {
    post(Command::Resize {
        width_px,
        height_px,
        density,
    });
}

// ── 输入 ─────────────────────────────────────────────────────────────────────

pub fn touch(input: TouchInput) {
    post(Command::Touch(input));
}

pub fn mouse(input: MouseInput) {
    post(Command::Mouse(input));
}

pub fn key(input: KeyInput) {
    post(Command::Key(input));
}

pub fn ime(event: ImeEvent) {
    post(Command::Ime(event));
}

pub fn avoid_area(insets: InsetsPx) {
    post(Command::SafeArea(insets));
}

pub fn keyboard_height(height_px: i32) {
    post(Command::KeyboardHeight(height_px));
}

pub fn lifecycle(phase: AppLifecyclePhase) {
    post(Command::Lifecycle(phase));
}

/// 系统返回手势/按键（ArkUI 侧转调）。
pub fn back() {
    post(Command::Back);
}

/// `document_picker` 的异步结果。
pub fn picker_result(id: u64, paths: Vec<PathBuf>) {
    post(Command::PickerResult { id, paths });
}

/// 系统剪贴板内容 → 后端影子副本（在渲染线程应用）。
pub fn set_clipboard(text: Option<String>) {
    post(Command::SetClipboard(text));
}

/// ArkTS 的 `memoryLevel` 回调。
pub fn memory_warning() {
    post(Command::MemoryWarning);
}

/// 外部 `openUrl` 回程。
pub fn open_urls(urls: Vec<String>) {
    post(Command::OpenUrls(urls));
}

/// ability 被重新拉到前台（非生命周期阶段变化）。
pub fn reopen() {
    post(Command::Reopen);
}

/// 唤醒渲染线程（vsync 回调、NAPI 命令、帧需求都走这里）。
pub fn wake_render_thread() {
    if let Some(dispatcher) = DISPATCHER.lock().as_ref() {
        dispatcher.wake();
    }
}

fn post(command: Command) {
    COMMANDS.lock().push(command);
    wake_render_thread();
}

// ── 渲染线程 ─────────────────────────────────────────────────────────────────

fn default_launch(app: &mut App) {
    // 没有根 crate 的启动闭包时也把渲染链路走通：打开一个空视图窗口。
    // 这正是地图 §8.5 第 2 步“屏幕上出纯色 clear”的最小宿主。
    match app.open_window(gpui::WindowOptions::default(), |_, cx| cx.new(|_| gpui::Empty)) {
        Ok(_) => log::info!("gpui-ohos: opened the default empty window"),
        Err(error) => log::error!("gpui-ohos: default open_window failed: {error:#}"),
    }
}

fn render_thread() {
    let options = OPTIONS.lock().clone().unwrap_or_default();
    let dispatcher = crate::dispatcher::OhosDispatcher::new();
    *DISPATCHER.lock() = Some(Arc::clone(&dispatcher));

    let platform = crate::platform::OhosPlatform::new(
        Arc::clone(&dispatcher),
        options.density,
        options.width_px,
        options.height_px,
        options.dirs(),
    );
    *PLATFORM.lock() = Some(Arc::clone(&platform));

    crate::frame_source::install();
    log::info!(
        "gpui-ohos: render thread up (density={}, {}×{}px)",
        options.density,
        options.width_px,
        options.height_px
    );

    let mut app: Option<ApplicationHandle> = None;

    loop {
        let commands: Vec<Command> = COMMANDS.lock().drain(..).collect();
        for command in commands {
            match command {
                Command::Attach {
                    window,
                    width_px,
                    height_px,
                    density,
                } => {
                    handle_attach(&platform, &mut app, window, width_px, height_px, density);
                }
                Command::Detach { ack } => {
                    if let Some(window) = platform.primary_window() {
                        window.term_surface();
                    }
                    CURRENT_SURFACE.store(0, Ordering::SeqCst);
                    // 渲染线程已经放弃 renderer 对 surface 的引用，宿主可以安全销毁。
                    ack.complete();
                }
                Command::Resize {
                    width_px,
                    height_px,
                    density,
                } => {
                    platform.set_display_size(width_px, height_px, density);
                    if let Some(window) = platform.primary_window() {
                        window.resize_surface(width_px, height_px, density);
                    }
                }
                Command::Touch(input) => match platform.primary_window() {
                    Some(window) => window.handle_touch(input),
                    None => log::debug!("touch dropped: no window yet"),
                },
                Command::Mouse(input) => match platform.primary_window() {
                    Some(window) => window.handle_mouse(input),
                    None => log::debug!("mouse dropped: no window yet"),
                },
                Command::Key(input) => match platform.primary_window() {
                    Some(window) => window.handle_key(input),
                    None => log::debug!("key dropped: no window yet"),
                },
                Command::Ime(event) => crate::text_input::enqueue(event),
                Command::SafeArea(insets) => {
                    if let Some(window) = platform.primary_window() {
                        window.set_safe_area(insets);
                    }
                }
                Command::KeyboardHeight(height) => {
                    if let Some(window) = platform.primary_window() {
                        window.set_keyboard_height(height);
                    }
                }
                Command::Lifecycle(phase) => platform.handle_lifecycle(phase),
                Command::Back => {
                    if let Some(window) = platform.primary_window() {
                        window.handle_back();
                    }
                }
                Command::PickerResult { id, paths } => {
                    crate::document_picker::resolve(id, paths);
                }
                Command::SetClipboard(text) => platform.set_clipboard_text(text),
                Command::MemoryWarning => platform.handle_memory_warning(),
                Command::OpenUrls(urls) => platform.deliver_open_urls(urls),
                Command::Reopen => platform.deliver_reopen(),
                Command::Quit => platform.request_quit(),
            }
        }

        // `platform.tick()` 就是 dispatcher.tick()；这里只调一次，避免重复处理延迟任务。
        dispatcher.tick();
        dispatcher.flush_main_thread_tasks();
        // 用 load 而不是 take：`request_frame` 里的包装回调要用 swap 读到同一个
        // 标记来决定 `force_render`；在循环里消费掉会让 IME 文本这一帧不被重绘。
        if crate::TEXT_INPUT_DIRTY.load(Ordering::Acquire) {
            crate::frame_source::schedule_frame();
        }
        crate::frame_source::tick();

        let window = platform.primary_window();
        let can_draw = window.as_ref().is_some_and(|window| window.can_draw());
        if can_draw
            && crate::frame_source::take_frame()
            && let Some(window) = &window
        {
            window.request_frame();
        }

        if platform.should_quit() {
            log::info!("gpui-ohos: quitting render loop");
            break;
        }

        let timeout = crate::frame_source::poll_timeout(dispatcher.next_delayed_due(), can_draw);
        dispatcher.wait_for_work(timeout);
    }

    if let Some(window) = platform.primary_window() {
        window.term_surface();
    }
    drop(app);
    crate::frame_source::uninstall();
    dispatcher.shutdown();
    *PLATFORM.lock() = None;
    *DISPATCHER.lock() = None;
    *STARTED.lock() = false;
    emit("quit", "");
    *SHUTDOWN.lock() = true;
    SHUTDOWN_CVAR.notify_all();
    log::info!("gpui-ohos: render thread stopped");
}

fn handle_attach(
    platform: &Arc<crate::platform::OhosPlatform>,
    app: &mut Option<ApplicationHandle>,
    window: NativeWindow,
    width_px: u32,
    height_px: u32,
    density: f32,
) {
    CURRENT_SURFACE.store(window.as_ptr() as u64, Ordering::SeqCst);
    platform.set_display_size(width_px, height_px, density);

    if let Some(existing) = platform.primary_window() {
        if existing.native_window_ptr() == Some(window.as_ptr()) {
            // 同一个 surface 的 `OnSurfaceChanged`：尺寸可能变了，但不要重建 surface。
            existing.resize_surface(width_px, height_px, density);
        } else if let Err(error) = existing.init_surface(window, width_px, height_px, density) {
            log::error!("gpui-ohos: re-attaching surface failed: {error:#}");
            return;
        }
        existing.set_active(true);
        return;
    }

    let window = match platform.open_window(window, width_px, height_px, density) {
        Ok(window) => window,
        Err(error) => {
            log::error!("gpui-ohos: open_window failed: {error:#}");
            return;
        }
    };
    window.set_active(true);
    emit(
        "ready",
        &serde_json::json!({
            "widthPx": width_px,
            "heightPx": height_px,
            "density": density,
        })
        .to_string(),
    );

    if app.is_none() {
        // 首个 surface 就绪后才构造 `Application`：`App::new_app` 断言主线程，
        // 而这里正是 dispatcher 的主线程（R3）；`run_embedded` 里的
        // `Platform::run` 立即调用 launch，`cx.open_window` 于是能找到已存在的窗口（R5）。
        let mut application = Application::with_platform(
            crate::platform::SharedPlatform::new(Arc::clone(platform)).into_rc(),
        );
        if let Some(configure) = CONFIGURE.lock().take() {
            application = configure(application);
        }
        let launch = LAUNCH
            .lock()
            .take()
            .unwrap_or_else(|| Box::new(default_launch) as Launch);
        *app = Some(application.run_embedded(launch));
        log::info!("gpui-ohos: application launched on first surface");
        // 首个 surface 的 swapchain 是空的；主动要一帧，避免宿主不调用任何
        // `schedule_frame` 时屏幕停在上一帧（地图 §8.5 第 2 步）。
        crate::frame_source::schedule_frame();
    }
}
