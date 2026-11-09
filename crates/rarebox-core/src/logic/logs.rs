//! 日志查看器：抓取 Logcat、按级别筛选并导出。

use chrono::Local;

use crate::adb::parse_logcat;
use crate::config::{downloads_dir, short_path};
use crate::engine::{Core, Ctx};
use crate::state::{LogFilter, ToastKind};

pub const DISPLAY_LIMIT: usize = 500;

impl Core {
    pub(crate) fn load_logs(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在抓取 Logcat…",
            move || {
                let output = adb.serial_command(&serial, &["logcat", "-d", "-t", "1500"])?;
                if !output.success() {
                    anyhow::bail!("{}", output.error_message());
                }
                Ok(output.stdout)
            },
            |core, result, cx| match result {
                Ok(raw) => {
                    let count = parse_logcat(&raw).len();
                    core.state.logs_raw = raw.clone();
                    core.state.logs = parse_logcat(&raw);
                    core.state.logs_at = Some(Local::now().format("%H:%M:%S").to_string());
                    cx.toast(ToastKind::Success, format!("已抓取 {count} 条日志"));
                }
                Err(err) => {
                    cx.toast(ToastKind::Error, format!("抓取日志失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn clear_logcat(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在清空设备日志…",
            move || {
                let output = adb.serial_command(&serial, &["logcat", "-c"])?;
                if !output.success() {
                    anyhow::bail!("{}", output.error_message());
                }
                Ok(())
            },
            |core, result, cx| match result {
                Ok(()) => {
                    core.state.logs.clear();
                    core.state.logs_raw.clear();
                    core.state.logs_at = None;
                    cx.toast(ToastKind::Success, "设备日志与本地列表已清空");
                }
                Err(err) => cx.toast(ToastKind::Error, format!("清空日志失败：{err:#}")),
            },
        );
    }

    pub(crate) fn export_logs(&mut self, query: String, filter: LogFilter, cx: &mut Ctx) {
        let query = query.trim().to_ascii_lowercase();
        let exported: Vec<String> = self
            .state
            .logs
            .iter()
            .filter(|line| filter.accepts(line.level))
            .filter(|line| {
                query.is_empty()
                    || line.message.to_ascii_lowercase().contains(&query)
                    || line.tag.to_ascii_lowercase().contains(&query)
                    || line.raw.to_ascii_lowercase().contains(&query)
            })
            .map(|line| line.raw.clone())
            .collect();
        if exported.is_empty() {
            cx.toast(ToastKind::Info, "当前筛选下没有可导出的日志");
            return;
        }
        let content = exported.join("\n");
        let path = downloads_dir().join(format!(
            "rarebox-logcat-{}.txt",
            Local::now().format("%Y%m%d-%H%M%S")
        ));
        match std::fs::write(&path, content) {
            Ok(()) => {
                self.state.last_export = Some(path.clone());
                cx.toast(
                    ToastKind::Success,
                    format!("已导出 {} 条日志到 {}", exported.len(), short_path(&path)),
                );
            }
            Err(err) => {
                cx.toast(ToastKind::Error, format!("导出日志失败：{err}"));
            }
        }
    }
}
