//! 签名域的状态与服务：账号、团队、云端资源列表。
//!
//! UI 只调用这里的方法；每个方法在失败时返回可展示的 `anyhow` 错误，401 会
//! 自动刷新一次 jwt 后重试。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Result, anyhow, bail};

use super::api::{ApiError, EcoClient};
use super::auth::LoginSession;
use super::model::{AuthInfo, CertKind, CloudCert, CloudDevice, CloudProfile, LocalIdentity, Team};
use super::store::SigningStore;

#[derive(Clone)]
pub struct SigningService {
    pub store: SigningStore,
    pub api: EcoClient,
    pub auth: Option<AuthInfo>,
    pub teams: Vec<Team>,
    pub certs: Vec<CloudCert>,
    pub devices: Vec<CloudDevice>,
    pub profiles: Vec<CloudProfile>,
    pub identities: HashMap<CertKind, LocalIdentity>,
    /// 最近一次加载云端资源时的错误，供 UI 展示但不阻断其它卡片。
    pub last_error: Option<String>,
}

impl Default for SigningService {
    fn default() -> Self {
        Self::new()
    }
}

impl SigningService {
    pub fn new() -> Self {
        let store = SigningStore::new(SigningStore::default_root());
        let auth = store.load_auth();
        let mut identities = HashMap::new();
        if let Some(auth) = &auth {
            for kind in CertKind::ALL {
                if let Some(identity) = store.load_identity(auth.team_id_or_user(), kind) {
                    identities.insert(kind, identity);
                }
            }
        }
        Self {
            store,
            api: EcoClient::new(),
            auth,
            teams: Vec::new(),
            certs: Vec::new(),
            devices: Vec::new(),
            profiles: Vec::new(),
            identities,
            last_error: None,
        }
    }

    pub fn is_logged_in(&self) -> bool {
        self.auth
            .as_ref()
            .map(AuthInfo::is_logged_in)
            .unwrap_or(false)
    }

    pub fn team_id(&self) -> String {
        self.auth
            .as_ref()
            .map(|auth| auth.team_id_or_user().to_string())
            .unwrap_or_default()
    }

    pub fn current_team(&self) -> Option<&Team> {
        let team_id = self.team_id();
        self.teams.iter().find(|team| team.id == team_id)
    }

    pub fn identity(&self, kind: CertKind) -> Option<&LocalIdentity> {
        self.identities.get(&kind)
    }

    /// 打开浏览器等待登录，成功后加载团队并保存 token。
    pub fn login(
        &mut self,
        cancel: Arc<AtomicBool>,
        log: &mut dyn FnMut(&str),
        open_url: &mut dyn FnMut(&str) -> Result<()>,
    ) -> Result<()> {
        let session = LoginSession::bind(cancel)?;
        let url = session.login_url();
        log(&format!("登录回调监听 {}", session.port_label()));
        log("已打开浏览器，请在页面完成华为账号登录…");
        open_url(&url).map_err(|err| anyhow!("无法打开浏览器：{err}"))?;
        let token = session.wait_for_token()?;
        log("已收到登录回调，正在交换账号令牌…");
        let mut auth = self.api.token_by_temp_token(&token)?;
        auth.team_id = auth.user_id.clone();
        let teams = match self.api.teams(&auth) {
            Ok(teams) => teams,
            Err(err) => {
                log(&format!("读取团队信息失败（可稍后刷新）：{err:#}"));
                Vec::new()
            }
        };
        if let Some(team) = teams.first() {
            auth.team_id = team.id.clone();
        }
        self.store.save_auth(&auth)?;
        self.teams = teams;
        self.auth = Some(auth);
        self.load_local_identities();
        log("登录成功");
        self.reload_all(log);
        Ok(())
    }

    pub fn logout(&mut self) -> Result<()> {
        self.store.clear_auth()?;
        self.auth = None;
        self.teams.clear();
        self.certs.clear();
        self.devices.clear();
        self.profiles.clear();
        self.identities.clear();
        Ok(())
    }

    pub fn select_team(&mut self, team_id: &str) -> Result<()> {
        let Some(auth) = self.auth.as_mut() else {
            bail!("请先登录");
        };
        auth.team_id = team_id.to_string();
        let auth = auth.clone();
        self.store.save_auth(&auth)?;
        self.certs.clear();
        self.devices.clear();
        self.profiles.clear();
        self.identities.clear();
        for kind in CertKind::ALL {
            if let Some(identity) = self.store.load_identity(team_id, kind) {
                self.identities.insert(kind, identity);
            }
        }
        Ok(())
    }

