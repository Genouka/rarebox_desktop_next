//! 调试/发布 Profile 的申请、下载与离线校验。
//!
//! Profile 是 AGC 签发的 CMS（`.p7b`），内嵌 JSON 描述 bundle、证书、ACL 与
//! 调试设备。可复用的前提是这些字段都与当前请求一致；否则重新申请，避免
//! 安装阶段才报 9568322。

use std::path::Path;

use anyhow::{Context as _, Result, anyhow, bail};
use hapsigner::ProfileVerifier;
use serde_json::Value;

use super::api::EcoClient;
use super::model::{AuthInfo, CachedProfile, CertKind, LocalIdentity, ProfileContent, now_secs};
use super::store::SigningStore;

pub fn parse_profile(path: &Path) -> Result<ProfileContent> {
    let bytes =
        std::fs::read(path).with_context(|| format!("无法读取 Profile {}", path.display()))?;
    parse_profile_bytes(&bytes)
}

pub fn parse_profile_bytes(bytes: &[u8]) -> Result<ProfileContent> {
    if let Ok(verified) = ProfileVerifier::verify(bytes) {
        let content = parse_profile_json(&verified.content)?;
        let mut content = content;
        content.signature_verified = true;
        return Ok(content);
    }
    // AGC 的 Profile 可能使用当前 vendored 验证器尚未覆盖的证书链/算法。
    // 内容本身通过 HTTPS 下载，这里仍提取 JSON 做校验，但标记未验签。
    let content = extract_embedded_json(bytes)
        .ok_or_else(|| anyhow!("Profile 不是有效的 CMS 或 JSON 文档"))?;
    let mut content = parse_profile_json(&content)?;
    content.signature_verified = false;
    Ok(content)
}

fn extract_embedded_json(bytes: &[u8]) -> Option<Vec<u8>> {
    let start = bytes.iter().position(|byte| *byte == b'{')?;
    let end = bytes.iter().rposition(|byte| *byte == b'}')?;
    if end <= start {
        return None;
    }
    Some(bytes[start..=end].to_vec())
}

fn parse_profile_json(bytes: &[u8]) -> Result<ProfileContent> {
    let value: Value = serde_json::from_slice(bytes).context("Profile 内嵌内容不是有效 JSON")?;
    let bundle = value.get("bundle-info").unwrap_or(&Value::Null);
    let acls = value.get("acls").unwrap_or(&Value::Null);
    let debug = value.get("debug-info").unwrap_or(&Value::Null);
    Ok(ProfileContent {
        bundle_name: string(bundle, "bundle-name"),
        profile_type: string(&value, "type"),
        not_after: int(value.pointer("/validity/not-after")),
        development_certificate: string(bundle, "development-certificate"),
        distribution_certificate: string(bundle, "distribution-certificate"),
        udids: strings(debug, "device-ids"),
        allowed_acls: strings(acls, "allowed-acls"),
        signature_verified: false,
    })
}

