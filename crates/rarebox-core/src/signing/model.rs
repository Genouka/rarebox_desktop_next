//! HarmonyOS 签名域的数据模型：账号、云端资源、本地签名材料与 HAP 元数据。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// AGC 允许通过 Profile 授予的 ACL 权限白名单。
///
/// 与 Zykit / auto-installer 使用的列表一致。AGC 调整可申请 ACL 集合时，
/// 这里需要同步；未知权限不会自动写入 Profile，而是留给界面提示。
pub const ACL_PERMISSIONS: &[&str] = &[
    "ohos.permission.SYSTEM_FLOAT_WINDOW",
    "ohos.permission.READ_CONTACTS",
    "ohos.permission.WRITE_CONTACTS",
    "ohos.permission.READ_AUDIO",
    "ohos.permission.WRITE_AUDIO",
    "ohos.permission.READ_IMAGEVIDEO",
    "ohos.permission.WRITE_IMAGEVIDEO",
    "ohos.permission.READ_WRITE_DESKTOP_DIRECTORY",
    "ohos.permission.ACCESS_DDK_USB",
    "ohos.permission.ACCESS_DDK_HID",
    "ohos.permission.READ_PASTEBOARD",
    "ohos.permission.FILE_ACCESS_PERSIST",
    "ohos.permission.INTERCEPT_INPUT_EVENT",
    "ohos.permission.INPUT_MONITORING",
    "ohos.permission.SHORT_TERM_WRITE_IMAGEVIDEO",
    "ohos.permission.READ_WRITE_USER_FILE",
    "ohos.permission.READ_WRITE_USB_DEV",
    "ohos.permission.GET_WIFI_PEERS_MAC",
    "ohos.permission.SET_TELEPHONY_ESIM_STATE_OPEN",
    "ohos.permission.kernel.DISABLE_CODE_MEMORY_PROTECTION",
    "ohos.permission.kernel.ALLOW_WRITABLE_CODE_MEMORY",
    "ohos.permission.kernel.ALLOW_EXECUTABLE_FORT_MEMORY",
    "ohos.permission.MANAGE_PASTEBOARD_APP_SHARE_OPTION",
    "ohos.permission.MANAGE_UDMF_APP_SHARE_OPTION",
    "ohos.permission.ACCESS_DISK_PHY_INFO",
    "ohos.permission.PRELOAD_FILE",
    "ohos.permission.SET_PAC_URL",
    "ohos.permission.PERSONAL_MANAGE_RESTRICTIONS",
    "ohos.permission.START_PROVISIONING_MESSAGE",
    "ohos.permission.USE_FRAUD_CALL_LOG_PICKER",
    "ohos.permission.USE_FRAUD_MESSAGES_PICKER",
    "ohos.permission.PERSISTENT_BLUETOOTH_PEERS_MAC",
    "ohos.permission.ACCESS_VIRTUAL_SCREEN",
    "ohos.permission.MANAGE_APN_SETTING",
    "ohos.permission.GET_WIFI_LOCAL_MAC",
    "ohos.permission.kernel.ALLOW_USE_JITFORT_INTERFACE",
    "ohos.permission.GET_ETHERNET_LOCAL_MAC",
    "ohos.permission.kernel.DISABLE_GOTPLT_RO_PROTECTION",
    "ohos.permission.USE_FRAUD_APP_PICKER",
    "ohos.permission.ACCESS_DDK_DRIVERS",
    "ohos.permission.ACCESS_DDK_SCSI_PERIPHERAL",
    "ohos.permission.kernel.SUPPORT_PLUGIN",
    "ohos.permission.CUSTOM_SANDBOX",
    "ohos.permission.MANAGE_SCREEN_TIME_GUARD",
    "ohos.permission.CUSTOMIZE_SAVE_BUTTON",
    "ohos.permission.GET_ABILITY_INFO",
    "ohos.permission.ACCESS_FIDO2_ONLINEAUTH",
    "ohos.permission.USE_FLOAT_BALL",
    "ohos.permission.DLP_GET_HIDE_STATUS",
    "ohos.permission.READ_LOCAL_DEVICE_NAME",
    "ohos.permission.KEEP_BACKGROUND_RUNNING_SYSTEM",
    "ohos.permission.LINKTURBO",
    "ohos.permission.ACCESS_NET_TRACE_INFO",
    "ohos.permission.READ_WHOLE_CALENDAR",
    "ohos.permission.WRITE_WHOLE_CALENDAR",
    "ohos.permission.SET_SYSTEMSHARE_APPLAUNCHTRUSTLIST",
    "ohos.permission.HOOK_KEY_EVENT",
    "ohos.permission.WEB_NATIVE_MESSAGING",
    "ohos.permission.SUBSCRIBE_NOTIFICATION",
    "ohos.permission.CUSTOM_SCREEN_RECORDING",
    "ohos.permission.GET_IP_MAC_INFO",
    "ohos.permission.ACCESS_USER_FULL_DISK",
    "ohos.permission.kernel.LOAD_INDEPENDENT_LIBRARY",
    "ohos.permission.CRYPTO_EXTENSION_REGISTER",
];

