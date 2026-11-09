//! HarmonyOS 签名与安装域。
//!
//! 模块划分：
//! - `auth`/`api`：华为开发者账号登录与 AGC Connect API；
//! - `identity`/`profile`：本地密钥、证书与 Profile 的申请/校验；
//! - `hap`：HAP/HSP/APP 元数据与 ACL 提取；
//! - `sign`：vendored `hapsigner` 封装；
//! - `service`/`flow`：状态与七步签名安装流水线。
//!
//! 所有函数都是阻塞的，UI 必须放到后台线程执行。

pub mod api;
pub mod auth;
pub mod flow;
pub mod hap;
pub mod identity;
pub mod model;
pub mod profile;
pub mod service;
pub mod sign;
pub mod store;
pub mod ui;

pub use flow::{FlowEvent, FlowOutcome, FlowRequest, Stage, StageStatus};
pub use model::{ACL_PERMISSIONS, CertKind, HapMeta, SigningBlock};
pub use service::SigningService;
pub use ui::{ImportFile, LoginPhase, SigningUi};
