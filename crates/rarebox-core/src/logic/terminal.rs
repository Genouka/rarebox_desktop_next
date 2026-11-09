//! 终端页：在 Android（adb）设备上执行 shell 命令并保留最近记录。

use std::time::Instant;

use chrono::Local;

use crate::engine::{Core, Ctx};
use crate::state::{TerminalEntry, ToastKind};

const MAX_HISTORY: usize = 30;
const MAX_OUTPUT: usize = 24_000;

impl Core {
    pub(crate) fn submit_terminal(&mut self, command: String, cx: &mut Ctx) {
        let command = command.trim().to_string();
        if command.is_empty() {
            cx.toast(ToastKind::Info, "请输入要执行的命令");
            return;
        }
        self.state.terminal_input.clear();
        self.run_terminal_command(command, cx);
    }

    pub(crate) fn run_quick_command(&mut self, command: String, cx: &mut Ctx) {
        self.state.terminal_input = command.clone();
        self.submit_terminal(command, cx);
    }

    fn run_terminal_command(&mut self, command: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
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
                let output = adb.shell(&serial, &command)?;
                let elapsed = started.elapsed().as_millis();
                let mut text = output.combined();
                if text.trim().is_empty() {
                    text = match output.code {
                        Some(0) => "（命令执行成功，无输出）".to_string(),
                        Some(code) => format!("（命令退出码 {code}，无输出）"),
                        None => "（没有捕获到输出）".to_string(),
                    };
                }
                let mut truncated = false;
                if text.chars().count() > MAX_OUTPUT {
                    text = text.chars().take(MAX_OUTPUT).collect();
                    truncated = true;
                }
                if truncated {
                    text.push_str("\n…（输出过长，已截断）");
                }
                Ok((command, text, output.success(), elapsed))
            },
            |core, result, cx| match result {
                Ok((command, output, success, elapsed)) => {
                    core.state.terminal_history.insert(
                        0,
                        TerminalEntry {
                            command,
                            output,
                            success,
                            duration_ms: elapsed,
                            time: Local::now().format("%H:%M:%S").to_string(),
                        },
                    );
                    core.state.terminal_history.truncate(MAX_HISTORY);
                }
                Err(err) => {
                    cx.toast(ToastKind::Error, format!("命令执行失败：{err:#}"));
                }
            },
        );
    }
}

fn ellipsis(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut result: String = text.chars().take(max).collect();
        result.push('…');
        result
    }
}
