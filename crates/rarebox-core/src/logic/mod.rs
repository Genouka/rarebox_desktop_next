//! 引擎业务逻辑，按页面拆分。所有方法都运行在引擎的串行线程上。

mod apps;
mod connect;
pub(crate) mod debug;
mod device;
mod files;
mod harmony;
pub(crate) mod logs;
mod power;
pub(crate) mod remote;
pub(crate) mod screen;
mod settings;
mod signing;
mod terminal;

use anyhow::{Context as _, Result};

use crate::adb::AdbClient;
use crate::command::Command;
use crate::engine::{Core, Ctx};
use crate::hdc::HdcClient;

impl Core {
    pub(crate) fn bootstrap(&mut self, cx: &mut Ctx) {
        self.refresh_devices(cx);
        self.refresh_hdc(cx);
    }

    pub(crate) fn dispatch(&mut self, command: Command, cx: &mut Ctx) {
        match command {
            Command::RefreshDevices => self.refresh_devices(cx),
            Command::RefreshHdc => self.refresh_hdc(cx),
            Command::LoadSnapshot => self.load_snapshot(cx),
            Command::LoadHdcSnapshot => self.load_hdc_snapshot(cx),
            Command::SelectDevice { serial } => self.select_device(serial, cx),
            Command::SelectHdcDevice { key } => self.select_hdc_device(key, cx),
            Command::SetDeviceKind { kind } => self.set_device_kind(kind, cx),
            Command::WirelessConnect { address } => self.wireless_connect(address, cx),
            Command::WirelessPair { address, code } => self.wireless_pair(address, code, cx),
            Command::WirelessDisconnect => self.wireless_disconnect(cx),
            Command::EnableTcpip => self.enable_tcpip(cx),
            Command::DisconnectDevice { serial } => self.disconnect_device(serial, cx),
            Command::InstallDriver => self.install_driver(cx),
            Command::UninstallDriver => self.uninstall_driver(cx),

            Command::FetchLegal => self.fetch_legal(cx),
            Command::AcceptLegal => self.accept_legal(),
            Command::CheckUpdates { manual } => self.check_updates(cx, manual),
            Command::RefreshAnnouncements => self.refresh_announcements(cx),
            Command::DownloadUpdate => self.download_update(cx),

            Command::LoadPackages => self.load_packages(cx),
            Command::PickInstallApks => self.pick_install_apks(cx),
            Command::InstallApks { paths } => self.install_apks(paths, cx),
            Command::CancelInstall => self.cancel_install(),
            Command::ToggleInstallOption { toggle } => self.toggle_install_option(toggle),
            Command::RunInstallPlan => self.run_install_plan(cx),
            Command::LaunchPackage { package } => self.launch_package(package, cx),
            Command::StopPackage { package } => self.stop_package(package, cx),
            Command::ClearPackage { package } => self.clear_package(package, cx),
            Command::UninstallPackage { package } => self.uninstall_package(package, cx),
            Command::ExportPackage { package } => self.export_package(package, cx),

            Command::ListFiles => self.list_files(cx),
            Command::NavigateFiles { path } => self.navigate_files(path, cx),
            Command::GoParentFiles => self.go_parent_files(cx),
            Command::PullFile { remote } => self.pull_file(remote, cx),
            Command::PickPushFiles => self.pick_push_files(cx),
            Command::PushFiles { paths } => self.push_files(paths, cx),

            Command::LoadImes => self.load_imes(cx),
            Command::ActivateIme { id } => self.activate_ime(id, cx),
            Command::SetImeEnabled { id, enabled } => self.set_ime_enabled(id, enabled, cx),
            Command::OpenImeSettings => self.open_ime_settings(cx),
            Command::OpenAppOps { package } => self.open_app_ops(package, cx),
            Command::LoadAppOps { package } => self.load_app_ops(package, cx),
            Command::ApplyCustomAppOp { package, op, mode } => {
                self.apply_custom_app_op(package, op, mode, cx)
            }
            Command::SetAppOp { package, op, mode } => self.set_app_op(package, op, mode, cx),
            Command::ResetAppOps { package } => self.reset_app_ops_confirmed(package, cx),

            Command::LoadLogs => self.load_logs(cx),
            Command::ClearLogcat => self.clear_logcat(cx),
            Command::ExportLogs { query, filter } => self.export_logs(query, filter, cx),

            Command::TakeScreenshot => self.take_screenshot(cx),
            Command::RecordScreen => self.record_screen(cx),
            Command::ApplyDisplaySize { size } => self.apply_display_size(size, cx),
            Command::ResetDisplaySize => self.reset_display_size(cx),
            Command::ApplyDensity { density } => self.apply_density(density, cx),
            Command::ResetDensity => self.reset_density(cx),
            Command::SetRotation { rotation } => self.set_rotation(rotation, cx),
            Command::SendKey { code, label } => self.send_key(code, label, cx),
            Command::SendText { text } => self.send_text(text, cx),

            Command::SubmitTerminal { command } => self.submit_terminal(command, cx),
            Command::RunQuickCommand { command } => self.run_quick_command(command, cx),

            Command::StartRemote => self.start_remote(cx),
            Command::StopRemote => self.stop_remote(cx),
            Command::RestartRemote => self.restart_remote(cx),
            Command::RemotePointerDown { x, y } => self.remote_pointer_down(x, y, cx),
            Command::RemotePointerMove { x, y } => self.remote_pointer_move(x, y, cx),
            Command::RemotePointerUp { x, y } => self.remote_pointer_up(x, y, cx),
            Command::SendRemoteKey { keycode } => self.send_remote_key(keycode, cx),
            Command::SendRemoteText { text } => self.send_remote_text(text, cx),

            Command::ApplyAdbPath { path } => self.apply_adb_path(path, cx),
            Command::ResetAdbToAuto => self.reset_adb_to_auto(cx),
            Command::ApplyHdcPath { path } => self.apply_hdc_path(path, cx),
            Command::ResetHdcToAuto => self.reset_hdc_to_auto(cx),
            Command::OpenConfigDir => self.open_config_dir(cx),
            Command::SetScreenshotDir { path } => self.set_screenshot_dir(path, cx),
            Command::PickScreenshotDir => self.pick_screenshot_dir(cx),
            Command::ResetScreenshotDir => self.reset_screenshot_dir(cx),
            Command::OpenMediaDir => self.open_media_dir(cx),
            Command::SetTheme { dark, seed } => self.set_theme(dark, seed),
            Command::KillAdbProcess => self.kill_adb_process(cx),
            Command::KillHdcProcess => self.kill_hdc_process(cx),
            Command::SetKillProcessesOnExit { value } => self.set_kill_processes_on_exit(value),

            Command::EnsureSigningLoaded => self.ensure_signing_loaded(cx),
            Command::RefreshSigning => self.refresh_signing(cx),
            Command::SigningLogin => self.signing_login(cx),
            Command::SigningLogout => self.signing_logout(cx),
            Command::SigningSelectTeam { team_id } => self.signing_select_team(team_id, cx),
            Command::SigningSwitchBlock { block } => self.signing_switch_block(block, cx),
            Command::SigningApplyCert { reset } => self.signing_apply_cert(reset, cx),
            Command::SigningSetInputs { paths } => self.signing_set_inputs(paths, cx),
            Command::SigningToggleImported => self.signing_toggle_imported(),
            Command::PickSigningHap => self.signing_pick_hap(cx),
            Command::PickSigningImportFile { which } => self.signing_pick_import_file(which, cx),
            Command::SigningImportFile { which, path } => self.signing_import_file(which, path),
            Command::LoadSigningUdid => self.load_signing_udid(cx),
            Command::SigningRegisterDevice => self.signing_register_device(cx),
            Command::SigningStart => self.signing_start(cx),
            Command::SigningToggleAcl { permission } => self.signing_toggle_acl(permission),
            Command::SigningToggleReplace { value } => self.signing_toggle_replace(value),
            Command::SigningToggleResetCert => self.signing_toggle_reset_cert(),
            Command::SigningToggleRefreshProfile => self.signing_toggle_refresh_profile(),
            Command::SigningToggleLogs => self.signing_toggle_logs(),
            Command::SigningSetImportAlias { value } => {
                self.state.signing.set_import_alias(value)
            }
            Command::SigningSetImportPassword { value } => {
                self.state.signing.set_import_password(value)
            }
            Command::SigningSetAclQuery { query } => self.state.signing.set_acl_query(query),

            Command::HdcConnect { address } => self.hdc_connect(address, cx),
            Command::HdcDisconnect => self.hdc_disconnect(cx),
            Command::HdcRestartServer => self.restart_hdc_server(cx),
            Command::HdcEnableWireless { port } => self.enable_hdc_wireless(port, cx),
            Command::HdcDisableWireless => self.disable_hdc_wireless(cx),
            Command::HdcLoadForwards => self.load_hdc_forwards(cx),
            Command::HdcToggleForwardDirection => self.toggle_hdc_forward_direction(),
            Command::HdcAddForward {
                local,
                remote,
                reverse,
            } => self.add_hdc_forward(local, remote, reverse, cx),
            Command::HdcRemoveForward { forward } => self.remove_hdc_forward(forward, cx),
            Command::HdcLoadPackages => self.load_hdc_packages(cx),
            Command::PickInstallHaps => self.pick_install_haps(cx),
            Command::HdcInstallHaps { paths } => self.install_hap_files(paths, cx),
            Command::HdcToggleInstallOption { shared } => {
                self.toggle_hdc_install_option(shared)
            }
            Command::HdcLaunchPackage { bundle } => self.launch_hdc_package(bundle, cx),
            Command::HdcStopPackage { bundle } => self.stop_hdc_package(bundle, cx),
            Command::HdcUninstallPackage { bundle } => self.uninstall_hdc_package(bundle, cx),
            Command::HdcCopyBundle { bundle } => self.copy_hdc_bundle(bundle, cx),
            Command::HdcLoadBundleDetails { bundle } => self.load_hdc_bundle_details(bundle, cx),
            Command::HdcCloseBundleDetails => self.close_hdc_bundle_details(),
            Command::HdcClearAppData { bundle, cache } => {
                self.clear_hdc_app_data(bundle, cache, cx)
            }
            Command::HdcOpenScheme { uri } => self.open_hdc_scheme(uri, cx),
            Command::HdcLoadFiles => self.load_hdc_files(cx),
            Command::HdcNavigateFiles { path } => self.navigate_hdc_files(path, cx),
            Command::HdcGoParentFiles => self.go_parent_hdc_files(cx),
            Command::HdcPullFile { remote } => self.pull_hdc_file(remote, cx),
            Command::PickPushHdcFiles => self.pick_push_hdc_files(cx),
            Command::HdcUploadFiles { paths } => self.upload_hdc_files(paths, cx),
            Command::HdcSendKey { key } => self.hdc_send_key(key, cx),
            Command::HdcTap { x, y, tap } => self.hdc_tap(x, y, tap, cx),
            Command::HdcFling { direction } => self.hdc_fling(direction, cx),
            Command::HdcInputText { x, y, text } => self.hdc_input_text(x, y, text, cx),
            Command::HdcWakeScreen => self.hdc_wake_screen(cx),
            Command::HdcSubmitTerminal { command } => self.submit_hdc_terminal(command, cx),
            Command::HdcLoadLogs { tag, filter } => self.load_hdc_logs(tag, filter, cx),
            Command::HdcExportLogs => self.export_hdc_logs(cx),
            Command::HdcClearLogs => self.clear_hdc_logs(cx),
            Command::HdcExportFaultlog => self.export_hdc_faultlog(cx),
            Command::HdcExportHilogFiles => self.export_hdc_hilog_files(cx),
            Command::HdcScreenshot => self.hdc_screenshot(cx),
            Command::HdcReboot { target } => self.hdc_reboot(target, cx),
            Command::HdcOpenReboot { target } => self.open_hdc_reboot(target, cx),
            Command::HdcOpenRestartServer => self.open_hdc_server_restart(cx),
            Command::HdcOpenClearLogs => self.open_hdc_clear_logs(cx),
            Command::HdcOpenClearAppData { bundle, cache } => {
                self.open_hdc_clear_app_data(bundle, cache, cx)
            }

            Command::ExecuteConfirm { kind } => self.execute_confirm(kind, cx),
            Command::RequestReboot { target } => self.request_reboot(target, cx),
        }
    }