/// 校验 Profile 是否满足本次签名请求；返回可展示的警告。
pub fn validate(
    content: &ProfileContent,
    package: &str,
    kind: CertKind,
    identity: &LocalIdentity,
    udids: &[String],
    acl: &[String],
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let expected_type = match kind {
        CertKind::Debug => "debug",
        CertKind::Release => "release",
    };
    if !content.profile_type.eq_ignore_ascii_case(expected_type) {
        bail!(
            "Profile 类型为「{}」，与{}不匹配",
            content.profile_type,
            kind.label()
        );
    }
    if content.bundle_name != package {
        bail!(
            "Profile 绑定包名 {}，与目标 {} 不一致",
            content.bundle_name,
            package
        );
    }
    if !content.signature_verified {
        warnings.push("Profile 的 CMS 签名未能离线验证，仅校验了内嵌内容".to_string());
    }
    let cert = content.certificate();
    if cert.is_empty() {
        bail!("Profile 未包含签名证书");
    }
    if let Some(spki) = super::identity::cert_spki_sha256(cert.as_bytes())? {
        if !identity.spki_sha256.is_empty() && spki != identity.spki_sha256 {
            bail!("Profile 内嵌的证书与本地签名密钥不一致");
        }
    }
    if content.is_expired() {
        bail!(
            "Profile 已过期（{}）",
            super::model::format_timestamp(content.not_after)
        );
    }
    if content.not_after > 0 && content.not_after < now_secs() + 3600 {
        warnings.push("Profile 将在 1 小时内过期".to_string());
    }
    if kind == CertKind::Debug {
        let allows_any = content.udids.iter().any(|udid| udid == "*");
        if !allows_any {
            for udid in udids {
                if !content.udids.iter().any(|item| item == udid) {
                    bail!("Profile 未包含当前设备 UDID {udid}");
                }
            }
        }
    }
    let granted: std::collections::BTreeSet<&str> =
        content.allowed_acls.iter().map(String::as_str).collect();
    let missing: Vec<&str> = acl
        .iter()
        .map(String::as_str)
        .filter(|permission| !granted.contains(permission))
        .collect();
    if !missing.is_empty() {
        bail!("Profile 缺少 ACL 权限：{}", missing.join(", "));
    }
    Ok(warnings)
}

#[allow(clippy::too_many_arguments)]
pub fn ensure_profile(
    api: &EcoClient,
    auth: &AuthInfo,
    store: &SigningStore,
    team_id: &str,
    kind: CertKind,
    package: &str,
    identity: &LocalIdentity,
    device_ids: &[String],
    udids: &[String],
    acl: &[String],
    force: bool,
    log: &mut dyn FnMut(&str),
) -> Result<CachedProfile> {
    if !force {
        if let Some(cached) = store.find_profile(team_id, package, kind) {
            let path = store.team_dir(team_id).join(&cached.file);
            if path.exists() {
                if let Ok(content) = parse_profile(&path) {
                    let matches_cert = cached.cert_id == identity.cert_id;
                    let same_acl = acl.iter().all(|permission| cached.acl.contains(permission));
                    let same_devices = kind != CertKind::Debug
                        || udids
                            .iter()
                            .all(|udid| cached.udids.iter().any(|item| item == udid));
                    if matches_cert
                        && same_acl
                        && same_devices
                        && validate(&content, package, kind, identity, udids, acl).is_ok()
                    {
                        log(&format!("复用本地 Profile {}", path.display()));
                        return Ok(cached);
                    }
                }
            }
        }
    }

    if kind != CertKind::Debug {
        bail!("发布 Profile 请在 AGC 网站申请后，通过「导入签名材料」使用");
    }
    let profile_name = profile_name(kind, package);
    log(&format!("申请{} {}…", kind.short_label(), profile_name));
    let created = api.create_debug_profile(
        auth,
        &profile_name,
        &identity.cert_id,
        package,
        device_ids,
        acl,
    )?;
    let id = if created.id.is_empty() {
        format!("{}", now_secs())
    } else {
        created.id.clone()
    };
    let path = store.profile_path(team_id, package, &id);
    api.download(&created.provision_download_url, &path)?;
    let content = parse_profile(&path)?;
    let validation = validate(&content, package, kind, identity, udids, acl);
    let warnings = match validation {
        Ok(warnings) => warnings,
        Err(error) => {
            // 新创建的 Profile 不可用时及时清理，避免占用配额。
            let _ = std::fs::remove_file(&path);
            let _ = api.delete_profiles(auth, std::slice::from_ref(&id));
            return Err(error);
        }
    };
    for warning in warnings {
        log(&warning);
    }
    if content.not_after > 0 && content.not_after < now_secs() + 60 {
        let _ = std::fs::remove_file(&path);
        let _ = api.delete_profiles(auth, std::slice::from_ref(&id));
        bail!("AGC 返回的 Profile 已过期");
    }

    let cached = CachedProfile {
        package_name: package.to_string(),
        kind,
        profile_id: id,
        file: path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        cert_id: identity.cert_id.clone(),
        acl: acl.to_vec(),
        device_ids: device_ids.to_vec(),
        udids: udids.to_vec(),
        expires_at: content.not_after,
        created_at: now_secs(),
    };
    let stale: Vec<String> = store
        .load_profiles(team_id)
        .into_iter()
        .filter(|existing| {
            existing.package_name == package
                && existing.kind == kind
                && existing.profile_id != cached.profile_id
        })
        .map(|existing| existing.profile_id)
        .filter(|id| !id.is_empty())
        .collect();
    store.upsert_profile(team_id, cached.clone())?;

    // 重新申请成功后清理同一包名、同类型的旧 Profile，避免占配额。
    if !stale.is_empty() {
        log(&format!("清理旧 Profile {} 个", stale.len()));
        if let Err(err) = api.delete_profiles(auth, &stale) {
            log(&format!("清理旧 Profile 失败（不影响本次签名）：{err:#}"));
        }
    }
    Ok(cached)
}

