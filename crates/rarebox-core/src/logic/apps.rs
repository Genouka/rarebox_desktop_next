//! 应用管理：读取、筛选、启动、卸载与导出 Android 应用。

use std::path::PathBuf;

use anyhow::{Context as _, bail};

use crate::adb::{self, parse_package_path};
use crate::config::{self, downloads_dir};
use crate::engine::{Core, Ctx};
use crate::event::PickPurpose;
use crate::install::{self, InstallBatch, InstallToggle};
use crate::state::ToastKind;

impl Core {
    pub(crate) fn load_packages(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.state.package_loaded_for = Some(serial.clone());
        self.run_task(
            cx,
            "正在读取应用列表…",
            move || {
                let all = adb.shell(&serial, "pm list packages --show-versioncode")?;
                if !all.success() {
                    bail!("{}", all.error_message());
                }
                let third_party = adb.shell(&serial, "pm list packages -3")?;
                if !third_party.success() {
                    bail!("{}", third_party.error_message());
                }
                Ok(adb::merge_packages(&all.stdout, &third_party.stdout))
            },
            |core, result, cx| match result {
                Ok(packages) => {
                    let count = packages.len();
                    core.state.packages = packages;
                    cx.toast(ToastKind::Success, format!("已读取 {count} 个应用"));
                    core.load_package_metadata(cx);
                }
                Err(err) => {
                    core.state.package_loaded_for = None;
                    cx.toast(ToastKind::Error, format!("读取应用失败：{err:#}"));
                }
            },
        );
    }

    /// 读取 APK 里的应用名称与版本信息，并补到当前列表上。
    ///
    /// 这一步比 `pm list packages` 慢得多（要逐个打开 APK），
    /// 因此放在列表显示之后单独执行，解析完成后直接刷新界面。
    fn load_package_metadata(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        if self.state.package_metadata_for.is_some() {
            return;
        }
        self.state.package_metadata_for = Some(serial.clone());
        self.run_task(
            cx,
            "正在解析应用名称…",
            {
                let serial = serial.clone();
                move || adb.package_metadata(&serial)
            },
            move |core, result, cx| {
                let current = core.state.package_metadata_for.as_deref() == Some(serial.as_str());
                core.state.package_metadata_for = None;
                if !current {
                    return;
                }
                match result {
                    Ok(metadata) => {
                        adb::apply_package_metadata(&mut core.state.packages, &metadata);
                    }
                    Err(err) => cx.toast(
                        ToastKind::Error,
                        format!("应用名称解析失败，暂时显示包名：{err:#}"),
                    ),
                }
            },
        );
    }

    /// 读取所选 APK 的包名与 split，组成安装方案。
    ///
    /// 分组必须在安装前完成：分包要靠包名归并，读不出包名的文件也要先告诉用户，
    /// 而不是等到 adb 报错。
    pub(crate) fn install_apks(&mut self, paths: Vec<PathBuf>, cx: &mut Ctx) {
        if self.require_device().is_err() {
            cx.toast(ToastKind::Error, "请先连接并选择一台手表");
            return;
        }
        self.state.install_batch = None;
        self.run_task(
            cx,
            "正在读取 APK 信息…",
            move || Ok(install::plan_installs(&paths)),
            |core, result, cx| match result {
                Ok(plan) if plan.is_empty() => {
                    cx.toast(ToastKind::Error, "没有可以安装的 APK");
                }
                Ok(plan) => {
                    let targets = plan.groups.len();
                    let failed = plan.failures.len();
                    core.state.install_plan = Some(plan);
                    if targets == 0 {
                        cx.toast(
                            ToastKind::Error,
                            format!("{failed} 个 APK 无法安装，请查看下方原因"),
                        );
                    }
                }
                Err(err) => cx.toast(ToastKind::Error, format!("读取 APK 失败：{err:#}")),
            },
        );
    }

    pub(crate) fn cancel_install(&mut self) {
        self.state.install_plan = None;
    }

    /// 切换一个安装参数，并写回配置。
    pub(crate) fn toggle_install_option(&mut self, toggle: InstallToggle) {
        let value = !self.state.config.install.get(toggle);
        self.state.config.install.set(toggle, value);
        // 覆盖安装是其余参数的前提，关掉它就没有意义的参数一起复位。
        if toggle == InstallToggle::Replace && !value {
            self.state.config.install.downgrade = false;
        }
        self.save_config();
    }

    /// 执行当前方案：逐个应用安装，单个失败不影响其余应用。
    pub(crate) fn run_install_plan(&mut self, cx: &mut Ctx) {
        let Some(plan) = self.state.install_plan.clone() else {
            return;
        };
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        self.state.install_plan = None;
        let options = self.state.config.install;
        let total = plan.groups.len();
        let label = if total == 1 {
            format!("正在安装 {}…", plan.groups[0].title())
        } else {
            format!("正在安装 {total} 个应用…")
        };
        self.run_task(
            cx,
            label,
            move || {
                let mut batch = InstallBatch::default();
                batch.reports.extend(plan.failures);
                // 串行执行：设备上的 PackageManager 不支持并行安装会话。
                for group in &plan.groups {
                    batch
                        .reports
                        .push(install::run_install(&adb, &serial, group, &options));
                }
                Ok(batch)
            },
            |core, result, cx| match result {
                Ok(batch) => {
                    let (succeeded, failed) = (batch.succeeded(), batch.failed());
                    if failed == 0 {
                        cx.toast(ToastKind::Success, format!("{succeeded} 个应用安装成功"));
                    } else if succeeded == 0 {
                        cx.toast(ToastKind::Error, format!("{failed} 个应用安装失败"));
                    } else {
                        cx.toast(
                            ToastKind::Error,
                            format!("{succeeded} 个成功，{failed} 个失败"),
                        );
                    }
                    core.state.install_batch = Some(batch);
                    core.load_packages(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("安装失败：{err:#}")),
            },
        );
    }

    /// 应用在界面上的显示名：优先应用名称，其次包名。
    pub fn package_display_name(&self, package: &str) -> String {
        self.state
            .packages
            .iter()
            .find(|candidate| candidate.name == package)
            .map(|candidate| candidate.label_or_name().to_string())
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| package.to_string())
    }