    pub fn adb_client(&self) -> Result<AdbClient> {
        let path = self
            .state
            .adb_path
            .clone()
            .context("未找到 ADB，请先在设置中指定 adb 可执行文件")?;
        Ok(AdbClient::new(path))
    }

    pub fn require_device(&self) -> Result<(AdbClient, String)> {
        let adb = self.adb_client()?;
        let serial = self
            .state
            .selected_serial
            .clone()
            .context("请先连接并选择一台手表")?;
        Ok((adb, serial))
    }

    pub fn hdc_client(&self) -> Result<HdcClient> {
        let path = self
            .state
            .harmony
            .path
            .clone()
            .context("未找到 hdc：内置 hdc 不可用，请在设置中指定 hdc 路径")?;
        Ok(HdcClient::new(path))
    }

    pub fn require_hdc_device(&self) -> Result<(HdcClient, String)> {
        let client = self.hdc_client()?;
        let key = self
            .state
            .harmony
            .selected
            .clone()
            .context("请先连接并选择一台 HarmonyOS 设备")?;
        Ok((client, key))
    }


    fn fetch_legal(&mut self, cx: &mut Ctx) {
        let eula = crate::legal::fetch_legal(crate::legal::LegalDocKind::Eula);
        let privacy = crate::legal::fetch_legal(crate::legal::LegalDocKind::Privacy);
        let mut error: Vec<String> = Vec::new();
        let eula = match eula {
            Ok(doc) => Some(doc.content),
            Err(err) => {
                error.push(format!("{err:#}"));
                Some(crate::legal::FALLBACK_EULA.to_string())
            }
        };
        let privacy = match privacy {
            Ok(doc) => Some(doc.content),
            Err(err) => {
                error.push(format!("{err:#}"));
                Some(crate::legal::FALLBACK_PRIVACY.to_string())
            }
        };
        cx.emit(crate::event::Event::Legal(crate::event::LegalDocuments {
            eula,
            privacy,
            error: (!error.is_empty()).then(|| error.join("；")),
        }));
    }

