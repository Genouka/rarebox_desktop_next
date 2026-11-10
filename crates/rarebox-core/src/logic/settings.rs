//! 设置页：ADB、hdc、媒体目录与关于信息。

use crate::config;
use crate::engine::{Core, Ctx};
use crate::event::{DirectoryPurpose, Event};
use crate::state::ToastKind;

impl Core {
    pub(crate) fn apply_adb_path(&mut self, path: String, cx: &mut Ctx) {
        let path = path.trim().to_string();
        self.state.config.adb_path = path;
        self.state.adb_input = self.state.config.adb_path.clone();
        self.save_config();
        self.state.adb_path = None;
        self.state.adb_version = None;
        self.state.adb_error = None;
        cx.toast(
            ToastKind::Info,
            if self.state.config.adb_path.is_empty() {
                "已切换为自动检测 ADB".to_string()
            } else {
                "正在使用自定义 ADB 路径".to_string()
            },
        );
        self.refresh_devices(cx);
    }

    pub(crate) fn reset_adb_to_auto(&mut self, cx: &mut Ctx) {
        self.state.adb_input.clear();
        self.state.config.adb_path.clear();
        self.save_config();
        cx.toast(ToastKind::Info, "正在自动检测 ADB…");
        self.refresh_devices(cx);
    }

    pub(crate) fn apply_hdc_path(&mut self, path: String, cx: &mut Ctx) {
        let path = path.trim().to_string();
        self.state.config.hdc_path = path;
        self.state.hdc_input = self.state.config.hdc_path.clone();
        self.save_config();
        self.state.harmony.path = None;
        self.state.harmony.version = None;
        self.state.harmony.error = None;
        self.state.harmony.devices.clear();
        self.state.harmony.selected = None;
        self.reset_hdc_device_state(cx);
        cx.toast(
            ToastKind::Info,
            if self.state.config.hdc_path.is_empty() {
                "已切换为自动检测 hdc（优先使用内置版本）".to_string()
            } else {
                "正在使用自定义 hdc 路径".to_string()
            },
        );
        self.refresh_hdc(cx);
    }

    pub(crate) fn reset_hdc_to_auto(&mut self, cx: &mut Ctx) {
        self.state.hdc_input.clear();
        self.state.config.hdc_path.clear();
        self.save_config();
        cx.toast(ToastKind::Info, "正在自动检测 hdc（优先使用内置版本）…");
        self.refresh_hdc(cx);
    }

    pub(crate) fn open_config_dir(&mut self, cx: &mut Ctx) {
        if let Some(path) = config::config_path() {
            let target = path.parent().unwrap_or(&path).to_path_buf();
            if let Err(err) = std::fs::create_dir_all(&target) {
                cx.toast(ToastKind::Error, format!("无法创建配置目录：{err}"));
                return;
            }
            cx.emit(Event::OpenPath(target.to_string_lossy().to_string()));
        } else {
            cx.toast(ToastKind::Error, "无法确定配置目录");
        }
    }

    pub(crate) fn pick_screenshot_dir(&mut self, cx: &mut Ctx) {
        cx.pick_directory(DirectoryPurpose::Screenshot);
    }

    pub(crate) fn set_screenshot_dir(&mut self, path: String, cx: &mut Ctx) {
        self.state.screenshot_input = path.clone();
        self.state.config.screenshot_dir = Some(path.clone());
        self.save_config();
        cx.toast(ToastKind::Success, format!("截图目录已设为 {path}"));
    }

    pub(crate) fn open_media_dir(&mut self, cx: &mut Ctx) {
        let dir = self.state.config.media_dir();
        cx.emit(Event::OpenPath(dir.to_string_lossy().to_string()));
    }

    pub(crate) fn reset_screenshot_dir(&mut self, cx: &mut Ctx) {
        self.state.screenshot_input.clear();
        self.state.config.screenshot_dir = None;
        self.save_config();
        cx.toast(ToastKind::Success, "已恢复默认截图目录");
    }

    pub(crate) fn set_theme(&mut self, dark: bool, seed: u32) {
        self.state.config.dark = dark;
        self.state.config.seed = seed;
        self.save_config();
    }

    /// 手动结束 ADB 服务端进程。
    pub(crate) fn kill_adb_process(&mut self, cx: &mut Ctx) {
        let result = self.adb_client();
        let Ok(client) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在结束 ADB 进程…",
            move || client.kill_server(),
            |core, result, cx| match result {
                Ok(output) if output.success() => {
                    core.state.devices.clear();
                    core.state.selected_serial = None;
                    core.reset_device_state(cx);
                    cx.toast(ToastKind::Success, "ADB 进程已结束");
                }
                Ok(output) => cx.toast(
                    ToastKind::Error,
                    format!("结束 ADB 进程失败：{}", output.error_message()),
                ),
                Err(err) => cx.toast(ToastKind::Error, format!("结束 ADB 进程失败：{err:#}")),
            },
        );
    }

    /// 手动结束 hdc 服务端进程。
    pub(crate) fn kill_hdc_process(&mut self, cx: &mut Ctx) {
        let result = self.hdc_client();
        let Ok(client) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在结束 HDC 进程…",
            move || client.kill_server(),
            |core, result, cx| match result {
                Ok(output) if !output.is_failure() => {
                    core.state.harmony.devices.clear();
                    core.state.harmony.selected = None;
                    core.reset_hdc_device_state(cx);
                    cx.toast(ToastKind::Success, "HDC 进程已结束");
                }
                Ok(output) => cx.toast(
                    ToastKind::Error,
                    format!("结束 HDC 进程失败：{}", output.error_message()),
                ),
                Err(err) => cx.toast(ToastKind::Error, format!("结束 HDC 进程失败：{err:#}")),
            },
        );
    }

    /// 持久化「关闭程序时自动结束进程」开关。
    pub(crate) fn set_kill_processes_on_exit(&mut self, value: bool) {
        self.state.config.kill_processes_on_exit = value;
        self.save_config();
    }
}