    pub(crate) fn launch_package(&mut self, package: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let display = self.package_display_name(&package);
        let activity = format!("正在启动 {display}…");
        let done_label = display.clone();
        self.run_task(
            cx,
            activity,
            move || {
                let resolved = adb.shell(
                    &serial,
                    &format!("cmd package resolve-activity --brief {package}"),
                )?;
                let component = resolved
                    .stdout
                    .lines()
                    .map(str::trim)
                    .rfind(|line| line.contains('/'))
                    .map(str::to_string);
                let output = if let Some(component) = component {
                    adb.shell(&serial, &format!("am start -n {component}"))?
                } else {
                    adb.shell(
                        &serial,
                        &format!("monkey -p {package} -c android.intent.category.LAUNCHER 1"),
                    )?
                };
                if !output.success() {
                    bail!("{}", output.error_message());
                }
                Ok(output.combined())
            },
            move |_core, result, cx| match result {
                Ok(_) => cx.toast(ToastKind::Success, format!("已启动 {done_label}")),
                Err(err) => cx.toast(ToastKind::Error, format!("启动失败：{err:#}")),
            },
        );
    }

    pub(crate) fn stop_package(&mut self, package: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let label = self.package_display_name(&package);
        self.run_task(
            cx,
            format!("正在停止 {label}…"),
            move || {
                let output = adb.shell(&serial, &format!("am force-stop {package}"))?;
                if !output.success() {
                    bail!("{}", output.error_message());
                }
                Ok(())
            },
            move |_core, result, cx| match result {
                Ok(()) => cx.toast(ToastKind::Success, format!("已停止 {label}")),
                Err(err) => cx.toast(ToastKind::Error, format!("停止失败：{err:#}")),
            },
        );
    }

    pub(crate) fn clear_package(&mut self, package: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let label = self.package_display_name(&package);
        self.run_task(
            cx,
            format!("正在清除 {label} 的数据…"),
            move || {
                let output = adb.shell(&serial, &format!("pm clear --user 0 {package}"))?;
                if !output.success() || output.stdout.contains("Failed") {
                    bail!("{}", output.error_message());
                }
                Ok(output.combined())
            },
            move |_core, result, cx| match result {
                Ok(_) => cx.toast(ToastKind::Success, format!("已清除 {label} 的数据")),
                Err(err) => cx.toast(ToastKind::Error, format!("清除失败：{err:#}")),
            },
        );
    }

    pub(crate) fn uninstall_package(&mut self, package: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let target = package.clone();
        let display = self.package_display_name(&package);
        self.run_task(
            cx,
            format!("正在卸载 {display}…"),
            move || {
                let output = adb.shell(&serial, &format!("pm uninstall --user 0 {target}"))?;
                if !output.success() || output.stdout.contains("Failure") {
                    bail!("{}", output.error_message());
                }
                Ok(())
            },
            move |core, result, cx| match result {
                Ok(()) => {
                    core.state
                        .packages
                        .retain(|candidate| candidate.name != package);
                    cx.toast(ToastKind::Success, format!("已卸载 {display}"));
                }
                Err(err) => cx.toast(ToastKind::Error, format!("卸载失败：{err:#}")),
            },
        );
    }

    pub(crate) fn export_package(&mut self, package: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let display = self.package_display_name(&package);
        self.run_task(
            cx,
            format!("正在导出 {display}…"),
            move || {
                let output = adb.shell(&serial, &format!("pm path {package}"))?;
                if !output.success() {
                    bail!("{}", output.error_message());
                }
                let remote = parse_package_path(&output.stdout)
                    .context("没有找到 APK 在设备上的路径（可能已被卸载）")?;
                let local = downloads_dir().join(format!("{package}.apk"));
                let local_string = local.to_string_lossy().to_string();
                let pulled =
                    adb.serial_command(&serial, &["pull", remote.as_str(), local_string.as_str()])?;
                if !pulled.success() {
                    bail!("{}", pulled.error_message());
                }
                Ok(local)
            },
            move |core, result, cx| match result {
                Ok(path) => {
                    core.state.last_export = Some(path.clone());
                    cx.toast(
                        ToastKind::Success,
                        format!("{display} 已导出到 {}", config::short_path(&path)),
                    );
                }
                Err(err) => cx.toast(ToastKind::Error, format!("导出失败：{err:#}")),
            },
        );
    }

    pub(crate) fn pick_install_apks(&mut self, cx: &mut Ctx) {
        if self.require_device().is_err() {
            cx.toast(ToastKind::Error, "请先连接并选择一台手表");
            return;
        }
        cx.pick_files(PickPurpose::InstallApk);
    }
}
