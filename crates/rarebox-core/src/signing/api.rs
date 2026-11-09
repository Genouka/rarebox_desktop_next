//! Connect API / DevEco Eco 客户端。
//!
//! 这里集中处理未公开接口的地址、请求头、`ret.code` 响应包与错误翻译。
//! 其余模块只看到强类型结果，避免 AGC 字段变化扩散到 UI。

use std::collections::BTreeSet;
use std::fmt;
use std::fs::File;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use serde_json::{Value, json};

use super::model::{AuthInfo, CertKind, CloudCert, CloudDevice, CloudProfile, Team};

const ECO_BASE: &str = "https://connect-api.cloud.huawei.com/api";
const AUTH_BASE: &str = "https://cn.devecostudio.huawei.com";
const USER_AGENT: &str = "RareBoxDesktop/0.1";

#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub code: i64,
    pub message: String,
}

impl ApiError {
    pub fn unauthorized(&self) -> bool {
        self.status == 401 || self.code == 401
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(
                formatter,
                "AGC 请求失败（HTTP {} / code {}）",
                self.status, self.code
            )
        } else {
            write!(formatter, "{}", self.message)
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Clone)]
pub struct EcoClient {
    agent: ureq::Agent,
    base: String,
}

impl Default for EcoClient {
    fn default() -> Self {
        Self::new()
    }
}

