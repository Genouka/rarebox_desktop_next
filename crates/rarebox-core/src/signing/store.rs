//! 签名材料的本地存储。
//!
//! 目录布局（均位于应用数据目录下）：
//!
//! ```text
//! signing/
//!   auth.json
//!   teams/<team-id>/
//!     identity-debug.json
//!     identity-release.json
//!     rarebox-debug.p12 / .csr / .cer
//!     rarebox-release.p12 / .csr / .cer
//!     profiles/<bundle>_<profile-id>.p7b
//!     signed/<file>-signed.hap
//! ```
//!
//! `auth.json` 和 identity 文件包含凭据，Unix 上写入后收紧为 0600；Windows
//! 暂不额外设置 ACL，依赖用户目录的默认权限。

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use super::model::{AuthInfo, CachedProfile, CertKind, LocalIdentity};

#[derive(Clone, Debug)]
pub struct SigningStore {
    root: PathBuf,
}

impl SigningStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_root() -> PathBuf {
        crate::config::data_dir().join("signing")
    }

    pub fn auth_path(&self) -> PathBuf {
        self.root.join("auth.json")
    }

    pub fn team_dir(&self, team_id: &str) -> PathBuf {
        self.root.join("teams").join(sanitize(team_id))
    }

    pub fn identity_path(&self, team_id: &str, kind: CertKind) -> PathBuf {
        self.team_dir(team_id)
            .join(format!("identity-{}.json", kind_file(kind)))
    }

    pub fn keystore_path(&self, team_id: &str, kind: CertKind) -> PathBuf {
        self.team_dir(team_id)
            .join(format!("{}.p12", kind.file_stem()))
    }

    pub fn csr_path(&self, team_id: &str, kind: CertKind) -> PathBuf {
        self.team_dir(team_id)
            .join(format!("{}.csr", kind.file_stem()))
    }

    pub fn profile_path(&self, team_id: &str, package: &str, profile_id: &str) -> PathBuf {
        self.team_dir(team_id).join("profiles").join(format!(
            "{}_{}.p7b",
            sanitize(package),
            sanitize(profile_id)
        ))
    }

    pub fn imported_dir(&self) -> PathBuf {
        self.root.join("imported")
    }

    pub fn load_auth(&self) -> Option<AuthInfo> {
        read_json(&self.auth_path()).filter(|auth: &AuthInfo| auth.is_logged_in())
    }

    pub fn save_auth(&self, auth: &AuthInfo) -> Result<()> {
        write_json(&self.auth_path(), auth)
    }

    pub fn clear_auth(&self) -> Result<()> {
        let path = self.auth_path();
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("无法删除登录信息 {}", path.display()))?;
        }
        Ok(())
    }

    pub fn load_identity(&self, team_id: &str, kind: CertKind) -> Option<LocalIdentity> {
        read_json(&self.identity_path(team_id, kind))
    }

    pub fn save_identity(&self, team_id: &str, identity: &LocalIdentity) -> Result<()> {
        write_json(&self.identity_path(team_id, identity.kind), identity)
    }

    pub fn load_profiles(&self, team_id: &str) -> Vec<CachedProfile> {
        read_json::<Vec<CachedProfile>>(&self.profiles_path(team_id)).unwrap_or_default()
    }

    pub fn save_profiles(&self, team_id: &str, profiles: &[CachedProfile]) -> Result<()> {
        write_json(&self.profiles_path(team_id), &profiles)
    }

    pub fn upsert_profile(&self, team_id: &str, profile: CachedProfile) -> Result<()> {
        let mut profiles = self.load_profiles(team_id);
        profiles.retain(|existing| {
            !(existing.package_name == profile.package_name && existing.kind == profile.kind)
        });
        profiles.push(profile);
        self.save_profiles(team_id, &profiles)
    }

    pub fn remove_profiles_of_kind(&self, team_id: &str, kind: CertKind) -> Result<()> {
        let mut profiles = self.load_profiles(team_id);
        profiles.retain(|profile| profile.kind != kind);
        self.save_profiles(team_id, &profiles)
    }

    pub fn find_profile(
        &self,
        team_id: &str,
        package: &str,
        kind: CertKind,
    ) -> Option<CachedProfile> {
        self.load_profiles(team_id)
            .into_iter()
            .find(|profile| profile.package_name == package && profile.kind == kind)
    }

    fn profiles_path(&self, team_id: &str) -> PathBuf {
        self.team_dir(team_id).join("profiles.json")
    }
}

fn kind_file(kind: CertKind) -> &'static str {
    match kind {
        CertKind::Debug => "debug",
        CertKind::Release => "release",
    }
}

/// 目录名只保留可移植字符，避免团队 ID 中的特殊字符影响路径。
fn sanitize(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('.').to_string();
    if cleaned.is_empty() {
        "default".to_string()
    } else {
        cleaned
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub(crate) fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("无法创建目录 {}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(value).context("序列化签名数据失败")?;
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, &json).with_context(|| format!("无法写入 {}", temp.display()))?;
    restrict_permissions(&temp);
    // Windows 的 `rename` 不会覆盖已存在的目标文件；先删除再重命名。
    #[cfg(windows)]
    if path.exists() {
        std::fs::remove_file(path).with_context(|| format!("无法替换 {}", path.display()))?;
    }
    std::fs::rename(&temp, path).with_context(|| format!("无法保存 {}", path.display()))?;
    restrict_permissions(path);
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_team_ids_and_package_names() {
        assert_eq!(sanitize("team/../a b"), "team_.._a_b");
        assert_eq!(sanitize(".."), "default");
        assert_eq!(sanitize("2850086000506643987"), "2850086000506643987");
    }

    #[test]
    fn profile_cache_replaces_same_package_and_kind() {
        let dir =
            std::env::temp_dir().join(format!("rarebox-signing-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SigningStore::new(dir.clone());
        store
            .upsert_profile(
                "team",
                CachedProfile {
                    package_name: "com.example".into(),
                    kind: CertKind::Debug,
                    profile_id: "1".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .upsert_profile(
                "team",
                CachedProfile {
                    package_name: "com.example".into(),
                    kind: CertKind::Debug,
                    profile_id: "2".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let profiles = store.load_profiles("team");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].profile_id, "2");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
