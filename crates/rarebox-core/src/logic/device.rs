//! 设备发现、选择与快照。

use anyhow::Context as _;
use chrono::Local;

use std::path::PathBuf;

use crate::adb::{AdbClient, Device, DeviceSnapshot, MdnsService, detect_adb};
use crate::engine::{Core, Ctx};
use crate::hdc::{self, HdcClient, HdcDevice};
use crate::state::{DeviceKind, ToastKind};

/// 一次 ADB 探测的完整结果。
struct AdbProbe {
    path: PathBuf,
    version: String,
    devices: Vec<Device>,
    mdns: Vec<MdnsService>,
}

/// 一次 hdc 探测的完整结果。
struct HdcProbe {
    path: PathBuf,
    version: String,
    devices: Vec<HdcDevice>,
}

fn probe_adb(configured: &str) -> anyhow::Result<AdbProbe> {
    let hint = if cfg!(target_os = "android") {
        "未找到可用的 ADB，请在设置中指定 adb 路径，或使用无线 ADB 连接网络设备"
    } else {
        "未找到可用的 ADB，请在设置中指定 platform-tools/adb.exe"
    };
    let (path, version) = detect_adb(configured).context(hint)?;
    let client = AdbClient::new(path.clone());
    let devices = client.devices()?;
    let mdns = client.mdns_services().unwrap_or_default();
    Ok(AdbProbe {
        path,
        version,
        devices,
        mdns,
    })
}

fn probe_hdc(configured: &str) -> anyhow::Result<HdcProbe> {
    let hint = if cfg!(target_os = "android") {
        "未找到可用的 hdc：内置 libhdc.so 不可用；请在设置中指定 hdc 路径"
    } else {
        "未找到可用的 hdc：内置 hdc 不可用，且本机未安装 DevEco Studio / OpenHarmony SDK；请在设置中指定 hdc 路径"
    };
    let (path, version) = hdc::detect_hdc(configured).context(hint)?;
    let client = HdcClient::new(path.clone());
    let devices = client.targets()?;
    Ok(HdcProbe {
        path,
        version,
        devices,
    })
}

impl Core {
    pub(crate) fn refresh_devices(&mut self, cx: &mut Ctx) {
        let configured = self.state.config.adb_path.clone();
        self.run_task(
            cx,
            "正在查找 ADB 与设备…",
            move || {
                let hint = if cfg!(target_os = "android") {
                    "未找到可用的 ADB，请在设置中指定 adb 路径，或使用无线 ADB 连接网络设备"
                } else {
                    "未找到可用的 ADB，请在设置中指定 platform-tools/adb.exe"
                };
                let (path, version) = detect_adb(&configured).context(hint)?;
                let client = AdbClient::new(path.clone());
                let devices = client.devices()?;
                Ok((path, version, devices))
            },
            |core, result, cx| match result {
                Ok((path, version, devices)) => {
                    core.state.adb_path = Some(path);
                    core.state.adb_version = Some(version);
                    core.state.adb_error = None;
                    core.apply_devices(devices, cx);
                    if core.state.selected_serial.is_some() {
                        core.load_snapshot(cx);
                    }
                }
                Err(err) => {
                    core.state.adb_path = None;
                    core.state.adb_version = None;
                    core.state.adb_error = Some(format!("{err:#}"));
                    core.state.devices.clear();
                    core.state.selected_serial = None;
                    core.state.snapshot = None;
                    cx.toast(ToastKind::Error, format!("ADB 未就绪：{err:#}"));
                }
            },
        );
    }

    fn apply_devices(&mut self, devices: Vec<Device>, cx: &mut Ctx) {
        let previous = self.state.selected_serial.clone();
        self.state.devices = devices;
        let online: Vec<String> = self
            .state
            .devices
            .iter()
            .filter(|device| device.is_online())
            .map(|device| device.serial.clone())
            .collect();
        let next = previous
            .clone()
            .filter(|serial| online.iter().any(|candidate| candidate == serial))
            .or_else(|| online.first().cloned());
        self.state.selected_serial = next.clone();
        if next != previous {
            self.reset_device_state(cx);
        }
    }

    /// 同时探测 ADB / hdc / mDNS，并据此决定当前工作区。
    ///
    /// 两个工具互不依赖，缺一个不影响另一个；只有「自动」模式才会改变
    /// 当前工作区，且只在明确只有一侧在线时切换，避免两侧都插着时来回跳。
    pub(crate) fn refresh_all(&mut self, cx: &mut Ctx) {
        let adb_configured = self.state.config.adb_path.clone();
        let hdc_configured = self.state.config.hdc_path.clone();
        self.run_task(
            cx,
            "正在识别设备…",
            move || {
                let (adb, hdc) = std::thread::scope(|scope| {
                    let adb = scope.spawn(move || probe_adb(&adb_configured));
                    let hdc = scope.spawn(move || probe_hdc(&hdc_configured));
                    (
                        adb.join().unwrap_or_else(|_| Err(anyhow::anyhow!("ADB 探测线程异常"))),
                        hdc.join().unwrap_or_else(|_| Err(anyhow::anyhow!("hdc 探测线程异常"))),
                    )
                });
                let mdns = adb
                    .as_ref()
                    .ok()
                    .map(|probe| probe.mdns.clone())
                    .unwrap_or_default();
                Ok((adb, hdc, mdns))
            },
            |core, result, cx| {
                let (adb, hdc, mdns) = match result {
                    Ok(value) => value,
                    Err(err) => {
                        cx.toast(ToastKind::Error, format!("识别设备失败：{err:#}"));
                        return;
                    }
                };

                match adb {
                    Ok(probe) => {
                        core.state.adb_path = Some(probe.path);
                        core.state.adb_version = Some(probe.version);
                        core.state.adb_error = None;
                        core.apply_devices(probe.devices, cx);
                    }
                    Err(err) => {
                        core.state.adb_path = None;
                        core.state.adb_version = None;
                        core.state.adb_error = Some(format!("{err:#}"));
                        core.state.devices.clear();
                        core.state.selected_serial = None;
                        core.state.snapshot = None;
                    }
                }
                core.state.mdns = mdns;

                match hdc {
                    Ok(probe) => {
                        core.state.harmony.path = Some(probe.path);
                        core.state.harmony.version = Some(probe.version);
                        core.state.harmony.error = None;
                        core.apply_hdc_devices(probe.devices, cx);
                    }
                    Err(err) => {
                        core.state.harmony.path = None;
                        core.state.harmony.version = None;
                        core.state.harmony.error = Some(format!("{err:#}"));
                        core.state.harmony.devices.clear();
                        core.state.harmony.selected = None;
                        core.reset_hdc_device_state(cx);
                    }
                }

                core.resolve_auto_kind(cx);
                // 识别完再补读快照，能力矩阵才有数据；已加载的不重复读。
                if core.state.is_harmony() {
                    if core.state.harmony.selected.is_some() && core.state.harmony.snapshot.is_none()
                    {
                        core.load_hdc_snapshot(cx);
                    }
                } else if core.state.selected_serial.is_some() && core.state.snapshot.is_none() {
                    core.load_snapshot(cx);
                }
            },
        );
    }

