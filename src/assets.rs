//! 应用资源：图标目录定位与 Android 端的内嵌图标兜底。

use std::borrow::Cow;
#[cfg(not(target_os = "android"))]
use std::path::PathBuf;

use anyhow::Result;
use gpui::{AssetSource, SharedString};
use material3_gpui::Md3Assets;

include!(concat!(env!("OUT_DIR"), "/embedded_icons.rs"));

/// 编译期嵌入的 SVG 图标。
///
/// 桌面端 `Md3Assets` 会先读 `assets/icons`；只有磁盘上找不到时才回退到这里。
/// Android 端没有图标目录，因此始终由这里提供。
pub struct EmbeddedIcons;

impl AssetSource for EmbeddedIcons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(EMBEDDED_ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(EMBEDDED_ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

/// 构造应用的资源源。Android 不访问文件系统图标目录。
pub fn md3_assets() -> impl AssetSource {
    #[cfg(target_os = "android")]
    {
        Md3Assets::new().with_fallback(EmbeddedIcons)
    }
    #[cfg(not(target_os = "android"))]
    {
        Md3Assets::new()
            .with_icon_dir(icon_dir())
            .with_fallback(EmbeddedIcons)
    }
}

#[cfg(not(target_os = "android"))]
fn icon_dir() -> PathBuf {
    let candidates = [
        std::env::var_os("RAREBOX_ICON_DIR").map(PathBuf::from),
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join("assets/icons"))),
        // macOS .app 包内：<Contents>/MacOS/rarebox-desktop → <Contents>/Resources/assets/icons
        std::env::current_exe().ok().and_then(|path| {
            path.parent()
                .and_then(|macos| macos.parent())
                .map(|contents| contents.join("Resources/assets/icons"))
        }),
        std::env::current_dir()
            .ok()
            .map(|path| path.join("assets/icons")),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/icons")),
    ];

    candidates
        .into_iter()
        .flatten()
        .find(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("assets/icons"))
}
