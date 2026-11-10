//! 浏览器会话：连接、设备操作与安装流程。
//!
//! 所有操作都直接通过 WebUSB 上的 ADB 协议完成，不依赖本机 adb server。
//! 会话是单设备的：浏览器版一次只维护一条 USB 连接，避免多个页面争用接口。

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use web_sys::UsbDevice;

use crate::adb::{Adb, ShellOutput};
use crate::auth::AdbKey;
use crate::info::{self, FileEntry, PackageEntry, SNAPSHOT_SCRIPT, Snapshot};
use crate::usb::UsbLink;

const KEY_STORAGE: &str = "rarebox.adb_key";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub serial: String,
    pub model: String,
    pub product: String,
    pub device: String,
    pub features: Vec<String>,
    pub vendor_id: u16,
    pub product_id: u16,
    pub usb_product_name: Option<String>,
    pub shell_v2: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub info: DeviceInfo,
    pub banner: String,
}

struct Device {
    info: DeviceInfo,
    usb: Adb<UsbLink>,
}

pub struct WebSession {
    key: Option<AdbKey>,
    device: Option<Device>,
}

impl Default for WebSession {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSession {
    pub fn new() -> Self {
        Self {
            key: load_saved_key(),
            device: None,
        }
    }

    pub fn info(&self) -> Option<&DeviceInfo> {
        self.device.as_ref().map(|device| &device.info)
    }

    fn ensure_key(&mut self) -> Result<()> {
        if self.key.is_some() {
            return Ok(());
        }
        let key = match load_saved_key() {
            Some(key) => key,
            None => {
                let key = AdbKey::generate()?;
                save_key(&key);
                key
            }
        };
        self.key = Some(key);
        Ok(())
    }

    /// 连接一台已授权的 USB 设备。
    pub async fn connect(&mut self, usb_device: UsbDevice) -> Result<Connected> {
        self.disconnect().await;
        self.ensure_key()?;
        let key = self.key.clone().expect("key just ensured");

        let usb = UsbLink::open(usb_device.clone()).await?;
        let mut adb = Adb::new(usb);
        let banner = match adb.handshake(&key).await {
            Ok(banner) => banner,
            Err(err) => {
                adb.link_mut().close().await;
                return Err(err.context("USB 已连接，但 ADB 握手失败"));
            }
        };

        let serial = adb
            .shell_text("getprop ro.serialno")
            .await
            .ok()
            .map(|serial| serial.trim().to_string())
            .filter(|serial| !serial.is_empty())
            .or_else(|| usb_device.serial_number())
            .unwrap_or_else(|| "usb-device".to_string());

        let info = DeviceInfo::from_banner(&banner, &usb_device, &serial, adb.supports_shell_v2());
        self.device = Some(Device {
            info: info.clone(),
            usb: adb,
        });
        Ok(Connected { info, banner })
    }

    pub async fn disconnect(&mut self) {
        if let Some(mut device) = self.device.take() {
            device.usb.link_mut().close().await;
        }
    }

    fn device(&mut self) -> Result<&mut Device> {
        self.device
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("请先通过 USB 连接设备"))
    }

