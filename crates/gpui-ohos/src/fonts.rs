//! HarmonyOS 系统字体装配。
//!
//! cosmic-text 的 `FontSystem::new()` 在非 Linux/Windows 目标上找不到字体
//! （`gpui_wgpu::CosmicTextSystem` 因此在 OHOS 上用
//! `new_without_system_fonts`），所以这里按 `/system/fonts/` 的真实文件表
//! 逐个 `read` 后 `add_fonts`——与 Android 后端的做法一致。
//!
//! 文件表来自本机 SDK 的系统 fontconfig
//! （`DevEco Studio/sdk/default/openharmony/previewer/common/bin/fontconfig_ohos.json`，
//! 设备上对应 `font_dir: ["/system/fonts/"]`）。**顺序就是回退顺序**：
//!
//! 1. 拉丁主字体 `HarmonyOS_Sans.ttf`；
//! 2. `NotoSans[wdth,wght].ttf`（拉丁/多语言回退）；
//! 3. 等宽 `NotoSansMono[wdth,wght].ttf`（RareBox 的终端与日志视图）；
//! 4. 简体 `HarmonyOS_Sans_SC.ttf`、繁体 `HarmonyOS_Sans_TC.ttf`
//!    **必须都加载**——它们是独立文件，不是主字体的子集，漏掉就是整屏豆腐块（R8）；
//! 5. Emoji、数字、窄体、衬线等可选字体。
//!
//! 每个文件都先判断存在性：预览器/模拟器的 `/system/fonts/` 与真机不同，
//! 缺字体只应降级，不应让平台启动失败。

use gpui::PlatformTextSystem;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

/// RareBox 的 UI 字体族名（`theme.rs` 的 OHOS 分支使用）。
pub const UI_FONT: &str = "HarmonyOS Sans";

/// RareBox 的等宽字体族名（`ui.rs` 的 `MONO_FONT` OHOS 分支使用）。
///
/// `NotoSansMono[wdth,wght].ttf` 是可变字体；cosmic-text 取默认实例，
/// 字宽即正常宽度（地图 §5.3 第 6 条的 `wdth` 陷阱）。
pub const MONO_FONT: &str = "Noto Sans Mono";

/// 真机上的系统字体目录。
pub const SYSTEM_FONT_DIR: &str = "/system/fonts";

/// [`SYSTEM_FONT_FILES`] 中的一项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemFontFile {
    /// `/system/fonts/` 下的文件名。
    pub file_name: &'static str,
    /// 字体族名，仅用于日志与文档。
    pub family: &'static str,
    /// 该文件是否是 CJK 覆盖所必需的（缺失要报 `warn` 而不是 `debug`）。
    pub essential: bool,
}

/// 按回退顺序排列的系统字体文件表（地图 §5.2）。
pub const SYSTEM_FONT_FILES: &[SystemFontFile] = &[
    SystemFontFile {
        file_name: "HarmonyOS_Sans.ttf",
        family: UI_FONT,
        essential: true,
    },
    SystemFontFile {
        file_name: "HarmonyOS_Sans_Italic.ttf",
        family: UI_FONT,
        essential: false,
    },
    SystemFontFile {
        file_name: "NotoSans[wdth,wght].ttf",
        family: "Noto Sans",
        essential: false,
    },
    SystemFontFile {
        file_name: "NotoSansMono[wdth,wght].ttf",
        family: MONO_FONT,
        essential: false,
    },
    SystemFontFile {
        file_name: "HarmonyOS_Sans_SC.ttf",
        family: "HarmonyOS Sans SC",
        essential: true,
    },
    SystemFontFile {
        file_name: "HarmonyOS_Sans_TC.ttf",
        family: "HarmonyOS Sans TC",
        essential: true,
    },
    SystemFontFile {
        file_name: "HMOSColorEmojiCompat.ttf",
        family: "HMOS Color Emoji",
        essential: false,
    },
    SystemFontFile {
        file_name: "HMOSColorEmojiFlags.ttf",
        family: "HMOS Color Emoji Flags",
        essential: false,
    },
    SystemFontFile {
        file_name: "HarmonyOS_Sans_Digit.ttf",
        family: "HarmonyOS Sans Digit",
        essential: false,
    },
    SystemFontFile {
        file_name: "HarmonyOS_Sans_Condensed.ttf",
        family: "HarmonyOS Sans Condensed",
        essential: false,
    },
    SystemFontFile {
        file_name: "NotoSerif[wdth,wght].ttf",
        family: "Noto Serif",
        essential: false,
    },
    SystemFontFile {
        file_name: "HYQiHeiL3.ttf",
        family: "HYQiHei L3",
        essential: false,
    },
];