pub fn profile_name(kind: CertKind, package: &str) -> String {
    let mut name = format!("{}-{}", kind.file_stem(), package);
    if name.chars().count() > 64 {
        name = name.chars().take(64).collect();
    }
    name
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn int(value: Option<&Value>) -> i64 {
    value
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> ProfileContent {
        ProfileContent {
            bundle_name: "com.example.app".into(),
            profile_type: "debug".into(),
            not_after: now_secs() + 86_400,
            development_certificate: "x".into(),
            udids: vec!["UDID1".into()],
            allowed_acls: vec!["ohos.permission.READ_CONTACTS".into()],
            signature_verified: true,
            ..Default::default()
        }
    }

    fn identity() -> LocalIdentity {
        LocalIdentity {
            kind: CertKind::Debug,
            keystore: String::new(),
            store_password: String::new(),
            key_alias: String::new(),
            key_password: String::new(),
            csr: String::new(),
            cert_id: String::new(),
            cert_name: String::new(),
            cert_path: String::new(),
            spki_sha256: String::new(),
            cert_expires_at: 0,
            created_at: 0,
        }
    }

    #[test]
    fn rejects_wrong_bundle_and_missing_device() {
        let mut value = content();
        assert!(
            validate(
                &value,
                "com.example.app",
                CertKind::Debug,
                &identity(),
                &["UDID1".into()],
                &[]
            )
            .is_ok()
        );
        assert!(
            validate(
                &value,
                "com.other.app",
                CertKind::Debug,
                &identity(),
                &["UDID1".into()],
                &[]
            )
            .is_err()
        );
        value.udids = vec!["UDID2".into()];
        assert!(
            validate(
                &value,
                "com.example.app",
                CertKind::Debug,
                &identity(),
                &["UDID1".into()],
                &[]
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_missing_acl_and_expired_profile() {
        let mut value = content();
        assert!(
            validate(
                &value,
                "com.example.app",
                CertKind::Debug,
                &identity(),
                &["UDID1".into()],
                &["ohos.permission.READ_CONTACTS".into()]
            )
            .is_ok()
        );
        value.allowed_acls.clear();
        assert!(
            validate(
                &value,
                "com.example.app",
                CertKind::Debug,
                &identity(),
                &["UDID1".into()],
                &["ohos.permission.READ_CONTACTS".into()]
            )
            .is_err()
        );
        let mut value = content();
        value.not_after = now_secs() - 10;
        assert!(
            validate(
                &value,
                "com.example.app",
                CertKind::Debug,
                &identity(),
                &["UDID1".into()],
                &[]
            )
            .is_err()
        );
    }

    #[test]
    fn parses_raw_json_fallback() {
        let raw =
            br#"{"type":"debug","bundle-info":{"bundle-name":"a"},"validity":{"not-after":1}}"#;
        let content = extract_embedded_json(raw).unwrap();
        let parsed = parse_profile_json(&content).unwrap();
        assert_eq!(parsed.bundle_name, "a");
        assert_eq!(parsed.profile_type, "debug");
    }
}
