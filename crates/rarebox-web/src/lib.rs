//! RareBox Web：在浏览器里通过 WebUSB 直接使用 ADB。
//!
//! 浏览器版只支持 USB 连接。无线调试需要 TCP，而浏览器无法访问设备的
//! 局域网端口；界面层会拦截无线入口并提示下载桌面客户端。
//!
//! 这里的导出函数都由 `web/app.js` 调用，返回 JSON 字符串或 `Uint8Array`。

pub mod adb;
pub mod auth;
pub mod info;

#[cfg(target_family = "wasm")]
mod session;
#[cfg(target_family = "wasm")]
mod usb;

#[cfg(target_family = "wasm")]
use std::cell::RefCell;
#[cfg(target_family = "wasm")]
use std::sync::Arc;
#[cfg(target_family = "wasm")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_family = "wasm")]
use serde::Serialize;
#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

#[cfg(target_family = "wasm")]
use session::WebSession;

#[cfg(target_family = "wasm")]
thread_local! {
    /// 会话被异步操作“取走”期间为 `None`，其它调用会得到“忙”而不是重入。
    static SESSION: RefCell<Option<WebSession>> = RefCell::new(Some(WebSession::new()));
    static LOG_STOP: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

#[cfg(target_family = "wasm")]
fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[cfg(target_family = "wasm")]
fn busy() -> JsValue {
    JsValue::from_str("另一个 USB 操作正在进行，请稍候")
}

#[cfg(target_family = "wasm")]
fn to_json(value: &impl Serialize) -> Result<String, JsValue> {
    serde_json::to_string(value).map_err(js_error)
}

/// 从 TLS 取出会话。`Drop` 时归还，保证 await 期间不持有 `RefCell` 借用。
#[cfg(target_family = "wasm")]
struct SessionGuard {
    inner: Option<WebSession>,
}

#[cfg(target_family = "wasm")]
impl SessionGuard {
    fn get(&mut self) -> &mut WebSession {
        self.inner.as_mut().expect("session guard already consumed")
    }
}

#[cfg(target_family = "wasm")]
impl Drop for SessionGuard {
    fn drop(&mut self) {
        let Some(session) = self.inner.take() else {
            return;
        };
        SESSION.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut() {
                *slot = Some(session);
            }
        });
    }
}

#[cfg(target_family = "wasm")]
fn take_session() -> Result<SessionGuard, JsValue> {
    SESSION.with(|cell| {
        let mut slot = cell.borrow_mut();
        let session = slot.take().ok_or_else(busy)?;
        Ok(SessionGuard {
            inner: Some(session),
        })
    })
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn usb_supported() -> bool {
    usb::usb_available()
}

/// 已授权给本页面的 USB 设备。不会弹选择框。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn list_authorized_devices() -> Result<String, JsValue> {
    let devices = usb::authorized_devices()
        .await
        .map_err(|err| js_error(format!("{err:#}")))?;
    let entries: Vec<_> = devices
        .iter()
        .enumerate()
        .map(|(index, (_, description))| {
            serde_json::json!({
                "index": index,
                "vendorId": description.vendor_id,
                "productId": description.product_id,
                "productName": description.product_name,
                "manufacturerName": description.manufacturer_name,
                "serialNumber": description.serial_number,
                "hasAdbInterface": description.has_adb_interface,
            })
        })
        .collect();
    serde_json::to_string(&entries).map_err(js_error)
}

/// 弹出浏览器的 USB 选择框并连接选中的设备。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn request_and_connect() -> Result<String, JsValue> {
    let device = usb::request_device()
        .await
        .map_err(|err| js_error(format!("{err:#}")))?;
    connect_usb_device(device).await
}

/// 连接 `list_authorized_devices` 返回的某台已授权设备（不弹选择框）。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn connect_authorized(index: usize) -> Result<String, JsValue> {
    let devices = usb::authorized_devices()
        .await
        .map_err(|err| js_error(format!("{err:#}")))?;
    let (device, _) = devices
        .into_iter()
        .nth(index)
        .ok_or_else(|| JsValue::from_str("USB 设备列表已变化，请刷新后重试"))?;
    connect_usb_device(device).await
}

#[cfg(target_family = "wasm")]
async fn connect_usb_device(device: web_sys::UsbDevice) -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    let connected = session.connect(device).await.map_err(js_error)?;
    to_json(&connected)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn disconnect() -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.disconnect().await;
    Ok(())
}

/// 当前连接信息；未连接时返回 `null`。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn device_info() -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    match session.info() {
        Some(info) => to_json(info),
        None => Ok("null".to_string()),
    }
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn snapshot() -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    to_json(&session.snapshot().await.map_err(js_error)?)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn list_files(path: String) -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    to_json(&session.list_files(&path).await.map_err(js_error)?)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn pull_file(path: String) -> Result<Vec<u8>, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.pull(&path).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn push_file(directory: String, name: String, data: Vec<u8>) -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session
        .push(&directory, &name, &data)
        .await
        .map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn delete_path(path: String) -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.delete(&path).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn make_directory(path: String) -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.mkdir(&path).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn list_packages() -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    to_json(&session.packages().await.map_err(js_error)?)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn launch_package(package: String) -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.launch(&package).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn stop_package(package: String) -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.stop(&package).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn clear_package(package: String) -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.clear_data(&package).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn uninstall_package(package: String) -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.uninstall(&package).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn export_package(package: String) -> Result<Vec<u8>, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.export_package(&package).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn screenshot() -> Result<Vec<u8>, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.screenshot().await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn record_screen(seconds: u32) -> Result<Vec<u8>, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.record_screen(seconds).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn send_key(code: u32) -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.send_key(code).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn send_text(text: String) -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.send_text(&text).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn apply_display_size(width: u32, height: u32) -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session
        .apply_display_size(width, height)
        .await
        .map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn reset_display_size() -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.reset_display_size().await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn apply_density(density: u32) -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.apply_density(density).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn reset_density() -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.reset_density().await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn set_rotation(rotation: u8) -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.set_rotation(rotation).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn reboot(target: String) -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.reboot(&target).await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn clear_logs() -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    session.clear_logs().await.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn terminal(command: String) -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    to_json(&session.terminal(&command).await.map_err(js_error)?)
}

/// 开始流式抓取 logcat。该 Promise 在停止或设备断开时结束。
///
/// 调用方不应 `await` 它，否则会阻塞界面操作；用 `stop_logcat()` 停止。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn start_logcat(level: String, callback: js_sys::Function) -> Result<(), JsValue> {
    let mut guard = take_session()?;
    let stop = Arc::new(AtomicBool::new(false));
    LOG_STOP.with(|slot| *slot.borrow_mut() = Some(stop.clone()));

    let session = guard.get();
    let result = session.logcat(&level, &callback, &stop).await;

    LOG_STOP.with(|slot| *slot.borrow_mut() = None);
    result.map_err(js_error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn stop_logcat() {
    LOG_STOP.with(|slot| {
        if let Some(flag) = slot.borrow().as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
    });
}

/// 安装一组 APK。`bundle` 是多个 APK 首尾相接的数据，`manifest_json`
/// 描述每个文件的名称与长度，`options_json` 是 `-r/-d/-g/-t` 开关。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub async fn install_apks(
    bundle: Vec<u8>,
    manifest_json: String,
    options_json: String,
) -> Result<String, JsValue> {
    let mut guard = take_session()?;
    let session = guard.get();
    let report = session
        .install(&bundle, &manifest_json, &options_json)
        .await
        .map_err(js_error)?;
    to_json(&report)
}