    pub fn serial(&self) -> Result<String> {
        Ok(self
            .device
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("请先通过 USB 连接设备"))?
            .info
            .serial
            .clone())
    }

    async fn shell(&mut self, command: &str) -> Result<ShellOutput> {
        self.device()?.usb.shell(command).await
    }

    async fn run_checked(&mut self, command: &str) -> Result<String> {
        let output = self.shell(command).await?;
        if !output.success() {
            let text = output.text();
            if !text.trim().is_empty() {
                bail!("{}", text.trim());
            }
            bail!("命令执行失败（退出码 {}）", output.exit_code.unwrap_or(255));
        }
        Ok(output.text())
    }

    pub async fn snapshot(&mut self) -> Result<Snapshot> {
        let serial = self.serial()?;
        let raw = self.shell_text_command(SNAPSHOT_SCRIPT).await?;
        Ok(info::parse_snapshot(&serial, &raw))
    }

    async fn shell_text_command(&mut self, command: &str) -> Result<String> {
        self.shell(command).await.map(|output| output.text())
    }

    pub async fn list_files(&mut self, path: &str) -> Result<Vec<FileEntry>> {
        let path = normalize_remote_path(path);
        let entries = self.device()?.usb.list(&path).await?;
        Ok(entries.into_iter().map(FileEntry::from).collect())
    }

    pub async fn pull(&mut self, remote: &str) -> Result<Vec<u8>> {
        let remote = remote.trim();
        if remote.is_empty() {
            bail!("请指定要下载的设备路径");
        }
        self.device()?.usb.pull(remote).await
    }

    pub async fn push(&mut self, directory: &str, name: &str, data: &[u8]) -> Result<String> {
        let name = safe_file_name(name)?;
        let directory = normalize_remote_path(directory);
        self.run_checked(&format!("mkdir -p {}", shell_quote(&directory)))
            .await?;
        let remote = join_remote(&directory, name);
        let device = self.device()?;
        device
            .usb
            .push(&remote, data, now_secs())
            .await
            .with_context(|| format!("上传 {name} 失败"))?;
        Ok(remote)
    }

    pub async fn delete(&mut self, path: &str) -> Result<()> {
        let path = path.trim();
        if path.is_empty() || path == "/" {
            bail!("不能删除根目录");
        }
        self.run_checked(&format!("rm -rf {}", shell_quote(path)))
            .await?;
        Ok(())
    }

    pub async fn mkdir(&mut self, path: &str) -> Result<()> {
        let path = normalize_remote_path(path);
        self.run_checked(&format!("mkdir -p {}", shell_quote(&path)))
            .await?;
        Ok(())
    }

    pub async fn packages(&mut self) -> Result<Vec<PackageEntry>> {
        // `--show-versioncode` 从 Android 7 起可用；老设备退回不带版本号的列表。
        let all = match self.shell("pm list packages --show-versioncode").await {
            Ok(output) if output.success() && !output.text().contains("Error") => output.text(),
            _ => self.run_checked("pm list packages").await?,
        };
        let third_party = self.run_checked("pm list packages -3").await?;
        Ok(info::merge_packages(&all, &third_party))
    }

    pub async fn launch(&mut self, package: &str) -> Result<String> {
        let package = validate_package(package)?;
        self.run_checked(&format!(
            "monkey -p {package} -c android.intent.category.LAUNCHER 1"
        ))
        .await
    }

    pub async fn stop(&mut self, package: &str) -> Result<()> {
        let package = validate_package(package)?;
        self.run_checked(&format!("am force-stop {package}"))
            .await?;
        Ok(())
    }

    pub async fn clear_data(&mut self, package: &str) -> Result<String> {
        let package = validate_package(package)?;
        self.run_checked(&format!("pm clear {package}")).await
    }

    pub async fn uninstall(&mut self, package: &str) -> Result<String> {
        let package = validate_package(package)?;
        let output = self.run_checked(&format!("pm uninstall {package}")).await?;
        if output.contains("Failure") || output.contains("DELETE_FAILED") {
            bail!("{}", output.trim());
        }
        Ok(output)
    }

    /// 读取应用当前安装的 base APK 内容。
    pub async fn export_package(&mut self, package: &str) -> Result<Vec<u8>> {
        let package = validate_package(package)?;
        let output = self.run_checked(&format!("pm path {package}")).await?;
        let path = info::parse_package_path(&output)
            .ok_or_else(|| anyhow::anyhow!("没有找到 {package} 的 APK 路径"))?;
        self.device()?.usb.pull(&path).await
    }

    pub async fn screenshot(&mut self) -> Result<Vec<u8>> {
        let bytes = self.device()?.usb.exec("screencap -p").await?;
        if !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            bail!("设备返回的截图不是有效的 PNG 数据");
        }
        Ok(bytes)
    }

    pub async fn record_screen(&mut self, seconds: u32) -> Result<Vec<u8>> {
        if !(1..=60).contains(&seconds) {
            bail!("录屏时长必须在 1–60 秒之间");
        }
        let stamp = now_millis();
        let remote = format!("/sdcard/rarebox-web-{stamp}.mp4");
        let command = format!(
            "screenrecord --time-limit {seconds} --bit-rate 6000000 {}",
            shell_quote(&remote)
        );
        let result = async {
            self.run_checked(&command).await?;
            self.device()?.usb.pull(&remote).await
        }
        .await;
        let _ = self
            .run_checked(&format!("rm -f {}", shell_quote(&remote)))
            .await;
        result
    }

    pub async fn send_key(&mut self, code: u32) -> Result<()> {
        self.run_checked(&format!("input keyevent {code}")).await?;
        Ok(())
    }

    pub async fn send_text(&mut self, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            bail!("请先输入要发送的文本");
        }
        let escaped = text
            .replace('\\', "\\\\")
            .replace(' ', "%s")
            .replace('\'', "'\\''");
        self.run_checked(&format!("input text '{escaped}'")).await?;
        Ok(())
    }

    pub async fn apply_display_size(&mut self, width: u32, height: u32) -> Result<()> {
        if !(240..=2000).contains(&width) || !(240..=2000).contains(&height) {
            bail!("分辨率超出安全范围（240–2000）");
        }
        self.run_checked(&format!("wm size {width}x{height}"))
            .await?;
        Ok(())
    }

    pub async fn reset_display_size(&mut self) -> Result<()> {
        self.run_checked("wm size reset").await?;
        Ok(())
    }

    pub async fn apply_density(&mut self, density: u32) -> Result<()> {
        if !(72..=960).contains(&density) {
            bail!("DPI 超出安全范围（72–960）");
        }
        self.run_checked(&format!("wm density {density}")).await?;
        Ok(())
    }

    pub async fn reset_density(&mut self) -> Result<()> {
        self.run_checked("wm density reset").await?;
        Ok(())
    }

    pub async fn set_rotation(&mut self, rotation: u8) -> Result<()> {
        if rotation > 3 {
            bail!("旋转角度必须是 0–3");
        }
        self.run_checked(&format!(
            "settings put system accelerometer_rotation 0; settings put system user_rotation {rotation}"
        ))
        .await?;
        Ok(())
    }

    pub async fn reboot(&mut self, target: &str) -> Result<String> {
        let target = match target.trim() {
            "" => String::new(),
            "recovery" | "bootloader" | "fastbootd" | "sideload" | "sideload-auto-reboot" => {
                format!(" {}", target.trim())
            }
            other => bail!("不支持的重启模式：{other}"),
        };
        let command = format!("reboot{target}");
        let local = self.device()?.usb.open(&format!("exec:{command}")).await?;
        let mut output = Vec::new();
        loop {
            let chunk = {
                let device = self.device()?;
                timeout(device.usb.read_stream(local, 64 * 1024), 1200).await
            };
            match chunk {
                // 设备在重启过程中可能来不及回 CLSE；命令已经发出即可。
                None => break,
                Some(Ok(Some(bytes))) => output.extend_from_slice(&bytes),
                Some(Ok(None)) => break,
                Some(Err(err)) => {
                    self.device()?.usb.close_stream(local).await;
                    return Err(err);
                }
            }
        }
        self.device()?.usb.close_stream(local).await;
        Ok(String::from_utf8_lossy(&output).to_string())
    }

    pub async fn clear_logs(&mut self) -> Result<()> {
        self.run_checked("logcat -c").await?;
        Ok(())
    }

    pub async fn terminal(&mut self, command: &str) -> Result<CommandResult> {
        let command = command.trim();
        if command.is_empty() {
            bail!("请输入要执行的命令");
        }
        let output = self.shell(command).await?;
        Ok(CommandResult::from(output))
    }

    /// 流式读取 logcat；每段 UTF-8 文本交给 `callback`。
    pub async fn logcat(
        &mut self,
        level: &str,
        callback: &js_sys::Function,
        stop: &AtomicBool,
    ) -> Result<()> {
        let filter = match level {
            "verbose" => "*:V",
            "debug" => "*:D",
            "info" => "*:I",
            "warn" => "*:W",
            "error" => "*:E",
            _ => "*:V",
        };
        let command = format!("logcat -v threadtime '{filter}'");
        let local = self.device()?.usb.open(&format!("exec:{command}")).await?;
        let result = async {
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let chunk = {
                    let device = self.device()?;
                    timeout(device.usb.read_stream(local, 64 * 1024), 400).await
                };
                match chunk {
                    // 超时后重新检查停止标记；设备安静时也能及时停止。
                    None => continue,
                    Some(Ok(Some(bytes))) => {
                        let text = String::from_utf8_lossy(&bytes);
                        let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&text));
                    }
                    Some(Ok(None)) => break,
                    Some(Err(err)) => return Err(err),
                }
            }
            Ok(())
        }
        .await;
        if let Ok(device) = self.device() {
            device.usb.close_stream(local).await;
        }
        result
    }

    /// 安装一组 APK（主包 + split）。`manifest` 描述拼接在 `bundle` 里的文件。
    pub async fn install(
        &mut self,
        bundle: &[u8],
        manifest_json: &str,
        options_json: &str,
    ) -> Result<InstallReport> {
        let manifest: InstallManifest =
            serde_json::from_str(manifest_json).context("安装清单格式错误")?;
        if manifest.files.is_empty() {
            bail!("没有选择 APK");
        }
        let options: InstallOptions =
            serde_json::from_str(options_json).context("安装参数格式错误")?;

        let mut offset = 0usize;
        for file in &manifest.files {
            let name = safe_file_name(&file.name)?;
            if file.size == 0 {
                bail!("{name} 是空文件");
            }
            if offset + file.size > bundle.len() {
                bail!("APK 数据不完整");
            }
            if !name.to_ascii_lowercase().ends_with(".apk") {
                bail!("{name} 不是 APK 文件");
            }
            offset += file.size;
        }
        if offset != bundle.len() {
            bail!("APK 数据长度与清单不一致");
        }

        let total: usize = manifest.files.iter().map(|file| file.size).sum();
        let stamp = now_millis();
        let staging = format!("/data/local/tmp/rarebox-web-{stamp}");
        self.run_checked(&format!("mkdir -p {}", shell_quote(&staging)))
            .await?;

        let mut offset = 0usize;
        let mut pushed = Vec::new();
        for file in &manifest.files {
            let name = safe_file_name(&file.name)?;
            let data = &bundle[offset..offset + file.size];
            offset += file.size;
            let remote = format!("{staging}/{name}");
            if let Err(err) = self.device()?.usb.push(&remote, data, now_secs()).await {
                let _ = self
                    .run_checked(&format!("rm -rf {}", shell_quote(&staging)))
                    .await;
                return Err(err).with_context(|| format!("推送 {name} 失败"));
            }
            pushed.push((name.to_string(), file.size, remote));
        }

        let flags = options.flags();
        let create = format!(
            "pm install-create {flags} -S {total}",
            flags = flags.join(" "),
        );
        let create_output = self.run_checked(&create).await;

        let report = match create_output {
            Ok(output) => match parse_session_id(&output) {
                Some(session) => {
                    let mut detail = String::new();
                    let mut ok = true;
                    for (name, size, remote) in &pushed {
                        let command = format!(
                            "pm install-write -S {size} {session} {} {}",
                            shell_quote(name),
                            shell_quote(remote)
                        );
                        match self.run_checked(&command).await {
                            Ok(text) => {
                                detail.push_str(&text);
                                detail.push('\n');
                                if install_output_failed(&text) {
                                    ok = false;
                                }
                            }
                            Err(err) => {
                                ok = false;
                                detail.push_str(&format!("{name}: {err:#}\n"));
                            }
                        }
                    }
                    if ok {
                        let commit = self
                            .run_checked(&format!("pm install-commit {session}"))
                            .await;
                        match commit {
                            Ok(text) => {
                                detail.push_str(&text);
                                if install_output_failed(&text) {
                                    ok = false;
                                }
                            }
                            Err(err) => {
                                ok = false;
                                detail.push_str(&format!("提交安装失败：{err:#}"));
                            }
                        }
                    }
                    InstallReport {
                        success: ok,
                        output: format!("{output}\n{detail}").trim().to_string(),
                        session: Some(session),
                    }
                }
                None => {
                    // 老设备可能不返回 session 格式；单 APK 时退回 `pm install`。
                    if pushed.len() == 1 {
                        let (_, _, remote) = &pushed[0];
                        match self
                            .run_checked(&format!(
                                "pm install {flags} {}",
                                shell_quote(remote),
                                flags = flags.join(" ")
                            ))
                            .await
                        {
                            Ok(text) => InstallReport {
                                success: !install_output_failed(&text),
                                output: text,
                                session: None,
                            },
                            Err(err) => {
                                let _ = self
                                    .run_checked(&format!("rm -rf {}", shell_quote(&staging)))
                                    .await;
                                return Err(err);
                            }
                        }
                    } else {
                        let _ = self
                            .run_checked(&format!("rm -rf {}", shell_quote(&staging)))
                            .await;
                        bail!("设备没有返回安装会话编号，无法安装分包 APK：{output}");
                    }
                }
            },
            Err(err) => {
                let _ = self
                    .run_checked(&format!("rm -rf {}", shell_quote(&staging)))
                    .await;
                return Err(err);
            }
        };

        let _ = self
            .run_checked(&format!("rm -rf {}", shell_quote(&staging)))
            .await;
        Ok(report)
    }
}

