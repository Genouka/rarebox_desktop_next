//! HarmonyOS 设备操作：hdc 连接、应用、文件、终端、日志、截图与重启。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use chrono::Local;

use crate::config::{downloads_dir, short_path};
use crate::engine::{Core, Ctx};
use crate::event::{Event, PickPurpose};
use crate::hdc::{
    self, HdcClient, HdcDevice, HdcForward, HdcInstallOptions, HdcKey, HdcRebootTarget,
    HdcSwipeDirection, HdcTap, HilogOptions,
};
use crate::state::{
    ConfirmKind, DeviceKind, HdcInstallReport, LogFilter, TerminalEntry, ToastKind,
};

const MAX_TERMINAL: usize = 20;
const MAX_OUTPUT: usize = 20_000;
const MAX_LOGS: usize = 1_500;

impl Core {
    /// 检测 hdc 并刷新设备列表。与 ADB 的 `refresh_devices` 相互独立。
    pub(crate) fn refresh_hdc(&mut self, cx: &mut Ctx) {
        let configured = self.state.config.hdc_path.clone();
        self.run_task(
            cx,
            "正在查找 hdc 与 HarmonyOS 设备…",
            move || {
                let hint = if cfg!(target_os = "android") {
                    "未找到可用的 hdc：内置 libhdc.so 不可用；请在设置中指定 hdc 路径"
                } else {
                    "未找到可用的 hdc：内置 hdc 不可用，且本机未安装 DevEco Studio / OpenHarmony SDK；请在设置中指定 hdc 路径"
                };
                let (path, version) = hdc::detect_hdc(&configured).context(hint)?;
                let client = HdcClient::new(path.clone());
                let devices = client.targets()?;
                Ok((path, version, devices))
            },
            |core, result, cx| match result {
                Ok((path, version, devices)) => {
                    core.state.harmony.path = Some(path);
                    core.state.harmony.version = Some(version);
                    core.state.harmony.error = None;
                    core.apply_hdc_devices(devices, cx);
                    if core.state.harmony.selected.is_some() {
                        core.load_hdc_snapshot(cx);
                        core.load_hdc_forwards(cx);
                    }
                }
                Err(err) => {
                    core.state.harmony.path = None;
                    core.state.harmony.version = None;
                    core.state.harmony.error = Some(format!("{err:#}"));
                    core.state.harmony.devices.clear();
                    core.state.harmony.selected = None;
                    core.reset_hdc_device_state(cx);
                    cx.toast(ToastKind::Error, format!("HDC 未就绪：{err:#}"));
                }
            },
        );
    }

    fn apply_hdc_devices(&mut self, devices: Vec<HdcDevice>, cx: &mut Ctx) {
        let previous = self.state.harmony.selected.clone();
        self.state.harmony.devices = devices;
        let next = previous
            .filter(|key| {
                self.state
                    .harmony
                    .devices
                    .iter()
                    .any(|device| &device.key == key)
            })
            .or_else(|| {
                self.state
                    .harmony
                    .devices
                    .iter()
                    .find(|device| device.is_online())
                    .map(|device| device.key.clone())
            })
            .or_else(|| {
                self.state
                    .harmony
                    .devices
                    .first()
                    .map(|device| device.key.clone())
            });
        if next != self.state.harmony.selected {
            self.state.harmony.selected = next;
            self.reset_hdc_device_state(cx);
        }
    }

    pub(crate) fn reset_hdc_device_state(&mut self, _cx: &mut Ctx) {
        self.state.harmony.snapshot = None;
        self.state.harmony.snapshot_at = None;
        self.state.harmony.packages.clear();
        self.state.harmony.packages_for = None;
        self.state.harmony.bundle_details = None;
        self.state.harmony.details_for = None;
        self.state.harmony.forwards.clear();
        self.state.harmony.install_reports.clear();
        self.state.harmony.files.clear();
        self.state.harmony.files_path = "/data/local/tmp".to_string();
        self.state.harmony.logs.clear();
        self.state.harmony.logs_at = None;
        self.state.harmony.screenshot = None;
        // 设备切换后 UDID/Profile 绑定关系失效，签名页必须重新读取。
        self.state.signing.udid = None;
        self.state.signing.outcome = None;
        self.state.signing.reset_stages();
    }

    pub(crate) fn select_hdc_device(&mut self, key: String, cx: &mut Ctx) {
        if self.state.harmony.selected.as_deref() == Some(key.as_str()) {
            self.set_device_kind(DeviceKind::Harmony, cx);
            return;
        }
        let was_android = !self.is_harmony();
        self.state.harmony.selected = Some(key);
        self.reset_hdc_device_state(cx);
        if was_android {
            // 切换工作区会负责读取快照；先换好设备，避免用旧设备触发自动加载。
            self.set_device_kind(DeviceKind::Harmony, cx);
        } else {
            self.load_hdc_snapshot(cx);
        }
        self.load_hdc_forwards(cx);
    }

