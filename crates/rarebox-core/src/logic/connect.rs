//! 连接页：Android（ADB）与 HarmonyOS（hdc）的连接引导、配对、TCP/IP 与设备切换。

use anyhow::bail;

use crate::engine::{Core, Ctx};
use crate::state::ToastKind;

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
