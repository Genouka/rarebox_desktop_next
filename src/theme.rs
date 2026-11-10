//! RareBox Desktop 主题：动态色 + Material 3 Expressive 字阶与运动方案。

use gpui::App;
use gpui::SharedString;
use material3_gpui::motion::MotionScheme;
use material3_gpui::prelude::*;
use material3_gpui::theme::font_family;

/// 可选的品牌种子色。
pub const SEEDS: [(u32, &str); 5] = [
    (0x00_69_6D, "湖水蓝"),
    (0x2E_6C_4F, "森野绿"),
    (0x67_50_A4, "经典紫"),
    (0x8B_50_00, "琥珀橙"),
    (0x9C_40_4E, "莓果红"),
];

pub fn accent_name(seed: u32) -> &'static str {
    SEEDS
        .iter()
        .find(|(value, _)| *value == seed)
        .map(|(_, name)| *name)
        .unwrap_or("自定义")
}

pub fn apply(cx: &mut App, seed: u32, dark: bool) {
    Theme::set(cx, build(seed, dark));
    cx.refresh_windows();
}

pub fn build(seed: u32, dark: bool) -> Theme {
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    let base = Theme::from_seed(seed, mode, Profile::Baseline2021);
    let tokens = TokenSet::builder(Profile::Baseline2021, *base.colors())
        .with_motion(MotionScheme::expressive())
        .with_typography(TypeScale::expressive())
        .build();
    Theme::from_token_set(tokens, mode, ui_font_family())
}

fn ui_font_family() -> SharedString {
    #[cfg(target_os = "windows")]
    {
        font_family("Segoe UI")
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        font_family("SF Pro Text")
    }
    #[cfg(target_os = "android")]
    {
        // gpui-mobile 会加载 /system/fonts 下的 Roboto；Android 没有独立的
        // "Noto Sans" 常规字体文件（只有各语种与 CJK 变体）。
        font_family("Roboto")
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )))]
    {
        font_family("Noto Sans")
    }
}
