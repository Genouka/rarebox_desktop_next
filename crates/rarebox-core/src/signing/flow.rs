//! 七步签名安装流水线：解析 → 账号 → 证书 → 设备 → Profile → 签名 → 安装。
//!
//! 该模块不依赖 GPUI；UI 通过 `FlowEvent` 更新步骤状态。网络、密钥和 ZIP
//! 操作全部是阻塞调用，必须在后台线程运行。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};

use crate::hdc::{self, HdcClient, HdcInstallOptions};

use super::model::{CertKind, HapMeta, ImportedMaterial, LocalIdentity, SigningBlock};
use super::service::SigningService;
use super::{hap, identity, profile, sign};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Parse,
    Account,
    Certificate,
    Device,
    Profile,
    Sign,
    Install,
}

impl Stage {
    pub const ALL: [Stage; 7] = [
        Stage::Parse,
        Stage::Account,
        Stage::Certificate,
        Stage::Device,
        Stage::Profile,
        Stage::Sign,
        Stage::Install,
    ];

    pub fn index(self) -> usize {
        match self {
            Stage::Parse => 0,
            Stage::Account => 1,
            Stage::Certificate => 2,
            Stage::Device => 3,
            Stage::Profile => 4,
            Stage::Sign => 5,
            Stage::Install => 6,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Parse => "解析安装包",
            Stage::Account => "账号验证",
            Stage::Certificate => "准备证书",
            Stage::Device => "注册调试设备",
            Stage::Profile => "准备 Profile",
            Stage::Sign => "签名应用",
            Stage::Install => "安装应用",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageStatus {
    Pending,
    Running,
    Success,
    Failed,
}

#[derive(Clone, Debug)]
pub enum FlowEvent {
    Stage {
        stage: Stage,
        status: StageStatus,
        message: String,
    },
    Log(String),
}

#[derive(Clone, Debug)]
pub struct FlowInstallReport {
    pub title: String,
    pub detail: String,
    pub success: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FlowOutcome {
    pub bundle: String,
    pub signed: Vec<PathBuf>,
    pub reports: Vec<FlowInstallReport>,
}

pub struct FlowRequest {
    pub inputs: Vec<PathBuf>,
    pub kind: CertKind,
    pub block: SigningBlock,
    pub replace: bool,
    pub shared: bool,
    pub reset_cert: bool,
    pub refresh_profile: bool,
    /// 用户在 ACL 面板中的最终选择；`None` 表示使用 module.json 推导结果。
    pub acl: Option<Vec<String>>,
    pub extra_device_ids: Vec<String>,
    pub udid: String,
    pub hdc: HdcClient,
    pub device_key: String,
}

/// 手动导入材料的签名安装请求。
pub struct ImportedFlowRequest {
    pub inputs: Vec<PathBuf>,
    pub material: ImportedMaterial,
    pub block: SigningBlock,
    pub replace: bool,
    pub shared: bool,
    pub udid: String,
    pub hdc: HdcClient,
    pub device_key: String,
    pub output_dir: PathBuf,
}

pub fn run(
    service: &mut SigningService,
    request: &FlowRequest,
    emit: &mut dyn FnMut(FlowEvent),
) -> Result<FlowOutcome> {
    emit_stage(emit, Stage::Parse, StageStatus::Running, String::new());
    let parsed = match parse_inputs(&request.inputs) {
        Ok(parsed) => parsed,
        Err(err) => {
            emit_stage(emit, Stage::Parse, StageStatus::Failed, format!("{err:#}"));
            return Err(err);
        }
    };
    let bundle = parsed[0].0.bundle_name.clone();
    let mut acl: Vec<String> = request.acl.clone().unwrap_or_else(|| {
        parsed
            .iter()
            .flat_map(|(meta, _)| meta.acl.iter().cloned())
            .collect()
    });
    acl.sort();
    acl.dedup();
    emit_stage(
        emit,
        Stage::Parse,
        StageStatus::Success,
        format!(
            "{} · {} 个文件 · {} 项 ACL",
            bundle,
            parsed.len(),
            acl.len()
        ),
    );

    emit_stage(emit, Stage::Account, StageStatus::Running, String::new());
    if let Err(err) = service.ensure_auth_fresh() {
        emit_stage(
            emit,
            Stage::Account,
            StageStatus::Failed,
            format!("{err:#}"),
        );
        return Err(err);
    }
    let auth = service
        .auth
        .clone()
        .ok_or_else(|| anyhow!("请先登录华为开发者账号"))?;
    let team_id = auth.team_id_or_user().to_string();
    let nick = if auth.nick_name.is_empty() {
        auth.user_id.clone()
    } else {
        auth.nick_name.clone()
    };
    emit_stage(
        emit,
        Stage::Account,
        StageStatus::Success,
        format!(
            "{nick} · {}",
            service
                .current_team()
                .map(|team| team.name.as_str())
                .unwrap_or(&team_id)
        ),
    );

    emit_stage(
        emit,
        Stage::Certificate,
        StageStatus::Running,
        String::new(),
    );
    let identity = match identity::ensure_identity(
        &service.api,
        &auth,
        &service.store,
        &team_id,
        request.kind,
        request.reset_cert,
        &mut |message| emit_log(emit, message),
    ) {
        Ok(identity) => identity,
        Err(err) => {
            emit_stage(
                emit,
                Stage::Certificate,
                StageStatus::Failed,
                format!("{err:#}"),
            );
            return Err(err);
        }
    };
    service.identities.insert(request.kind, identity.clone());
    emit_stage(
        emit,
        Stage::Certificate,
        StageStatus::Success,
        format!("{} · {}", identity.cert_name, identity.cert_id),
    );

    emit_stage(emit, Stage::Device, StageStatus::Running, String::new());
    let udid = request.udid.trim().to_string();
    if udid.is_empty() {
        emit_stage(
            emit,
            Stage::Device,
            StageStatus::Failed,
            "未读取到设备 UDID",
        );
        bail!("未读取到设备 UDID，请重新连接设备");
    }
    if udid.len() != 64 {
        emit_log(
            emit,
            &format!(
                "警告：UDID 长度为 {}（常见为 64），AGC 可能拒绝注册该设备",
                udid.len()
            ),
        );
    }
    let mut device_ids = Vec::new();
    if request.kind == CertKind::Debug {
        match service.ensure_device(&udid, &mut |message| emit_log(emit, message)) {
            Ok(device) => {
                device_ids.push(device.id.clone());
                emit_stage(
                    emit,
                    Stage::Device,
                    StageStatus::Success,
                    format!("{} · {}", device.device_name, udid),
                );
            }
            Err(err) => {
                emit_stage(emit, Stage::Device, StageStatus::Failed, format!("{err:#}"));
                return Err(err);
            }
        }
    } else {
        emit_stage(
            emit,
            Stage::Device,
            StageStatus::Success,
            format!("发布 Profile 不绑定设备 · {udid}"),
        );
    }
    for id in &request.extra_device_ids {
        if !id.is_empty() && !device_ids.contains(id) {
            device_ids.push(id.clone());
        }
    }

    let mut signed = Vec::new();
    let mut reports;
    let mut attempt = 0;
    loop {
        let mut used = BTreeSet::new();
        emit_stage(emit, Stage::Profile, StageStatus::Running, String::new());
        let cached = match profile::ensure_profile(
            &service.api,
            &auth,
            &service.store,
            &team_id,
            request.kind,
            &bundle,
            &identity,
            &device_ids,
            &[udid.clone()],
            &acl,
            request.refresh_profile || request.reset_cert || attempt > 0,
            &mut |message| emit_log(emit, message),
        ) {
            Ok(cached) => cached,
            Err(err) => {
                emit_stage(
                    emit,
                    Stage::Profile,
                    StageStatus::Failed,
                    format!("{err:#}"),
                );
                return Err(err);
            }
        };
        let profile_path = service.store.team_dir(&team_id).join(&cached.file);
        emit_stage(
            emit,
            Stage::Profile,
            StageStatus::Success,
            format!("{} · {}", cached.profile_id, profile_path.display()),
        );

        emit_stage(emit, Stage::Sign, StageStatus::Running, String::new());
        signed.clear();
        for (meta, path) in &parsed {
            let output = unique_output(&service.store, &team_id, path, &mut used);
            if let Err(err) = sign::sign_hap(
                &service.store,
                &team_id,
                &identity,
                &profile_path,
                path,
                &output,
                request.block,
            ) {
                emit_stage(emit, Stage::Sign, StageStatus::Failed, format!("{err:#}"));
                return Err(err).with_context(|| format!("签名 {} 失败", path.display()));
            }
            emit_log(
                emit,
                &format!(
                    "已签名 {} → {}（{}）",
                    meta.bundle_name,
                    output.display(),
                    request.block.label()
                ),
            );
            signed.push(output);
        }
        emit_stage(
            emit,
            Stage::Sign,
            StageStatus::Success,
            format!("{} 个文件 · {}", signed.len(), request.block.label()),
        );

        emit_stage(emit, Stage::Install, StageStatus::Running, String::new());
        reports = install_all(
            &request.hdc,
            &request.device_key,
            &signed,
            request.replace,
            request.shared,
        );
        let failed = reports.iter().filter(|report| !report.success).count();
        let succeeded = reports.len() - failed;

        if attempt == 0
            && reports
                .iter()
                .any(|report| !report.success && report.detail.contains("9568322"))
        {
            attempt = 1;
            emit_log(
                emit,
                "安装提示 Profile/证书不匹配，重新申请 Profile 后重试一次…",
            );
            continue;
        }

        if failed == 0 {
            emit_stage(
                emit,
                Stage::Install,
                StageStatus::Success,
                format!("{succeeded} 个文件安装成功"),
            );
        } else {
            emit_stage(
                emit,
                Stage::Install,
                StageStatus::Failed,
                format!("{succeeded} 个成功 · {failed} 个失败"),
            );
        }
        return Ok(FlowOutcome {
            bundle,
            signed,
            reports,
        });
    }
}

fn emit_stage(
    emit: &mut dyn FnMut(FlowEvent),
    stage: Stage,
    status: StageStatus,
    message: impl Into<String>,
) {
    emit(FlowEvent::Stage {
        stage,
        status,
        message: message.into(),
    });
}

fn emit_log(emit: &mut dyn FnMut(FlowEvent), message: &str) {
    emit(FlowEvent::Log(message.to_string()));
}

fn install_all(
    hdc: &HdcClient,
    device_key: &str,
    signed: &[PathBuf],
    replace: bool,
    shared: bool,
) -> Vec<FlowInstallReport> {
    let options = HdcInstallOptions { replace, shared };
    let mut reports = Vec::new();
    for path in signed {
        let title = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        match hdc.install(device_key, path, options) {
            Ok(output) => {
                let text = output.combined();
                let success = hdc::install_succeeded(&text);
                let detail = if success {
                    "安装成功".to_string()
                } else if let Some(hint) = hdc::install_error_hint(&text) {
                    hint.to_string()
                } else {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        output.error_message()
                    } else {
                        trimmed.chars().take(500).collect()
                    }
                };
                reports.push(FlowInstallReport {
                    title,
                    detail,
                    success,
                });
            }
            Err(err) => reports.push(FlowInstallReport {
                title,
                detail: format!("{err:#}"),
                success: false,
            }),
        }
    }
    reports
}

pub fn run_imported(
    request: &ImportedFlowRequest,
    emit: &mut dyn FnMut(FlowEvent),
) -> Result<FlowOutcome> {
    emit_stage(emit, Stage::Parse, StageStatus::Running, String::new());
    let parsed = match parse_inputs(&request.inputs) {
        Ok(parsed) => parsed,
        Err(err) => {
            emit_stage(emit, Stage::Parse, StageStatus::Failed, format!("{err:#}"));
            return Err(err);
        }
    };
    let bundle = parsed[0].0.bundle_name.clone();
    let mut acl: Vec<String> = parsed
        .iter()
        .flat_map(|(meta, _)| meta.acl.iter().cloned())
        .collect();
    acl.sort();
    acl.dedup();
    emit_stage(
        emit,
        Stage::Parse,
        StageStatus::Success,
        format!("{} · {} 个文件 · 导入材料", bundle, parsed.len()),
    );

    emit_stage(emit, Stage::Account, StageStatus::Running, String::new());
    emit_stage(
        emit,
        Stage::Account,
        StageStatus::Success,
        "使用导入的签名材料，跳过华为账号",
    );

    emit_stage(
        emit,
        Stage::Certificate,
        StageStatus::Running,
        String::new(),
    );
    let cert_bytes = match std::fs::read(&request.material.cert) {
        Ok(bytes) => bytes,
        Err(err) => {
            emit_stage(
                emit,
                Stage::Certificate,
                StageStatus::Failed,
                format!("{err:#}"),
            );
            return Err(err.into());
        }
    };
    let spki = match super::identity::cert_spki_sha256(&cert_bytes) {
        Ok(Some(spki)) => spki,
        Ok(None) => {
            let err = anyhow!("导入的证书不是有效的 X.509 证书");
            emit_stage(
                emit,
                Stage::Certificate,
                StageStatus::Failed,
                err.to_string(),
            );
            return Err(err);
        }
        Err(err) => {
            emit_stage(
                emit,
                Stage::Certificate,
                StageStatus::Failed,
                format!("{err:#}"),
            );
            return Err(err);
        }
    };
    if let Err(err) = sign::validate_material(&sign::SigningFiles {
        keystore: &request.material.keystore,
        store_password: &request.material.store_password,
        key_alias: &request.material.key_alias,
        key_password: &request.material.key_password,
        cert: &request.material.cert,
        profile: &request.material.profile,
    }) {
        emit_stage(
            emit,
            Stage::Certificate,
            StageStatus::Failed,
            format!("{err:#}"),
        );
        return Err(err);
    }
    emit_stage(
        emit,
        Stage::Certificate,
        StageStatus::Success,
        request
            .material
            .cert
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "已导入证书".to_string()),
    );

    emit_stage(emit, Stage::Device, StageStatus::Running, String::new());
    let udid = request.udid.trim().to_string();
    if udid.is_empty() {
        let err = anyhow!("未读取到设备 UDID，请重新连接设备");
        emit_stage(emit, Stage::Device, StageStatus::Failed, err.to_string());
        return Err(err);
    }
    emit_stage(emit, Stage::Device, StageStatus::Success, udid.clone());

    emit_stage(emit, Stage::Profile, StageStatus::Running, String::new());
    let profile_content = match super::profile::parse_profile(&request.material.profile) {
        Ok(content) => content,
        Err(err) => {
            emit_stage(
                emit,
                Stage::Profile,
                StageStatus::Failed,
                format!("{err:#}"),
            );
            return Err(err);
        }
    };
    let kind = match profile_content.profile_type.to_ascii_lowercase().as_str() {
        "debug" => CertKind::Debug,
        "release" => CertKind::Release,
        other => {
            let err = anyhow!("不支持的 Profile 类型：{other}");
            emit_stage(emit, Stage::Profile, StageStatus::Failed, err.to_string());
            return Err(err);
        }
    };
    let identity = LocalIdentity {
        kind,
        keystore: String::new(),
        store_password: String::new(),
        key_alias: String::new(),
        key_password: String::new(),
        csr: String::new(),
        cert_id: String::new(),
        cert_name: "imported".to_string(),
        cert_path: String::new(),
        spki_sha256: spki,
        cert_expires_at: 0,
        created_at: 0,
    };
    if let Err(err) = super::profile::validate(
        &profile_content,
        &bundle,
        kind,
        &identity,
        std::slice::from_ref(&udid),
        &acl,
    ) {
        emit_stage(
            emit,
            Stage::Profile,
            StageStatus::Failed,
            format!("{err:#}"),
        );
        return Err(err);
    }
    emit_stage(
        emit,
        Stage::Profile,
        StageStatus::Success,
        format!("{} · {}", kind.short_label(), profile_content.bundle_name),
    );

    emit_stage(emit, Stage::Sign, StageStatus::Running, String::new());
    let mut signed = Vec::new();
    let mut used = BTreeSet::new();
    for (meta, path) in &parsed {
        let output = unique_in_dir(&request.output_dir, path, &mut used);
        if let Err(err) = sign::sign_hap_with(
            sign::SigningFiles {
                keystore: &request.material.keystore,
                store_password: &request.material.store_password,
                key_alias: &request.material.key_alias,
                key_password: &request.material.key_password,
                cert: &request.material.cert,
                profile: &request.material.profile,
            },
            path,
            &output,
            request.block,
        ) {
            emit_stage(emit, Stage::Sign, StageStatus::Failed, format!("{err:#}"));
            return Err(err).with_context(|| format!("签名 {} 失败", path.display()));
        }
        emit_log(
            emit,
            &format!("已签名 {} → {}", meta.bundle_name, output.display()),
        );
        signed.push(output);
    }
    emit_stage(
        emit,
        Stage::Sign,
        StageStatus::Success,
        format!("{} 个文件 · {}", signed.len(), request.block.label()),
    );

    emit_stage(emit, Stage::Install, StageStatus::Running, String::new());
    let reports = install_all(
        &request.hdc,
        &request.device_key,
        &signed,
        request.replace,
        request.shared,
    );
    let failed = reports.iter().filter(|report| !report.success).count();
    if failed == 0 {
        emit_stage(
            emit,
            Stage::Install,
            StageStatus::Success,
            format!("{} 个文件安装成功", reports.len()),
        );
    } else {
        emit_stage(
            emit,
            Stage::Install,
            StageStatus::Failed,
            format!("{} 个成功 · {failed} 个失败", reports.len() - failed),
        );
    }
    Ok(FlowOutcome {
        bundle,
        signed,
        reports,
    })
}

fn parse_inputs(paths: &[PathBuf]) -> Result<Vec<(HapMeta, PathBuf)>> {
    if paths.is_empty() {
        bail!("请选择要签名的 HAP / HSP 文件");
    }
    let mut parsed = Vec::new();
    for path in paths {
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("app"))
            .unwrap_or(false)
        {
            bail!("暂不支持对 .app 重新签名，请选择 .hap / .hsp");
        }
        let meta = hap::parse(path)?;
        parsed.push((meta, path.clone()));
    }
    let bundle = parsed[0].0.bundle_name.clone();
    if parsed.iter().any(|(meta, _)| meta.bundle_name != bundle) {
        bail!("一次只能签名同一个 bundle 的安装包");
    }
    Ok(parsed)
}

fn unique_output(
    store: &super::store::SigningStore,
    team_id: &str,
    input: &Path,
    used: &mut BTreeSet<PathBuf>,
) -> PathBuf {
    unique_in_dir(&store.team_dir(team_id).join("signed"), input, used)
}

fn unique_in_dir(dir: &Path, input: &Path, used: &mut BTreeSet<PathBuf>) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "app".to_string());
    let extension = input
        .extension()
        .map(|ext| ext.to_string_lossy().to_string())
        .unwrap_or_else(|| "hap".to_string());
    let mut index = 0;
    loop {
        let name = if index == 0 {
            format!("{stem}-signed.{extension}")
        } else {
            format!("{stem}-{}-signed.{extension}", index + 1)
        };
        let output = dir.join(name);
        if used.insert(output.clone()) {
            return output;
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_indexes_are_stable() {
        for (index, stage) in Stage::ALL.iter().enumerate() {
            assert_eq!(stage.index(), index);
        }
    }

    #[test]
    fn rejects_mixed_bundles() {
        let dir = std::env::temp_dir().join(format!("rarebox-flow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("a.hap");
        let second = dir.join("b.hap");
        write_hap(&first, "com.example.a");
        write_hap(&second, "com.example.b");
        assert!(parse_inputs(&[first, second]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_hap(path: &Path, bundle: &str) {
        use std::io::Write as _;
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("module.json", options).unwrap();
        write!(
            zip,
            r#"{{"app":{{"bundleName":"{bundle}"}},"module":{{"name":"entry"}}}}"#
        )
        .unwrap();
        zip.finish().unwrap();
    }
}
