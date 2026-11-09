//! 首次启动时的 EULA / 隐私政策获取与展示。
//!
//! 应用通过 HTTPS 从 API 站点拉取最新条款文本；若网络不可达，则回退到内置文案，
//! 保证「首次启动必须展示并确认」这一要求在任何情况下都成立。

use anyhow::{Context as _, anyhow};
use serde::Deserialize;

/// 应用要求接受的最低条款版本。若已保存的 `accepted_legal_version` 与之不同，则重新展示。
pub const LEGAL_VERSION: &str = "1.0";

/// 条款文档类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegalDocKind {
    Eula,
    Privacy,
}

impl LegalDocKind {
    pub fn doc(self) -> &'static str {
        match self {
            LegalDocKind::Eula => "eula",
            LegalDocKind::Privacy => "privacy",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            LegalDocKind::Eula => "最终用户许可协议（EULA）",
            LegalDocKind::Privacy => "隐私政策",
        }
    }
}

/// 从 API 返回的条款文档结构。
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct LegalDoc {
    pub version: String,
    pub title: String,
    pub content: String,
}

/// 从 API 拉取指定类型的条款文档。失败返回错误，由调用方决定回退。
pub fn fetch_legal(kind: LegalDocKind) -> anyhow::Result<LegalDoc> {
    let url = format!(
        "https://rareboxnextapi.qiumingsanyu.top/api/legal?doc={}",
        kind.doc()
    );
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(8))
        .build();
    let response = agent
        .get(&url)
        .call()
        .map_err(|err| anyhow!("无法获取{}：{err}", kind.title()))?;
    response.into_json::<LegalDoc>().context("解析法律条款失败")
}

/// 离线时的内置 EULA 文案（与 API 站点内容一致，作为网络不可达的回退）。
pub const FALLBACK_EULA: &str = include_str!("../../../vendor/legal/eula.txt");

/// 离线时的内置隐私政策文案。
pub const FALLBACK_PRIVACY: &str = include_str!("../../../vendor/legal/privacy.txt");
