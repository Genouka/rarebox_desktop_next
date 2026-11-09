//! 本地密钥库、CSR 与 AGC 证书。
//!
//! 每个团队、每种证书类型（调试/发布）使用一套独立身份，避免调试证书和发布
//! 证书共用密钥。生成与签名全部由 vendored `hapsigner` 完成，不依赖 Java。

use std::path::Path;

use anyhow::{Context as _, Result, anyhow, bail};
use der::{Decode, DecodePem, Encode};
use hapsigner::{
    CertificateSignatureAlgorithm, CsrParameters, IssuingKey, KeySize, KeystoreEntry,
    KeystoreFormat, certificate_request_pem, generate_csr, generate_key_pair,
    parse_distinguished_name, placeholder_certificate, public_key_from_der, write_keystore,
};
use rand::Rng as _;
use sha2::{Digest as _, Sha256};
use x509_cert::Certificate;

use super::api::EcoClient;
use super::model::{AuthInfo, CertKind, CloudCert, LocalIdentity, now_secs};
use super::store::SigningStore;

/// 云端存在同名证书，但公钥与本地密钥不匹配；需要用户确认后重置。
#[derive(Debug)]
pub struct CertNeedsReset {
    pub message: String,
}

impl std::fmt::Display for CertNeedsReset {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CertNeedsReset {}

pub fn ensure_identity(
    api: &EcoClient,
    auth: &AuthInfo,
    store: &SigningStore,
    team_id: &str,
    kind: CertKind,
    reset: bool,
    log: &mut dyn FnMut(&str),
) -> Result<LocalIdentity> {
    if kind != CertKind::Debug {
        bail!("发布证书请在 AGC 网站申请后，通过「导入签名材料」使用");
    }
    let mut identity = load_or_create(store, team_id, kind, log)?;
    let keystore = store.keystore_path(team_id, kind);
    if !keystore.exists() {
        log("密钥库缺失，重新生成签名身份");
        identity = create_identity(store, team_id, kind, log)?;
    }

    let cert_path = store.team_dir(team_id).join(&identity.cert_path);
    let cloud_certs = api.certs(auth)?;
    let mut named: Vec<CloudCert> = cloud_certs
        .into_iter()
        .filter(|cert| cert.cert_name == identity.cert_name && cert.cert_type == kind.cert_type())
        .collect();
    named.sort_by_key(|cert| cert.expire_time);

    if identity.cert_id.is_empty() || !cert_path.exists() {
        if let Some(cert) = named.iter().find(|cert| !cert.is_expired()) {
            log(&format!("复用云端证书 {}", cert.cert_name));
            download_cert(api, auth, cert, &cert_path)?;
            if let Some(spki) = cert_spki_sha256(&std::fs::read(&cert_path)?)? {
                if spki != identity.spki_sha256 {
                    return Err(CertNeedsReset {
                        message: format!(
                            "云端证书 {} 与本地密钥不匹配。请点击「重置证书」删除并重新申请。",
                            cert.cert_name
                        ),
                    }
                    .into());
                }
            }
            apply_cert_metadata(&mut identity, cert, &cert_path)?;
            store.save_identity(team_id, &identity)?;
            return Ok(identity);
        }
    } else if let Ok(bytes) = std::fs::read(&cert_path) {
        if let (Some(spki), Ok(expires)) = (cert_spki_sha256(&bytes)?, cert_not_after(&bytes)) {
            if spki == identity.spki_sha256 && expires > now_secs() {
                if let Some(cert) = named.iter().find(|cert| cert.id == identity.cert_id) {
                    if !cert.is_expired() {
                        identity.cert_expires_at = expires;
                        store.save_identity(team_id, &identity)?;
                        return Ok(identity);
                    }
                } else if named.is_empty() {
                    // 证书列表接口偶发返回空列表时，不重复申请证书；本地证书仍可用于
                    // 创建 Profile，若云端实际已删除，后续步骤会给出明确的证书错误。
                    log("AGC 未返回证书列表，继续使用本地有效证书");
                    identity.cert_expires_at = expires;
                    store.save_identity(team_id, &identity)?;
                    return Ok(identity);
                }
            }
        }
    }

    let expired_ids: Vec<String> = named
        .iter()
        .filter(|cert| cert.is_expired())
        .map(|cert| cert.id.clone())
        .collect();
    if !expired_ids.is_empty() {
        log(&format!("清理过期证书 {} 个", expired_ids.len()));
        if let Err(err) = api.delete_certs(auth, &expired_ids) {
            log(&format!("清理过期证书失败：{err:#}"));
        }
        let cached_profiles: Vec<String> = store
            .load_profiles(team_id)
            .into_iter()
            .filter(|profile| profile.kind == kind)
            .map(|profile| profile.profile_id)
            .filter(|id| !id.is_empty())
            .collect();
        if !cached_profiles.is_empty() {
            let _ = api.delete_profiles(auth, &cached_profiles);
            let _ = store.remove_profiles_of_kind(team_id, kind);
        }
        if expired_ids.iter().any(|id| id == &identity.cert_id) {
            identity.cert_id.clear();
            identity.cert_expires_at = 0;
            store.save_identity(team_id, &identity)?;
        }
        named.retain(|cert| !cert.is_expired());
    }

    if !named.is_empty() {
        if !reset {
            return Err(CertNeedsReset {
                message: format!(
                    "AGC 已存在 {} 证书「{}」，但本地密钥无法匹配。请点击「重置证书」删除旧证书后重新申请。",
                    kind.short_label(),
                    identity.cert_name
                ),
            }
            .into());
        }
        // 先清理本地缓存的 Profile（它们内嵌旧证书），再删除证书，避免
        // AGC 上残留无法复用的 Profile 占用配额。
        let cached_profiles: Vec<String> = store
            .load_profiles(team_id)
            .into_iter()
            .filter(|profile| profile.kind == kind)
            .map(|profile| profile.profile_id)
            .filter(|id| !id.is_empty())
            .collect();
        if !cached_profiles.is_empty() {
            if let Err(err) = api.delete_profiles(auth, &cached_profiles) {
                log(&format!("清理旧 Profile 失败（继续重置证书）：{err:#}"));
            }
        }
        log(&format!("删除同名旧证书 {} 个", named.len()));
        let ids: Vec<String> = named.iter().map(|cert| cert.id.clone()).collect();
        api.delete_certs(auth, &ids)?;
        identity.cert_id.clear();
        identity.cert_expires_at = 0;
        store.save_identity(team_id, &identity)?;
        store.remove_profiles_of_kind(team_id, kind)?;
    }

    let csr = std::fs::read_to_string(store.team_dir(team_id).join(&identity.csr))
        .context("读取 CSR 失败")?;
    log(&format!("申请{}…", kind.label()));
    let cert = api.create_cert(auth, kind, &identity.cert_name, &csr)?;
    download_cert(api, auth, &cert, &cert_path)?;
    let spki = cert_spki_sha256(&std::fs::read(&cert_path)?)?
        .ok_or_else(|| anyhow!("下载的证书无法解析"))?;
    if spki != identity.spki_sha256 {
        bail!("AGC 返回的证书与本地密钥不匹配，请重试或重置证书");
    }
    apply_cert_metadata(&mut identity, &cert, &cert_path)?;
    identity.cert_expires_at = cert_not_after(&std::fs::read(&cert_path)?).unwrap_or(0);
    store.save_identity(team_id, &identity)?;
    log(&format!("证书已保存：{}", cert_path.display()));
    Ok(identity)
}

fn load_or_create(
    store: &SigningStore,
    team_id: &str,
    kind: CertKind,
    log: &mut dyn FnMut(&str),
) -> Result<LocalIdentity> {
    if let Some(identity) = store.load_identity(team_id, kind) {
        let keystore = store.team_dir(team_id).join(&identity.keystore);
        let csr = store.team_dir(team_id).join(&identity.csr);
        if keystore.exists() && csr.exists() {
            return Ok(identity);
        }
    }
    create_identity(store, team_id, kind, log)
}

fn create_identity(
    store: &SigningStore,
    team_id: &str,
    kind: CertKind,
    log: &mut dyn FnMut(&str),
) -> Result<LocalIdentity> {
    log("生成本地 P-256 密钥库与 CSR…");
    let pair = generate_key_pair(KeySize::EccP256)?;
    let subject = parse_distinguished_name(&format!(
        "C=CN,O=HUAWEI,OU=HUAWEI IDE,CN={}",
        kind.file_stem()
    ))?;
    let key = IssuingKey::from_pkcs8_der(pair.private_key_der())?;
    let csr_der = generate_csr(&CsrParameters {
        subject,
        public_key: public_key_from_der(pair.public_key_der())?,
        signing_key: &key,
        signature_algorithm: CertificateSignatureAlgorithm::EcdsaSha256,
    })?;
    let csr_pem = certificate_request_pem(&csr_der)?;
    let password = random_password();
    let placeholder = placeholder_certificate(&pair, kind.file_stem())?;
    let keystore = write_keystore(
        KeystoreFormat::Pkcs12,
        &password,
        &KeystoreEntry {
            alias: "rarebox",
            key_password: &password,
            private_key_der: pair.private_key_der(),
            certificate_chain: &[placeholder],
        },
    )?;

    let dir = store.team_dir(team_id);
    std::fs::create_dir_all(&dir).with_context(|| format!("无法创建 {}", dir.display()))?;
    let keystore_path = store.keystore_path(team_id, kind);
    let csr_path = store.csr_path(team_id, kind);
    let keystore_name = keystore_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("{}.p12", kind.file_stem()));
    let csr_name = csr_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("{}.csr", kind.file_stem()));
    std::fs::write(&keystore_path, &keystore)
        .with_context(|| format!("无法写入密钥库 {}", keystore_path.display()))?;
    std::fs::write(&csr_path, csr_pem.as_bytes())
        .with_context(|| format!("无法写入 CSR {}", csr_path.display()))?;

    let identity = LocalIdentity {
        kind,
        keystore: keystore_name,
        store_password: password.clone(),
        key_alias: "rarebox".to_string(),
        key_password: password,
        csr: csr_name,
        cert_id: String::new(),
        cert_name: kind.file_stem().to_string(),
        cert_path: format!("{}.cer", kind.file_stem()),
        spki_sha256: hex(&Sha256::digest(pair.public_key_der())),
        cert_expires_at: 0,
        created_at: now_secs(),
    };
    store.save_identity(team_id, &identity)?;
    Ok(identity)
}