    /// 自动模式下根据在线设备切换工作区。
    fn resolve_auto_kind(&mut self, cx: &mut Ctx) {
        if !self.state.kind_mode.is_auto() {
            return;
        }
        let android_online = self.state.devices.iter().any(Device::is_online);
        let harmony_online = self
            .state
            .harmony
            .devices
            .iter()
            .any(|device| device.is_online());
        let next = match (android_online, harmony_online) {
            (true, false) => Some(DeviceKind::Android),
            (false, true) => Some(DeviceKind::Harmony),
            // 两侧都有或都没有：保持当前工作区，避免来回抖动。
            _ => None,
        };
        if let Some(next) = next
            && self.state.device_kind != next
        {
            self.set_device_kind(next, cx);
        }
    }

    pub(crate) fn set_kind_mode(&mut self, mode: crate::state::KindMode, cx: &mut Ctx) {
        self.state.kind_mode = mode;
        match mode.locked() {
            Some(kind) => self.set_device_kind(kind, cx),
            None => self.resolve_auto_kind(cx),
        }
    }

    pub(crate) fn select_device(&mut self, serial: String, cx: &mut Ctx) {
        if self.state.device_kind != DeviceKind::Android {
            self.state.device_kind = DeviceKind::Android;
        }
        if self.state.selected_serial.as_deref() == Some(serial.as_str()) {
            return;
        }
        self.state.selected_serial = Some(serial);
        self.reset_device_state(cx);
        self.load_snapshot(cx);
    }

    pub(crate) fn reset_device_state(&mut self, cx: &mut Ctx) {
        self.stop_remote(cx);
        self.state.snapshot = None;
        self.state.snapshot_at = None;
        self.state.packages.clear();
        self.state.package_loaded_for = None;
        self.state.package_metadata_for = None;
        self.state.install_batch = None;
        self.state.install_plan = None;
        self.state.logs.clear();
        self.state.logs_raw.clear();
        self.state.logs_at = None;
        self.state.files_entries.clear();
        self.state.files_selected = None;
        self.state.files_path = "/sdcard".to_string();
        self.state.screen_size.clear();
        self.state.screen_density.clear();
        self.state.imes.clear();
        self.state.ime_loaded_for = None;
        self.state.appops_ops.clear();
        self.state.appops_loaded_for = None;
    }

    pub(crate) fn load_snapshot(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在读取设备信息…",
            move || adb.snapshot(&serial),
            |core, result, cx| match result {
                Ok(snapshot) => {
                    core.state.adb_error = None;
                    core.state.snapshot_at = Some(Local::now().format("%H:%M:%S").to_string());
                    core.sync_screen_fields(&snapshot);
                    core.state.snapshot = Some(snapshot);
                }
                Err(err) => {
                    cx.toast(ToastKind::Error, format!("读取设备信息失败：{err:#}"));
                }
            },
        );
    }

    fn sync_screen_fields(&mut self, snapshot: &DeviceSnapshot) {
        let size = snapshot.display.size().to_string();
        if !size.is_empty() {
            self.state.screen_size = size;
        }
        let density = snapshot.display.density().to_string();
        if !density.is_empty() {
            self.state.screen_density = density;
        }
    }

    pub(crate) fn set_device_kind(&mut self, kind: DeviceKind, cx: &mut Ctx) {
        if self.state.device_kind == kind {
            return;
        }
        self.state.device_kind = kind;
        if kind == DeviceKind::Harmony {
            self.stop_remote(cx);
            if self.state.harmony.path.is_none() {
                self.refresh_hdc(cx);
            } else if self.state.harmony.selected.is_some() && self.state.harmony.snapshot.is_none()
            {
                self.load_hdc_snapshot(cx);
                self.load_hdc_forwards(cx);
            }
        } else if self.state.adb_path.is_none() {
            self.refresh_devices(cx);
        } else if self.state.selected_serial.is_some() && self.state.snapshot.is_none() {
            self.load_snapshot(cx);
        }
    }
}

impl Core {
    pub fn has_device(&self) -> bool {
        self.state.has_device()
    }

    pub fn is_harmony(&self) -> bool {
        self.state.is_harmony()
    }
}