    fn accept_legal(&mut self) {
        self.state.config.accepted_legal_version =
            Some(crate::legal::LEGAL_VERSION.to_string());
        self.save_config();
    }

    fn check_updates(&mut self, cx: &mut Ctx, manual: bool) {
        if self.state.update.checking {
            return;
        }
        self.state.update.checking = true;
        self.run_task(
            cx,
            "正在检查更新…",
            crate::update::check,
            move |core, result, cx| {
                core.state.update.checking = false;
                match result {
                    Ok(status) => {
                        let available = status.update_available;
                        core.state.update.status = Some(status);
                        if !available && manual {
                            cx.toast(
                                crate::state::ToastKind::Success,
                                format!("已是最新版本 {}", crate::update::APP_VERSION),
                            );
                        }
                    }
                    Err(err) => {
                        if manual {
                            cx.toast(
                                crate::state::ToastKind::Error,
                                format!("检查更新失败：{err:#}"),
                            );
                        }
                    }
                }
            },
        );
    }

    fn refresh_announcements(&mut self, cx: &mut Ctx) {
        self.run_task(
            cx,
            "正在获取公告…",
            crate::update::announcements,
            |core, result, _cx| match result {
                Ok(items) => {
                    core.state.announcements = items;
                    core.state.announcements_error = None;
                }
                Err(err) => {
                    if core.state.announcements.is_empty() {
                        core.state.announcements_error = Some(format!("无法获取公告：{err:#}"));
                    } else {
                        core.state.announcements_error = Some(format!(
                            "无法获取历史公告：{err:#}；以下为最近一次获取的公告。"
                        ));
                    }
                }
            },
        );
    }