    pub(crate) fn load_hdc_snapshot(&mut self, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在读取 HarmonyOS 设备信息…",
            move || client.snapshot(&key),
            |core, result, cx| match result {
                Ok(snapshot) => {
                    core.state.harmony.snapshot_at =
                        Some(Local::now().format("%H:%M:%S").to_string());
                    core.state.harmony.snapshot = Some(snapshot);
                }
                Err(err) => {
                    cx.toast(ToastKind::Error, format!("读取设备信息失败：{err:#}"));
                }
            },
        );
    }

    // ---------------------------------------------------------------------
    // 连接
    // ---------------------------------------------------------------------

    pub(crate) fn hdc_connect(&mut self, address: String, cx: &mut Ctx) {
        let raw = address.trim().to_string();
        if raw.is_empty() {
            cx.toast(ToastKind::Info, "请输入设备地址，例如 192.168.1.20:5555");
            return;
        }
        let address = if raw.contains(':') {
            raw
        } else {
            format!("{raw}:5555")
        };
        let result = self.hdc_client();
        let Ok(client) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let task_address = address.clone();
        self.run_task(
            cx,
            format!("正在连接 {address}…"),
            move || {
                let output = client.tconn(&task_address)?;
                let text = output.combined();
                let lower = text.to_ascii_lowercase();
                if lower.contains("fail") {
                    bail!("{text}");
                }
                Ok(address)
            },
            move |core, result, cx| match result {
                Ok(address) => {
                    core.state.harmony.selected = Some(address.clone());
                    core.reset_hdc_device_state(cx);
                    core.set_device_kind(DeviceKind::Harmony, cx);
                    cx.toast(ToastKind::Success, format!("已连接 {address}"));
                    core.refresh_hdc(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("连接失败：{err:#}")),
            },
        );
    }

    pub(crate) fn hdc_disconnect(&mut self, cx: &mut Ctx) {
        let raw = self.state.harmony.address.trim().to_string();
        let key = self
            .state
            .harmony
            .selected
            .clone()
            .filter(|key| key.contains(':'))
            .or_else(|| {
                (!raw.is_empty()).then(|| {
                    if raw.contains(':') {
                        raw
                    } else {
                        format!("{raw}:5555")
                    }
                })
            });
        let Some(address) = key else {
            cx.toast(ToastKind::Info, "请选择一台无线设备，或填写要断开的地址");
            return;
        };
        let result = self.hdc_client();
        let Ok(client) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let task_address = address.clone();
        self.run_task(
            cx,
            format!("正在断开 {address}…"),
            move || {
                let output = client.tconn_remove(&task_address)?;
                let text = output.combined();
                if text.to_ascii_lowercase().contains("fail") {
                    bail!("{text}");
                }
                Ok(address)
            },
            move |core, result, cx| match result {
                Ok(address) => {
                    cx.toast(ToastKind::Success, format!("已断开 {address}"));
                    core.refresh_hdc(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("断开失败：{err:#}")),
            },
        );
    }

    pub(crate) fn restart_hdc_server(&mut self, cx: &mut Ctx) {
        let result = self.hdc_client();
        let Ok(client) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在重启 hdc 服务…",
            move || {
                client.restart_server()?;
                Ok(())
            },
            |core, result, cx| match result {
                Ok(()) => {
                    cx.toast(ToastKind::Success, "hdc 服务已重启");
                    core.refresh_hdc(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("重启失败：{err:#}")),
            },
        );
    }

    pub(crate) fn open_hdc_server_restart(&mut self, cx: &mut Ctx) {
        cx.confirm_kind(
            "重启 hdc 服务？",
            "将执行 hdc kill -r。USB 设备会自动重连；无线调试连接会断开，需要重新连接。",
            "重启服务",
            ConfirmKind::HdcRestartServer,
        );
    }

    /// 通过 USB 设备开启 `tmode port`，再用设备 IP 自动执行 `tconn`。
    pub(crate) fn enable_hdc_wireless(&mut self, port: String, cx: &mut Ctx) {
        let Some(port) = parse_port(&port) else {
            cx.toast(ToastKind::Error, "端口必须是 1-65535");
            return;
        };
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let is_usb = self
            .state
            .harmony
            .devices
            .iter()
            .find(|device| device.key == key)
            .map(|device| device.connection == hdc::HdcConnection::Usb)
            .unwrap_or(!key.contains(':'));
        if !is_usb {
            cx.toast(ToastKind::Info, "请选择 USB 设备后再开启无线调试端口");
            return;
        }
        let known_ip = self
            .state
            .harmony
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.ip.clone());
        self.run_task(
            cx,
            format!("正在通过 USB 开启端口 {port}…"),
            move || {
                let output = client.tmode_port(&key, port)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                let mut candidates = Vec::new();
                if let Ok(Some(ip)) = client.ip_address(&key)
                    && !candidates.contains(&ip)
                {
                    candidates.push(ip);
                }
                if let Some(ip) = known_ip
                    && !candidates.contains(&ip)
                {
                    candidates.push(ip);
                }
                if candidates.is_empty() {
                    bail!("端口已开启，但无法读取设备 IP，请查看设备 Wi-Fi IP 后手动连接");
                }
                let mut errors = Vec::new();
                for ip in candidates {
                    let address = format!("{ip}:{port}");
                    for _ in 0..4 {
                        match client.tconn(&address) {
                            Ok(output) if !output.is_failure() => return Ok(address),
                            Ok(output) => errors.push(output.combined()),
                            Err(err) => errors.push(format!("{err:#}")),
                        }
                        std::thread::sleep(Duration::from_millis(700));
                    }
                }
                bail!(
                    "端口已开启，但自动连接失败：{}",
                    errors.last().cloned().unwrap_or_default()
                );
            },
            move |core, result, cx| match result {
                Ok(address) => {
                    core.state.harmony.address = address.clone();
                    core.state.harmony.selected = Some(address.clone());
                    cx.toast(ToastKind::Success, format!("无线调试已开启：{address}"));
                    core.refresh_hdc(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("开启失败：{err:#}")),
            },
        );
    }

    pub(crate) fn disable_hdc_wireless(&mut self, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let usb_key = self
            .state
            .harmony
            .devices
            .iter()
            .find(|device| device.connection == hdc::HdcConnection::Usb && device.is_online())
            .map(|device| device.key.clone());
        let target = if key.contains(':') {
            usb_key.unwrap_or_else(|| key.clone())
        } else {
            key.clone()
        };
        let address = key.contains(':').then_some(key);
        self.run_task(
            cx,
            "正在关闭无线调试端口…",
            move || {
                let output = client.tmode_close(&target)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                if let Some(address) = address {
                    let _ = client.tconn_remove(&address);
                }
                Ok(())
            },
            |core, result, cx| match result {
                Ok(()) => {
                    cx.toast(ToastKind::Success, "无线调试端口已关闭");
                    core.refresh_hdc(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("关闭失败：{err:#}")),
            },
        );
    }

    pub(crate) fn load_hdc_forwards(&mut self, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在读取 hdc 端口转发…",
            move || client.forwards(&key),
            |core, result, cx| match result {
                Ok(forwards) => {
                    core.state.harmony.forwards = forwards;
                }
                Err(err) => {
                    core.state.harmony.forwards.clear();
                    cx.toast(ToastKind::Error, format!("读取端口转发失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn toggle_hdc_forward_direction(&mut self) {
        self.state.harmony.forward_reverse = !self.state.harmony.forward_reverse;
    }

    pub(crate) fn add_hdc_forward(
        &mut self,
        local: String,
        remote: String,
        reverse: bool,
        cx: &mut Ctx,
    ) {
        let Some(local_port) = parse_port(&local) else {
            cx.toast(ToastKind::Error, "本地端口必须是 1-65535");
            return;
        };
        let Some(remote_port) = parse_port(&remote) else {
            cx.toast(ToastKind::Error, "设备端口必须是 1-65535");
            return;
        };
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            format!("正在添加{}端口转发…", if reverse { "反向" } else { "正向" }),
            move || {
                let output = client.add_forward(&key, local_port, remote_port, reverse)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            |core, result, cx| match result {
                Ok(()) => {
                    cx.toast(ToastKind::Success, "端口转发已添加");
                    core.load_hdc_forwards(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("添加失败：{err:#}")),
            },
        );
    }

    pub(crate) fn remove_hdc_forward(&mut self, forward: HdcForward, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在移除端口转发…",
            move || {
                let output = client.remove_forward(&key, &forward)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            |core, result, cx| match result {
                Ok(()) => {
                    cx.toast(ToastKind::Success, "端口转发已移除");
                    core.load_hdc_forwards(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("移除失败：{err:#}")),
            },
        );
    }

    // ---------------------------------------------------------------------
    // 应用
    // ---------------------------------------------------------------------

    pub(crate) fn load_hdc_packages(&mut self, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.state.harmony.packages_for = Some(key.clone());
        self.run_task(
            cx,
            "正在读取 HarmonyOS 应用列表…",
            move || client.packages(&key),
            |core, result, cx| match result {
                Ok(packages) => {
                    let count = packages.len();
                    core.state.harmony.packages = packages;
                    cx.toast(ToastKind::Success, format!("已读取 {count} 个应用"));
                }
                Err(err) => {
                    core.state.harmony.packages_for = None;
                    cx.toast(ToastKind::Error, format!("读取应用失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn pick_install_haps(&mut self, cx: &mut Ctx) {
        if self.require_hdc_device().is_err() {
            cx.toast(ToastKind::Error, "请先连接并选择一台 HarmonyOS 设备");
            return;
        }
        cx.pick_files(PickPurpose::InstallHap);
    }

    pub(crate) fn install_hap_files(&mut self, files: Vec<PathBuf>, cx: &mut Ctx) {
        let accepted: Vec<PathBuf> = files
            .into_iter()
            .filter(|path| {
                matches!(
                    path.extension()
                        .and_then(|extension| extension.to_str())
                        .map(str::to_ascii_lowercase)
                        .as_deref(),
                    Some("hap" | "hsp" | "app")
                )
            })
            .collect();
        if accepted.is_empty() {
            cx.toast(ToastKind::Error, "请选择 .hap / .hsp / .app 文件");
            return;
        }
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let options = HdcInstallOptions {
            replace: self.state.harmony.install_replace,
            shared: self.state.harmony.install_shared,
        };
        let count = accepted.len();
        self.state.harmony.install_reports.clear();
        self.run_task(
            cx,
            format!("正在安装 {count} 个 HarmonyOS 包…"),
            move || {
                let mut reports = Vec::new();
                for file in &accepted {
                    let title = file
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| short_path(file));
                    match client.install(&key, file, options) {
                        Ok(output) => {
                            let text = output.combined();
                            let success = hdc::install_succeeded(&text);
                            let detail = if success {
                                "安装成功".to_string()
                            } else if let Some(hint) = hdc::install_error_hint(&text) {
                                hint.to_string()
                            } else {
                                let trimmed = text.trim();
                                if trimmed.is_empty() {
                                    output.error_message()
                                } else {
                                    trimmed.chars().take(400).collect()
                                }
                            };
                            reports.push(HdcInstallReport {
                                title,
                                detail,
                                success,
                            });
                        }
                        Err(err) => reports.push(HdcInstallReport {
                            title,
                            detail: format!("{err:#}"),
                            success: false,
                        }),
                    }
                }
                Ok(reports)
            },
            |core, result, cx| match result {
                Ok(reports) => {
                    let succeeded = reports.iter().filter(|report| report.success).count();
                    let failed = reports.len().saturating_sub(succeeded);
                    core.state.harmony.install_reports = reports;
                    if succeeded > 0 {
                        core.load_hdc_packages(cx);
                    }
                    cx.toast(
                        if failed == 0 {
                            ToastKind::Success
                        } else {
                            ToastKind::Error
                        },
                        format!("安装完成：{succeeded} 个成功 · {failed} 个失败"),
                    );
                }
                Err(err) => cx.toast(ToastKind::Error, format!("安装失败：{err:#}")),
            },
        );
    }

    pub(crate) fn toggle_hdc_install_option(&mut self, shared: bool) {
        if shared {
            self.state.harmony.install_shared = !self.state.harmony.install_shared;
        } else {
            self.state.harmony.install_replace = !self.state.harmony.install_replace;
        }
    }

    pub(crate) fn launch_hdc_package(&mut self, bundle: String, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let target = bundle.clone();
        let cached_ability = self
            .state
            .harmony
            .bundle_details
            .as_ref()
            .filter(|info| info.bundle == bundle)
            .and_then(|info| info.main_ability.clone());
        self.run_task(
            cx,
            format!("正在启动 {bundle}…"),
            move || {
                let ability = cached_ability.or_else(|| {
                    client
                        .bundle_info(&key, &target)
                        .ok()
                        .and_then(|info| info.main_ability)
                });
                let output = match ability {
                    Some(ability) => client.start_ability(&key, &target, &ability)?,
                    None => client.start_bundle(&key, &target)?,
                };
                let text = output.combined();
                let lower = text.to_ascii_lowercase();
                if lower.contains("fail") || lower.contains("error:") {
                    bail!("{}", text.trim());
                }
                Ok(())
            },
            move |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, format!("已启动 {bundle}")),
                Err(err) => cx.toast(ToastKind::Error, format!("启动失败：{err:#}")),
            },
        );
    }

    pub(crate) fn stop_hdc_package(&mut self, bundle: String, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let target = bundle.clone();
        self.run_task(
            cx,
            format!("正在停止 {bundle}…"),
            move || {
                let output = client.force_stop(&key, &target)?;
                let text = output.combined();
                if text.to_ascii_lowercase().contains("fail") {
                    bail!("{}", text.trim());
                }
                Ok(())
            },
            move |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, format!("已停止 {bundle}")),
                Err(err) => cx.toast(ToastKind::Error, format!("停止失败：{err:#}")),
            },
        );
    }

    pub(crate) fn uninstall_hdc_package(&mut self, bundle: String, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let target = bundle.clone();
        self.run_task(
            cx,
            format!("正在卸载 {bundle}…"),
            move || {
                let output = client.uninstall(&key, &target)?;
                let text = output.combined();
                if !hdc::uninstall_succeeded(&text) {
                    bail!("{}", text.trim());
                }
                Ok(())
            },
            move |core, result, cx| match result {
                Ok(()) => {
                    core.state
                        .harmony
                        .packages
                        .retain(|package| package.name != bundle);
                    if core
                        .state
                        .harmony
                        .bundle_details
                        .as_ref()
                        .is_some_and(|info| info.bundle == bundle)
                    {
                        core.state.harmony.bundle_details = None;
                        core.state.harmony.details_for = None;
                    }
                    cx.toast(ToastKind::Success, format!("已卸载 {bundle}"));
                }
                Err(err) => cx.toast(ToastKind::Error, format!("卸载失败：{err:#}")),
            },
        );
    }

    pub(crate) fn copy_hdc_bundle(&mut self, bundle: String, cx: &mut Ctx) {
        cx.emit(Event::CopyText(bundle.clone()));
        cx.toast(ToastKind::Info, format!("已复制 {bundle}"));
    }

    pub(crate) fn load_hdc_bundle_details(&mut self, bundle: String, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.state.harmony.bundle_details = None;
        self.state.harmony.details_for = Some(bundle.clone());
        self.run_task(
            cx,
            format!("正在读取 {bundle} 的详情…"),
            move || client.bundle_info(&key, &bundle),
            |core, result, cx| match result {
                Ok(info) => {
                    core.state.harmony.details_for = None;
                    core.state.harmony.bundle_details = Some(info);
                }
                Err(err) => {
                    core.state.harmony.details_for = None;
                    cx.toast(ToastKind::Error, format!("读取详情失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn close_hdc_bundle_details(&mut self) {
        self.state.harmony.bundle_details = None;
        self.state.harmony.details_for = None;
    }

    pub(crate) fn open_hdc_clear_app_data(
        &mut self,
        bundle: String,
        cache_only: bool,
        cx: &mut Ctx,
    ) {
        if cache_only {
            self.clear_hdc_app_data(bundle, true, cx);
            return;
        }
        cx.confirm_kind(
            format!("清除 {bundle} 的全部数据？"),
            "将执行 bm clean -d，删除该应用的账号、设置与文件，效果等同于重置应用。",
            "清除数据",
            ConfirmKind::HdcClearAppData {
                bundle,
                cache: false,
            },
        );
    }

    pub(crate) fn clear_hdc_app_data(&mut self, bundle: String, cache_only: bool, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let target = bundle.clone();
        self.run_task(
            cx,
            format!(
                "正在清除 {bundle} 的{}…",
                if cache_only { "缓存" } else { "数据" }
            ),
            move || {
                let output = client.clean_app(&key, &target, cache_only)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            move |core, result, cx| match result {
                Ok(()) => {
                    cx.toast(
                        ToastKind::Success,
                        format!(
                            "已清除 {bundle} 的{}",
                            if cache_only { "缓存" } else { "数据" }
                        ),
                    );
                    if !cache_only {
                        core.load_hdc_bundle_details(bundle, cx);
                    }
                }
                Err(err) => cx.toast(ToastKind::Error, format!("清除失败：{err:#}")),
            },
        );
    }

    pub(crate) fn open_hdc_scheme(&mut self, uri: String, cx: &mut Ctx) {
        let uri = uri.trim().to_string();
        if uri.is_empty() {
            cx.toast(ToastKind::Info, "请输入 Scheme 或 URL");
            return;
        }
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let target = uri.clone();
        self.run_task(
            cx,
            format!("正在打开 {uri}…"),
            move || {
                let output = client.start_uri(&key, &target)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            move |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, format!("已打开 {uri}")),
                Err(err) => cx.toast(ToastKind::Error, format!("打开失败：{err:#}")),
            },
        );
    }

    // ---------------------------------------------------------------------
    // 文件
    // ---------------------------------------------------------------------

    pub(crate) fn load_hdc_files(&mut self, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let path = self.state.harmony.files_path.clone();
        self.run_task(
            cx,
            format!("正在读取 {path}…"),
            move || {
                let target = if path == "/" {
                    path.clone()
                } else {
                    format!("{path}/")
                };
                let command = format!("ls -la {}", shell_quote(&target));
                let output = client.shell(&key, &command)?;
                if !output.success() && output.stdout.trim().is_empty() {
                    bail!("{}", output.error_message());
                }
                Ok(crate::adb::parse_ls(&output.stdout))
            },
            |core, result, cx| match result {
                Ok(entries) => {
                    core.state.harmony.files = entries;
                }
                Err(err) => {
                    core.state.harmony.files.clear();
                    cx.toast(ToastKind::Error, format!("读取目录失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn navigate_hdc_files(&mut self, path: String, cx: &mut Ctx) {
        let mut target = path.trim().to_string();
        if target.is_empty() {
            target = "/data/local/tmp".to_string();
        }
        if !target.starts_with('/') {
            target = format!("/{target}");
        }
        while target.len() > 1 && target.ends_with('/') {
            target.pop();
        }
        self.state.harmony.files_path = target;
        self.state.harmony.files.clear();
        self.load_hdc_files(cx);
    }

    pub(crate) fn go_parent_hdc_files(&mut self, cx: &mut Ctx) {
        let current = self
            .state
            .harmony
            .files_path
            .trim_end_matches('/')
            .to_string();
        let parent = match current.rfind('/') {
            Some(0) | None => "/".to_string(),
            Some(index) => current[..index].to_string(),
        };
        self.navigate_hdc_files(parent, cx);
    }

    pub(crate) fn pull_hdc_file(&mut self, remote: String, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let name = remote
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("rarebox-file")
            .to_string();
        let task_name = name.clone();
        self.run_task(
            cx,
            format!("正在拉取 {name}…"),
            move || {
                let local = unique_path(&downloads_dir().join(&task_name));
                let output = client.pull(&key, &remote, &local)?;
                let text = output.combined();
                if !output.success() || text.to_ascii_lowercase().contains("fail") {
                    bail!("{}", text.trim());
                }
                Ok(local)
            },
            move |core, result, cx| match result {
                Ok(path) => {
                    core.state.harmony.last_file = Some(path.clone());
                    cx.toast(
                        ToastKind::Success,
                        format!("{name} 已保存到 {}", short_path(&path)),
                    );
                }
                Err(err) => cx.toast(ToastKind::Error, format!("拉取失败：{err:#}")),
            },
        );
    }

    pub(crate) fn pick_push_hdc_files(&mut self, cx: &mut Ctx) {
        cx.pick_files(PickPurpose::PushHdcFiles);
    }

    pub(crate) fn upload_hdc_files(&mut self, files: Vec<PathBuf>, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let directory = self.state.harmony.files_path.clone();
        let count = files.len();
        self.run_task(
            cx,
            format!("正在上传 {count} 个文件…"),
            move || {
                for file in &files {
                    let name = file
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| "upload.bin".to_string());
                    let remote = format!("{}/{}", directory.trim_end_matches('/'), name);
                    let output = client.push(&key, file, &remote)?;
                    let text = output.combined();
                    if !output.success() || text.to_ascii_lowercase().contains("fail") {
                        bail!("{}", text.trim());
                    }
                }
                Ok(count)
            },
            move |core, result, cx| match result {
                Ok(count) => {
                    cx.toast(ToastKind::Success, format!("已上传 {count} 个文件"));
                    core.load_hdc_files(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("上传失败：{err:#}")),
            },
        );
    }

    // ---------------------------------------------------------------------
    // 控制
    // ---------------------------------------------------------------------

    pub(crate) fn hdc_send_key(&mut self, key_event: HdcKey, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            format!("正在发送{}键…", key_event.label()),
            move || {
                let output = client.uitest_key(&key, key_event)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            move |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, format!("已发送{}键", key_event.label())),
                Err(err) => cx.toast(ToastKind::Error, format!("按键失败：{err:#}")),
            },
        );
    }

    pub(crate) fn hdc_tap(&mut self, x: String, y: String, tap: HdcTap, cx: &mut Ctx) {
        let (Some(x), Some(y)) = (parse_coord(&x), parse_coord(&y)) else {
            cx.toast(ToastKind::Error, "坐标必须是 0-20000 之间的整数");
            return;
        };
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            format!("正在{}({x}, {y})…", tap.label()),
            move || {
                let output = client.uitest_tap(&key, x, y, tap)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            move |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, format!("已{}({x}, {y})", tap.label())),
                Err(err) => cx.toast(ToastKind::Error, format!("操作失败：{err:#}")),
            },
        );
    }

    pub(crate) fn hdc_fling(&mut self, direction: HdcSwipeDirection, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            format!("正在{}…", direction.label()),
            move || {
                let output = client.uitest_fling(&key, direction, 600)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            move |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, format!("已{}", direction.label())),
                Err(err) => cx.toast(ToastKind::Error, format!("滑动失败：{err:#}")),
            },
        );
    }

    pub(crate) fn hdc_input_text(&mut self, x: String, y: String, text: String, cx: &mut Ctx) {
        let (Some(x), Some(y)) = (parse_coord(&x), parse_coord(&y)) else {
            cx.toast(ToastKind::Error, "坐标必须是 0-20000 之间的整数");
            return;
        };
        let text = text.trim_end().to_string();
        if text.is_empty() {
            cx.toast(ToastKind::Info, "请输入要发送的文字");
            return;
        }
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let preview = text.chars().take(24).collect::<String>();
        self.run_task(
            cx,
            format!("正在输入文字到 ({x}, {y})…"),
            move || {
                let output = client.uitest_input_text(&key, x, y, &text)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            move |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, format!("已输入「{preview}」")),
                Err(err) => cx.toast(ToastKind::Error, format!("输入失败：{err:#}")),
            },
        );
    }

    pub(crate) fn hdc_wake_screen(&mut self, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在唤醒屏幕…",
            move || {
                let output = client.wake_screen(&key)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, "已发送唤醒指令"),
                Err(err) => cx.toast(ToastKind::Error, format!("唤醒失败：{err:#}")),
            },
        );
    }

    // ---------------------------------------------------------------------
    // 终端
    // ---------------------------------------------------------------------

    pub(crate) fn submit_hdc_terminal(&mut self, command: String, cx: &mut Ctx) {
        let command = command.trim().to_string();
        if command.is_empty() {
            cx.toast(ToastKind::Info, "请输入要执行的 hdc shell 命令");
            return;
        }
        self.state.harmony.terminal_input.clear();
        self.run_hdc_command(command, cx);
    }

    fn run_hdc_command(&mut self, command: String, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            format!("正在执行 {}", ellipsis(&command, 36)),
            move || {
                let started = Instant::now();
                let output = client.shell(&key, &command)?;
                let elapsed = started.elapsed().as_millis();
                let mut text = output.combined();
                if text.trim().is_empty() {
                    text = match output.code {
                        Some(0) => "（命令执行成功，无输出）".to_string(),
                        Some(code) => format!("（命令退出码 {code}，无输出）"),
                        None => "（没有捕获到输出）".to_string(),
                    };
                }
                if text.chars().count() > MAX_OUTPUT {
                    text = text.chars().take(MAX_OUTPUT).collect();
                    text.push_str("\n…（输出过长，已截断）");
                }
                let success = output.success() && !text.to_ascii_lowercase().contains("[fail]");
                Ok((command, text, success, elapsed))
            },
            |core, result, cx| match result {
                Ok((command, output, success, elapsed)) => {
                    core.state.harmony.terminal.insert(
                        0,
                        TerminalEntry {
                            command,
                            output,
                            success,
                            duration_ms: elapsed,
                            time: Local::now().format("%H:%M:%S").to_string(),
                        },
                    );
                    core.state.harmony.terminal.truncate(MAX_TERMINAL);
                }
                Err(err) => {
                    cx.toast(ToastKind::Error, format!("命令执行失败：{err:#}"));
                }
            },
        );
    }

    // ---------------------------------------------------------------------
    // 日志
    // ---------------------------------------------------------------------

    pub(crate) fn load_hdc_logs(&mut self, tag: String, filter: LogFilter, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let level = filter.hilog_level().map(str::to_string);
        let tag = tag.trim().to_string();
        let options = HilogOptions {
            level,
            tag: (!tag.is_empty()).then_some(tag.clone()),
        };
        let task_title = if tag.is_empty() {
            "正在抓取 HiLog…".to_string()
        } else {
            format!("正在抓取 HiLog（标签 {tag}）…")
        };
        self.run_task(
            cx,
            task_title,
            move || {
                let output = client.hilog_capture(&key, &options)?;
                let text = if output.stdout.trim().is_empty() {
                    output.combined()
                } else {
                    output.stdout.clone()
                };
                let mut lines = hdc::parse_hilog(&text);
                let total = lines.len();
                if total > MAX_LOGS {
                    lines = lines.split_off(total - MAX_LOGS);
                }
                Ok(lines)
            },
            |core, result, cx| match result {
                Ok(lines) => {
                    let count = lines.len();
                    core.state.harmony.logs = lines;
                    core.state.harmony.logs_at = Some(Local::now().format("%H:%M:%S").to_string());
                    cx.toast(ToastKind::Success, format!("已抓取 {count} 行 HiLog"));
                }
                Err(err) => cx.toast(ToastKind::Error, format!("抓取 HiLog 失败：{err:#}")),
            },
        );
    }

    pub(crate) fn export_hdc_logs(&mut self, cx: &mut Ctx) {
        if self.state.harmony.logs.is_empty() {
            cx.toast(ToastKind::Info, "没有可导出的日志");
            return;
        }
        let raw = self
            .state
            .harmony
            .logs
            .iter()
            .map(|line| line.raw.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let path = downloads_dir().join(format!(
            "hilog-{}.log",
            Local::now().format("%Y%m%d-%H%M%S")
        ));
        match std::fs::write(&path, raw) {
            Ok(()) => {
                self.state.harmony.last_log_export = Some(path.clone());
                cx.toast(
                    ToastKind::Success,
                    format!("日志已导出到 {}", short_path(&path)),
                );
            }
            Err(err) => cx.toast(ToastKind::Error, format!("导出失败：{err}")),
        }
    }

    pub(crate) fn open_hdc_clear_logs(&mut self, cx: &mut Ctx) {
        cx.confirm_kind(
            "清空设备日志缓冲区？",
            "将执行 hilog -r，设备上已有的 HiLog 缓冲区内容会被删除。",
            "清空日志",
            ConfirmKind::HdcClearLogs,
        );
    }

    pub(crate) fn clear_hdc_logs(&mut self, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在清空设备 HiLog 缓冲区…",
            move || {
                let output = client.hilog_clear(&key)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(())
            },
            |core, result, cx| match result {
                Ok(()) => {
                    core.state.harmony.logs.clear();
                    core.state.harmony.logs_at = None;
                    cx.toast(ToastKind::Success, "已清空设备 HiLog 缓冲区");
                }
                Err(err) => cx.toast(ToastKind::Error, format!("清空失败：{err:#}")),
            },
        );
    }

    pub(crate) fn export_hdc_faultlog(&mut self, cx: &mut Ctx) {
        self.export_hdc_log_dir("/data/log/faultlog/faultlogger/", "harmony-faultlog", cx);
    }

    pub(crate) fn export_hdc_hilog_files(&mut self, cx: &mut Ctx) {
        self.export_hdc_log_dir("/data/log/hilog/", "harmony-hilog", cx);
    }

    fn export_hdc_log_dir(&mut self, remote: &'static str, prefix: &'static str, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let dir = unique_path(
            &downloads_dir().join(format!("{prefix}-{}", Local::now().format("%Y%m%d-%H%M%S"))),
        );
        self.run_task(
            cx,
            "正在导出设备日志目录…",
            move || {
                std::fs::create_dir_all(&dir)?;
                let output = client.pull(&key, remote, &dir)?;
                if output.is_failure() {
                    bail!("{}", output.combined().trim());
                }
                Ok(dir)
            },
            move |core, result, cx| match result {
                Ok(dir) => {
                    core.state.harmony.last_log_export = Some(dir.clone());
                    cx.toast(
                        ToastKind::Success,
                        format!("日志目录已导出到 {}", short_path(&dir)),
                    );
                }
                Err(err) => cx.toast(ToastKind::Error, format!("导出失败：{err:#}")),
            },
        );
    }

    // ---------------------------------------------------------------------
    // 截图与重启
    // ---------------------------------------------------------------------

    pub(crate) fn hdc_screenshot(&mut self, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let dir = self.state.config.media_dir();
        let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
        self.run_task(
            cx,
            "正在截取 HarmonyOS 屏幕…",
            move || {
                let mut errors = Vec::new();
                let jpeg_remote = format!("/data/local/tmp/rarebox-{stamp}.jpeg");
                let jpeg_local = unique_path(&dir.join(format!("rarebox-harmony-{stamp}.jpeg")));
                if let Ok(shot) = client.shell(&key, &format!("snapshot_display -f {jpeg_remote}"))
                {
                    if !shot.is_failure() {
                        match client.pull(&key, &jpeg_remote, &jpeg_local) {
                            Ok(pulled) if !pulled.is_failure() && file_has_data(&jpeg_local) => {
                                let _ = client.shell(&key, &format!("rm -f {jpeg_remote}"));
                                return Ok(jpeg_local);
                            }
                            Ok(pulled) => errors.push(pulled.combined()),
                            Err(err) => errors.push(format!("{err:#}")),
                        }
                    } else {
                        errors.push(shot.combined());
                    }
                }
                let png_remote = format!("/data/local/tmp/rarebox-{stamp}.png");
                let png_local = unique_path(&dir.join(format!("rarebox-harmony-{stamp}.png")));
                match client.screen_cap(&key, &png_remote) {
                    Ok(captured) if !captured.is_failure() => {
                        match client.pull(&key, &png_remote, &png_local) {
                            Ok(pulled) if !pulled.is_failure() && file_has_data(&png_local) => {
                                let _ = client.shell(&key, &format!("rm -f {png_remote}"));
                                return Ok(png_local);
                            }
                            Ok(pulled) => errors.push(pulled.combined()),
                            Err(err) => errors.push(format!("{err:#}")),
                        }
                    }
                    Ok(captured) => errors.push(captured.combined()),
                    Err(err) => errors.push(format!("{err:#}")),
                }
                let _ = client.shell(&key, &format!("rm -f {jpeg_remote} {png_remote}"));
                bail!(
                    "snapshot_display 与 uitest screenCap 均失败：{}",
                    errors
                        .into_iter()
                        .map(|error| error.trim().to_string())
                        .filter(|error| !error.is_empty())
                        .collect::<Vec<_>>()
                        .join("；")
                )
            },
            |core, result, cx| match result {
                Ok(path) => {
                    core.state.harmony.screenshot = Some(path.clone());
                    cx.toast(
                        ToastKind::Success,
                        format!("截图已保存到 {}", short_path(&path)),
                    );
                }
                Err(err) => cx.toast(ToastKind::Error, format!("截图失败：{err:#}")),
            },
        );
    }

    pub(crate) fn open_hdc_reboot(&mut self, target: HdcRebootTarget, cx: &mut Ctx) {
        let Some(key) = self.state.harmony.selected.clone() else {
            cx.toast(ToastKind::Error, "请先连接并选择一台 HarmonyOS 设备");
            return;
        };
        cx.confirm_kind(
            format!("{} HarmonyOS 设备？", target.label()),
            format!(
                "将对 {key} 执行 hdc target boot{}。{}",
                if target.args().is_empty() {
                    String::new()
                } else {
                    format!(" {}", target.args().join(" "))
                },
                match target {
                    HdcRebootTarget::System => "设备会立即重启，未保存的数据可能丢失。",
                    HdcRebootTarget::Shutdown => "设备会立即关机。",
                    HdcRebootTarget::Recovery => "设备会重启到恢复模式。",
                    HdcRebootTarget::Bootloader => "设备会重启到 Bootloader。",
                }
            ),
            target.label(),
            ConfirmKind::HdcReboot(target),
        );
    }

    pub(crate) fn hdc_reboot(&mut self, target: HdcRebootTarget, cx: &mut Ctx) {
        let result = self.require_hdc_device();
        let Ok((client, key)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            format!("正在{}设备…", target.label()),
            move || {
                let output = client.reboot(&key, target)?;
                let text = output.combined();
                if text.to_ascii_lowercase().contains("fail") {
                    bail!("{}", text.trim());
                }
                Ok(())
            },
            move |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, target.outcome()),
                Err(err) => cx.toast(ToastKind::Error, format!("操作失败：{err:#}")),
            },
        );
    }
}

fn ellipsis(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut result: String = text.chars().take(max.saturating_sub(1)).collect();
        result.push('…');
        result
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn unique_path(base: &Path) -> PathBuf {
    if !base.exists() {
        return base.to_path_buf();
    }
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let extension = base
        .extension()
        .map(|extension| extension.to_string_lossy().to_string());
    for index in 1..1000 {
        let name = match &extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    base.to_path_buf()
}

fn parse_port(raw: &str) -> Option<u16> {
    raw.trim().parse::<u16>().ok().filter(|port| *port > 0)
}

fn parse_coord(raw: &str) -> Option<u32> {
    raw.trim()
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= 20_000)
}

fn file_has_data(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsis_keeps_short_commands() {
        assert_eq!(ellipsis("bm dump -a", 36), "bm dump -a");
        let long = "x".repeat(60);
        assert!(ellipsis(&long, 36).chars().count() <= 36);
    }

    #[test]
    fn unique_path_appends_index() {
        let dir = std::env::temp_dir().join("rarebox-hdc-test");
        let _ = std::fs::create_dir_all(&dir);
        let base = dir.join("sample.bin");
        let _ = std::fs::write(&base, b"x");
        let next = unique_path(&base);
        assert_ne!(next, base);
        assert!(next.to_string_lossy().contains("sample (1).bin"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