impl EcoClient {
    pub fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout(Duration::from_secs(45))
            .user_agent(USER_AGENT)
            .build();
        Self {
            agent,
            base: ECO_BASE.to_string(),
        }
    }

    // ------------------------------------------------------------------
    // 登录
    // ------------------------------------------------------------------

    pub fn token_by_temp_token(&self, temp_token: &str) -> Result<AuthInfo> {
        let temp_token = temp_token
            .split('&')
            .next()
            .unwrap_or(temp_token)
            .trim()
            .trim_start_matches("tempToken=");
        let url = format!(
            "{AUTH_BASE}/authrouter/auth/api/temptoken/check?site=CN&tempToken={}&appid=1007&version=0.0.0",
            query_encode(temp_token)
        );
        let response = self
            .agent
            .get(&url)
            .set("User-Agent", "Mozilla/5.0")
            .call()
            .map_err(|err| anyhow!("获取登录令牌失败：{err}"))?;
        let text = response.into_string().context("读取登录令牌响应失败")?;
        let jwt = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .pointer("/ret/msg")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.trim().to_string());
        if jwt.is_empty() {
            return Err(anyhow!("登录令牌为空，请重新登录"));
        }
        self.user_info_by_jwt(&jwt, false)
    }

    pub fn refresh_token(&self, auth: &AuthInfo) -> Result<AuthInfo> {
        if auth.jwt_token.is_empty() {
            return Err(anyhow!("登录信息缺少 jwtToken，请重新登录"));
        }
        self.user_info_by_jwt(&auth.jwt_token, true)
    }

    fn user_info_by_jwt(&self, jwt: &str, refresh: bool) -> Result<AuthInfo> {
        let url = format!("{AUTH_BASE}/authrouter/auth/api/jwToken/check");
        let response = self
            .agent
            .get(&url)
            .set("jwtToken", jwt)
            .set("refresh", if refresh { "true" } else { "false" })
            .set("User-Agent", "Mozilla/5.0")
            .call()
            .map_err(|err| anyhow!("获取账号信息失败：{err}"))?;
        let value: Value = response.into_json().context("解析账号信息失败")?;
        let info = value
            .get("userInfo")
            .ok_or_else(|| anyhow!("账号信息缺少 userInfo，登录可能已失效"))?;
        let user_id = string_field(info, "userId");
        let access_token = string_field(info, "accessToken");
        if access_token.is_empty() {
            return Err(anyhow!("账号信息缺少 accessToken，请重新登录"));
        }
        Ok(AuthInfo {
            access_token,
            team_id: user_id.clone(),
            user_id,
            nick_name: string_field(info, "nickName"),
            jwt_token: jwt.to_string(),
            login_at: super::model::now_secs(),
            expires_in: 86_400,
        })
    }

    // ------------------------------------------------------------------
    // 账号 / 团队
    // ------------------------------------------------------------------

    pub fn teams(&self, auth: &AuthInfo) -> Result<Vec<Team>> {
        let value = self.send(
            "GET",
            &format!(
                "{}/ups/user-permission-service/v1/user-team-list",
                self.base
            ),
            Some(auth),
            None,
        )?;
        let teams = value
            .get("teams")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| Team {
                        id: string_field(item, "id"),
                        name: string_field(item, "name"),
                    })
                    .filter(|team| !team.id.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Ok(teams)
    }

    // ------------------------------------------------------------------
    // 证书
    // ------------------------------------------------------------------

    pub fn certs(&self, auth: &AuthInfo) -> Result<Vec<CloudCert>> {
        let value = self.send(
            "GET",
            &format!("{}/cps/harmony-cert-manage/v1/cert/list", self.base),
            Some(auth),
            None,
        )?;
        let mut certs = Vec::new();
        let mut seen = BTreeSet::new();
        for key in ["certList", "certInfo"] {
            if let Some(items) = value.get(key).and_then(Value::as_array) {
                for item in items {
                    let cert = parse_cert(item);
                    let key = if cert.id.is_empty() {
                        format!("{}:{}", cert.cert_name, cert.cert_object_id)
                    } else {
                        cert.id.clone()
                    };
                    if seen.insert(key) {
                        certs.push(cert);
                    }
                }
            }
        }
        Ok(certs)
    }

    pub fn create_cert(
        &self,
        auth: &AuthInfo,
        kind: CertKind,
        name: &str,
        csr: &str,
    ) -> Result<CloudCert> {
        let body = json!({
            "csr": csr,
            "certName": name,
            "certType": kind.cert_type(),
        });
        let value = self.send(
            "POST",
            &format!("{}/cps/harmony-cert-manage/v1/cert/add", self.base),
            Some(auth),
            Some(&body),
        )?;
        let cert = value
            .get("harmonyCert")
            .or_else(|| value.get("certInfo"))
            .map(parse_cert)
            .ok_or_else(|| {
                let message = value
                    .pointer("/ret/msg")
                    .and_then(Value::as_str)
                    .unwrap_or("证书创建失败");
                anyhow!("{message}")
            })?;
        Ok(cert)
    }

    pub fn delete_certs(&self, auth: &AuthInfo, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.send(
            "DELETE",
            &format!("{}/cps/harmony-cert-manage/v1/cert/delete", self.base),
            Some(auth),
            Some(&json!({ "certIds": ids })),
        )?;
        Ok(())
    }

    pub fn cert_download_url(&self, auth: &AuthInfo, cert: &CloudCert) -> Result<String> {
        if !cert.cert_download_url.is_empty() {
            return Ok(cert.cert_download_url.clone());
        }
        self.object_download_url(auth, &cert.cert_object_id)
    }

    /// 通过 AGC 对象 ID 换取下载地址（证书、Profile 通用）。
    pub fn object_download_url(&self, auth: &AuthInfo, object_id: &str) -> Result<String> {
        if object_id.is_empty() {
            return Err(anyhow!("AGC 对象没有可用的下载标识"));
        }
        let value = self.send(
            "POST",
            &format!("{}/amis/app-manage/v1/objects/url/reapply", self.base),
            Some(auth),
            Some(&json!({ "sourceUrls": object_id })),
        )?;
        let url = value
            .get("urlsInfo")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("newUrl"))
            .and_then(Value::as_str)
            .or_else(|| value.pointer("/urlInfo/url").and_then(Value::as_str))
            .unwrap_or_default();
        if url.is_empty() {
            return Err(anyhow!("AGC 未返回对象下载地址"));
        }
        Ok(url.to_string())
    }

    // ------------------------------------------------------------------
    // 设备
    // ------------------------------------------------------------------

    pub fn devices(&self, auth: &AuthInfo) -> Result<Vec<CloudDevice>> {
        let mut devices = Vec::new();
        let mut seen = BTreeSet::new();
        for page in 1..=10 {
            let url = format!(
                "{}/cps/device-manage/v1/device/list?start={}&pageSize=100&encodeFlag=0",
                self.base,
                (page - 1) * 100 + 1
            );
            let value = self.send("GET", &url, Some(auth), None)?;
            let items = value
                .get("list")
                .or_else(|| value.get("deviceList"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let count = items.len();
            for item in items {
                let device = parse_device(&item);
                let key = if device.id.is_empty() {
                    device.udid.clone()
                } else {
                    device.id.clone()
                };
                if seen.insert(key) {
                    devices.push(device);
                }
            }
            if count < 100 {
                break;
            }
        }
        Ok(devices)
    }

    pub fn add_device(&self, auth: &AuthInfo, name: &str, udid: &str) -> Result<()> {
        self.send(
            "POST",
            &format!("{}/cps/device-manage/v1/device/add", self.base),
            Some(auth),
            Some(&json!({
                "deviceName": name,
                "udid": udid,
                "deviceType": 4,
            })),
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Profile
    // ------------------------------------------------------------------

    /// 申请调试 Profile。发布 Profile 没有在 auto-installer 的可用流程中出现，
    /// 这里不猜测发布接口；发布签名应使用导入的 DevEco/AGC 材料。
    pub fn create_debug_profile(
        &self,
        auth: &AuthInfo,
        name: &str,
        cert_id: &str,
        package_name: &str,
        device_ids: &[String],
        acl: &[String],
    ) -> Result<CloudProfile> {
        let value = self.send(
            "POST",
            &format!(
                "{}/cps/provision-manage/v1/ide/test/provision/add",
                self.base
            ),
            Some(auth),
            Some(&json!({
                "provisionName": name,
                "certList": [cert_id],
                "packageName": package_name,
                "deviceList": device_ids,
                "aclPermissionList": acl,
            })),
        )?;
        let url = value
            .get("provisionFileUrl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut profile = value
            .get("provisionInfo")
            .map(parse_profile)
            .unwrap_or_default();
        if profile.provision_download_url.is_empty() {
            profile.provision_download_url = url;
        }
        if profile.id.is_empty() {
            profile.id = string_field(&value, "id");
        }
        if profile.provision_download_url.is_empty() {
            let message = value
                .pointer("/ret/msg")
                .and_then(Value::as_str)
                .unwrap_or("AGC 未返回 Profile 下载链接");
            return Err(anyhow!("{message}"));
        }
        profile.provision_name = name.to_string();
        profile.provision_type = CertKind::Debug.profile_type();
        profile.cert_id = cert_id.to_string();
        profile.acl_permission_list = acl.to_vec();
        Ok(profile)
    }

    pub fn profiles(&self, auth: &AuthInfo) -> Result<Vec<CloudProfile>> {
        let value = self.send(
            "GET",
            &format!(
                "{}/publish/v3/provision/list?fromRecCount=1&maxReqCount=100",
                self.base
            ),
            Some(auth),
            None,
        )?;
        let items = value
            .get("provisionList")
            .or_else(|| value.get("list"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(items.iter().map(parse_profile).collect())
    }

    pub fn delete_profiles(&self, auth: &AuthInfo, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.send(
            "POST",
            &format!("{}/publish/v3/provision/delete", self.base),
            Some(auth),
            Some(&json!({ "provisionIds": ids })),
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // 传输
    // ------------------------------------------------------------------

    pub fn download(&self, url: &str, path: &Path) -> Result<()> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|err| anyhow!("下载失败：{err}"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("无法创建目录 {}", parent.display()))?;
        }
        let mut reader = response.into_reader();
        let mut file =
            File::create(path).with_context(|| format!("无法写入 {}", path.display()))?;
        std::io::copy(&mut reader, &mut file)
            .with_context(|| format!("下载文件写入失败 {}", path.display()))?;
        let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if size == 0 {
            return Err(anyhow!("下载到的文件为空"));
        }
        Ok(())
    }

    fn send(
        &self,
        method: &str,
        url: &str,
        auth: Option<&AuthInfo>,
        body: Option<&Value>,
    ) -> Result<Value> {
        let mut request = self.agent.request(method, url);
        if let Some(auth) = auth {
            request = request
                .set("oauth2Token", &auth.access_token)
                .set("teamId", auth.team_id_or_user())
                .set("uid", &auth.user_id);
        }
        let result = match body {
            Some(body) => request.send_json(body.clone()),
            None => request.call(),
        };
        match result {
            Ok(response) => {
                let text = response.into_string().context("读取 AGC 响应失败")?;
                if text.trim().is_empty() {
                    return Err(anyhow!("AGC 返回空响应"));
                }
                let value: Value = serde_json::from_str(&text)
                    .map_err(|_| anyhow!("AGC 返回了非 JSON 响应（请检查网络、代理或账号地区）"))?;
                check_envelope(value, 200)
            }
            Err(ureq::Error::Status(status, response)) => {
                let text = response.into_string().unwrap_or_default();
                let value = serde_json::from_str::<Value>(&text).ok();
                let (code, message) = value
                    .as_ref()
                    .and_then(|value| value.get("ret"))
                    .map(|ret| {
                        (
                            ret.get("code")
                                .and_then(Value::as_i64)
                                .unwrap_or(status as i64),
                            ret.get("msg")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        )
                    })
                    .unwrap_or((status as i64, text.trim().chars().take(300).collect()));
                Err(ApiError {
                    status,
                    code,
                    message: human_error(status, code, &message),
                }
                .into())
            }
            Err(err) => Err(anyhow!("网络请求失败：{err}")),
        }
    }
}

fn check_envelope(value: Value, status: u16) -> Result<Value> {
    if let Some(ret) = value.get("ret") {
        let code = ret.get("code").and_then(Value::as_i64).unwrap_or(0);
        if code != 0 {
            let message = ret.get("msg").and_then(Value::as_str).unwrap_or_default();
            return Err(ApiError {
                status,
                code,
                message: human_error(status, code, message),
            }
            .into());
        }
    }
    Ok(value)
}

/// 把 AGC 的英文/原始错误翻译成可操作的中文。
pub fn human_error(status: u16, code: i64, message: &str) -> String {
    if status == 401 || code == 401 {
        return "登录信息已失效，请重新登录".to_string();
    }
    if status == 403 && message.is_empty() {
        return "账号无 AGC 权限，或网络地区受限（请关闭代理并使用国内网络）".to_string();
    }
    let lower = message.to_ascii_lowercase();
    if lower.contains("exceeds limit") || lower.contains("limit exceeded") {
        return "AGC 配额已用尽（证书或 Profile 数量/月度签名次数），请到 AGC 清理或更换账号/团队"
            .to_string();
    }
    if lower.contains("certlist") || lower.contains("cert list") || lower.contains("cert not exist")
    {
        return "当前证书已失效或不存在，请回到证书卡片重新申请".to_string();
    }
    if lower.contains("appid") && lower.contains("not exist") {
        return "该包名尚未在 AGC 创建应用，请先在 AGC 创建应用后重试".to_string();
    }
    if lower.contains("acl") && (lower.contains("audit") || lower.contains("permission")) {
        return format!("ACL 权限未通过审核或不可申请：{message}");
    }
    if lower.contains("login") || lower.contains("token") {
        return "登录信息已失效，请重新登录".to_string();
    }
    if message.is_empty() {
        format!("AGC 请求失败（HTTP {status} / code {code}）")
    } else {
        format!("AGC 错误[{code}]：{message}")
    }
}

fn parse_cert(value: &Value) -> CloudCert {
    CloudCert {
        id: string_field(value, "id"),
        cert_name: string_field(value, "certName"),
        cert_object_id: string_field(value, "certObjectId"),
        cert_type: int_field(value, "certType") as i32,
        expire_time: timestamp_field(value, "expireTime"),
        cert_download_url: string_field(value, "certDownloadUrl"),
    }
}

fn parse_device(value: &Value) -> CloudDevice {
    CloudDevice {
        id: string_field(value, "id"),
        device_name: string_field(value, "deviceName"),
        udid: string_field(value, "udid"),
    }
}

fn parse_profile(value: &Value) -> CloudProfile {
    CloudProfile {
        id: string_field(value, "id"),
        provision_name: string_field(value, "provisionName"),
        provision_type: int_field(value, "provisionType") as i32,
        cert_name: string_field(value, "certName"),
        cert_id: string_field(value, "certId"),
        package_name: string_field(value, "packageName"),
        provision_download_url: string_field(value, "provisionDownloadUrl"),
        expire_time: timestamp_field(value, "expireTime"),
        acl_permission_list: string_array(value, "aclPermissionList"),
        acl_permission_audit_state: int_field(value, "aclPermissionAuditState") as i32,
        device_list: value
            .get("deviceList")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(parse_device).collect())
            .unwrap_or_default(),
    }
}

fn timestamp_field(value: &Value, key: &str) -> i64 {
    let raw = int_field(value, key);
    if raw > 10_000_000_000 {
        raw / 1000
    } else {
        raw
    }
}

fn string_field(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn int_field(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(0)
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
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

fn query_encode(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_cert_list_shapes() {
        let auth = AuthInfo::default();
        let value = json!({
            "ret": {"code": 0},
            "certList": [{"id": "1", "certName": "a", "certType": 1, "expireTime": 1000}],
            "certInfo": [{"id": "2", "certName": "b", "certType": 2}]
        });
        let _ = auth;
        let mut certs = Vec::new();
        for key in ["certList", "certInfo"] {
            if let Some(items) = value.get(key).and_then(Value::as_array) {
                certs.extend(items.iter().map(parse_cert));
            }
        }
        assert_eq!(certs.len(), 2);
        assert_eq!(certs[0].cert_type, 1);
        assert_eq!(certs[1].cert_name, "b");
    }

    #[test]
    fn maps_quota_and_cert_errors() {
        assert!(human_error(200, 1001, "certList is invalid").contains("证书"));
        assert!(human_error(200, 1002, "exceeds limit").contains("配额"));
        assert!(human_error(401, 401, "").contains("重新登录"));
    }

    #[test]
    fn envelope_code_is_rejected() {
        let value = json!({"ret": {"code": 7, "msg": "bad"}});
        let error = check_envelope(value, 200).unwrap_err();
        let api = error.downcast_ref::<ApiError>().unwrap();
        assert_eq!(api.code, 7);
    }

    #[test]
    fn parses_profile_creation_response() {
        let value = json!({
            "provisionFileUrl": "https://example.com/a.p7b",
            "provisionInfo": {
                "id": "profile-1",
                "provisionName": "rarebox-debug-com.example",
                "provisionType": 1,
                "expireTime": 2000000000,
                "aclPermissionList": ["ohos.permission.READ_CONTACTS"],
                "deviceList": [{"id": "d1", "udid": "u1", "deviceName": "phone"}]
            }
        });
        let mut profile = value.get("provisionInfo").map(parse_profile).unwrap();
        if profile.provision_download_url.is_empty() {
            profile.provision_download_url = value
                .get("provisionFileUrl")
                .and_then(Value::as_str)
                .unwrap()
                .to_string();
        }
        assert_eq!(profile.id, "profile-1");
        assert_eq!(profile.cert_id, "");
        assert_eq!(profile.device_list.len(), 1);
        assert_eq!(profile.acl_permission_list.len(), 1);
    }

    #[test]
    fn normalizes_millisecond_timestamps() {
        assert_eq!(
            timestamp_field(&json!({"expireTime": 1_700_000_000_000i64}), "expireTime"),
            1_700_000_000
        );
        assert_eq!(
            timestamp_field(&json!({"expireTime": 1_700_000_000i64}), "expireTime"),
            1_700_000_000
        );
        assert_eq!(
            timestamp_field(&json!({"expireTime": "1700000000000"}), "expireTime"),
            1_700_000_000
        );
    }

    #[test]
    fn escapes_package_names() {
        assert_eq!(query_encode("com.example.app"), "com.example.app");
        assert_eq!(query_encode("a b&c"), "a%20b%26c");
    }
}
