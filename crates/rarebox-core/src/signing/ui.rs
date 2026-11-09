//! 签名页的引擎状态（账号、证书、Profile、七步流水线进度）。
//!
//! 渲染在宿主 UI；网络、密钥与签名全部在引擎后台线程，`FlowEvent` 会转成状态事件。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::model::{CachedProfile, ImportedMaterial};
use super::{
    CertKind, FlowEvent, FlowOutcome, HapMeta, SigningBlock, SigningService, Stage, StageStatus,
};

#[derive(Clone, Debug, Default)]
pub enum LoginPhase {
    #[default]
    Idle,
    Waiting,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportFile {
    Keystore,
    Cert,
    Profile,
}

pub struct SigningUi {
    pub service: SigningService,
    pub login: LoginPhase,
    pub cancel_login: Option<Arc<AtomicBool>>,
    pub busy: bool,
    pub initialized: bool,
    pub kind: CertKind,
    pub use_imported: bool,
    pub import_keystore: Option<PathBuf>,
    pub import_cert: Option<PathBuf>,
    pub import_profile: Option<PathBuf>,
    pub import_alias: String,
    pub import_password: String,
    pub block: SigningBlock,
    pub stages: [StageStatus; 7],
    pub stage_messages: [String; 7],
    pub logs: Vec<String>,
    pub acl: Vec<String>,
    pub acl_query: String,
    pub package: Option<HapMeta>,
    pub inputs: Vec<PathBuf>,
    pub udid: Option<String>,
    pub reset_cert: bool,
    pub refresh_profile: bool,
    pub replace: bool,
    pub shared: bool,
    pub show_logs: bool,
    pub outcome: Option<FlowOutcome>,
    pub last_signed: Option<PathBuf>,
    pub local_profiles: Vec<CachedProfile>,
    pub error: Option<String>,
    pub generation: u64,
}

impl Default for SigningUi {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SigningUi {
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
            login: self.login.clone(),
            cancel_login: self.cancel_login.clone(),
            busy: self.busy,
            initialized: self.initialized,
            kind: self.kind,
            use_imported: self.use_imported,
            import_keystore: self.import_keystore.clone(),
            import_cert: self.import_cert.clone(),
            import_profile: self.import_profile.clone(),
            import_alias: self.import_alias.clone(),
            import_password: self.import_password.clone(),
            block: self.block,
            stages: self.stages,
            stage_messages: self.stage_messages.clone(),
            logs: self.logs.clone(),
            acl: self.acl.clone(),
            acl_query: self.acl_query.clone(),
            package: self.package.clone(),
            inputs: self.inputs.clone(),
            udid: self.udid.clone(),
            reset_cert: self.reset_cert,
            refresh_profile: self.refresh_profile,
            replace: self.replace,
            shared: self.shared,
            show_logs: self.show_logs,
            outcome: self.outcome.clone(),
            last_signed: self.last_signed.clone(),
            local_profiles: self.local_profiles.clone(),
            error: self.error.clone(),
            generation: self.generation,
        }
    }
}

impl std::fmt::Debug for SigningUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningUi")
            .field("login", &self.login)
            .field("busy", &self.busy)
            .field("initialized", &self.initialized)
            .field("kind", &self.kind)
            .field("use_imported", &self.use_imported)
            .field("import_alias", &self.import_alias)
            .field("block", &self.block)
            .field("stages", &self.stages)
            .field("logs", &self.logs)
            .field("acl", &self.acl)
            .field("package", &self.package)
            .field("inputs", &self.inputs)
            .field("udid", &self.udid)
            .field("outcome", &self.outcome)
            .field("local_profiles", &self.local_profiles)
            .field("error", &self.error)
            .field("generation", &self.generation)
            .finish()
    }
}