pub fn is_acl_permission(permission: &str) -> bool {
    ACL_PERMISSIONS.contains(&permission)
}

/// 华为开发者账号的登录信息。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AuthInfo {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub user_id: String,
    /// AGC 团队 ID；个人账号默认等于 `user_id`。
    #[serde(default)]
    pub team_id: String,
    #[serde(default)]
    pub nick_name: String,
    #[serde(default)]
    pub jwt_token: String,
    /// 登录时间（Unix 秒）。
    #[serde(default)]
    pub login_at: i64,
    #[serde(default)]
    pub expires_in: i64,
}

impl AuthInfo {
    pub fn is_logged_in(&self) -> bool {
        !self.access_token.is_empty() && !self.jwt_token.is_empty()
    }

    pub fn team_id_or_user(&self) -> &str {
        if self.team_id.is_empty() {
            &self.user_id
        } else {
            &self.team_id
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Team {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// 证书类型。AGC 的 `certType`：1=调试，2=发布。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CertKind {
    #[default]
    Debug,
    Release,
}

impl CertKind {
    pub const ALL: [CertKind; 2] = [CertKind::Debug, CertKind::Release];

    pub fn cert_type(self) -> i32 {
        match self {
            CertKind::Debug => 1,
            CertKind::Release => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CertKind::Debug => "调试证书",
            CertKind::Release => "发布证书",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            CertKind::Debug => "调试",
            CertKind::Release => "发布",
        }
    }

    pub fn file_stem(self) -> &'static str {
        match self {
            CertKind::Debug => "rarebox-debug",
            CertKind::Release => "rarebox-release",
        }
    }

    pub fn profile_type(self) -> i32 {
        match self {
            CertKind::Debug => 1,
            CertKind::Release => 2,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CloudCert {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub cert_name: String,
    #[serde(default)]
    pub cert_object_id: String,
    #[serde(default)]
    pub cert_type: i32,
    #[serde(default)]
    pub expire_time: i64,
    #[serde(default)]
    pub cert_download_url: String,
}

impl CloudCert {
    pub fn is_expired(&self) -> bool {
        self.expire_time > 0 && now_secs() >= self.expire_time
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CloudDevice {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub udid: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CloudProfile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub provision_name: String,
    #[serde(default)]
    pub provision_type: i32,
    #[serde(default)]
    pub cert_name: String,
    #[serde(default)]
    pub cert_id: String,
    #[serde(default)]
    pub package_name: String,
    #[serde(default)]
    pub provision_download_url: String,
    #[serde(default)]
    pub expire_time: i64,
    #[serde(default)]
    pub acl_permission_list: Vec<String>,
    #[serde(default)]
    pub acl_permission_audit_state: i32,
    #[serde(default)]
    pub device_list: Vec<CloudDevice>,
}

impl CloudProfile {
    pub fn is_expired(&self) -> bool {
        self.expire_time > 0 && now_secs() >= self.expire_time
    }
}

/// 本地保存的签名身份：一套密钥库、CSR 和 AGC 证书。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalIdentity {
    pub kind: CertKind,
    /// 相对团队目录的文件名。
    pub keystore: String,
    pub store_password: String,
    pub key_alias: String,
    pub key_password: String,
    pub csr: String,
    pub cert_id: String,
    pub cert_name: String,
    pub cert_path: String,
    /// 本地密钥的 SubjectPublicKeyInfo SHA-256，用于发现证书/密钥不匹配。
    #[serde(default)]
    pub spki_sha256: String,
    #[serde(default)]
    pub cert_expires_at: i64,
    #[serde(default)]
    pub created_at: i64,
}

/// Profile 的离线校验结果。
#[derive(Clone, Debug, Default)]
pub struct ProfileContent {
    pub bundle_name: String,
    pub profile_type: String,
    pub not_after: i64,
    pub development_certificate: String,
    pub distribution_certificate: String,
    pub udids: Vec<String>,
    pub allowed_acls: Vec<String>,
    pub signature_verified: bool,
}

impl ProfileContent {
    pub fn certificate(&self) -> &str {
        if !self.development_certificate.is_empty() {
            &self.development_certificate
        } else {
            &self.distribution_certificate
        }
    }

    pub fn is_expired(&self) -> bool {
        self.not_after > 0 && now_secs() >= self.not_after
    }
}

/// 本地 Profile 缓存。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CachedProfile {
    pub package_name: String,
    pub kind: CertKind,
    pub profile_id: String,
    /// 相对团队目录的文件名。
    pub file: String,
    pub cert_id: String,
    #[serde(default)]
    pub acl: Vec<String>,
    #[serde(default)]
    pub device_ids: Vec<String>,
    #[serde(default)]
    pub udids: Vec<String>,
    #[serde(default)]
    pub expires_at: i64,
    #[serde(default)]
    pub created_at: i64,
}

/// 手动导入的签名材料（DevEco / AGC 网站生成）。
///
/// 自动申请只覆盖调试证书与调试 Profile；发布证书/Profile 需要先在 AGC
/// 申请并下载，再通过这里导入，避免调用未公开且无法验证的发布接口。
#[derive(Clone, Debug)]
pub struct ImportedMaterial {
    pub keystore: PathBuf,
    pub store_password: String,
    pub key_alias: String,
    pub key_password: String,
    pub cert: PathBuf,
    pub profile: PathBuf,
}

/// HAP/HSP/APP 的包元数据。
#[derive(Clone, Debug, Default)]
pub struct HapMeta {
    pub path: PathBuf,
    pub bundle_name: String,
    pub version_name: String,
    pub version_code: u64,
    pub module_name: String,
    pub module_type: String,
    pub permissions: Vec<String>,
    pub acl: Vec<String>,
    pub min_api: Option<u32>,
    pub device_types: Vec<String>,
    /// APP 包内嵌的 HAP 数量；普通 HAP 为 0。
    pub inner_haps: usize,
}

/// 签名块版本。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SigningBlock {
    /// V2：`HAP Sig Block 42`，兼容较旧设备。
    V2,
    /// V3：DevEco 6.x / Hvigor 默认。
    #[default]
    V3,
}

impl SigningBlock {
    pub fn compatible_version(self) -> u32 {
        match self {
            SigningBlock::V2 => 7,
            SigningBlock::V3 => 9,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SigningBlock::V2 => "V2（兼容旧设备）",
            SigningBlock::V3 => "V3（默认）",
        }
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

pub fn format_timestamp(secs: i64) -> String {
    if secs <= 0 {
        return "—".to_string();
    }
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "—".to_string())
}
