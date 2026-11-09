//! 屏幕与显示页：截图、录屏、分辨率、DPI、旋转与输入控制。

use std::path::PathBuf;

use anyhow::{Context as _, bail};

use chrono::Local;

use crate::config::short_path;
use crate::engine::{Core, Ctx};
use crate::state::ToastKind;

impl Core {
    pub(crate) fn take_screenshot(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let dir = self.state.config.media_dir();
        self.run_task(
            cx,
            "正在截取屏幕…",
            move || {
                let bytes = adb.exec_out_bytes(&serial, &["screencap", "-p"])?;
                if !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                    bail!("设备返回的截图数据不是有效的 PNG，可能被系统换行转换破坏");
                }
                let path = dir.join(format!(
                    "rarebox-screen-{}.png",
                    Local::now().format("%Y%m%d-%H%M%S")
                ));
                std::fs::write(&path, bytes)
                    .with_context(|| format!("无法写入截图 {}", path.display()))?;
                Ok(path)
            },
            |core, result, cx| match result {
                Ok(path) => {
                    core.state.last_screenshot = Some(path.clone());
                    cx.toast(
                        ToastKind::Success,
                        format!("截图已保存到 {}", short_path(&path)),
                    );
                }
                Err(err) => cx.toast(ToastKind::Error, format!("截图失败：{err:#}")),
            },
        );
    }

    pub(crate) fn record_screen(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let dir = self.state.config.media_dir();
        self.run_task(
            cx,
            "正在录制 10 秒屏幕…",
            move || {
                let stamp = Local::now().format("%Y%m%d-%H%M%S");
                let remote = format!("/sdcard/rarebox-{stamp}.mp4");
                let record = adb.shell(
                    &serial,
                    &format!("screenrecord --time-limit 10 --bit-rate 6000000 {remote}"),
                )?;
                if !record.success() {
                    bail!("{}", record.error_message());
                }
                let path: PathBuf = dir.join(format!("rarebox-record-{stamp}.mp4"));
                let path_string = path.to_string_lossy().to_string();
                let pulled =
                    adb.serial_command(&serial, &["pull", remote.as_str(), path_string.as_str()])?;
                if !pulled.success() {
                    bail!("{}", pulled.error_message());
                }
                if let Err(err) = adb.shell(&serial, &format!("rm -f {remote}")) {
                    eprintln!("RareBox: 清理设备录屏文件失败: {err:#}");
                }
                Ok(path)
            },
            |core, result, cx| match result {
                Ok(path) => {
                    core.state.last_recording = Some(path.clone());
                    cx.toast(
                        ToastKind::Success,
                        format!("录屏已保存到 {}", short_path(&path)),
                    );
                }
                Err(err) => cx.toast(ToastKind::Error, format!("录屏失败：{err:#}")),
            },
        );
    }

    pub(crate) fn apply_display_size(&mut self, raw: String, cx: &mut Ctx) {
        let raw = raw.trim().to_string();
        let Some((width, height)) = raw.split_once('x') else {
            cx.toast(ToastKind::Error, "分辨率格式应为 宽x高，例如 450x450");
            return;
        };
        let (Ok(width), Ok(height)) = (width.trim().parse::<u32>(), height.trim().parse::<u32>())
        else {
            cx.toast(ToastKind::Error, "分辨率必须由两个正整数组成");
            return;
        };
        if !(240..=2000).contains(&width) || !(240..=2000).contains(&height) {
            cx.toast(ToastKind::Error, "分辨率超出安全范围（240–2000）");
            return;
        }
        self.run_shell_command(
            cx,
            "正在应用分辨率…",
            format!("wm size {width}x{height}"),
            format!("分辨率已设置为 {width}x{height}"),
            true,
        );
    }

    pub(crate) fn reset_display_size(&mut self, cx: &mut Ctx) {
        self.run_shell_command(
            cx,
            "正在重置分辨率…",
            "wm size reset".to_string(),
            "分辨率已恢复为物理值".to_string(),
            true,
        );
    }

    pub(crate) fn apply_density(&mut self, raw: String, cx: &mut Ctx) {
        let raw = raw.trim().to_string();
        let Ok(density) = raw.parse::<u32>() else {
            cx.toast(ToastKind::Error, "DPI 必须是数字，例如 320");
            return;
        };
        if !(72..=960).contains(&density) {
            cx.toast(ToastKind::Error, "DPI 超出安全范围（72–960）");
            return;
        }
        self.run_shell_command(
            cx,
            "正在应用 DPI…",
            format!("wm density {density}"),
            format!("DPI 已设置为 {density}"),
            true,
        );
    }

    pub(crate) fn reset_density(&mut self, cx: &mut Ctx) {
        self.run_shell_command(
            cx,
            "正在重置 DPI…",
            "wm density reset".to_string(),
            "DPI 已恢复为物理值".to_string(),
            true,
        );
    }

    pub(crate) fn set_rotation(&mut self, rotation: u8, cx: &mut Ctx) {
        self.run_shell_command(
            cx,
            "正在旋转屏幕…",
            format!(
                "settings put system accelerometer_rotation 0; settings put system user_rotation {rotation}"
            ),
            format!("屏幕已旋转到 {}", rotation_label(rotation)),
            true,
        );
    }

    pub(crate) fn send_key(&mut self, code: u32, label: String, cx: &mut Ctx) {
        self.run_shell_command(
            cx,
            format!("正在发送 {label} 键…"),
            format!("input keyevent {code}"),
            format!("已发送 {label} 键"),
            false,
        );
    }

    pub(crate) fn send_text(&mut self, text: String, cx: &mut Ctx) {
        let text = text.trim().to_string();
        if text.is_empty() {
            cx.toast(ToastKind::Info, "请先输入要发送的文本");
            return;
        }
        let escaped = text
            .replace('\\', "\\\\")
            .replace(' ', "%s")
            .replace('\'', "'\\''");
        self.state.screen_text.clear();
        self.run_shell_command(
            cx,
            "正在发送文本…",
            format!("input text '{escaped}'"),
            "文本已发送到设备".to_string(),
            false,
        );
    }

    fn run_shell_command(
        &mut self,
        cx: &mut Ctx,
        label: impl Into<String>,
        command: String,
        success: String,
        refresh_snapshot: bool,
    ) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            label,
            move || {
                let output = adb.shell(&serial, &command)?;
                if !output.success() && output.stdout.trim().is_empty() {
                    bail!("{}", output.error_message());
                }
                Ok(output.combined())
            },
            move |core, result, cx| match result {
                Ok(output) => {
                    cx.toast(ToastKind::Success, success);
                    if !output.trim().is_empty() {
                        eprintln!("RareBox: shell: {}", output.trim());
                    }
                    if refresh_snapshot {
                        core.load_snapshot(cx);
                    }
                }
                Err(err) => cx.toast(ToastKind::Error, format!("操作失败：{err:#}")),
            },
        );
    }
}

pub fn rotation_label(rotation: u8) -> &'static str {
    match rotation {
        1 => "90°",
        2 => "180°",
        3 => "270°",
        _ => "0°",
    }
}
