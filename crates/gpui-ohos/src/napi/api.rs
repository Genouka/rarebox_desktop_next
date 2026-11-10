//! ArkTS ⇄ Rust 的导出面。
//!
//! 全部函数名与参数形状是宿主页面的契约（地图 §4.3，名称固定）：
//!
//! ```text
//! start(optionsJson)                 shutdown()
//! attach_surface(surfaceIdOrPointer, density)   detach_surface()
//! resize(widthPx, heightPx)
//! touch(eventJson)  mouse(eventJson)  key(eventJson)  ime(actionJson)
//! avoid_area(top, bottom, left, right)            keyboard_height(px)
//! lifecycle(state)                   on_event(callback)
//! ```
//!
//! 额外的辅助导出（宿主在实现对应 ArkTS 逻辑时必需，不改变上面的约定）：
//!
//! ```text
//! resolve_picker(requestId, pathsJson)   // 文件选择结果回程
//! set_clipboard(textOrNullJson)          // 系统剪贴板 → 后端影子副本
//! back()                                 // 系统返回手势
//! memory_warning()                       // 内存压力
//! open_urls(urlsJson)                    // 外部 URL 回程
//! ```
//!
//! 事件（`on_event` 的回调）是单参数 JSON：
//! `{"name": "...", "payload": <任意 JSON>}`。

