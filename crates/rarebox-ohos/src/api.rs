//! ArkTS 侧的 Node-API 导出面（只在 `target_env = "ohos"` 下编译）。
//!
//! 宿主声明见 `ohos/entry/src/main/cpp/types`。命令、事件、状态全部是 JSON
//! 字符串，与桌面 C ABI、Android JNI 共用同一套协议：
//!
//! - 命令：`{"cmd": "refresh_devices"}`（`Command` 为 internally tagged）
//! - 事件：`Event::to_protocol_json`；画面帧不走这里（P1 单独二进制通道）
//! - 状态：`rarebox_ffi::state_json`（camelCase，与 Kotlin/JS 习惯一致）
//!
//! 线程模型：Node-API 调用发生在 ArkTS 线程；引擎命令只是投递（立即返回），
//! 事件由 `registry` 的转发线程消费后经 `ThreadsafeFunction` 回到 ArkTS 线程。

use std::sync::Arc;

use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::{Error, Function, Result, Status};
use napi_ohos::threadsafe_function::{ThreadsafeCallContext, ThreadsafeFunctionCallMode};

use crate::registry;

/// `version(): string` → `"rarebox-ohos 0.1.0"`。
#[napi(js_name = "version")]
pub fn version() -> String {
    crate::version_string()
}

/// `createEngine(configJson: string): number`
///
/// 空字符串表示读取本地配置（`Config::load`）；非空时按 `Config` 反序列化。
/// 返回的句柄从 1 递增，永不复用（`Engine` 建在后台线程上，本调用立即返回）。
#[napi(js_name = "createEngine")]
pub fn create_engine(config_json: String) -> f64 {
    registry::create(registry::config_from_json(&config_json)) as f64
}

/// `sendCommand(handle: number, commandJson: string): void`
///
/// 命令解析失败或引擎已停止时抛 `napi::Error`。
#[napi(js_name = "sendCommand")]
pub fn send_command(handle: f64, command_json: String) -> Result<()> {
    let handle = parse_handle(handle)?;
    let engine = registry::engine(handle).ok_or_else(|| unknown_handle(handle))?;
    let command = registry::command_from_json(&command_json)
        .map_err(|err| Error::new(Status::InvalidArg, err))?;
    registry::send_command(&engine, command)
        .map_err(|err| Error::new(Status::GenericFailure, err))
}

/// `stateJson(handle: number): string` → 最近一次发布的状态快照。
#[napi(js_name = "stateJson")]
pub fn state_json(handle: f64) -> Result<String> {
    let handle = parse_handle(handle)?;
    let engine = registry::engine(handle).ok_or_else(|| unknown_handle(handle))?;
    Ok(registry::state_json(&engine))
}

/// `disposeEngine(handle: number): void`
///
/// 幂等：未知句柄直接忽略。销毁会先请求核心引擎线程退出并等它结束，再关闭
/// 事件通道并等待转发线程退出（TSFN 随之释放），句柄摘除后重复调用安全。
#[napi(js_name = "disposeEngine")]
pub fn dispose_engine(handle: f64) {
    if let Ok(handle) = parse_handle(handle) {
        registry::dispose(handle);
    }
}

/// `setEventListener(handle: number, cb: (eventJson: string) => void): void`
///
/// 事件经 `ThreadsafeFunction` 从引擎线程回到 ArkTS 线程；画面帧不在此通道。
/// 同一引擎重复调用会替换旧回调（旧 TSFN 随之释放）。
#[napi(js_name = "setEventListener")]
pub fn set_event_listener(handle: f64, callback: Function<String, ()>) -> Result<()> {
    let handle = parse_handle(handle)?;
    let tsfn = callback
        .build_threadsafe_function()
        .build_callback(|ctx: ThreadsafeCallContext<String>| Ok(ctx.value))?;
    let sink: registry::EventSink = Arc::new(move |event: rarebox_core::Event| {
        if let Some(json) = event.to_protocol_json() {
            // NonBlocking：引擎线程不等待 ArkTS 事件循环。
            let _ = tsfn.call(json, ThreadsafeFunctionCallMode::NonBlocking);
        }
    });
    if registry::set_sink(handle, Some(sink)) {
        Ok(())
    } else {
        Err(unknown_handle(handle))
    }
}

fn parse_handle(value: f64) -> Result<u64> {
    registry::parse_handle(value).map_err(|err| Error::new(Status::InvalidArg, err))
}

fn unknown_handle(handle: u64) -> Error {
    Error::new(Status::InvalidArg, format!("unknown engine handle: {handle}"))
}
