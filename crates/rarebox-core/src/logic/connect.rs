//! 连接页：Android（ADB）与 HarmonyOS（hdc）的连接引导、配对、TCP/IP 与设备切换。

use anyhow::bail;

use crate::engine::{Core, Ctx};
use crate::state::{DeviceKind, ToastKind};

impl Core {
    pub(crate) fn wireless_connect(&mut self, address: String, cx: &mut Ctx) {
        let mut address = address.trim().to_string();
        if address.is_empty() {
            cx.toast(ToastKind::Info, "请输入手表的无线调试地址");
            return;
        }
        if !address.contains(':') {
            address = format!("{address}:5555");
        }
        let result = self.adb_client();
        let Ok(adb) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            format!("正在连接 {address}…"),
            move || {
                let output = adb.run(&["connect".to_string(), address.clone()])?;
                let combined = output.combined();
                if !output.success()
                    || combined.to_ascii_lowercase().contains("failed")
                    || combined.to_ascii_lowercase().contains("cannot")
                {
                    bail!("{}", output.error_message());
                }
                Ok(combined)
            },
            move |core, result, cx| match result {
                Ok(message) => {
                    cx.toast(ToastKind::Success, message.trim().to_string());
                    core.refresh_devices(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("无线连接失败：{err:#}")),
            },
        );
    }

    pub(crate) fn wireless_pair(&mut self, address: String, code: String, cx: &mut Ctx) {
        let address = address.trim().to_string();
        let code = code.trim().to_string();
        if address.is_empty() || code.is_empty() {
            cx.toast(ToastKind::Info, "请填写配对地址与配对码");
            return;
        }
        let result = self.adb_client();
        let Ok(adb) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在配对无线调试…",
            move || {
                let output = adb.run(&["pair".to_string(), address.clone(), code.clone()])?;
                let combined = output.combined();
                if !output.success()
                    || combined.to_ascii_lowercase().contains("failed")
                    || combined.to_ascii_lowercase().contains("cannot")
                {
                    bail!("{}", output.error_message());
                }
                Ok(combined)
            },
            move |core, result, cx| match result {
                Ok(message) => {
                    cx.toast(ToastKind::Success, message.trim().to_string());
                    core.refresh_devices(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("无线配对失败：{err:#}")),
            },
        );
    }

    pub(crate) fn wireless_disconnect(&mut self, cx: &mut Ctx) {
        let result = self.adb_client();
        let Ok(adb) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在断开无线连接…",
            move || {
                let output = adb.run(&["disconnect".to_string()])?;
                if !output.success() {
                    bail!("{}", output.error_message());
                }
                Ok(output.combined())
            },
            |core, result, cx| match result {
                Ok(_) => {
                    cx.toast(ToastKind::Success, "已断开无线连接");
                    core.refresh_devices(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("断开失败：{err:#}")),
            },
        );
    }

    pub(crate) fn enable_tcpip(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在切换 ADB 到 TCP/IP 模式…",
            move || {
                let output = adb.serial_command(&serial, &["tcpip", "5555"])?;
                if !output.success() {
                    bail!("{}", output.error_message());
                }
                Ok(output.combined())
            },
            |core, result, cx| match result {
                Ok(message) => {
                    let ip = core
                        .state
                        .snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.ip.clone())
                        .unwrap_or_else(|| "<手表 IP>".to_string());
                    core.state.wireless_address = format!("{ip}:5555");
                    cx.toast(
                        ToastKind::Success,
                        format!(
                            "已开启 TCP/IP 5555 · {}. 拔掉数据线后用无线连接。",
                            message.trim()
                        ),
                    );
                }
                Err(err) => cx.toast(ToastKind::Error, format!("开启 TCP/IP 失败：{err:#}")),
            },
        );
    }

    /// 断开指定设备的无线连接。
    pub(crate) fn disconnect_device(&mut self, serial: String, cx: &mut Ctx) {
        let result = self.adb_client();
        let Ok(adb) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let label = serial.clone();
        self.run_task(
            cx,
            format!("正在断开 {serial}…"),
            move || {
                let output = adb.run(&["disconnect".to_string(), serial.clone()])?;
                if !output.success() {
                    bail!("{}", output.error_message());
                }
                Ok(output.combined())
            },
            move |core, result, cx| match result {
                Ok(_) => {
                    cx.toast(ToastKind::Success, format!("已断开 {label}"));
                    core.refresh_devices(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("断开失败：{err:#}")),
            },
        );
    }

    /// 安装内置的 Windows USB 驱动（UniversalAdbDriver）。
    ///
    /// 真正写驱动存储需要管理员权限；`adb_driver::install` 内部会按需弹出 UAC 提权，
    /// 这里只负责把它放进后台任务并转成提示。
    pub(crate) fn install_driver(&mut self, cx: &mut Ctx) {
        self.run_task(
            cx,
            "正在安装 USB 驱动…",
            crate::adb_driver::install,
            |_core, result, cx| match result {
                Ok(()) => cx.toast(
                    ToastKind::Success,
                    "USB 驱动已安装，重新插拔手表即可被 ADB 识别",
                ),
                Err(err) => cx.toast(ToastKind::Error, format!("驱动安装失败：{err:#}")),
            },
        );
    }

    /// 卸载内置的 Windows USB 驱动。同样会在需要管理员时弹出 UAC。
    pub(crate) fn uninstall_driver(&mut self, cx: &mut Ctx) {
        self.run_task(
            cx,
            "正在卸载 USB 驱动…",
            crate::adb_driver::uninstall,
            |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, "USB 驱动已卸载"),
                Err(err) => cx.toast(ToastKind::Error, format!("驱动卸载失败：{err:#}")),
            },
        );
    }
}