use crate::{
    host::{self, StartOptions},
    surface::InsetsPx,
    text_input::{self, ImeEvent},
    window::{KeyInput, MouseInput, TouchInput},
};
use gpui::AppLifecyclePhase;
use napi_derive_ohos::napi;
use napi_ohos::{
    bindgen_prelude::{Error, Function, Result, Status},
    threadsafe_function::{ThreadsafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use parking_lot::Mutex;

/// `start(optionsJson: string): void`
///
/// 解析 `{ dataDir, cacheDir, filesDir, density, fontScale, windowId, widthPx, heightPx }`
/// 并启动渲染线程；本调用立即返回。首个 surface 就绪后通过 `on_event` 收到 `"ready"`。
#[napi(js_name = "start")]
pub fn start(options_json: String) -> Result<()> {
    let options: StartOptions = if options_json.trim().is_empty() {
        StartOptions::default()
    } else {
        serde_json::from_str(&options_json)
            .map_err(|error| Error::new(Status::InvalidArg, format!("start(optionsJson): {error}")))?
    };
    host::start(options);
    Ok(())
}

/// `shutdown(): void` —— 请求退出并等待渲染线程结束（最多 5 秒）。
#[napi(js_name = "shutdown")]
pub fn shutdown() {
    host::shutdown();
}

/// `attach_surface(surfaceIdOrPointer: number, density: number): void`
///
/// `0` 表示使用 XComponent 回调保存的指针；否则先按 surface id 解析，
/// 失败再按指针解释。`density` 是 `display.getDefaultDisplaySync().densityPixels`。
#[napi(js_name = "attach_surface")]
pub fn attach_surface(surface_id_or_pointer: f64, density: f64) -> Result<()> {
    host::attach_surface(surface_id_or_pointer, density as f32)
        .map_err(|error| Error::new(Status::GenericFailure, format!("{error:#}")))
}

/// `detach_surface(): void` —— 阻塞到渲染线程释放 surface（R10）。
#[napi(js_name = "detach_surface")]
pub fn detach_surface() {
    host::detach_surface();
}

/// `resize(widthPx: number, heightPx: number): void`
///
/// 密度沿用最近一次 `attach_surface`/`start` 的值；ArkTS 的 `windowSizeChange`
/// 给出的是 vp，宿主需自行乘 density。
#[napi(js_name = "resize")]
pub fn resize(width_px: f64, height_px: f64) {
    host::resize(
        width_px.max(0.0) as u32,
        height_px.max(0.0) as u32,
        host::surface_density(),
    );
}

/// `touch(eventJson: string): void`
///
/// `{ id, phase: "down"|"up"|"move"|"cancel", x, y, force? }`，`x/y` 单位 **vp**。
#[napi(js_name = "touch")]
pub fn touch(event_json: String) -> Result<()> {
    let input: TouchInput = parse_json(&event_json, "touch")?;
    let density = host::surface_density();
    host::touch(TouchInput {
        x: crate::vp_to_px(input.x, density),
        y: crate::vp_to_px(input.y, density),
        ..input
    });
    Ok(())
}

/// `mouse(eventJson: string): void`
///
/// `{ action: "press"|"release"|"move"|"cancel"|"scroll"|"hover", button?, x, y,
///    modifiers?, deltaX?, deltaY? }`，`x/y/delta` 单位 **vp**。
#[napi(js_name = "mouse")]
pub fn mouse(event_json: String) -> Result<()> {
    let input: MouseInput = parse_json(&event_json, "mouse")?;
    let density = host::surface_density();
    host::mouse(MouseInput {
        x: crate::vp_to_px(input.x, density),
        y: crate::vp_to_px(input.y, density),
        delta_x: crate::vp_to_px(input.delta_x, density),
        delta_y: crate::vp_to_px(input.delta_y, density),
        ..input
    });
    Ok(())
}

/// `key(eventJson: string): void`
///
/// `{ code, action: 0|1, modifiers?, unicode?, capsLock? }`，`code` 是
/// `oh_key_code.h` 的数值（`KEYCODE_A = 2017` 等）。
#[napi(js_name = "key")]
pub fn key(event_json: String) -> Result<()> {
    let input: KeyInput = parse_json(&event_json, "key")?;
    host::key(input);
    Ok(())
}

/// `ime(actionJson: string): number`
///
/// 返回会话号（仅 `action: "begin"` 有意义；其它动作回传 0）。
///
/// ```json
/// {"action":"begin", "inputType": 0}
/// {"action":"preview", "session":1, "text":"ni", "start":0, "end":2}
/// {"action":"insert",  "session":1, "text":"你"}
/// {"action":"finishPreview", "session":1, "text":"你"}
/// {"action":"delete",  "session":1, "before":1, "after":0, "codePoints":false}
/// {"action":"done",    "session":1}
/// ```
#[napi(js_name = "ime")]
pub fn ime(action_json: String) -> Result<f64> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ImeAction {
        action: String,
        #[serde(default)]
        session: u64,
        #[serde(default)]
        text: String,
        #[serde(default)]
        start: usize,
        #[serde(default)]
        end: usize,
        #[serde(default)]
        before: usize,
        #[serde(default)]
        after: usize,
        #[serde(default)]
        code_points: bool,
        #[serde(default)]
        #[allow(dead_code)]
        input_type: u32,
    }

    let action: ImeAction = parse_json(&action_json, "ime")?;
    match action.action.as_str() {
        "begin" => return Ok(text_input::new_session() as f64),
        "preview" => host::ime(
            ImeEvent::new(action.session, text_input::IME_KIND_COMPOSE, action.text)
                .with_selection(action.start, action.end),
        ),
        "insert" => host::ime(ImeEvent::new(
            action.session,
            text_input::IME_KIND_COMMIT,
            action.text,
        )),
        "finishPreview" => {
            host::ime(ImeEvent::new(
                action.session,
                text_input::IME_KIND_COMMIT,
                action.text,
            ));
        }
        "delete" => {
            let kind = if action.code_points {
                text_input::IME_KIND_DELETE_IN_CODE_POINTS
            } else {
                text_input::IME_KIND_DELETE_SURROUNDING
            };
            host::ime(
                ImeEvent::new(action.session, kind, "").with_selection(action.before, action.after),
            );
        }
        "done" => host::ime(ImeEvent::new(action.session, text_input::IME_KIND_DONE, "")),
        "moveCursor" | "functionKey" | "selectByRange" | "handleExtendAction" => {
            // 这些动作需要 `PlatformInputHandler` 的选区/命令通道；GPUI 目前用
            // 键盘事件表达它们（moveCursor 由 ArkTS 侧翻译成方向键）。
            log::debug!("ime action `{}` has no direct GPUI mapping; ignored", action.action);
        }
        other => {
            return Err(Error::new(
                Status::InvalidArg,
                format!("ime(actionJson): unknown action `{other}`"),
            ));
        }
    }
    Ok(0.0)
}

/// `avoid_area(top, bottom, left, right): void` —— 单位 px（宿主已乘 density）。
#[napi(js_name = "avoid_area")]
pub fn avoid_area(top: f64, bottom: f64, left: f64, right: f64) {
    host::avoid_area(InsetsPx::new(
        top.max(0.0) as i32,
        right.max(0.0) as i32,
        bottom.max(0.0) as i32,
        left.max(0.0) as i32,
    ));
}

/// `keyboard_height(px: number): void` —— 软键盘遮挡高度，px，0 表示收起。
#[napi(js_name = "keyboard_height")]
pub fn keyboard_height(px: f64) {
    host::keyboard_height(px.max(0.0) as i32);
}

/// `lifecycle(state: string): void`
///
/// `"active" | "inactive" | "background" | "foreground" | "memoryWarning" |
///  "reopen"`。前四个映射到 GPUI 的 [`AppLifecyclePhase`]。
#[napi(js_name = "lifecycle")]
pub fn lifecycle(state: String) -> Result<()> {
    let phase = match state.as_str() {
        "active" => AppLifecyclePhase::Active,
        "inactive" => AppLifecyclePhase::Inactive,
        "background" => AppLifecyclePhase::Background,
        "foreground" => AppLifecyclePhase::Foreground,
        "memoryWarning" => {
            host::memory_warning();
            return Ok(());
        }
        "reopen" => {
            host::reopen();
            return Ok(());
        }
        other => {
            return Err(Error::new(
                Status::InvalidArg,
                format!("lifecycle(state): unknown state `{other}`"),
            ));
        }
    };
    host::lifecycle(phase);
    Ok(())
}

/// `on_event(callback: (eventJson: string) => void): void`
///
/// 注册（或替换）事件接收器。事件在 ArkTS 线程执行，投递模式为 NonBlocking。
#[napi(js_name = "on_event")]
pub fn on_event(callback: Function<String, ()>) -> Result<()> {
    let threadsafe = callback
        .build_threadsafe_function()
        .build_callback(|context: ThreadsafeCallContext<String>| Ok(context.value))?;

    let threadsafe = std::sync::Arc::new(threadsafe);
    let for_sink = std::sync::Arc::clone(&threadsafe);
    // 先释放旧 sink，再安装新的：旧 TSFN 的 Drop 会 abort 队列。
    host::set_event_sink(None);
    host::set_event_sink(Some(std::sync::Arc::new(move |event: String| {
        // 渲染线程不等待 ArkTS 事件循环（NonBlocking）。
        let _ = for_sink.call(event, ThreadsafeFunctionCallMode::NonBlocking);
    })));
    *SINK.lock() = Some(threadsafe);
    Ok(())
}

/// 保留一份 TSFN，避免 `on_event` 返回后句柄被回收。
static SINK: Mutex<Option<std::sync::Arc<ThreadsafeFunction<String, (), String, Status, false>>>> =
    Mutex::new(None);

// ── 额外辅助导出 ─────────────────────────────────────────────────────────────

/// `resolve_picker(requestId: number, pathsJson: string): void`
#[napi(js_name = "resolve_picker")]
pub fn resolve_picker(request_id: f64, paths_json: String) -> Result<()> {
    let paths = crate::document_picker::parse_paths(&paths_json)
        .map_err(|error| Error::new(Status::InvalidArg, format!("{error:#}")))?;
    host::picker_result(request_id.max(0.0) as u64, paths);
    Ok(())
}

/// `set_clipboard(textOrNullJson: string): void`
///
/// 系统剪贴板内容同步到后端（`read_from_clipboard` 是同步 API，无法现取）。
/// 传 `"null"` 表示清空。
#[napi(js_name = "set_clipboard")]
pub fn set_clipboard(text_or_null_json: String) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(&text_or_null_json).map_err(|error| {
        Error::new(
            Status::InvalidArg,
            format!("set_clipboard expects a JSON string or null: {error}"),
        )
    })?;
    let text = match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text),
        other => {
            return Err(Error::new(
                Status::InvalidArg,
                format!("set_clipboard expects string|null, got {other}"),
            ));
        }
    };
    host::set_clipboard(text);
    Ok(())
}

/// `back(): void` —— 系统返回手势/按键（ArkUI `onBackPress` 转调）。
#[napi(js_name = "back")]
pub fn back() {
    host::back();
}

/// `memory_warning(): void`
#[napi(js_name = "memory_warning")]
pub fn memory_warning() {
    host::memory_warning();
}

/// `open_urls(urlsJson: string): void` —— 外部 `openUrl` 回程。
#[napi(js_name = "open_urls")]
pub fn open_urls(urls_json: String) -> Result<()> {
    let urls: Vec<String> = serde_json::from_str(&urls_json)
        .map_err(|error| Error::new(Status::InvalidArg, format!("open_urls: {error}")))?;
    host::open_urls(urls);
    Ok(())
}

fn parse_json<T: serde::de::DeserializeOwned>(json: &str, export: &str) -> Result<T> {
    serde_json::from_str(json)
        .map_err(|error| Error::new(Status::InvalidArg, format!("{export}(eventJson): {error}")))
}