/// 一次字体装载的结果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontLoadReport {
    /// 实际读取并交给 cosmic-text 的文件名（保持回退顺序）。
    pub loaded: Vec<&'static str>,
    /// 不存在的文件。
    pub missing: Vec<&'static str>,
    /// 存在但读取失败的 `(文件名, 错误)`。
    pub failed: Vec<(&'static str, String)>,
}

impl FontLoadReport {
    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }
}

/// 在 `root` 下按 [`SYSTEM_FONT_FILES`] 的顺序挑出存在的文件。
///
/// 纯文件系统逻辑，宿主测试用临时目录覆盖。
pub fn existing_font_files(root: &Path) -> (Vec<(&'static SystemFontFile, PathBuf)>, Vec<&'static str>) {
    let mut present = Vec::new();
    let mut missing = Vec::new();
    for file in SYSTEM_FONT_FILES {
        let path = root.join(file.file_name);
        if path.is_file() {
            present.push((file, path));
        } else {
            missing.push(file.file_name);
        }
    }
    (present, missing)
}

/// 把 `root` 下的系统字体装进 `text_system`。
///
/// 返回报告；调用方通常只记日志。`text_system.add_fonts` 接受
/// `Vec<Cow<'static, [u8]>>`，一次调用即可，cosmic-text 会按传入顺序
/// 建立 family 匹配（见地图 §5.3 第 2 条）。
pub fn load_system_fonts(
    text_system: &dyn PlatformTextSystem,
    root: &Path,
) -> FontLoadReport {
    let (present, missing) = existing_font_files(root);
    let mut report = FontLoadReport {
        missing,
        ..FontLoadReport::default()
    };

    let mut fonts: Vec<Cow<'static, [u8]>> = Vec::with_capacity(present.len());
    for (file, path) in present {
        match std::fs::read(&path) {
            Ok(bytes) => {
                if is_emoji_font(file.file_name) && !font_has_cbdt_tables(&bytes) {
                    // swash 渲染 CBDT/CBLC 位图与 COLR v0；COLR v1 不行。
                    log::warn!(
                        "{} has no CBDT table (COLR v1?); emoji may not render",
                        file.file_name
                    );
                }
                log::debug!(
                    "loaded system font {} ({} bytes, family {})",
                    file.file_name,
                    bytes.len(),
                    file.family
                );
                report.loaded.push(file.file_name);
                fonts.push(Cow::Owned(bytes));
            }
            Err(error) => {
                log::warn!("failed to read {}: {error}", path.display());
                report.failed.push((file.file_name, error.to_string()));
            }
        }
    }

    for file_name in &report.missing {
        let essential = SYSTEM_FONT_FILES
            .iter()
            .any(|file| file.file_name == *file_name && file.essential);
        if essential {
            log::warn!("essential system font missing: {file_name}");
        } else {
            log::debug!("optional system font missing: {file_name}");
        }
    }

    if !fonts.is_empty()
        && let Err(error) = text_system.add_fonts(fonts)
    {
        log::error!("failed to add HarmonyOS system fonts: {error:#}");
    }

    report
}

fn is_emoji_font(file_name: &str) -> bool {
    file_name.contains("Emoji")
}

/// 字体文件是否含 CBDT（Color Bitmap Data Table）表。
///
/// swash 能渲染 CBDT 位图 emoji；COLR v1（Android 13+/新 HarmonyOS 用的格式）
/// 不能。这里只读 12 字节 offset table + 每条 16 字节的 table record。
pub fn font_has_cbdt_tables(data: &[u8]) -> bool {
    if data.len() < 12 {
        return false;
    }
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    let table_dir_end = 12 + num_tables.saturating_mul(16);
    if data.len() < table_dir_end {
        return false;
    }
    (0..num_tables).any(|index| {
        let offset = 12 + index * 16;
        &data[offset..offset + 4] == b"CBDT"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gpui-ohos-fonts-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ui_and_mono_fonts_are_the_expected_families() {
        assert_eq!(UI_FONT, "HarmonyOS Sans");
        assert_eq!(MONO_FONT, "Noto Sans Mono");
        // 这两个族名必须出现在文件表里，否则主题会回退到不存在的字体。
        assert!(
            SYSTEM_FONT_FILES
                .iter()
                .any(|file| file.family == UI_FONT)
        );
        assert!(
            SYSTEM_FONT_FILES
                .iter()
                .any(|file| file.family == MONO_FONT)
        );
    }

    #[test]
    fn load_order_keeps_latin_before_cjk_and_emoji() {
        let names: Vec<&str> = SYSTEM_FONT_FILES.iter().map(|f| f.file_name).collect();
        let latin = names
            .iter()
            .position(|name| *name == "HarmonyOS_Sans.ttf")
            .unwrap();
        let sc = names
            .iter()
            .position(|name| *name == "HarmonyOS_Sans_SC.ttf")
            .unwrap();
        let tc = names
            .iter()
            .position(|name| *name == "HarmonyOS_Sans_TC.ttf")
            .unwrap();
        let emoji = names
            .iter()
            .position(|name| *name == "HMOSColorEmojiCompat.ttf")
            .unwrap();
        assert!(latin < sc && latin < tc);
        assert!(sc < emoji && tc < emoji);
        assert!(names.contains(&"NotoSansMono[wdth,wght].ttf"));
    }

    #[test]
    fn missing_fonts_are_skipped_and_existing_ones_kept_in_order() {
        let dir = unique_temp_dir("order");
        fs::write(dir.join("HarmonyOS_Sans.ttf"), [0u8; 4]).unwrap();
        fs::write(dir.join("HarmonyOS_Sans_TC.ttf"), [0u8; 4]).unwrap();

        let (present, missing) = existing_font_files(&dir);
        assert_eq!(
            present
                .iter()
                .map(|(file, _)| file.file_name)
                .collect::<Vec<_>>(),
            vec!["HarmonyOS_Sans.ttf", "HarmonyOS_Sans_TC.ttf"]
        );
        assert!(missing.contains(&"HarmonyOS_Sans_SC.ttf"));
        // 顺序必须等于文件表顺序，而不是目录枚举顺序。
        assert!(present[0].0.file_name == "HarmonyOS_Sans.ttf");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loader_reports_loaded_and_missing_files() {
        let dir = unique_temp_dir("load");
        fs::write(dir.join("HarmonyOS_Sans.ttf"), [1u8; 8]).unwrap();
        fs::write(dir.join("HarmonyOS_Sans_SC.ttf"), [2u8; 16]).unwrap();

        // NoopTextSystem 接受 add_fonts 调用但什么都不做；这里只验证选择逻辑。
        let text_system = gpui::NoopTextSystem::new();
        let report = load_system_fonts(&text_system, &dir);

        assert_eq!(
            report.loaded,
            vec!["HarmonyOS_Sans.ttf", "HarmonyOS_Sans_SC.ttf"]
        );
        assert!(report.missing.contains(&"HarmonyOS_Sans_TC.ttf"));
        assert!(report.failed.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_directory_yields_no_fonts() {
        let text_system = gpui::NoopTextSystem::new();
        let report = load_system_fonts(&text_system, Path::new("Z:/definitely/missing"));
        assert!(report.is_empty());
        assert_eq!(report.missing.len(), SYSTEM_FONT_FILES.len());
    }

    #[test]
    fn cbdt_detection_reads_the_table_directory() {
        // 12 字节头 + 2 条 16 字节记录；第二条是 CBDT。
        let mut font = vec![0u8; 12 + 32];
        font[4..6].copy_from_slice(&2u16.to_be_bytes());
        font[12..16].copy_from_slice(b"glyf");
        font[28..32].copy_from_slice(b"CBDT");
        assert!(font_has_cbdt_tables(&font));

        font[28..32].copy_from_slice(b"COLR");
        assert!(!font_has_cbdt_tables(&font));
        assert!(!font_has_cbdt_tables(&font[..10]));
    }
}