fn download_cert(api: &EcoClient, auth: &AuthInfo, cert: &CloudCert, path: &Path) -> Result<()> {
    let temp = path.with_extension("cer.tmp");
    let url = api.cert_download_url(auth, cert)?;
    let mut downloaded = api.download(&url, &temp);
    if downloaded.is_err() && !cert.cert_object_id.is_empty() {
        // `certDownloadUrl` 偶尔是过期/需要重新签发的地址；回退到对象 ID。
        if let Ok(fallback) = api.object_download_url(auth, &cert.cert_object_id) {
            downloaded = api.download(&fallback, &temp);
        }
    }
    if let Err(error) = downloaded {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    let mut bytes = std::fs::read(&temp)?;
    if cert_spki_sha256(&bytes)?.is_none() && !cert.cert_object_id.is_empty() {
        if let Ok(fallback) = api.object_download_url(auth, &cert.cert_object_id) {
            if api.download(&fallback, &temp).is_ok() {
                bytes = std::fs::read(&temp)?;
            }
        }
    }
    if cert_spki_sha256(&bytes)?.is_none() {
        let _ = std::fs::remove_file(&temp);
        bail!("下载的证书不是有效的 X.509 证书");
    }
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&temp, path).with_context(|| format!("无法保存证书 {}", path.display()))?;
    Ok(())
}

