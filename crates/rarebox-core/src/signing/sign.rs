//! HAP/HSP 签名封装：只负责把本地材料交给 vendored `hapsigner` 并验签。

use std::path::Path;

use anyhow::{Context as _, Result, anyhow, bail};
use hapsigner::{
    ApplicationVerifier, HapSigner, InputFormat, KeystoreMaterial, SignOptions, SigningAlgorithm,
    SigningMaterial,
};

use super::model::{LocalIdentity, SigningBlock};
use super::store::SigningStore;

pub struct SigningFiles<'a> {
    pub keystore: &'a Path,
    pub store_password: &'a str,
    pub key_alias: &'a str,
    pub key_password: &'a str,
    pub cert: &'a Path,
    pub profile: &'a Path,
}

pub fn sign_hap(
    store: &SigningStore,
    team_id: &str,
    identity: &LocalIdentity,
    profile_path: &Path,
    input: &Path,
    output: &Path,
    block: SigningBlock,
) -> Result<()> {
    let team_dir = store.team_dir(team_id);
    let keystore = team_dir.join(&identity.keystore);
    let cert = team_dir.join(&identity.cert_path);
    sign_hap_with(
        SigningFiles {
            keystore: &keystore,
            store_password: &identity.store_password,
            key_alias: &identity.key_alias,
            key_password: &identity.key_password,
            cert: &cert,
            profile: profile_path,
        },
        input,
        output,
        block,
    )
}

pub fn sign_hap_with(
    files: SigningFiles<'_>,
    input: &Path,
    output: &Path,
    block: SigningBlock,
) -> Result<()> {
    match input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("hap" | "hsp") => {}
        Some("app") => bail!("暂不支持对 .app 重新签名，请选择 .hap / .hsp"),
        _ => bail!("仅支持签名 .hap / .hsp 文件"),
    }
    let keystore = std::fs::read(files.keystore)
        .with_context(|| format!("无法读取密钥库 {}", files.keystore.display()))?;
    let cert = std::fs::read(files.cert)
        .with_context(|| format!("无法读取证书 {}", files.cert.display()))?;
    let profile = std::fs::read(files.profile)
        .with_context(|| format!("无法读取 Profile {}", files.profile.display()))?;

    let material = SigningMaterial::from_keystore(KeystoreMaterial {
        keystore: &keystore,
        store_password: files.store_password,
        key_alias: files.key_alias,
        key_password: files.key_password,
        app_certificate_chain: &cert,
        profile,
        profile_signed: true,
        algorithm: SigningAlgorithm::EcdsaSha256,
    })
    .map_err(|err| anyhow!("加载签名材料失败：{err}"))?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("无法创建输出目录 {}", parent.display()))?;
    }
    let signer = HapSigner::new(
        material,
        SignOptions {
            compatible_version: block.compatible_version(),
            code_signing: true,
        },
    );
    signer
        .sign_file(input, output)
        .map_err(|err| anyhow!("签名失败：{err}"))?;
    let signed =
        std::fs::read(output).with_context(|| format!("读取签名结果 {} 失败", output.display()))?;
    if signed.is_empty() {
        bail!("签名结果为空");
    }
    ApplicationVerifier::new(&signed)
        .verify(InputFormat::Zip)
        .map_err(|err| anyhow!("签名结果验签失败：{err}"))?;
    Ok(())
}

