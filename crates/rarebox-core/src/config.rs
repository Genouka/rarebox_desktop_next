//! 本地配置读写。配置保存在用户配置目录，避免污染项目目录。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Android 等移动平台在启动时注入的目录。桌面端保持为空，走系统约定。
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static PICTURES_DIR: OnceLock<PathBuf> = OnceLock::new();
static DOWNLOADS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 由平台入口设置应用数据目录（Android 的 `files` 目录）。
#[cfg(target_os = "android")]
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

/// 由平台入口设置图片/媒体目录。
#[cfg(target_os = "android")]
pub fn set_pictures_dir(dir: PathBuf) {
    let _ = PICTURES_DIR.set(dir);
}

/// 由平台入口设置下载目录。
#[cfg(target_os = "android")]
pub fn set_downloads_dir(dir: PathBuf) {
    let _ = DOWNLOADS_DIR.set(dir);
}

/// RareBox Desktop 的持久化设置。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 用户指定的 adb 路径；为空时自动查找。
    pub adb_path: String,
    /// 用户指定的 hdc（HarmonyOS Device Connector）路径；为空时自动查找。
    pub hdc_path: String,
    /// 是否使用深色主题。
    pub dark: bool,
    /// 动态色种子色。
    pub seed: u32,
    /// 截屏 / 录屏保存目录；为空时使用图片目录下的 RareBox Desktop。
    pub screenshot_dir: Option<String>,
    /// APK 安装参数，跨重启保留。
    pub install: crate::install::InstallOptions,
    /// 已接受的 EULA / 隐私政策版本；为空表示尚未同意，首次启动需联网展示并确认。
    pub accepted_legal_version: Option<String>,
    /// 关闭程序时是否自动结束 ADB / HDC 服务端进程，默认开启。
    pub kill_processes_on_exit: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            adb_path: String::new(),
            hdc_path: String::new(),
            dark: false,
            seed: 0x00_69_6D,
            screenshot_dir: None,
            install: crate::install::InstallOptions::default(),
            accepted_legal_version: None,
            kill_processes_on_exit: true,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_else(|err| {
            eprintln!("RareBox Desktop: 配置读取失败，使用默认设置: {err}");
            Self::default()
        })
    }

    pub fn save(&self) {
        let Some(path) = config_path() else {
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            eprintln!("RareBox Desktop: 无法创建配置目录: {err}");
            return;
        }
        let json = match serde_json::to_string_pretty(self) {
            Ok(json) => json,
            Err(err) => {
                eprintln!("RareBox Desktop: 配置序列化失败: {err}");
                return;
            }
        };
        if let Err(err) = std::fs::write(&path, json) {
            eprintln!("RareBox Desktop: 配置写入失败: {err}");
        }
    }

    /// 截屏与录屏的保存目录。目录不存在时会尝试创建。
    pub fn media_dir(&self) -> PathBuf {
        let dir = self
            .screenshot_dir
            .as_deref()
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| pictures_dir().join("RareBox Desktop"));
        if let Err(err) = std::fs::create_dir_all(&dir) {
            eprintln!("RareBox Desktop: 无法创建媒体目录 {}: {err}", dir.display());
        }
        dir
    }
}

/// 应用数据目录，用于存放内置 scrcpy-server 等运行时资源。
pub fn data_dir() -> PathBuf {
    if let Some(dir) = DATA_DIR.get() {
        return dir.clone();
    }
    if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(|dir| PathBuf::from(dir).join("RareBox Desktop"))
            .unwrap_or_else(|| std::env::temp_dir().join("RareBox Desktop"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|dir| PathBuf::from(dir).join("Library/Application Support/RareBox Desktop"))
            .unwrap_or_else(|| std::env::temp_dir().join("RareBox Desktop"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|dir| PathBuf::from(dir).join(".local/share")))
            .map(|dir| dir.join("rarebox-desktop"))
            .unwrap_or_else(|| std::env::temp_dir().join("rarebox-desktop"))
    }
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(dir) = DATA_DIR.get() {
        return Some(dir.join("config.json"));
    }
    if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(|dir| {
            PathBuf::from(dir)
                .join("RareBox Desktop")
                .join("config.json")
        })
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|dir| {
            PathBuf::from(dir).join("Library/Application Support/RareBox Desktop/config.json")
        })
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|dir| PathBuf::from(dir).join(".config")))
            .map(|dir| dir.join("rarebox-desktop").join("config.json"))
    }
}