    fn download_update(&mut self, cx: &mut Ctx) {
        let Some(url) = self
            .state
            .update
            .status
            .as_ref()
            .and_then(|status| status.download_url.clone())
        else {
            cx.toast(crate::state::ToastKind::Error, "服务器未提供下载地址");
            return;
        };
        cx.open_url(url);
    }

    fn execute_confirm(&mut self, kind: crate::state::ConfirmKind, cx: &mut Ctx) {
        use crate::state::ConfirmKind;
        match kind {
            ConfirmKind::Uninstall(package) => self.uninstall_package(package, cx),
            ConfirmKind::ClearData(package) => self.clear_package(package, cx),
            ConfirmKind::Reboot(target) => self.reboot_device(target, cx),
            ConfirmKind::HdcUninstall(bundle) => self.uninstall_hdc_package(bundle, cx),
            ConfirmKind::HdcReboot(target) => self.hdc_reboot(target, cx),
            ConfirmKind::HdcRestartServer => self.restart_hdc_server(cx),
            ConfirmKind::HdcClearLogs => self.clear_hdc_logs(cx),
            ConfirmKind::HdcClearAppData { bundle, cache } => {
                self.clear_hdc_app_data(bundle, cache, cx)
            }
            ConfirmKind::AppOpsReset(package) => self.reset_app_ops_confirmed(package, cx),
        }
    }
}