fn install_output_failed(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("failure") || lower.contains("error") || lower.contains("failed")
}

impl DeviceInfo {
    fn from_banner(banner: &str, usb: &UsbDevice, serial: &str, shell_v2: bool) -> Self {
        let props = banner_properties(banner);
        let features = props
            .get("features")
            .map(|features| {
                features
                    .split(',')
                    .filter(|feature| !feature.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Self {
            serial: serial.to_string(),
            model: props.get("ro.product.model").cloned().unwrap_or_default(),
            product: props.get("ro.product.name").cloned().unwrap_or_default(),
            device: props.get("ro.product.device").cloned().unwrap_or_default(),
            features,
            vendor_id: usb.vendor_id(),
            product_id: usb.product_id(),
            usb_product_name: usb.product_name(),
            shell_v2,
        }
    }
}

fn banner_properties(banner: &str) -> HashMap<String, String> {
    let payload = banner.strip_prefix("device::").unwrap_or(banner);
    payload
        .split(';')
        .filter_map(|field| field.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub combined: String,
    pub exit_code: Option<u8>,
    pub success: bool,
}

impl From<ShellOutput> for CommandResult {
    fn from(output: ShellOutput) -> Self {
        let combined = output.text();
        Self {
            stdout: output.stdout.replace("\r\n", "\n"),
            stderr: output.stderr.replace("\r\n", "\n"),
            combined,
            exit_code: output.exit_code,
            success: output.success(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub success: bool,
    pub output: String,
    pub session: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstallManifest {
    files: Vec<InstallFile>,
}

#[derive(Debug, Deserialize)]
struct InstallFile {
    name: String,
    size: usize,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct InstallOptions {
    replace: bool,
    downgrade: bool,
    grant_permissions: bool,
    allow_test: bool,
}

impl InstallOptions {
    fn flags(&self) -> Vec<&'static str> {
        let mut flags = Vec::with_capacity(4);
        if self.replace || self.downgrade {
            flags.push("-r");
        }
        if self.downgrade {
            flags.push("-d");
        }
        if self.grant_permissions {
            flags.push("-g");
        }
        if self.allow_test {
            flags.push("-t");
        }
        flags
    }
}

fn parse_session_id(output: &str) -> Option<String> {
    let start = output.rfind('[')?;
    let end = output.rfind(']')?;
    if start >= end {
        return None;
    }
    let id = output[start + 1..end].trim();
    if !id.is_empty() && id.chars().all(|ch| ch.is_ascii_digit()) {
        return Some(id.to_string());
    }
    None
}

fn validate_package(package: &str) -> Result<&str> {
    let package = package.trim();
    if package.is_empty()
        || !package
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_')
    {
        bail!("包名不合法");
    }
    Ok(package)
}

fn safe_file_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        bail!("文件名不合法：{name}");
    }
    Ok(name)
}

fn normalize_remote_path(path: &str) -> String {
    let mut path = path.trim().to_string();
    if path.is_empty() {
        path = "/sdcard".to_string();
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    path
}

fn join_remote(directory: &str, name: &str) -> String {
    if directory == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", directory.trim_end_matches('/'), name)
    }
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// 等待 `future`，超时返回 `None`。用于让 logcat 定期检查停止标记。
async fn timeout<F: Future>(future: F, millis: i32) -> Option<F::Output> {
    use futures::future::{Either, select};

    let sleep = sleep_ms(millis);
    futures::pin_mut!(future);
    futures::pin_mut!(sleep);
    match select(future, sleep).await {
        Either::Left((output, _)) => Some(output),
        Either::Right(_) => None,
    }
}

async fn sleep_ms(millis: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, millis);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
fn now_millis() -> u64 {
    js_sys::Date::now() as u64
}

fn now_secs() -> u32 {
    (now_millis() / 1000) as u32
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn load_saved_key() -> Option<AdbKey> {
    let raw = storage()?.get_item(KEY_STORAGE).ok().flatten()?;
    let der = base64::engine::general_purpose::STANDARD.decode(raw).ok()?;
    AdbKey::from_pkcs8_der(&der).ok()
}

fn save_key(key: &AdbKey) {
    let Some(storage) = storage() else {
        return;
    };
    if let Ok(der) = key.to_pkcs8_der() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(der);
        let _ = storage.set_item(KEY_STORAGE, &encoded);
    }
}