impl SigningUi {
    pub fn new() -> Self {
        let service = SigningService::new();
        let local_profiles = service.store.load_profiles(&service.team_id());
        Self {
            service,
            login: LoginPhase::Idle,
            cancel_login: None,
            busy: false,
            initialized: false,
            kind: CertKind::Debug,
            use_imported: false,
            import_keystore: None,
            import_cert: None,
            import_profile: None,
            import_alias: "rarebox".to_string(),
            import_password: String::new(),
            block: SigningBlock::V3,
            stages: [StageStatus::Pending; 7],
            stage_messages: std::array::from_fn(|_| String::new()),
            logs: Vec::new(),
            acl: Vec::new(),
            acl_query: String::new(),
            package: None,
            inputs: Vec::new(),
            udid: None,
            reset_cert: false,
            refresh_profile: false,
            replace: true,
            shared: false,
            show_logs: false,
            outcome: None,
            last_signed: None,
            local_profiles,
            error: None,
            generation: 0,
        }
    }

    pub fn reload_local_profiles(&mut self) {
        self.local_profiles = self.service.store.load_profiles(&self.service.team_id());
    }

    pub fn reset_stages(&mut self) {
        self.stages = [StageStatus::Pending; 7];
        self.stage_messages = std::array::from_fn(|_| String::new());
    }

    pub fn push_log(&mut self, message: impl Into<String>) {
        let message = message.into();
        if message.trim().is_empty() {
            return;
        }
        self.logs.push(message);
        if self.logs.len() > 600 {
            let excess = self.logs.len() - 600;
            self.logs.drain(0..excess);
        }
    }

    pub fn apply_event(&mut self, event: FlowEvent) {
        match event {
            FlowEvent::Stage {
                stage,
                status,
                message,
            } => {
                let index = stage.index();
                self.stages[index] = status;
                if !message.is_empty() {
                    self.stage_messages[index] = message.clone();
                    self.push_log(format!("[{}] {message}", stage.label()));
                }
            }
            FlowEvent::Log(message) => self.push_log(message),
        }
    }

    pub fn stage_status(&self, stage: Stage) -> StageStatus {
        self.stages[stage.index()]
    }

    pub fn stage_message(&self, stage: Stage) -> &str {
        &self.stage_messages[stage.index()]
    }

    pub fn toggle_imported(&mut self) {
        self.use_imported = !self.use_imported;
        self.outcome = None;
        self.reset_stages();
        if let Some(meta) = &self.package {
            self.acl = meta.acl.clone();
        }
    }

    pub fn set_import_file(&mut self, which: ImportFile, path: PathBuf) {
        match which {
            ImportFile::Keystore => self.import_keystore = Some(path),
            ImportFile::Cert => self.import_cert = Some(path),
            ImportFile::Profile => self.import_profile = Some(path),
        }
    }

    pub fn imported_material(&self) -> Option<ImportedMaterial> {
        let keystore = self.import_keystore.clone()?;
        let cert = self.import_cert.clone()?;
        let profile = self.import_profile.clone()?;
        if self.import_alias.trim().is_empty()
            || !keystore.exists()
            || !cert.exists()
            || !profile.exists()
        {
            return None;
        }
        Some(ImportedMaterial {
            keystore,
            store_password: self.import_password.clone(),
            key_alias: self.import_alias.trim().to_string(),
            key_password: self.import_password.clone(),
            cert,
            profile,
        })
    }

    pub fn set_import_alias(&mut self, value: String) {
        self.import_alias = value;
    }

    pub fn set_import_password(&mut self, value: String) {
        self.import_password = value;
    }

    pub fn set_acl_query(&mut self, query: String) {
        self.acl_query = query;
    }

    pub fn acl_toggle(&mut self, permission: &str) {
        if let Some(index) = self.acl.iter().position(|item| item == permission) {
            self.acl.remove(index);
        } else {
            self.acl.push(permission.to_string());
            self.acl.sort();
        }
    }

    pub fn filtered_acl(&self) -> Vec<&'static str> {
        let query = self.acl_query.trim().to_ascii_lowercase();
        super::ACL_PERMISSIONS
            .iter()
            .copied()
            .filter(|permission| {
                query.is_empty() || permission.to_ascii_lowercase().contains(&query)
            })
            .collect()
    }
}