pub fn pictures_dir() -> PathBuf {
    if let Some(dir) = PICTURES_DIR.get() {
        return dir.clone();
    }
    if cfg!(target_os = "windows") {
        std::env::var_os("USERPROFILE")
            .map(|dir| PathBuf::from(dir).join("Pictures"))
            .unwrap_or_else(|| PathBuf::from("."))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|dir| PathBuf::from(dir).join("Pictures"))
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::var_os("XDG_PICTURES_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|dir| PathBuf::from(dir).join("Pictures")))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

pub fn downloads_dir() -> PathBuf {
    if let Some(dir) = DOWNLOADS_DIR.get() {
        let _ = std::fs::create_dir_all(dir);
        return dir.clone();
    }
    let dir = if cfg!(target_os = "windows") {
        std::env::var_os("USERPROFILE")
            .map(|dir| PathBuf::from(dir).join("Downloads"))
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::var_os("HOME")
            .map(|dir| PathBuf::from(dir).join("Downloads"))
            .unwrap_or_else(|| PathBuf::from("."))
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("RareBox Desktop: 无法创建下载目录 {}: {err}", dir.display());
    }
    dir
}

/// 把本地路径转换为适合展示的短文本。
pub fn short_path(path: &Path) -> String {
    let raw = path.display().to_string();
    const MAX: usize = 64;
    if raw.chars().count() <= MAX {
        raw
    } else {
        let tail: String = raw
            .chars()
            .rev()
            .take(MAX - 3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_sane() {
        let config = Config::default();
        assert!(config.adb_path.is_empty());
        assert!(config.hdc_path.is_empty());
        assert!(!config.dark);
        assert_ne!(config.seed, 0);
        assert!(config.kill_processes_on_exit);
    }

    #[test]
    fn config_roundtrips() {
        let config = Config {
            adb_path: "C:/platform-tools/adb.exe".to_string(),
            hdc_path: "C:/sdk/toolchains/hdc.exe".to_string(),
            dark: true,
            seed: 0x123456,
            screenshot_dir: Some("C:/shots".to_string()),
            accepted_legal_version: None,
            kill_processes_on_exit: false,
            install: crate::install::InstallOptions {
                downgrade: true,
                allow_test: true,
                ..crate::install::InstallOptions::default()
            },
        };
        let json = serde_json::to_string(&config).expect("serialize config");
        let parsed: Config = serde_json::from_str(&json).expect("deserialize config");
        assert_eq!(parsed.adb_path, config.adb_path);
        assert_eq!(parsed.hdc_path, config.hdc_path);
        assert!(parsed.dark);
        assert_eq!(parsed.seed, config.seed);
        assert!(!parsed.kill_processes_on_exit);
        assert!(parsed.install.downgrade);
        assert!(parsed.install.allow_test);
        assert!(parsed.install.replace);
    }

    #[test]
    fn old_config_without_install_options_still_loads() {
        // 旧版本配置文件里没有 install 字段，必须回落到默认参数。
        let parsed: Config = serde_json::from_str(
            r#"{"adb_path":"adb","dark":true,"seed":119,"screenshot_dir":null}"#,
        )
        .expect("deserialize legacy config");
        assert_eq!(parsed.install, crate::install::InstallOptions::default());
        // 旧配置没有该字段时必须回落到「默认开启」。
        assert!(parsed.kill_processes_on_exit);
    }

    #[test]
    fn shortens_long_paths() {
        let path = Path::new(
            "C:/very/long/path/that/keeps/going/and/going/and/going/and/going/and/going/file.png",
        );
        let short = short_path(path);
        assert!(short.starts_with('…'));
        assert!(short.chars().count() <= 64);
    }
}
