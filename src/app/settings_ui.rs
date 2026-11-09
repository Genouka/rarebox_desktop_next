//! settings 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::shell_ui::page_scroll;
use super::{Page, RareBox};
use crate::config::{self, short_path};
use crate::theme;
use crate::ui::{self, ActionKind, MONO_FONT};
use crate::update;
use gpui::{
    AnyElement, App, Context, ElementId, FontWeight, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, div, prelude::*, px, rgb,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

impl RareBox {
    pub(crate) fn render_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let adb_ready = self.adb_path.is_some();
        let adb_path = self
            .adb_path
            .as_ref()
            .map(|path| short_path(path))
            .unwrap_or_else(|| "尚未检测到 ADB".to_string());
        let adb_version = self.adb_version.clone().unwrap_or_else(|| "—".to_string());
        let adb_error = self.adb_error.clone();
        let hdc_ready = self.harmony.path.is_some();
        let hdc_path = self
            .harmony
            .path
            .as_ref()
            .map(|path| short_path(path))
            .unwrap_or_else(|| "尚未检测到 hdc".to_string());
        let hdc_version = self
            .harmony
            .version
            .clone()
            .unwrap_or_else(|| "—".to_string());
        let hdc_error = self.harmony.error.clone();
        let media_dir = self.config.media_dir();
        let config_file = config::config_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "（无法确定配置目录）".to_string());
        let checking = self.update.checking;
        let update_hint = match self.update.status.as_ref() {
            Some(status) if status.update_available => format!(
                "发现新版本 {}",
                status.latest_version.clone().unwrap_or_default()
            ),
            _ => format!("当前版本 {}", update::APP_VERSION),
        };
        let weak = cx.entity().downgrade();

        let appearance = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "外观",
                Some("动态色与 Material 3 Expressive"),
                None,
            ))
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.on_surface)
                                    .child("深色模式"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(colors.on_surface_variant)
                                    .child(format!(
                                        "当前：{}",
                                        if self.config.dark { "深色" } else { "浅色" }
                                    )),
                            ),
                    )
                    .child(self.dark_switch.clone()),
            )
            .child(
                div()
                    .mt(px(18.))
                    .pt(px(16.))
                    .border_t_1()
                    .border_color(colors.outline_variant.opacity(0.4))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.on_surface)
                            .child("强调色"),
                    )
                    .child(div().mt(px(10.)).flex().flex_wrap().gap(px(8.)).children(
                        theme::SEEDS.iter().map(|(seed, name)| {
                            seed_chip(cx, weak.clone(), *seed, name, self.config.seed == *seed)
                        }),
                    )),
            );

        let adb_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "ADB",
                Some("RareBox 会自动查找 platform-tools"),
                None,
            ))
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .child(ui::status_dot(if adb_ready {
                        colors.tertiary
                    } else {
                        colors.error
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.on_surface)
                                    .child(if adb_ready {
                                        "ADB 已就绪"
                                    } else {
                                        "ADB 未找到"
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_family(MONO_FONT)
                                    .text_color(colors.on_surface_variant)
                                    .truncate()
                                    .child(format!("{adb_path} · {adb_version}")),
                            ),
                    )
                    .child(ui::action_button(
                        cx,
                        "settings-adb-apply",
                        "应用并重连",
                        Some("refresh"),
                        ActionKind::Filled,
                        true,
                        cx.listener(|app, _, _, cx| app.apply_adb_settings(cx)),
                    )),
            )
            .child(div().mt(px(12.)).child(self.adb_field.clone()))
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::action_button(
                        cx,
                        "settings-adb-auto",
                        "自动检测",
                        Some("search"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.reset_adb_to_auto(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "settings-adb-open-config",
                        "打开配置目录",
                        Some("folder_open"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.open_config_dir(cx)),
                    )),
            )
            .when_some(adb_error, |el, error| {
                el.child(div().mt(px(10.)).child(ui::callout(
                    cx,
                    "warning",
                    format!("上次检测失败：{error}"),
                )))
            });

        let hdc_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "HDC（HarmonyOS）",
                Some(if cfg!(any(target_os = "android", target_os = "ios")) {
                    "Android 端已内置 hdc（libhdc.so）；留空时优先使用内置版本"
                } else {
                    "各平台已内置 hdc；留空时优先使用内置版本，也可指定外部路径"
                }),
                None,
            ))
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .child(ui::status_dot(if hdc_ready {
                        colors.tertiary
                    } else {
                        colors.error
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.on_surface)
                                    .child(if hdc_ready {
                                        "hdc 已就绪"
                                    } else {
                                        "hdc 未找到"
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_family(MONO_FONT)
                                    .text_color(colors.on_surface_variant)
                                    .truncate()
                                    .child(format!("{hdc_path} · {hdc_version}")),
                            ),
                    )
                    .child(ui::action_button(
                        cx,
                        "settings-hdc-apply",
                        "应用并重连",
                        Some("refresh"),
                        ActionKind::Filled,
                        true,
                        cx.listener(|app, _, _, cx| app.apply_hdc_settings(cx)),
                    )),
            )
            .child(div().mt(px(12.)).child(self.hdc_field.clone()))
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::action_button(
                        cx,
                        "settings-hdc-auto",
                        "自动检测",
                        Some("search"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.reset_hdc_to_auto(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "settings-hdc-open",
                        "打开 HarmonyOS 调试",
                        Some("devices"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.open_harmony_connect(cx)),
                    )),
            )
            .when_some(hdc_error, |el, error| {
                el.child(div().mt(px(10.)).child(ui::callout(
                    cx,
                    "warning",
                    format!("上次检测失败：{error}"),
                )))
            });

        let process_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "调试进程",
                Some("结束本机的 adb / hdc（hdb）后台服务"),
                None,
            ))
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::action_button(
                        cx,
                        "settings-kill-adb",
                        "结束 ADB 进程",
                        Some("stop_circle"),
                        ActionKind::Danger,
                        adb_ready,
                        cx.listener(|app, _, _, cx| app.kill_adb_process(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "settings-kill-hdc",
                        "结束 HDC 进程",
                        Some("stop_circle"),
                        ActionKind::Danger,
                        hdc_ready,
                        cx.listener(|app, _, _, cx| app.kill_hdc_process(cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(18.))
                    .pt(px(16.))
                    .border_t_1()
                    .border_color(colors.outline_variant.opacity(0.4))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.on_surface)
                                    .child("关闭程序时自动结束进程"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(colors.on_surface_variant)
                                    .child(format!(
                                        "默认开启；当前：{}",
                                        if self.config.kill_processes_on_exit {
                                            "开启"
                                        } else {
                                            "关闭"
                                        }
                                    )),
                            ),
                    )
                    .child(self.kill_on_exit_switch.clone()),
            )
            .child(div().mt(px(10.)).child(ui::callout(
                cx,
                "info",
                "结束进程后，下一次执行 ADB / HDC 操作时会自动重新启动服务；无线连接需要重新连接。",
            )));

        let media_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "截图与录屏",
                Some("本地保存位置"),
                None,
            ))
            .child(div().mt(px(16.)).child(self.screenshot_field.clone()))
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::action_button(
                        cx,
                        "settings-media-pick",
                        "选择目录",
                        Some("folder_open"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.pick_screenshot_dir(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "settings-media-open",
                        "打开目录",
                        Some("open_in_new"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.open_media_dir(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "settings-media-reset",
                        "恢复默认",
                        Some("restart_alt"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.reset_screenshot_dir(cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(10.))
                    .font_family(MONO_FONT)
                    .text_color(colors.on_surface_variant)
                    .truncate()
                    .child(short_path(&media_dir)),
            );

        let about = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "关于 RareBox Desktop",
                Some("Wear OS ADB 调试工具箱"),
                None,
            ))
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(info_line(
                        cx,
                        weak.clone(),
                        "版本",
                        update::APP_VERSION.to_string(),
                        false,
                    ))
                    .child(info_line(
                        cx,
                        weak.clone(),
                        "界面",
                        "Rust · GPUI · material3-gpui".to_string(),
                        false,
                    ))
                    .child(info_line(cx, weak.clone(), "ADB", adb_version, true))
                    .child(info_line(cx, weak.clone(), "HDC", hdc_version, true))
                    .child(info_line(
                        cx,
                        weak.clone(),
                        "远程控制",
                        format!(
                            "内置 scrcpy-server {}",
                            fadb_scrcpy::server::SCRCPY_VERSION
                        ),
                        true,
                    ))
                    .child(info_line(cx, weak.clone(), "配置", config_file, true))
                    .child(
                        div()
                            .pt(px(12.))
                            .border_t_1()
                            .border_color(colors.outline_variant.opacity(0.4))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w(px(0.))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(colors.on_surface)
                                            .child("软件更新"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(colors.on_surface_variant)
                                            .child(update_hint),
                                    ),
                            )
                            .child(ui::action_button(
                                cx,
                                "settings-check-update",
                                if checking { "检查中…" } else { "检查更新" },
                                Some("system_update"),
                                ActionKind::Tonal,
                                !checking,
                                cx.listener(|app, _, _, cx| app.check_updates(cx, true)),
                            )),
                    )
                    .child(ui::callout(
                        cx,
                        "verified",
                        "所有操作都直接调用 ADB 完成，不包含演示数据。设备需要开启 USB 调试并授权本机。",
                    )),
            );

        page_scroll(
            Page::Settings,
            &self.scroll_handles[Page::Settings as usize],
            div().w_full().flex().flex_col().gap(px(16.)).child(
                div()
                    .flex()
                    .gap(px(16.))
                    .when(!mobile, |el| el.items_start())
                    .when(mobile, |el| el.flex_col())
                    .child(
                        div()
                            .min_w(px(0.))
                            .when(!mobile, |el| el.flex_1())
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(appearance)
                            .child(media_card),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .when(!mobile, |el| el.flex_1())
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(adb_card)
                            .child(hdc_card)
                            .child(process_card)
                            .child(about),
                    ),
            ),
        )
        .into_any_element()
    }

    // ---------------------------------------------------------------------
    // ADB
    // ---------------------------------------------------------------------
}