    /// 验证 token；失效时用 jwt 刷新一次。
    pub fn ensure_auth_fresh(&mut self) -> Result<()> {
        let Some(auth) = self.auth.clone() else {
            bail!("请先登录华为开发者账号");
        };
        match self.api.teams(&auth) {
            Ok(teams) => {
                self.teams = teams;
                self.ensure_selected_team();
                Ok(())
            }
            Err(err) if is_unauthorized(&err) => {
                let refreshed = self.api.refresh_token(&auth)?;
                let mut refreshed = refreshed;
                refreshed.team_id = if auth.team_id.is_empty() {
                    refreshed.user_id.clone()
                } else {
                    auth.team_id.clone()
                };
                self.store.save_auth(&refreshed)?;
                self.auth = Some(refreshed.clone());
                self.teams = self.api.teams(&refreshed)?;
                self.ensure_selected_team();
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn load_local_identities(&mut self) {
        self.identities.clear();
        let team_id = self.team_id();
        if team_id.is_empty() {
            return;
        }
        for kind in CertKind::ALL {
            if let Some(identity) = self.store.load_identity(&team_id, kind) {
                self.identities.insert(kind, identity);
            }
        }
    }

    fn ensure_selected_team(&mut self) {
        let Some(auth) = self.auth.as_mut() else {
            return;
        };
        if !self.teams.iter().any(|team| team.id == auth.team_id) {
            if let Some(team) = self.teams.first() {
                auth.team_id = team.id.clone();
            } else if auth.team_id.is_empty() {
                auth.team_id = auth.user_id.clone();
            }
            let auth = auth.clone();
            let _ = self.store.save_auth(&auth);
        }
        self.load_local_identities();
    }

    /// 生成/复用本地身份并申请或下载 AGC 证书。
    pub fn ensure_identity(
        &mut self,
        kind: CertKind,
        reset: bool,
        log: &mut dyn FnMut(&str),
    ) -> Result<LocalIdentity> {
        let auth = self.require_auth()?;
        let team_id = self.team_id();
        let identity = super::identity::ensure_identity(
            &self.api,
            &auth,
            &self.store,
            &team_id,
            kind,
            reset,
            log,
        )?;
        self.identities.insert(kind, identity.clone());
        // 证书已经落盘；列表刷新失败不应该把申请结果判为失败。
        let _ = self.refresh_certs(log);
        Ok(identity)
    }

    pub fn refresh_certs(&mut self, log: &mut dyn FnMut(&str)) -> Result<()> {
        let auth = self.require_auth()?;
        match self.api.certs(&auth) {
            Ok(certs) => {
                self.certs = certs;
                Ok(())
            }
            Err(err) if is_unauthorized(&err) => {
                self.refresh_auth()?;
                self.certs = self.api.certs(&self.require_auth()?)?;
                Ok(())
            }
            Err(err) => {
                let message = format!("{err:#}");
                log(&message);
                self.last_error = Some(message);
                Err(err)
            }
        }
    }

    pub fn refresh_devices(&mut self, log: &mut dyn FnMut(&str)) -> Result<()> {
        let auth = self.require_auth()?;
        match self.api.devices(&auth) {
            Ok(devices) => {
                self.devices = devices;
                Ok(())
            }
            Err(err) if is_unauthorized(&err) => {
                self.refresh_auth()?;
                self.devices = self.api.devices(&self.require_auth()?)?;
                Ok(())
            }
            Err(err) => {
                let message = format!("{err:#}");
                log(&message);
                self.last_error = Some(message);
                Err(err)
            }
        }
    }

    pub fn refresh_profiles(&mut self, log: &mut dyn FnMut(&str)) -> Result<()> {
        let auth = self.require_auth()?;
        match self.api.profiles(&auth) {
            Ok(profiles) => {
                self.profiles = profiles;
                Ok(())
            }
            Err(err) if is_unauthorized(&err) => {
                self.refresh_auth()?;
                self.profiles = self.api.profiles(&self.require_auth()?)?;
                Ok(())
            }
            Err(err) => {
                let message = format!("{err:#}");
                log(&message);
                self.last_error = Some(message);
                Err(err)
            }
        }
    }

    pub fn reload_all(&mut self, log: &mut dyn FnMut(&str)) {
        self.last_error = None;
        if !self.is_logged_in() {
            return;
        }
        if let Err(err) = self.ensure_auth_fresh() {
            let message = format!("{err:#}");
            log(&message);
            self.last_error = Some(message);
            return;
        }
        // 各资源独立加载，单个接口失败不影响其它卡片。
        if let Err(err) = self.refresh_certs(log) {
            log(&format!("读取证书列表失败：{err:#}"));
        }
        if let Err(err) = self.refresh_devices(log) {
            log(&format!("读取设备列表失败：{err:#}"));
        }
        if let Err(err) = self.refresh_profiles(log) {
            log(&format!("读取 Profile 列表失败：{err:#}"));
        }
    }

    fn refresh_auth(&mut self) -> Result<()> {
        let auth = self.require_auth()?;
        let refreshed = self.api.refresh_token(&auth)?;
        let mut refreshed = refreshed;
        refreshed.team_id = auth.team_id.clone();
        self.store.save_auth(&refreshed)?;
        self.auth = Some(refreshed);
        Ok(())
    }

    fn require_auth(&self) -> Result<AuthInfo> {
        self.auth
            .clone()
            .filter(AuthInfo::is_logged_in)
            .ok_or_else(|| anyhow!("请先登录华为开发者账号"))
    }

    /// 按 UDID 查找已注册设备；没有则注册后重新加载。
    pub fn ensure_device(&mut self, udid: &str, log: &mut dyn FnMut(&str)) -> Result<CloudDevice> {
        let udid = udid.trim();
        if udid.is_empty() {
            bail!("无法读取设备 UDID，请重新连接设备");
        }
        if let Some(device) = self.devices.iter().find(|device| device.udid == udid) {
            return Ok(device.clone());
        }
        self.refresh_devices(log)?;
        if let Some(device) = self.devices.iter().find(|device| device.udid == udid) {
            return Ok(device.clone());
        }
        let prefix: String = udid.chars().take(12).collect();
        let name = format!("rarebox-{prefix}");
        log(&format!("注册调试设备 {name}…"));
        let auth = self.require_auth()?;
        self.api.add_device(&auth, &name, udid)?;
        self.refresh_devices(log)?;
        self.devices
            .iter()
            .find(|device| device.udid == udid)
            .cloned()
            .ok_or_else(|| anyhow!("设备注册后仍未在 AGC 列表中找到，请稍后重试"))
    }
}

pub fn is_unauthorized(err: &anyhow::Error) -> bool {
    err.downcast_ref::<ApiError>()
        .map(ApiError::unauthorized)
        .unwrap_or(false)
}
