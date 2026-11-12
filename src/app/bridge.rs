//! 引擎事件 → 界面状态镜像的同步。

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{ClipboardItem, Context, PathPromptOptions};

use rarebox_core::event::Event;
use rarebox_core::state::CoreState;
use rarebox_core::{Command, DirectoryPurpose, PickPurpose};

use super::{Activity, Confirm, RareBox};

impl RareBox {
    pub(crate) fn start_event_loop(&mut self, cx: &mut Context<Self>) {
        let events = self.core.events();
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            while let Ok(event) = events.recv().await {
                let keep = weak
                    .update(cx, |app, cx| app.handle_core_event(event, cx))
                    .is_ok();
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn apply_core_state(&mut self, cx: &mut Context<Self>) {
        let state = self.core.state();
        self.sync_state(state, cx);
    }

    pub(crate) fn fetch_legal(&mut self, _cx: &mut Context<Self>) {
        self.core_command(Command::FetchLegal);
    }

    pub(crate) fn set_signing_alias(&mut self, value: String) {
        self.signing.set_import_alias(value.clone());
        self.core_command(Command::SigningSetImportAlias { value });
    }

    pub(crate) fn set_signing_password(&mut self, value: String) {
        self.signing.set_import_password(value.clone());
        self.core_command(Command::SigningSetImportPassword { value });
    }

    pub(crate) fn set_signing_acl_query(&mut self, query: String) {
        self.signing.set_acl_query(query.clone());
        self.core_command(Command::SigningSetAclQuery { query });
    }

    fn handle_core_event(&mut self, event: Event, cx: &mut Context<Self>) -> bool {
        match event {
            Event::Changed => {
                self.apply_core_state(cx);
                cx.notify();
            }
            Event::Toast { kind, message } => self.toast(cx, kind, message),
            Event::Confirm(request) => {
                self.open_confirm(
                    Confirm {
                        title: request.title.into(),
                        message: request.message.into(),
                        confirm_label: request.confirm_label.into(),
                        kind: request.kind,
                    },
                    cx,
                );
            }
            Event::PickFiles(purpose) => self.pick_files(purpose, cx),
            Event::PickDirectory(purpose) => self.pick_directory(purpose, cx),
            Event::OpenUrl(url) => open_url(&url),
            Event::OpenPath(path) => {
                cx.open_with_system(&PathBuf::from(path));
            }
            Event::CopyText(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            Event::Legal(docs) => {
                if let Some(modal) = self.legal.as_mut() {
                    if let Some(eula) = docs.eula {
                        modal.eula = eula;
                    }
                    if let Some(privacy) = docs.privacy {
                        modal.privacy = privacy;
                    }
                    modal.loading = false;
                    modal.error = docs
                        .error
                        .map(|err| format!("{err}；已使用内置条款，仍可继续。"));
                    cx.notify();
                }
            }
            Event::Video(frame) => {
                if frame.generation == self.remote_generation {
                    if let Some(image) = build_remote_image(&frame) {
                        let new_size = (frame.width as u16, frame.height as u16);
                        if new_size != self.remote_size {
                            self.remote_size = new_size;
                        }
                        self.remote_frame_count = self.remote_frame_count.wrapping_add(1);
                        if let Some(old) = self.remote_frame.replace(image) {
                            self.remote_retired_frames.push(old);
                        }
                        cx.notify();
                    }
                }
            }
            Event::VideoEnded { .. } => {}
            Event::Quit => {
                cx.quit();
                return false;
            }
        }
        true
    }

    fn sync_state(&mut self, state: Arc<CoreState>, cx: &mut Context<Self>) {
        let prev = self.core_state.clone();

        self.device_kind = state.device_kind;
        self.kind_mode = state.kind_mode;
        self.config = state.config.clone();
        self.adb_path = state.adb_path.clone();
        self.adb_version = state.adb_version.clone();
        self.adb_error = state.adb_error.clone();
        self.mdns = state.mdns.clone();
        self.scan = state.scan.clone();
        self.connect_error = state.connect_error.clone();

        self.harmony.path = state.harmony.path.clone();
        self.harmony.version = state.harmony.version.clone();
        self.harmony.error = state.harmony.error.clone();
        self.harmony.devices = state.harmony.devices.clone();
        self.harmony.selected = state.harmony.selected.clone();
        self.harmony.snapshot = state.harmony.snapshot.clone();
        self.harmony.snapshot_at = state.harmony.snapshot_at.clone();
        self.harmony.packages = state.harmony.packages.clone();
        self.harmony.packages_for = state.harmony.packages_for.clone();
        self.harmony.install_reports = state.harmony.install_reports.clone();
        self.harmony.install_replace = state.harmony.install_replace;
        self.harmony.install_shared = state.harmony.install_shared;
        self.harmony.bundle_details = state.harmony.bundle_details.clone();
        self.harmony.details_for = state.harmony.details_for.clone();
        self.harmony.forwards = state.harmony.forwards.clone();
        self.harmony.forward_reverse = state.harmony.forward_reverse;
        self.harmony.files = state.harmony.files.clone();
        self.harmony.last_file = state.harmony.last_file.clone();
        self.harmony.logs = state.harmony.logs.clone();
        self.harmony.logs_at = state.harmony.logs_at.clone();
        self.harmony.last_log_export = state.harmony.last_log_export.clone();
        self.harmony.screenshot = state.harmony.screenshot.clone();

        if prev.harmony.address != state.harmony.address {
            set_field(&self.harmony_address_field, &state.harmony.address, cx);
        }
        self.harmony.address = state.harmony.address.clone();
        if prev.harmony.files_path != state.harmony.files_path {
            set_field(&self.harmony_files_field, &state.harmony.files_path, cx);
        }
        self.harmony.files_path = state.harmony.files_path.clone();

        self.signing = state.signing.clone();

        self.devices = state.devices.clone();
        self.selected_serial = state.selected_serial.clone();
        self.snapshot = state.snapshot.clone();
        self.snapshot_at = state.snapshot_at.clone();
        self.activity = state.activity.as_ref().map(|label| Activity {
            label: label.clone().into(),
        });

        self.packages = state.packages.clone();
        self.package_scope = state.package_scope;
        self.package_loaded_for = state.package_loaded_for.clone();
        self.package_metadata_for = state.package_metadata_for.clone();
        self.install_batch = state.install_batch.clone();
        self.install_plan = state.install_plan.clone();

        self.last_screenshot = state.last_screenshot.clone();
        self.last_recording = state.last_recording.clone();
        self.files_entries = state.files_entries.clone();
        self.files_selected = state.files_selected.clone();
        self.last_export = state.last_export.clone();
        if prev.files_path != state.files_path {
            set_field(&self.files_path_field, &state.files_path, cx);
        }
        self.files_path = state.files_path.clone();

        if prev.screen_size != state.screen_size {
            set_field(&self.screen_size_field, &state.screen_size, cx);
        }
        if prev.screen_density != state.screen_density {
            set_field(&self.screen_density_field, &state.screen_density, cx);
        }

        self.terminal_history = state.terminal_history.clone();
        self.logs = state.logs.clone();
        self.logs_raw = state.logs_raw.clone();
        self.logs_at = state.logs_at.clone();

        self.imes = state.imes.clone();
        self.ime_loaded_for = state.ime_loaded_for.clone();
        if prev.appops_package != state.appops_package {
            set_field(&self.appops_package_field, &state.appops_package, cx);
        }
        self.appops_package = state.appops_package.clone();
        self.appops_ops = state.appops_ops.clone();
        self.appops_loaded_for = state.appops_loaded_for.clone();

        self.remote_phase = state.remote_phase;
        self.remote_error = state.remote_error.clone();
        self.remote_size = state.remote_size;
        self.remote_generation = state.remote_generation;

        if prev.adb_input != state.adb_input {
            set_field(&self.adb_field, &state.adb_input, cx);
        }
        self.adb_input = state.adb_input.clone();
        if prev.hdc_input != state.hdc_input {
            set_field(&self.hdc_field, &state.hdc_input, cx);
        }
        self.hdc_input = state.hdc_input.clone();
        if prev.screenshot_input != state.screenshot_input {
            set_field(&self.screenshot_field, &state.screenshot_input, cx);
        }
        self.screenshot_input = state.screenshot_input.clone();
        if prev.wireless_address != state.wireless_address {
            set_field(&self.wireless_field, &state.wireless_address, cx);
        }
        self.wireless_address = state.wireless_address.clone();

        self.update.status = state.update.status.clone();
        self.update.checking = state.update.checking;

        if let Some(modal) = self.announcements.as_mut() {
            let changed = prev.announcements != state.announcements
                || prev.announcements_error != state.announcements_error;
            if changed {
                modal.items = state.announcements.clone();
                modal.error = state.announcements_error.clone();
                modal.loading = false;
                if !modal
                    .items
                    .iter()
                    .any(|item| Some(item.id) == modal.selected)
                {
                    modal.selected = modal.items.first().map(|item| item.id);
                }
            }
        }

        self.core_state = state;
        cx.notify();
    }

    fn pick_files(&mut self, purpose: PickPurpose, cx: &mut Context<Self>) {
        let (prompt, multiple) = match purpose {
            PickPurpose::InstallApk => ("选择要安装的 APK（分包可一次多选）", true),
            PickPurpose::InstallHap => ("选择要安装的 HAP / HSP / APP 包", true),
            PickPurpose::PushFiles => ("选择要上传到设备的文件", true),
            PickPurpose::PushHdcFiles => ("选择要上传到 HarmonyOS 设备的文件", true),
            PickPurpose::SigningHap => ("选择要签名并安装的 HAP / HSP 包", true),
            PickPurpose::SigningImportFile(which) => (
                match which {
                    rarebox_core::signing::ImportFile::Keystore => "选择密钥库（.p12 / .jks）",
                    rarebox_core::signing::ImportFile::Cert => "选择应用证书（.cer / .pem）",
                    rarebox_core::signing::ImportFile::Profile => "选择 Profile（.p7b）",
                },
                false,
            ),
        };
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple,
            prompt: Some(prompt.into()),
        });
        let core = self.core.clone();
        cx.spawn(async move |_, _cx| match receiver.await {
            Ok(Ok(Some(paths))) if !paths.is_empty() => {
                if let Some(command) = pick_command(purpose, paths)
                    && let Err(err) = core.command(command)
                {
                    eprintln!("RareBox Desktop: 文件选择结果提交失败: {err}");
                }
            }
            Ok(Ok(Some(_))) | Ok(Ok(None)) => {}
            Ok(Err(err)) => eprintln!("RareBox Desktop: 无法打开文件选择器: {err:#}"),
            Err(err) => eprintln!("RareBox Desktop: 文件选择已取消: {err}"),
        })
        .detach();
    }

    fn pick_directory(&mut self, purpose: DirectoryPurpose, cx: &mut Context<Self>) {
        let prompt = match purpose {
            DirectoryPurpose::Screenshot => "选择截图与录屏保存目录",
        };
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        let core = self.core.clone();
        cx.spawn(async move |_, _cx| match receiver.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let command = Command::SetScreenshotDir {
                        path: path.to_string_lossy().to_string(),
                    };
                    if let Err(err) = core.command(command) {
                        eprintln!("RareBox Desktop: 截图目录提交失败: {err}");
                    }
                }
            }
            Ok(Ok(None)) => {}
            Ok(Err(err)) => eprintln!("RareBox Desktop: 无法打开目录选择器: {err:#}"),
            Err(err) => eprintln!("RareBox Desktop: 目录选择已取消: {err}"),
        })
        .detach();
    }
}