/// 仅加载材料，不写文件；用于导入模式在签名前尽早发现口令/别名/证书问题。
pub fn validate_material(files: &SigningFiles<'_>) -> Result<()> {
    let keystore = std::fs::read(files.keystore)
        .with_context(|| format!("无法读取密钥库 {}", files.keystore.display()))?;
    let cert = std::fs::read(files.cert)
        .with_context(|| format!("无法读取证书 {}", files.cert.display()))?;
    let profile = std::fs::read(files.profile)
        .with_context(|| format!("无法读取 Profile {}", files.profile.display()))?;
    SigningMaterial::from_keystore(KeystoreMaterial {
        keystore: &keystore,
        store_password: files.store_password,
        key_alias: files.key_alias,
        key_password: files.key_password,
        app_certificate_chain: &cert,
        profile,
        profile_signed: true,
        algorithm: SigningAlgorithm::EcdsaSha256,
    })
    .map_err(|err| anyhow!("签名材料无效：{err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use der::{Decode as _, EncodePem as _};
    use hapsigner::{
        KeySize, KeystoreEntry, KeystoreFormat, ProfileSigner, SigningKey, generate_key_pair,
        placeholder_certificate, write_keystore,
    };

    use super::*;
    use crate::signing::model::{CertKind, SigningBlock};
    use crate::signing::store::SigningStore;

    #[test]
    fn signs_and_verifies_a_generated_hap() {
        let dir = std::env::temp_dir().join(format!("rarebox-sign-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let app = generate_key_pair(KeySize::EccP256).unwrap();
        let app_cert = placeholder_certificate(&app, "rarebox").unwrap();
        let profile_pair = generate_key_pair(KeySize::EccP256).unwrap();
        let profile_cert = placeholder_certificate(&profile_pair, "profile").unwrap();

        let app_cert_pem = x509_cert::Certificate::from_der(&app_cert)
            .unwrap()
            .to_pem(der::pem::LineEnding::LF)
            .unwrap();
        let profile_json = serde_json::json!({
            "version-code": 1,
            "version-name": "1.0.0",
            "uuid": "test",
            "type": "debug",
            "app-distribution-type": "os_integration",
            "validity": {"not-before": 1700000000u64, "not-after": 2000000000u64},
            "bundle-info": {
                "developer-id": "rarebox",
                "development-certificate": app_cert_pem,
                "bundle-name": "com.rarebox.test",
                "apl": "normal",
                "app-feature": "hos_normal_app"
            },
            "acls": {"allowed-acls": ["ohos.permission.READ_CONTACTS"]},
            "debug-info": {"device-id-type": "udid", "device-ids": ["*"]}
        });
        let profile_bytes = serde_json::to_vec(&profile_json).unwrap();
        let profile_signer = ProfileSigner::new(
            SigningKey {
                private_key_der: profile_pair.private_key_der().to_vec(),
                cert_chain: vec![profile_cert],
            },
            hapsigner::SigningAlgorithm::EcdsaSha256,
        );
        let signed_profile = profile_signer.sign(&profile_bytes).unwrap();

        let keystore = write_keystore(
            KeystoreFormat::Pkcs12,
            "rarebox123",
            &KeystoreEntry {
                alias: "rarebox",
                key_password: "rarebox123",
                private_key_der: app.private_key_der(),
                certificate_chain: &[app_cert.clone()],
            },
        )
        .unwrap();

        let store = SigningStore::new(dir.clone());
        let team_dir = store.team_dir("team");
        std::fs::create_dir_all(&team_dir).unwrap();
        std::fs::write(team_dir.join("rarebox-debug.p12"), keystore).unwrap();
        std::fs::write(
            team_dir.join("rarebox-debug.cer"),
            &x509_cert::Certificate::from_der(&app_cert)
                .unwrap()
                .to_pem(der::pem::LineEnding::LF)
                .unwrap(),
        )
        .unwrap();
        std::fs::write(team_dir.join("profile.p7b"), signed_profile).unwrap();

        let input = dir.join("entry.hap");
        {
            let file = std::fs::File::create(&input).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("module.json", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(
                br#"{"app":{"bundleName":"com.rarebox.test"},"module":{"name":"entry"}}"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }

        let identity = LocalIdentity {
            kind: CertKind::Debug,
            keystore: "rarebox-debug.p12".into(),
            store_password: "rarebox123".into(),
            key_alias: "rarebox".into(),
            key_password: "rarebox123".into(),
            csr: "rarebox-debug.csr".into(),
            cert_id: "1".into(),
            cert_name: "rarebox-debug".into(),
            cert_path: "rarebox-debug.cer".into(),
            spki_sha256: String::new(),
            cert_expires_at: 0,
            created_at: 0,
        };
        let output = dir.join("entry-signed.hap");
        sign_hap(
            &store,
            "team",
            &identity,
            &team_dir.join("profile.p7b"),
            &input,
            &output,
            SigningBlock::V3,
        )
        .unwrap();
        let signed = std::fs::read(&output).unwrap();
        assert!(!signed.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
