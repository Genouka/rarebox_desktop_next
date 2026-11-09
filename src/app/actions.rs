//! 界面动作 → 引擎命令的适配层。
//!
//! UI 只依赖这些方法名，不直接持有设备客户端；真正的执行与状态变更都在
//! `rarebox-core` 的串行线程里完成。

use gpui::Context;

use rarebox_core::adb::{AppOpMode, RebootTarget};
use rarebox_core::hdc::{HdcForward, HdcKey, HdcRebootTarget, HdcSwipeDirection, HdcTap};
use rarebox_core::install::InstallToggle;
use rarebox_core::signing::{ImportFile, SigningBlock};
use rarebox_core::state::ToastKind;
use rarebox_core::Command;

use super::RareBox;

impl RareBox {
    pub(crate) fn core_command(&self, command: Command) {
        if let Err(err) = self.core.command(command) {
            eprintln!("RareBox Desktop: 引擎已停止: {err}");
        }
    }

    fn require_device_hint(&mut self, cx: &mut Context<Self>) -> bool {
        if self.has_device() {
            return true;
        }
        self.toast(cx, ToastKind::Error, "请先连接并选择一台手表");
        false
    }

    // ---------------------------------------------------------------------
    // 设备与连接
    // ---------------------------------------------------------------------

    pub(crate) fn refresh_devices(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::RefreshDevices);
    }

    pub(crate) fn refresh_hdc(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::RefreshHdc);
    }

    pub(crate) fn load_hdc_snapshot(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::LoadHdcSnapshot);
    }

    pub(crate) fn select_device(&mut self, serial: String, _cx: &mut Context<Self>) {
        self.core_command(Command::SelectDevice { serial });
    }

    pub(crate) fn select_hdc_device(&mut self, key: String, _cx: &mut Context<Self>) {
        self.core_command(Command::SelectHdcDevice { key });
    }

    pub(crate) fn wireless_connect(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::WirelessConnect {
            address: self.wireless_address.clone(),
        });
    }

    pub(crate) fn wireless_pair(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::WirelessPair {
            address: self.pair_address.clone(),
            code: self.pair_code.clone(),
        });
    }

    pub(crate) fn wireless_disconnect(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::WirelessDisconnect);
    }

    pub(crate) fn enable_tcpip(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::EnableTcpip);
    }

    pub(crate) fn disconnect_device(&mut self, serial: String, _cx: &mut Context<Self>) {
        self.core_command(Command::DisconnectDevice { serial });
    }

    pub(crate) fn install_driver(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::InstallDriver);
    }

    pub(crate) fn uninstall_driver(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::UninstallDriver);
    }

    pub(crate) fn set_wireless_address(&mut self, address: String, cx: &mut Context<Self>) {
        self.wireless_address = address.clone();
        self.wireless_field
            .update(cx, |state, cx| state.set_value(&address, cx));
        cx.notify();
    }

    // ---------------------------------------------------------------------
    // 应用与文件
    // ---------------------------------------------------------------------

    pub(crate) fn load_packages(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::LoadPackages);
    }

    pub(crate) fn pick_and_install(&mut self, cx: &mut Context<Self>) {
        if !self.require_device_hint(cx) {
            return;
        }
        self.core_command(Command::PickInstallApks);
    }

    pub(crate) fn cancel_install(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::CancelInstall);
    }

    pub(crate) fn toggle_install_option(&mut self, toggle: InstallToggle, _cx: &mut Context<Self>) {
        self.core_command(Command::ToggleInstallOption { toggle });
    }

    pub(crate) fn run_install_plan(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::RunInstallPlan);
    }

    pub(crate) fn launch_package(&mut self, package: String, _cx: &mut Context<Self>) {
        self.core_command(Command::LaunchPackage { package });
    }

    pub(crate) fn stop_package(&mut self, package: String, _cx: &mut Context<Self>) {
        self.core_command(Command::StopPackage { package });
    }


    pub(crate) fn export_package(&mut self, package: String, _cx: &mut Context<Self>) {
        self.core_command(Command::ExportPackage { package });
    }

    pub(crate) fn list_files(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ListFiles);
    }

    pub(crate) fn navigate_files(&mut self, path: String, _cx: &mut Context<Self>) {
        self.core_command(Command::NavigateFiles { path });
    }

    pub(crate) fn go_parent_files(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::GoParentFiles);
    }

    pub(crate) fn pull_file(&mut self, remote: String, _cx: &mut Context<Self>) {
        self.core_command(Command::PullFile { remote });
    }

    pub(crate) fn pick_and_push(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::PickPushFiles);
    }

    // ---------------------------------------------------------------------
    // 调试工具
    // ---------------------------------------------------------------------

    pub(crate) fn load_imes(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::LoadImes);
    }

    pub(crate) fn activate_ime(&mut self, id: String, _cx: &mut Context<Self>) {
        self.core_command(Command::ActivateIme { id });
    }

    pub(crate) fn set_ime_enabled(&mut self, id: String, enabled: bool, _cx: &mut Context<Self>) {
        self.core_command(Command::SetImeEnabled { id, enabled });
    }

    pub(crate) fn open_ime_settings(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::OpenImeSettings);
    }

    pub(crate) fn open_app_ops(&mut self, package: String, _cx: &mut Context<Self>) {
        self.core_command(Command::OpenAppOps { package });
    }

    pub(crate) fn load_app_ops(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::LoadAppOps {
            package: self.appops_package.clone(),
        });
    }

    pub(crate) fn apply_custom_app_op(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ApplyCustomAppOp {
            package: self.appops_package.clone(),
            op: self.appops_op_input.clone(),
            mode: self.appops_mode,
        });
    }

    pub(crate) fn set_app_op(
        &mut self,
        package: String,
        op: String,
        mode: AppOpMode,
        _cx: &mut Context<Self>,
    ) {
        self.core_command(Command::SetAppOp { package, op, mode });
    }

    // ---------------------------------------------------------------------
    // 日志与屏幕
    // ---------------------------------------------------------------------

    pub(crate) fn load_logs(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::LoadLogs);
    }

    pub(crate) fn clear_logcat(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ClearLogcat);
    }

    pub(crate) fn export_logs(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ExportLogs {
            query: self.logs_query.clone(),
            filter: self.logs_filter,
        });
    }

    pub(crate) fn take_screenshot(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::TakeScreenshot);
    }

    pub(crate) fn record_screen(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::RecordScreen);
    }

    pub(crate) fn apply_display_size(&mut self, cx: &mut Context<Self>) {
        let size = self.screen_size_field.read(cx).value().to_string();
        self.core_command(Command::ApplyDisplaySize { size });
    }

    pub(crate) fn reset_display_size(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ResetDisplaySize);
    }

    pub(crate) fn apply_density(&mut self, cx: &mut Context<Self>) {
        let density = self.screen_density_field.read(cx).value().to_string();
        self.core_command(Command::ApplyDensity { density });
    }

    pub(crate) fn reset_density(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ResetDensity);
    }

    pub(crate) fn set_rotation(&mut self, rotation: u8, _cx: &mut Context<Self>) {
        self.core_command(Command::SetRotation { rotation });
    }

    pub(crate) fn send_key(&mut self, code: u32, label: &'static str, _cx: &mut Context<Self>) {
        self.core_command(Command::SendKey {
            code,
            label: label.to_string(),
        });
    }

    pub(crate) fn send_text(&mut self, cx: &mut Context<Self>) {
        let text = self.screen_text_field.read(cx).value().to_string();
        self.screen_text_field
            .update(cx, |state, cx| state.set_value("", cx));
        self.core_command(Command::SendText { text });
    }

    // ---------------------------------------------------------------------
    // 终端
    // ---------------------------------------------------------------------

    pub(crate) fn submit_terminal(&mut self, cx: &mut Context<Self>) {
        let command = self.terminal_input.clone();
        self.terminal_field
            .update(cx, |state, cx| state.set_value("", cx));
        self.terminal_input.clear();
        self.core_command(Command::SubmitTerminal { command });
    }

    pub(crate) fn run_quick_command(
        &mut self,
        command: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let command = command.into();
        self.terminal_input = command.clone();
        self.terminal_field
            .update(cx, |state, cx| state.set_value(&command, cx));
        self.submit_terminal(cx);
    }

    // ---------------------------------------------------------------------
    // 远程控制
    // ---------------------------------------------------------------------

    pub(crate) fn start_remote(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::StartRemote);
    }

    pub(crate) fn stop_remote(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::StopRemote);
        if let Some(frame) = self.remote_frame.take() {
            self.remote_retired_frames.push(frame);
        }
    }

    pub(crate) fn restart_remote(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::RestartRemote);
    }

    pub(crate) fn send_remote_key(&mut self, keycode: u32, _cx: &mut Context<Self>) {
        self.core_command(Command::SendRemoteKey { keycode });
    }

    pub(crate) fn send_remote_text(&mut self, cx: &mut Context<Self>) {
        let text = self.remote_text_input.clone();
        if text.trim().is_empty() {
            return;
        }
        self.remote_text_input.clear();
        self.remote_text_field
            .update(cx, |state, cx| state.set_value("", cx));
        self.core_command(Command::SendRemoteText { text });
        cx.notify();
    }

    // ---------------------------------------------------------------------
    // 设置与系统
    // ---------------------------------------------------------------------

    pub(crate) fn apply_adb_settings(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ApplyAdbPath {
            path: self.adb_input.clone(),
        });
    }

    pub(crate) fn reset_adb_to_auto(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ResetAdbToAuto);
    }

    pub(crate) fn apply_hdc_settings(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ApplyHdcPath {
            path: self.hdc_input.clone(),
        });
    }

    pub(crate) fn reset_hdc_to_auto(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ResetHdcToAuto);
    }

    pub(crate) fn open_config_dir(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::OpenConfigDir);
    }

    pub(crate) fn pick_screenshot_dir(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::PickScreenshotDir);
    }

    pub(crate) fn open_media_dir(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::OpenMediaDir);
    }

    pub(crate) fn reset_screenshot_dir(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::ResetScreenshotDir);
    }

    pub(crate) fn kill_adb_process(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::KillAdbProcess);
    }

    pub(crate) fn kill_hdc_process(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::KillHdcProcess);
    }

    // ---------------------------------------------------------------------
    // 签名
    // ---------------------------------------------------------------------

    pub(crate) fn ensure_signing_loaded(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::EnsureSigningLoaded);
    }

    pub(crate) fn refresh_signing(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::RefreshSigning);
    }

    pub(crate) fn signing_login(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningLogin);
    }

    pub(crate) fn signing_logout(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningLogout);
    }

    pub(crate) fn signing_select_team(&mut self, team_id: String, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningSelectTeam { team_id });
    }

    pub(crate) fn signing_switch_block(&mut self, block: SigningBlock, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningSwitchBlock { block });
    }

    pub(crate) fn signing_apply_cert(&mut self, reset: bool, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningApplyCert { reset });
    }

    pub(crate) fn signing_pick_hap(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::PickSigningHap);
    }

    pub(crate) fn signing_toggle_imported(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningToggleImported);
    }

    pub(crate) fn signing_pick_import_file(
        &mut self,
        which: ImportFile,
        _cx: &mut Context<Self>,
    ) {
        self.core_command(Command::PickSigningImportFile { which });
    }

    pub(crate) fn load_signing_udid(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::LoadSigningUdid);
    }

    pub(crate) fn signing_register_device(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningRegisterDevice);
    }

    pub(crate) fn signing_start(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningStart);
    }

    pub(crate) fn signing_toggle_acl(&mut self, permission: String, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningToggleAcl { permission });
    }

    pub(crate) fn signing_toggle_replace(&mut self, shared: bool, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningToggleReplace { value: shared });
    }

    pub(crate) fn signing_toggle_reset_cert(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningToggleResetCert);
    }

    pub(crate) fn signing_toggle_refresh_profile(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningToggleRefreshProfile);
    }

    pub(crate) fn signing_toggle_logs(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::SigningToggleLogs);
    }

    // ---------------------------------------------------------------------
    // HarmonyOS
    // ---------------------------------------------------------------------

    pub(crate) fn hdc_connect(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcConnect {
            address: self.harmony.address.clone(),
        });
    }

    pub(crate) fn hdc_disconnect(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcDisconnect);
    }


    pub(crate) fn open_hdc_server_restart(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcOpenRestartServer);
    }

    pub(crate) fn enable_hdc_wireless(&mut self, port: String, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcEnableWireless { port });
    }

    pub(crate) fn disable_hdc_wireless(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcDisableWireless);
    }

    pub(crate) fn load_hdc_forwards(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcLoadForwards);
    }

    pub(crate) fn toggle_hdc_forward_direction(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcToggleForwardDirection);
    }

    pub(crate) fn add_hdc_forward(
        &mut self,
        local: String,
        remote: String,
        _cx: &mut Context<Self>,
    ) {
        self.core_command(Command::HdcAddForward {
            local,
            remote,
            reverse: self.harmony.forward_reverse,
        });
    }

    pub(crate) fn remove_hdc_forward(&mut self, forward: HdcForward, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcRemoveForward { forward });
    }

    pub(crate) fn load_hdc_packages(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcLoadPackages);
    }

    pub(crate) fn pick_and_install_hap(&mut self, cx: &mut Context<Self>) {
        if self.require_hdc_hint(cx) {
            self.core_command(Command::PickInstallHaps);
        }
    }

    fn require_hdc_hint(&mut self, cx: &mut Context<Self>) -> bool {
        if self.harmony.selected.is_some() {
            return true;
        }
        self.toast(cx, ToastKind::Error, "请先连接并选择一台 HarmonyOS 设备");
        false
    }

    pub(crate) fn toggle_hdc_install_option(&mut self, shared: bool, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcToggleInstallOption { shared });
    }

    pub(crate) fn launch_hdc_package(&mut self, bundle: String, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcLaunchPackage { bundle });
    }

    pub(crate) fn stop_hdc_package(&mut self, bundle: String, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcStopPackage { bundle });
    }


    pub(crate) fn copy_hdc_bundle(&mut self, bundle: String, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcCopyBundle { bundle });
    }

    pub(crate) fn load_hdc_bundle_details(&mut self, bundle: String, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcLoadBundleDetails { bundle });
    }

    pub(crate) fn close_hdc_bundle_details(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcCloseBundleDetails);
    }

    pub(crate) fn open_hdc_clear_app_data(
        &mut self,
        bundle: String,
        cache_only: bool,
        _cx: &mut Context<Self>,
    ) {
        self.core_command(Command::HdcOpenClearAppData {
            bundle,
            cache: cache_only,
        });
    }


    pub(crate) fn open_hdc_scheme(&mut self, uri: String, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcOpenScheme { uri });
    }

    pub(crate) fn load_hdc_files(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcLoadFiles);
    }

    pub(crate) fn navigate_hdc_files(&mut self, path: String, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcNavigateFiles { path });
    }

    pub(crate) fn go_parent_hdc_files(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcGoParentFiles);
    }

    pub(crate) fn pull_hdc_file(&mut self, remote: String, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcPullFile { remote });
    }

    pub(crate) fn pick_and_push_hdc(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::PickPushHdcFiles);
    }

    pub(crate) fn hdc_send_key(&mut self, key: HdcKey, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcSendKey { key });
    }

    pub(crate) fn hdc_tap(&mut self, x: String, y: String, tap: HdcTap, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcTap { x, y, tap });
    }

    pub(crate) fn hdc_fling(&mut self, direction: HdcSwipeDirection, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcFling { direction });
    }

    pub(crate) fn hdc_input_text(
        &mut self,
        x: String,
        y: String,
        text: String,
        _cx: &mut Context<Self>,
    ) {
        self.core_command(Command::HdcInputText { x, y, text });
    }

    pub(crate) fn hdc_wake_screen(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcWakeScreen);
    }

    pub(crate) fn submit_hdc_terminal(&mut self, cx: &mut Context<Self>) {
        let command = self.harmony.terminal_input.clone();
        self.harmony_terminal_field
            .update(cx, |state, cx| state.set_value("", cx));
        self.harmony.terminal_input.clear();
        self.core_command(Command::HdcSubmitTerminal { command });
    }

    pub(crate) fn run_hdc_quick_command(
        &mut self,
        command: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let command = command.into();
        self.harmony.terminal_input = command.clone();
        self.harmony_terminal_field
            .update(cx, |state, cx| state.set_value(&command, cx));
        self.submit_hdc_terminal(cx);
    }

    pub(crate) fn load_hdc_logs(&mut self, cx: &mut Context<Self>) {
        let tag = self.harmony_logs_tag_field.read(cx).value().to_string();
        self.core_command(Command::HdcLoadLogs {
            tag,
            filter: self.harmony.logs_filter,
        });
    }

    pub(crate) fn export_hdc_logs(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcExportLogs);
    }

    pub(crate) fn open_hdc_clear_logs(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcOpenClearLogs);
    }


    pub(crate) fn export_hdc_faultlog(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcExportFaultlog);
    }

    pub(crate) fn export_hdc_hilog_files(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcExportHilogFiles);
    }

    pub(crate) fn hdc_screenshot(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcScreenshot);
    }

    pub(crate) fn open_hdc_reboot(&mut self, target: HdcRebootTarget, _cx: &mut Context<Self>) {
        self.core_command(Command::HdcOpenReboot { target });
    }


    // ---------------------------------------------------------------------
    // 重启菜单
    // ---------------------------------------------------------------------

    pub(crate) fn open_reboot_menu(
        &mut self,
        anchor: gpui::Point<gpui::Pixels>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        use material3_gpui::icon::IconName;
        use material3_gpui::overlay::{MenuItem, MenuState, show_menu};
        if !self.has_device() {
            self.toast(cx, ToastKind::Info, "请先连接手表再重启");
            return;
        }
        let weak = cx.entity().downgrade();
        let mut menu = MenuState::new();
        for target in RebootTarget::ALL {
            let weak = weak.clone();
            menu = menu.item(
                MenuItem::new(target.menu_label())
                    .icon(IconName::new(target.icon()))
                    .on_click(move |_, cx| {
                        if let Err(err) =
                            weak.update(cx, |app, cx| app.request_reboot(target, cx))
                        {
                            eprintln!("RareBox Desktop: 重启设备失败: {err}");
                        }
                    }),
            );
        }
        let height = RebootTarget::ALL.len() as f32 * 48. + 16.;
        let menu = menu.build(cx);
        show_menu(
            window,
            cx,
            menu,
            crate::ui::menu_anchor(window, anchor, 260., height),
        );
    }

    pub(crate) fn request_reboot(&mut self, target: RebootTarget, _cx: &mut Context<Self>) {
        self.core_command(Command::RequestReboot { target });
    }

    pub(crate) fn open_hdc_reboot_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        use material3_gpui::icon::IconName;
        use material3_gpui::overlay::{MenuItem, MenuState, show_menu};
        let weak = cx.entity().downgrade();
        let mut menu = MenuState::new();
        for target in HdcRebootTarget::ALL {
            let weak = weak.clone();
            menu = menu.item(
                MenuItem::new(target.label())
                    .icon(IconName::new(target.icon()))
                    .on_click(move |_, cx| {
                        if let Err(err) =
                            weak.update(cx, |app, cx| app.open_hdc_reboot(target, cx))
                        {
                            eprintln!("RareBox Desktop: 打开鸿蒙重启确认失败: {err}");
                        }
                    }),
            );
        }
        let menu = menu.build(cx);
        let height = HdcRebootTarget::ALL.len() as f32 * 48. + 16.;
        show_menu(
            window,
            cx,
            menu,
            crate::ui::menu_anchor(window, position, 240., height),
        );
    }

    // ---------------------------------------------------------------------
    // 确认框（UI 外部入口）
    // ---------------------------------------------------------------------

}