fn set_field(
    field: &gpui::Entity<material3_gpui::prelude::TextFieldState>,
    value: &str,
    cx: &mut Context<RareBox>,
) {
    field.update(cx, |state, cx| state.set_value(value, cx));
}

fn pick_command(purpose: PickPurpose, paths: Vec<PathBuf>) -> Option<Command> {
    Some(match purpose {
        PickPurpose::InstallApk => Command::InstallApks { paths },
        PickPurpose::InstallHap => Command::HdcInstallHaps { paths },
        PickPurpose::PushFiles => Command::PushFiles { paths },
        PickPurpose::PushHdcFiles => Command::HdcUploadFiles { paths },
        PickPurpose::SigningHap => Command::SigningSetInputs { paths },
        PickPurpose::SigningImportFile(which) => Command::SigningImportFile {
            which,
            path: paths.into_iter().next()?,
        },
    })
}

fn build_remote_image(frame: &rarebox_core::VideoFrame) -> Option<Arc<gpui::RenderImage>> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    let buffer = image::ImageBuffer::from_raw(frame.width, frame.height, frame.bgra.clone())?;
    let image = image::Frame::new(buffer);
    Some(Arc::new(gpui::RenderImage::new(smallvec::smallvec![image])))
}

fn open_url(url: &str) {
    #[cfg(target_os = "android")]
    {
        let _ = crate::android::open_url(url);
    }
    #[cfg(not(target_os = "android"))]
    {
        if let Err(err) = rarebox_core::update::open_url(url) {
            eprintln!("RareBox Desktop: 无法打开链接: {err:#}");
        }
    }
}