fn apply_cert_metadata(identity: &mut LocalIdentity, cert: &CloudCert, path: &Path) -> Result<()> {
    identity.cert_id = cert.id.clone();
    identity.cert_name = cert.cert_name.clone();
    identity.cert_path = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| identity.cert_path.clone());
    identity.cert_expires_at = if cert.expire_time > 0 {
        cert.expire_time
    } else {
        cert_not_after(&std::fs::read(path)?).unwrap_or(0)
    };
    Ok(())
}

/// 解析证书链中的第一张证书（AGC 下载的 `.cer` 可能是 PEM 或 DER）。
pub fn parse_first_certificate(bytes: &[u8]) -> Result<Certificate> {
    if let Some(pem) = first_pem_block(bytes) {
        return Certificate::from_pem(pem).context("解析 PEM 证书失败");
    }
    Certificate::from_der(bytes).context("解析 DER 证书失败")
}

fn first_pem_block(bytes: &[u8]) -> Option<&[u8]> {
    const BEGIN: &[u8] = b"-----BEGIN CERTIFICATE-----";
    const END: &[u8] = b"-----END CERTIFICATE-----";
    let start = bytes
        .windows(BEGIN.len())
        .position(|window| window == BEGIN)?;
    let after = &bytes[start..];
    let end = after.windows(END.len()).position(|window| window == END)? + END.len();
    Some(&after[..end])
}

pub fn cert_spki_sha256(bytes: &[u8]) -> Result<Option<String>> {
    let Ok(cert) = parse_first_certificate(bytes) else {
        return Ok(None);
    };
    let spki = cert
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .context("编码证书公钥失败")?;
    Ok(Some(hex(&Sha256::digest(&spki))))
}

pub fn cert_not_after(bytes: &[u8]) -> Result<i64> {
    let cert = parse_first_certificate(bytes)?;
    Ok(cert
        .tbs_certificate
        .validity
        .not_after
        .to_unix_duration()
        .as_secs() as i64)
}

fn random_password() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..28)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_identity_is_consistent() {
        let dir = std::env::temp_dir().join(format!("rarebox-identity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SigningStore::new(dir.clone());
        let identity = create_identity(&store, "team", CertKind::Debug, &mut |_| {}).unwrap();
        assert!(store.keystore_path("team", CertKind::Debug).exists());
        assert!(store.csr_path("team", CertKind::Debug).exists());
        let csr = std::fs::read_to_string(store.csr_path("team", CertKind::Debug)).unwrap();
        assert!(csr.starts_with("-----BEGIN NEW CERTIFICATE REQUEST-----"));
        assert_eq!(identity.spki_sha256.len(), 64);
        let loaded = store.load_identity("team", CertKind::Debug).unwrap();
        assert_eq!(loaded.spki_sha256, identity.spki_sha256);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_non_certificate_bytes() {
        assert!(cert_spki_sha256(b"not a cert").unwrap().is_none());
    }
}