/// 把底层报错归类，界面据此给修复入口（扫描 / 配对 / 换网）。
pub fn classify_connect_error(raw: &str) -> crate::state::ConnectErrorKind {
    use crate::state::ConnectErrorKind as Kind;
    let lower = raw.to_ascii_lowercase();
    if lower.contains("authenticate") || lower.contains("unauthorized") {
        Kind::NeedsPairing
    } else if lower.contains("refused")
        || lower.contains("failed to connect")
        || lower.contains("cannot connect")
        || lower.contains("connection reset")
    {
        Kind::PortMismatch
    } else if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("unreachable")
        || lower.contains("no route")
    {
        Kind::Unreachable
    } else {
        Kind::Other
    }
}

impl Core {
    /// 一个输入框连接任意设备：先试 ADB，再试 hdc，成功的一侧决定工作区。
    pub(crate) fn connect_smart(&mut self, address: String, code: String, cx: &mut Ctx) {
        let (host, port) = crate::scan::parse_endpoint(&address);
        let Some(host) = host else {
            cx.toast(ToastKind::Info, "请输入手表的无线调试地址");
            return;
        };
        let code = code.trim().to_string();
        let adb_path = self.state.adb_path.clone();
        let hdc_path = self.state.harmony.path.clone();
        let pair_address = port
            .map(|port| format!("{host}:{port}"))
            .unwrap_or_else(|| host.clone());
        let try_ports: Vec<u16> = match port {
            Some(port) => vec![port],
            // 没填端口时按两套工具的默认端口各试一次。
            None => vec![5555, 8710],
        };
        self.state.connect_error = None;
        let task_pair_address = pair_address.clone();
        self.run_task(
            cx,
            format!("正在连接 {host}…"),
            move || {
                let mut errors: Vec<String> = Vec::new();

                // 1. 有配对码时先 pairing（ADB TLS）。
                if !code.is_empty() {
                    let Some(adb_path) = adb_path.as_ref() else {
                        bail!("未找到 ADB，无法配对");
                    };
                    let adb = crate::adb::AdbClient::new(adb_path.clone());
                    let args = vec!["pair".to_string(), task_pair_address.clone(), code.clone()];
                    match adb.run_timed(&args, std::time::Duration::from_secs(20))? {
                        Some(output) if output.success() => {}
                        Some(output) => {
                            let text = output.combined();
                            bail!(
                                "{{\"stage\":\"pair\",\"text\":{}}}",
                                serde_json::to_string(text.trim()).unwrap_or_default()
                            );
                        }
                        None => bail!("{{\"stage\":\"pair\",\"text\":\"配对超时\"}}"),
                    }
                }

                // 2. ADB：逐个端口尝试。
                if let Some(adb_path) = adb_path.as_ref() {
                    let adb = crate::adb::AdbClient::new(adb_path.clone());
                    for port in &try_ports {
                        let address = format!("{host}:{port}");
                        let args = vec!["connect".to_string(), address.clone()];
                        if let Ok(Some(output)) =
                            adb.run_timed(&args, std::time::Duration::from_secs(4))
                        {
                            let text = output.combined();
                            let lower = text.to_ascii_lowercase();
                            if output.success()
                                && !lower.contains("failed")
                                && !lower.contains("cannot")
                            {
                                return Ok(SmartConnect {
                                    address,
                                    kind: crate::state::DeviceKind::Android,
                                });
                            }
                            errors.push(text);
                        }
                    }
                }

                // 3. hdc：逐个端口尝试。
                if let Some(hdc_path) = hdc_path.as_ref() {
                    let hdc = crate::hdc::HdcClient::new(hdc_path.clone());
                    for port in &try_ports {
                        let address = format!("{host}:{port}");
                        let output = hdc.tconn(&address)?;
                        let text = output.combined();
                        if !output.is_failure() {
                            return Ok(SmartConnect {
                                address,
                                kind: crate::state::DeviceKind::Harmony,
                            });
                        }
                        errors.push(text);
                    }
                }

                let detail = errors
                    .last()
                    .map(|text| text.trim().to_string())
                    .unwrap_or_else(|| "连接被拒绝".to_string());
                bail!(
                    "{{\"stage\":\"connect\",\"text\":{}}}",
                    serde_json::to_string(&detail).unwrap_or_default()
                )
            },
            move |core, result, cx| match result {
                Ok(done) => {
                    core.state.connect_error = None;
                    match done.kind {
                        DeviceKind::Android => {
                            core.state.kind_mode = crate::state::KindMode::Auto;
                            core.state.wireless_address = done.address.clone();
                            core.refresh_all(cx);
                        }
                        DeviceKind::Harmony => {
                            core.state.harmony.selected = Some(done.address.clone());
                            core.reset_hdc_device_state(cx);
                            core.set_device_kind(DeviceKind::Harmony, cx);
                            core.refresh_all(cx);
                        }
                    }
                    cx.toast(ToastKind::Success, format!("已连接 {}", done.address));
                }
                Err(err) => {
                    let raw = format!("{err:#}");
                    let detail = extract_error_text(&raw);
                    let kind = classify_connect_error(&detail);
                    core.state.connect_error = Some(crate::state::ConnectErrorInfo {
                        address: pair_address.clone(),
                        kind,
                        detail: detail.clone(),
                    });
                    cx.toast(ToastKind::Error, format!("连接失败：{detail}"));
                }
            },
        );
    }

