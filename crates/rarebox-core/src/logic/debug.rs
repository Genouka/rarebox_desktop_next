//! 调试工具页：输入法（ime）与 AppOps（appops）管理。

use anyhow::bail;

use crate::adb::AppOpMode;
use crate::engine::{Core, Ctx};
use crate::state::ToastKind;

impl Core {
    /// 读取设备输入法列表。
    pub(crate) fn load_imes(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.state.ime_loaded_for = Some(serial.clone());
        self.run_task(
            cx,
            "正在读取输入法列表…",
            move || adb.list_input_methods(&serial),
            |core, result, cx| match result {
                Ok(imes) => {
                    core.state.imes = imes;
                }
                Err(err) => {
                    core.state.ime_loaded_for = None;
                    cx.toast(ToastKind::Error, format!("读取输入法失败：{err:#}"));
                }
            },
        );
    }

    /// 激活一个输入法：必要时先启用，再设为当前默认。
    pub(crate) fn activate_ime(&mut self, id: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let label = ime_display(&id);
        self.run_task(
            cx,
            format!("正在激活输入法 {label}…"),
            move || adb.activate_input_method(&serial, &id),
            move |core, result, cx| match result {
                Ok(()) => {
                    cx.toast(ToastKind::Success, format!("已切换到 {label}"));
                    core.load_imes(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("激活失败：{err:#}")),
            },
        );
    }

    /// 启用或停用一个输入法。
    pub(crate) fn set_ime_enabled(&mut self, id: String, enabled: bool, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let label = ime_display(&id);
        let action = if enabled { "启用" } else { "停用" };
        self.run_task(
            cx,
            format!("正在{action}输入法 {label}…"),
            move || adb.set_input_method_enabled(&serial, &id, enabled),
            move |core, result, cx| match result {
                Ok(()) => {
                    cx.toast(ToastKind::Success, format!("已{action} {label}"));
                    core.load_imes(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("{action}失败：{err:#}")),
            },
        );
    }

    /// 打开系统输入法设置页。
    pub(crate) fn open_ime_settings(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            "正在打开系统输入法设置…",
            move || {
                let output = adb.shell(
                    &serial,
                    "am start -a android.settings.INPUT_METHOD_SETTINGS",
                )?;
                if !output.success() {
                    bail!("{}", output.error_message());
                }
                Ok(())
            },
            |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Info, "已在手表上打开输入法设置"),
                Err(err) => cx.toast(ToastKind::Error, format!("打开设置失败：{err:#}")),
            },
        );
    }

    /// 把包名写入状态并读取 AppOps（供应用管理页跳转使用）。
    pub(crate) fn open_app_ops(&mut self, package: String, cx: &mut Ctx) {
        self.state.appops_package = package.clone();
        self.load_app_ops(package, cx);
    }

    /// 读取一个包的 AppOps 列表。
    pub(crate) fn load_app_ops(&mut self, package: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let package = package.trim().to_string();
        if package.is_empty() {
            cx.toast(ToastKind::Info, "请先输入要查询的包名");
            return;
        }
        self.state.appops_package = package.clone();
        self.state.appops_loaded_for = Some(package.clone());
        self.run_task(
            cx,
            format!("正在读取 {package} 的 AppOps…"),
            {
                let package = package.clone();
                move || adb.app_ops(&serial, &package)
            },
            move |core, result, cx| match result {
                Ok(ops) => {
                    let count = ops.len();
                    core.state.appops_ops = ops;
                    cx.toast(
                        ToastKind::Success,
                        if count == 0 {
                            format!("{package} 没有调整过的 AppOps")
                        } else {
                            format!("{package} 共 {count} 项调整过的 AppOps")
                        },
                    );
                }
                Err(err) => {
                    core.state.appops_loaded_for = None;
                    cx.toast(ToastKind::Error, format!("读取 AppOps 失败：{err:#}"));
                }
            },
        );
    }

    /// 设置一个 AppOps 的模式（来自自定义输入框）。
    pub(crate) fn apply_custom_app_op(
        &mut self,
        package: String,
        op: String,
        mode: AppOpMode,
        cx: &mut Ctx,
    ) {
        let op = op.trim().to_string();
        if op.is_empty() {
            cx.toast(ToastKind::Info, "请输入 AppOps 操作名");
            return;
        }
        self.state.appops_op_input = op.clone();
        self.state.appops_mode = mode;
        self.set_app_op(package, op, mode, cx);
    }

    /// 修改一个 AppOps 的模式并刷新列表。
    pub(crate) fn set_app_op(
        &mut self,
        package: String,
        op: String,
        mode: AppOpMode,
        cx: &mut Ctx,
    ) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let package = package.trim().to_string();
        if package.is_empty() {
            cx.toast(ToastKind::Info, "请先输入要调整的包名");
            return;
        }
        let label = format!("{package} · {op} → {}", mode.label());
        self.run_task(
            cx,
            format!("正在设置 {label}…"),
            {
                let package = package.clone();
                move || adb.set_app_op(&serial, &package, &op, mode)
            },
            move |core, result, cx| match result {
                Ok(()) => {
                    cx.toast(ToastKind::Success, format!("已设置 {label}"));
                    core.load_app_ops(package, cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("设置失败：{err:#}")),
            },
        );
    }

    /// 确认弹窗回调：重置一个包的全部 AppOps。
    pub(crate) fn reset_app_ops_confirmed(&mut self, package: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.run_task(
            cx,
            format!("正在重置 {package} 的 AppOps…"),
            {
                let package = package.clone();
                move || adb.reset_app_ops(&serial, &package, None)
            },
            move |core, result, cx| match result {
                Ok(()) => {
                    cx.toast(ToastKind::Success, format!("已重置 {package} 的 AppOps"));
                    core.load_app_ops(package, cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("重置失败：{err:#}")),
            },
        );
    }
}

/// 输入法显示名：取组件名的最后一段。
pub fn ime_display(id: &str) -> String {
    let short = id
        .rsplit('/')
        .next()
        .unwrap_or(id)
        .rsplit('.')
        .next()
        .unwrap_or(id);
    if short.is_empty() {
        id.to_string()
    } else {
        short.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ime_display_uses_class_name() {
        assert_eq!(
            ime_display(
                "com.google.android.inputmethod.latin/com.android.inputmethod.latin.LatinIME"
            ),
            "LatinIME"
        );
        assert_eq!(
            ime_display("com.android.btservices/.BluetoothKeyboard"),
            "BluetoothKeyboard"
        );
    }
}
