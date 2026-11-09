//! 设备发现、选择与快照。

use anyhow::Context as _;
use chrono::Local;

use crate::adb::{AdbClient, Device, DeviceSnapshot, detect_adb};
use crate::engine::{Core, Ctx};
use crate::state::{DeviceKind, ToastKind};

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
            } else if self.state.harmony.selected.is_some()
                && self.state.harmony.snapshot.is_none()
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