    /// 盲扫：先 TCP 探活，再用 adb/hdc 校验协议。
    pub(crate) fn scan_devices(&mut self, address: String, deep: bool, cx: &mut Ctx) {
        let plan = crate::scan::build_plan(&address, deep);
        if plan.hosts.is_empty() {
            cx.toast(ToastKind::Error, "找不到本机网段，请直接输入手表的 IP");
            return;
        }
        let adb_path = self.state.adb_path.clone();
        let hdc_path = self.state.harmony.path.clone();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.scan_cancel = cancel.clone();
        self.state.scan = crate::state::ScanState {
            running: true,
            done: 0,
            total: plan.total() as u32,
            hits: Vec::new(),
            scope: plan.scope.clone(),
            deep,
        };
        let events = self.events.clone();
        self.run_task(
            cx,
            format!("正在扫描 {}…", plan.scope),
            move || {
                use std::sync::atomic::AtomicUsize;
                let done = AtomicUsize::new(0);
                let progress = events.clone();
                let cancel_probe = cancel.clone();
                let done_probe = &done;
                let hits = crate::scan::probe(
                    &plan,
                    std::time::Duration::from_millis(if deep { 120 } else { 300 }),
                    if deep { 128 } else { 64 },
                    &cancel_probe,
                    done_probe,
                );
                let _ = progress;
                let mut results = Vec::new();
                for hit in hits {
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    // 协议校验：adb connect / hdc tconn 成功才算设备。
                    let mut kind = None;
                    let mut state = "端口开放".to_string();
                    if let Some(adb_path) = adb_path.as_ref() {
                        let adb = crate::adb::AdbClient::new(adb_path.clone());
                        let args = vec!["connect".to_string(), hit.address.clone()];
                        if let Ok(Some(output)) =
                            adb.run_timed(&args, std::time::Duration::from_millis(1500))
                        {
                            let text = output.combined().to_ascii_lowercase();
                            if output.success()
                                && !text.contains("failed")
                                && !text.contains("cannot")
                            {
                                kind = Some(DeviceKind::Android);
                                state = "ADB 可连接".to_string();
                            }
                        }
                    }
                    if kind.is_none()
                        && let Some(hdc_path) = hdc_path.as_ref()
                    {
                        let hdc = crate::hdc::HdcClient::new(hdc_path.clone());
                        if let Ok(output) = hdc.tconn(&hit.address)
                            && !output.is_failure()
                        {
                            kind = Some(DeviceKind::Harmony);
                            state = "hdc 可连接".to_string();
                        }
                    }
                    results.push(crate::state::ScanHit {
                        address: hit.address,
                        kind,
                        state,
                    });
                }
                Ok(results)
            },
            move |core, result, cx| {
                core.state.scan.running = false;
                match result {
                    Ok(hits) => {
                        core.state.scan.done = core.state.scan.total;
                        core.state.scan.hits = hits;
                        if core.state.scan.hits.is_empty() {
                            cx.toast(ToastKind::Info, "没有扫到设备，试试手动输入地址");
                        }
                    }
                    Err(err) => {
                        cx.toast(ToastKind::Error, format!("扫描失败：{err:#}"));
                    }
                }
            },
        );
    }

    pub(crate) fn cancel_scan(&mut self) {
        self.scan_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.state.scan.running = false;
    }
}

/// 智能连接成功后的结果。
struct SmartConnect {
    address: String,
    kind: DeviceKind,
}

/// 从包装过的错误 JSON 里取出给用户看的正文。
fn extract_error_text(raw: &str) -> String {
    if let Some(start) = raw.find("{\"stage\"")
        && let Ok(value) =
            serde_json::from_str::<serde_json::Value>(&raw[start..raw.rfind('}').map_or(raw.len(), |i| i + 1)])
    {
        if let Some(text) = value.get("text").and_then(|text| text.as_str()) {
            return text.to_string();
        }
    }
    raw.to_string()
}
