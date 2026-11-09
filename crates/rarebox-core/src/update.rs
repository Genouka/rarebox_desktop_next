//! 版本更新与公告。
//!
//! 数据来自 RareBox API 站点：`/api/update/check` 同时返回更新检查结果和当前公告，
//! `/api/announcements?scope=history` 返回包含历史公告的完整列表。所有请求都有超时，
//! 失败时由界面提示，不影响其它功能。

use std::time::Duration;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use anyhow::{Context as _, Result, anyhow};
use serde::Deserialize;

/// 当前应用版本，取自 `Cargo.toml`，避免标题栏、设置页与更新检查各写一份。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

const API_BASE: &str = "https://rareboxnextapi.qiumingsanyu.top/api";
const CHANNEL: &str = "stable";
const TIMEOUT: Duration = Duration::from_secs(8);

/// 当前平台标识，与后台 `releases.platform` 对应。
pub fn platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// 后台返回的公告。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Announcement {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub level: String,
    /// 创建时间，形如 `2026-10-26 12:00:00`。
    #[serde(default)]
    pub created_at: Option<String>,
}

impl Announcement {
    /// 列表中显示的日期（只保留年月日）。
    pub fn date(&self) -> &str {
        self.created_at
            .as_deref()
            .and_then(|value| value.split_whitespace().next())
            .unwrap_or("")
    }

    /// 级别对应的中文标签与主题色索引。
    pub fn level_label(&self) -> &'static str {
        match self.level.as_str() {
            "warning" => "提醒",
            "critical" => "重要",
            _ => "公告",
        }
    }
}

/// `/api/update/check` 的响应。字段全部带默认值，兼容旧版后台。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct UpdateStatus {
    pub update_available: bool,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub channel: Option<String>,
    pub platform: Option<String>,
    pub download_url: Option<String>,
    pub file_size: u64,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    pub force_update: bool,
    pub required_min_version: Option<String>,
    pub reasons: Vec<String>,
    /// 没有发布记录时后台返回的原因（如 `no_release`）。
    pub reason: Option<String>,
    pub announcements: Vec<Announcement>,
}

impl UpdateStatus {
    /// 是否必须更新：后台标记强制更新，或当前版本低于最低支持版本。
    pub fn forced(&self) -> bool {
        self.force_update
            || self
                .reasons
                .iter()
                .any(|reason| reason == "forced" || reason == "below_minimum")
    }

    /// 更新说明；后台没有填写时返回 `None`。
    pub fn notes_text(&self) -> Option<&str> {
        self.notes
            .as_deref()
            .map(str::trim)
            .filter(|notes| !notes.is_empty())
    }
}

/// 请求最新版本信息，并附带当前生效的公告。
pub fn check() -> Result<UpdateStatus> {
    let url = format!(
        "{API_BASE}/update/check?version={}&platform={}&channel={CHANNEL}",
        APP_VERSION,
        platform()
    );
    fetch_json(&url)
}

/// 请求完整公告列表（含已过期公告，用于「以往公告」）。
pub fn announcements() -> Result<Vec<Announcement>> {
    #[derive(Deserialize)]
    struct Payload {
        #[serde(default)]
        announcements: Vec<Announcement>,
    }

    let url = format!("{API_BASE}/announcements?scope=history");
    let payload: Payload = fetch_json(&url)?;
    Ok(payload.announcements)
}

fn fetch_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let response = agent
        .get(url)
        .call()
        .map_err(|err| anyhow!("请求失败：{err}"))?;
    response.into_json::<T>().context("解析服务器响应失败")
}

/// 用系统默认浏览器打开链接。
pub fn open_url(url: &str) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(anyhow!("下载地址无效：{url}"));
    }
    if url.contains('"') || url.contains(['\r', '\n']) {
        return Err(anyhow!("下载地址包含非法字符"));
    }

    #[cfg(target_os = "android")]
    {
        // Android 上没有可执行的浏览器命令；由宿主（JNI / GPUI）处理 OpenUrl 事件。
        let _ = url;
        return Err(anyhow!("Android 端请由宿主打开链接"));
    }

    #[cfg(not(target_os = "android"))]
    {
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = std::process::Command::new("cmd");
            command.args(["/C", &format!("start \"\" \"{url}\"")]);
            command
        };
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut command = std::process::Command::new("open");
            command.arg(url);
            command
        };
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let mut command = {
            let mut command = std::process::Command::new("xdg-open");
            command.arg(url);
            command
        };

        // 应用是 GUI 程序；不加该标志时 `cmd /C start` 会在 Windows Terminal /
        // conhost 中弹出终端窗口。
        #[cfg(target_os = "windows")]
        command.creation_flags(0x0800_0000);
        command.spawn().context("无法启动系统浏览器")?;
        Ok(())
    }
}

/// 把字节数格式化为易读的大小。
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_update_check_response() {
        let raw = r#"{
            "update_available": true,
            "current_version": "0.1.0",
            "latest_version": "1.0.0",
            "channel": "stable",
            "platform": "windows",
            "download_url": "https://example.com/app.exe",
            "file_size": 123456,
            "notes": "修复若干问题",
            "published_at": "2026-10-26 12:00:00",
            "force_update": false,
            "required_min_version": "0.9.0",
            "reasons": ["new_version", "below_minimum"],
            "announcements": [
                { "id": 7, "title": "维护通知", "body": "今晚维护", "level": "warning", "created_at": "2026-10-20 09:00:00" }
            ]
        }"#;
        let status: UpdateStatus = serde_json::from_str(raw).unwrap();
        assert!(status.update_available);
        assert_eq!(status.latest_version.as_deref(), Some("1.0.0"));
        assert_eq!(status.file_size, 123456);
        assert!(status.forced(), "低于最低版本应视为强制更新");
        assert_eq!(status.announcements.len(), 1);
        assert_eq!(status.announcements[0].level_label(), "提醒");
        assert_eq!(status.announcements[0].date(), "2026-10-20");
    }

    #[test]
    fn parses_no_release_response() {
        let raw = r#"{
            "update_available": false,
            "current_version": "0.1.0",
            "latest_version": null,
            "reason": "no_release",
            "announcements": []
        }"#;
        let status: UpdateStatus = serde_json::from_str(raw).unwrap();
        assert!(!status.update_available);
        assert!(!status.forced());
        assert_eq!(status.reason.as_deref(), Some("no_release"));
        assert!(status.notes_text().is_none());
    }

    #[test]
    fn forced_flag_wins_over_reasons() {
        let status = UpdateStatus {
            update_available: true,
            force_update: true,
            ..Default::default()
        };
        assert!(status.forced());
    }

    #[test]
    fn formats_file_sizes() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn rejects_unsafe_download_urls() {
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("https://example.com/a\"b").is_err());
    }
}
