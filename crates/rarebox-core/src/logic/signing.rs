//! 鸿蒙签名页的状态与动作（逻辑层）。
//!
//! 网络、密钥与签名全部在后台线程；七步流水线通过消息队列把 `FlowEvent`
//! 送回串行线程，再以状态快照的形式发给宿主。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

use crate::engine::{Core, Ctx, Message, SigningMessage};
use crate::event::PickPurpose;
use crate::signing::ui::{ImportFile, LoginPhase, SigningUi};
use crate::signing::{
    self, FlowEvent, FlowOutcome, FlowRequest, SigningBlock, SigningService, Stage, StageStatus,
};
use crate::state::ToastKind;

impl Core {
    fn signing_task_busy(&self) -> bool {
        self.state.signing.busy || self.state.activity.is_some()
    }

    /// 首次进入签名页时加载账号与云端资源。
    pub(crate) fn ensure_signing_loaded(&mut self, cx: &mut Ctx) {
        if self.state.signing.initialized || self.signing_task_busy() {
            return;
        }
        self.state.signing.initialized = true;
        if self.state.signing.udid.is_none() {
            self.load_signing_udid(cx);
        }
        self.refresh_signing(cx);
    }

    pub(crate) fn refresh_signing(&mut self, cx: &mut Ctx) {
        if self.signing_task_busy() || !self.state.signing.service.is_logged_in() {
            return;
        }
        let mut service = self.state.signing.service.clone();
        let label = "正在读取华为账号与签名资源…";
        self.run_task(
            cx,
            label,
            move || {
                let mut logs = Vec::new();
                service.reload_all(&mut |message| logs.push(message.to_string()));
                Ok((service, logs))
            },
            |core, result, cx| match result {
                Ok((service, logs)) => {
                    let error = service.last_error.clone();
                    core.state.signing.service = service;
                    core.state.signing.reload_local_profiles();
                    core.state.signing.error = error;
                    for line in logs {
                        core.state.signing.push_log(line);
                    }
                }
                Err(err) => {
                    core.state.signing.error = Some(format!("{err:#}"));
                    cx.toast(ToastKind::Error, format!("读取签名资源失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn signing_login(&mut self, cx: &mut Ctx) {
        if matches!(self.state.signing.login, LoginPhase::Waiting) {
            if let Some(cancel) = &self.state.signing.cancel_login {
                cancel.store(true, Ordering::Relaxed);
            }
            self.state.signing.login = LoginPhase::Idle;
            cx.toast(ToastKind::Info, "已取消登录");
            return;
        }
        if self.signing_task_busy() {
            cx.toast(ToastKind::Info, "请等待当前任务完成");
            return;
        }
        let mut service = self.state.signing.service.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.state.signing.cancel_login = Some(cancel.clone());
        self.state.signing.login = LoginPhase::Waiting;
        self.state.signing.error = None;
        let event_sink = self.events.clone();
        self.run_task(
            cx,
            "等待浏览器完成华为账号登录…",
            move || {
                let mut logs = Vec::new();
                let url_sink = event_sink;
                let result = service.login(cancel, &mut |message| logs.push(message.to_string()), &mut |url| {
                    let _ = url_sink.try_send(crate::event::Event::OpenUrl(url.to_string()));
                    Ok(())
                });
                Ok((service, logs, result))
            },
            |core, result, cx| match result {
                Ok((service, logs, login)) => {
                    core.state.signing.service = service;
                    core.state.signing.reload_local_profiles();
                    core.state.signing.cancel_login = None;
                    for line in logs {
                        core.state.signing.push_log(line);
                    }
                    match login {
                        Ok(()) => {
                            core.state.signing.login = LoginPhase::Idle;
                            let name = core
                                .state
                                .signing
                                .service
                                .auth
                                .as_ref()
                                .map(|auth| auth.nick_name.clone())
                                .unwrap_or_default();
                            cx.toast(ToastKind::Success, format!("已登录：{name}"));
                            core.refresh_signing(cx);
                        }
                        Err(err) => {
                            let message = format!("{err:#}");
                            if message.contains("已取消登录") {
                                core.state.signing.login = LoginPhase::Idle;
                                cx.toast(ToastKind::Info, "已取消登录");
                            } else {
                                core.state.signing.login = LoginPhase::Failed(message.clone());
                                core.state.signing.push_log(&message);
                                cx.toast(ToastKind::Error, format!("登录失败：{message}"));
                            }
                        }
                    }
                }
                Err(err) => {
                    core.state.signing.login = LoginPhase::Failed(format!("{err:#}"));
                    cx.toast(ToastKind::Error, format!("登录任务失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn signing_logout(&mut self, cx: &mut Ctx) {
        if self.signing_task_busy() {
            cx.toast(ToastKind::Info, "请等待当前任务完成");
            return;
        }
        match self.state.signing.service.logout() {
            Ok(()) => {
                self.state.signing = SigningUi::new();
                self.state.signing.initialized = true;
                cx.toast(ToastKind::Info, "已退出华为账号");
            }
            Err(err) => cx.toast(ToastKind::Error, format!("退出登录失败：{err:#}")),
        }
    }

    pub(crate) fn signing_select_team(&mut self, team_id: String, cx: &mut Ctx) {
        if self.signing_task_busy() {
            cx.toast(ToastKind::Info, "请等待当前任务完成");
            return;
        }
        let mut service = self.state.signing.service.clone();
        self.run_task(
            cx,
            "正在切换团队…",
            move || {
                service.select_team(&team_id)?;
                let mut logs = Vec::new();
                service.reload_all(&mut |message| logs.push(message.to_string()));
                Ok((service, logs))
            },
            |core, result, cx| match result {
                Ok((service, logs)) => {
                    core.state.signing.service = service;
                    core.state.signing.reload_local_profiles();
                    for line in logs {
                        core.state.signing.push_log(line);
                    }
                    core.state.signing.acl.clear();
                }
                Err(err) => cx.toast(ToastKind::Error, format!("切换团队失败：{err:#}")),
            },
        );
    }

    pub(crate) fn signing_switch_block(&mut self, block: SigningBlock, _cx: &mut Ctx) {
        self.state.signing.block = block;
    }

    pub(crate) fn signing_apply_cert(&mut self, reset: bool, cx: &mut Ctx) {
        if self.signing_task_busy() {
            cx.toast(ToastKind::Info, "请等待当前任务完成");
            return;
        }
        if !self.state.signing.service.is_logged_in() {
            cx.toast(ToastKind::Error, "请先登录华为开发者账号");
            return;
        }
        let mut service = self.state.signing.service.clone();
        let kind = self.state.signing.kind;
        self.run_task(
            cx,
            if reset {
                "正在重置并申请证书…"
            } else {
                "正在准备证书…"
            },
            move || {
                let mut logs = Vec::new();
                let identity =
                    service.ensure_identity(kind, reset, &mut |message| logs.push(message.to_string()));
                Ok((service, logs, identity))
            },
            |core, result, cx| match result {
                Ok((service, logs, identity)) => {
                    core.state.signing.service = service;
                    core.state.signing.reload_local_profiles();
                    core.state.signing.reset_cert = false;
                    for line in logs {
                        core.state.signing.push_log(line);
                    }
                    match identity {
                        Ok(identity) => {
                            cx.toast(
                                ToastKind::Success,
                                format!("{} 已就绪", identity.cert_name),
                            );
                        }
                        Err(err) => {
                            let message = format!("{err:#}");
                            core.state.signing.error = Some(message.clone());
                            cx.toast(ToastKind::Error, message);
                        }
                    }
                }
                Err(err) => cx.toast(ToastKind::Error, format!("证书任务失败：{err:#}")),
            },
        );
    }

    pub(crate) fn signing_pick_hap(&mut self, cx: &mut Ctx) {
        if self.require_hdc_device().is_err() {
            cx.toast(ToastKind::Error, "请先连接并选择一台 HarmonyOS 设备");
            return;
        }
        cx.pick_files(PickPurpose::SigningHap);
    }

    pub(crate) fn signing_toggle_imported(&mut self) {
        if self.state.signing.busy {
            return;
        }
        self.state.signing.toggle_imported();
    }

    pub(crate) fn signing_pick_import_file(&mut self, which: ImportFile, cx: &mut Ctx) {
        cx.pick_files(PickPurpose::SigningImportFile(which));
    }

    pub(crate) fn signing_import_file(&mut self, which: ImportFile, path: PathBuf) {
        self.state.signing.set_import_file(which, path);
    }

    pub(crate) fn signing_set_inputs(&mut self, paths: Vec<PathBuf>, cx: &mut Ctx) {
        if self.state.signing.busy {
            cx.toast(ToastKind::Info, "签名任务进行中，暂时不能更换安装包");
            return;
        }
        self.state.signing.inputs = paths.clone();
        self.state.signing.package = None;
        self.state.signing.acl.clear();
        self.state.signing.outcome = None;
        self.state.signing.reset_stages();
        self.run_task(
            cx,
            "正在解析安装包…",
            move || {
                let mut metas = Vec::new();
                for path in &paths {
                    if path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| ext.eq_ignore_ascii_case("app"))
                        .unwrap_or(false)
                    {
                        anyhow::bail!("暂不支持对 .app 重新签名，请选择 .hap / .hsp");
                    }
                    metas.push(crate::signing::hap::parse(path)?);
                }
                let bundle = metas
                    .first()
                    .map(|meta| meta.bundle_name.clone())
                    .unwrap_or_default();
                if metas.iter().any(|meta| meta.bundle_name != bundle) {
                    anyhow::bail!("一次只能签名同一个 bundle 的安装包");
                }
                let mut acl: Vec<String> = metas
                    .iter()
                    .flat_map(|meta| meta.acl.iter().cloned())
                    .collect();
                acl.sort();
                acl.dedup();
                Ok((metas, acl))
            },
            |core, result, cx| match result {
                Ok((metas, acl)) => {
                    core.state.signing.package = metas.first().cloned();
                    core.state.signing.acl = acl;
                    core.state.signing.stages[Stage::Parse.index()] = StageStatus::Success;
                    core.state.signing.stage_messages[Stage::Parse.index()] = format!(
                        "{} · {} 个文件 · {} 项 ACL",
                        metas
                            .first()
                            .map(|meta| meta.bundle_name.as_str())
                            .unwrap_or_default(),
                        metas.len(),
                        core.state.signing.acl.len()
                    );
                    core.load_signing_udid(cx);
                }
                Err(err) => {
                    core.state.signing.stages[Stage::Parse.index()] = StageStatus::Failed;
                    core.state.signing.stage_messages[Stage::Parse.index()] = format!("{err:#}");
                    cx.toast(ToastKind::Error, format!("解析安装包失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn load_signing_udid(&mut self, cx: &mut Ctx) {
        if let Some(udid) = self
            .state
            .harmony
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.udid.clone())
            .filter(|udid| !udid.is_empty())
        {
            self.state.signing.udid = Some(udid);
            return;
        }
        let Ok((client, key)) = self.require_hdc_device() else {
            return;
        };
        self.run_task(
            cx,
            "正在读取设备 UDID…",
            move || client.udid(&key),
            |core, result, _cx| match result {
                Ok(Some(udid)) => {
                    core.state.signing.udid = Some(udid);
                }
                Ok(None) => {
                    core.state.signing.error = Some("设备未返回 UDID".into());
                }
                Err(err) => {
                    core.state.signing.error = Some(format!("读取 UDID 失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn signing_register_device(&mut self, cx: &mut Ctx) {
        let Some(udid) = self
            .state
            .signing
            .udid
            .clone()
            .filter(|udid| !udid.is_empty())
        else {
            cx.toast(ToastKind::Error, "请先读取设备 UDID");
            return;
        };
        if self.signing_task_busy() {
            cx.toast(ToastKind::Info, "请等待当前任务完成");
            return;
        }
        if !self.state.signing.service.is_logged_in() {
            cx.toast(ToastKind::Error, "请先登录华为开发者账号");
            return;
        }
        let mut service = self.state.signing.service.clone();
        self.run_task(
            cx,
            "正在注册调试设备…",
            move || {
                let mut logs = Vec::new();
                let device =
                    service.ensure_device(&udid, &mut |message| logs.push(message.to_string()));
                Ok((service, logs, device))
            },
            |core, result, cx| match result {
                Ok((service, logs, device)) => {
                    core.state.signing.service = service;
                    for line in logs {
                        core.state.signing.push_log(line);
                    }
                    match device {
                        Ok(device) => cx.toast(
                            ToastKind::Success,
                            format!("设备已注册：{}", device.device_name),
                        ),
                        Err(err) => {
                            cx.toast(ToastKind::Error, format!("注册设备失败：{err:#}"))
                        }
                    }
                }
                Err(err) => cx.toast(ToastKind::Error, format!("注册设备失败：{err:#}")),
            },
        );
    }

    pub(crate) fn signing_start(&mut self, cx: &mut Ctx) {
        if self.signing_task_busy() {
            cx.toast(ToastKind::Info, "请等待当前任务完成");
            return;
        }
        if self.state.signing.inputs.is_empty() {
            cx.toast(ToastKind::Error, "请先选择 HAP / HSP 文件");
            return;
        }
        if self.state.signing.udid.is_none() {
            self.load_signing_udid(cx);
            cx.toast(ToastKind::Info, "正在读取设备 UDID，请稍后再次开始");
            return;
        }
        let Ok((client, key)) = self.require_hdc_device() else {
            cx.toast(ToastKind::Error, "请先连接并选择一台 HarmonyOS 设备");
            return;
        };
        let udid = self.state.signing.udid.clone().unwrap_or_default();
        let inputs = self.state.signing.inputs.clone();
        let block = self.state.signing.block;
        let replace = self.state.signing.replace;
        let shared = self.state.signing.shared;

        if self.state.signing.use_imported {
            let Some(material) = self.state.signing.imported_material() else {
                cx.toast(
                    ToastKind::Error,
                    "请选择密钥库、证书、Profile，并填写密钥别名",
                );
                return;
            };
            let request = signing::flow::ImportedFlowRequest {
                inputs,
                material,
                block,
                replace,
                shared,
                udid,
                hdc: client,
                device_key: key,
                output_dir: self.state.signing.service.store.imported_dir(),
            };
            self.start_flow_task(
                cx,
                "正在使用导入材料签名并安装…",
                move |_, emit| signing::flow::run_imported(&request, emit),
            );
            return;
        }

        if !self.state.signing.service.is_logged_in() {
            cx.toast(
                ToastKind::Error,
                "请先登录华为开发者账号，或改用导入材料",
            );
            return;
        }
        let request = FlowRequest {
            inputs,
            kind: self.state.signing.kind,
            block,
            replace,
            shared,
            reset_cert: self.state.signing.reset_cert,
            refresh_profile: self.state.signing.refresh_profile,
            acl: self
                .state
                .signing
                .package
                .as_ref()
                .map(|_| self.state.signing.acl.clone()),
            extra_device_ids: Vec::new(),
            udid,
            hdc: client,
            device_key: key,
        };
        self.state.signing.reset_cert = false;
        self.state.signing.refresh_profile = false;
        self.start_flow_task(cx, "正在签名并安装…", move |service, emit| {
            signing::flow::run(service, &request, emit)
        });
    }

    fn start_flow_task<F>(&mut self, cx: &mut Ctx, label: &'static str, work: F)
    where
        F: FnOnce(&mut SigningService, &mut dyn FnMut(FlowEvent)) -> Result<FlowOutcome>
            + Send
            + 'static,
    {
        self.state.signing.reset_stages();
        self.state.signing.outcome = None;
        self.state.signing.error = None;
        self.state.signing.busy = true;
        self.state.signing.generation = self.state.signing.generation.wrapping_add(1);
        let generation = self.state.signing.generation;
        let mut service = self.state.signing.service.clone();
        self.state.activity = Some(label.to_string());

        let done = cx.done.clone();
        std::thread::Builder::new()
            .name("rarebox-signing".into())
            .spawn(move || {
                let progress = done.clone();
                let result = work(&mut service, &mut |event| {
                    let _ = progress.send_blocking(Message::Signing(SigningMessage::Event {
                        generation,
                        event,
                    }));
                });
                let _ = done.send_blocking(Message::Signing(SigningMessage::Done {
                    generation,
                    service,
                    result,
                }));
            })
            .expect("spawn signing flow");
    }

    pub(crate) fn apply_signing_message(
        &mut self,
        message: SigningMessage,
        cx: &mut Ctx,
    ) {
        match message {
            SigningMessage::Event { generation, event } => {
                if self.state.signing.generation == generation {
                    self.state.signing.apply_event(event);
                }
            }
            SigningMessage::Done {
                generation,
                service,
                result,
            } => {
                if self.state.signing.generation != generation {
                    return;
                }
                self.state.activity = None;
                self.state.signing.busy = false;
                self.state.signing.service = service;
                self.state.signing.reload_local_profiles();
                match result {
                    Ok(outcome) => {
                        self.state.signing.last_signed = outcome.signed.first().cloned();
                        let failed = outcome
                            .reports
                            .iter()
                            .filter(|report| !report.success)
                            .count();
                        if failed == 0 {
                            cx.toast(
                                ToastKind::Success,
                                format!(
                                    "{} · {} 个文件已签名并安装",
                                    outcome.bundle,
                                    outcome.reports.len()
                                ),
                            );
                        } else {
                            cx.toast(
                                ToastKind::Error,
                                format!(
                                    "安装完成：{} 个成功 · {failed} 个失败",
                                    outcome.reports.len() - failed
                                ),
                            );
                        }
                        self.state.signing.outcome = Some(outcome);
                    }
                    Err(err) => {
                        let message = format!("{err:#}");
                        self.state.signing.error = Some(message.clone());
                        self.state.signing.push_log(&message);
                        cx.toast(ToastKind::Error, format!("签名安装失败：{message}"));
                    }
                }
            }
        }
    }

    pub(crate) fn signing_toggle_acl(&mut self, permission: String) {
        self.state.signing.acl_toggle(&permission);
    }

    pub(crate) fn signing_toggle_replace(&mut self, shared: bool) {
        if shared {
            self.state.signing.shared = !self.state.signing.shared;
        } else {
            self.state.signing.replace = !self.state.signing.replace;
        }
    }

    pub(crate) fn signing_toggle_reset_cert(&mut self) {
        self.state.signing.reset_cert = !self.state.signing.reset_cert;
    }

    pub(crate) fn signing_toggle_refresh_profile(&mut self) {
        self.state.signing.refresh_profile = !self.state.signing.refresh_profile;
    }

    pub(crate) fn signing_toggle_logs(&mut self) {
        self.state.signing.show_logs = !self.state.signing.show_logs;
    }
}