fn seed_chip(
    cx: &App,
    weak: gpui::WeakEntity<RareBox>,
    seed: u32,
    name: &'static str,
    selected: bool,
) -> AnyElement {
    let colors = *cx.theme().colors();
    div()
        .id(("seed", seed as usize))
        .h(px(38.))
        .px(px(10.))
        .rounded(px(12.))
        .flex()
        .items_center()
        .gap(px(7.))
        .border_1()
        .border_color(if selected {
            colors.primary
        } else {
            colors.outline_variant
        })
        .bg(if selected {
            colors.primary.opacity(0.08)
        } else {
            colors.surface_container_low
        })
        .cursor_pointer()
        .hover(move |style| style.bg(colors.surface_container_high))
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .bg(gpui::Hsla::from(rgb(seed))),
        )
        .child(
            div()
                .text_size(px(11.))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if selected {
                    colors.primary
                } else {
                    colors.on_surface_variant
                })
                .child(name),
        )
        .when(selected, |el| {
            el.child(
                Icon::new(IconName::new("check"))
                    .size(px(13.))
                    .color(colors.primary),
            )
        })
        .on_click(move |_, _, cx| {
            if let Err(err) = weak.update(cx, |app, cx| app.set_seed(seed, cx)) {
                eprintln!("RareBox Desktop: 切换强调色失败: {err}");
            }
        })
        .into_any_element()
}

fn info_line(
    cx: &App,
    weak: gpui::WeakEntity<RareBox>,
    label: &'static str,
    value: String,
    mono: bool,
) -> AnyElement {
    let colors = *cx.theme().colors();
    let copy_value = value.clone();
    let id = ElementId::Name(format!("copy-settings-{label}").into());
    ui::copyable(
        div()
            .flex()
            .items_start()
            .gap(px(12.))
            .rounded(px(8.))
            .px(px(6.))
            .mx(px(-6.))
            .hover(move |style| style.bg(colors.surface_container))
            .child(
                div()
                    .w(px(52.))
                    .flex_none()
                    .text_size(px(10.5))
                    .text_color(colors.on_surface_variant)
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(colors.on_surface)
                    .when(mono, |el| el.font_family(MONO_FONT))
                    .child(value),
            ),
        weak,
        id,
        copy_value,
    )
    .into_any_element()
}
