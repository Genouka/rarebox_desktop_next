//! screen 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::rotation_label;
use super::shell_ui::{device_required, page_scroll};
use super::{Page, RareBox};
use crate::adb::DeviceSnapshot;
use crate::config::short_path;
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, App, Context, Div, Entity, FontWeight, IntoElement, ObjectFit, ParentElement,
    Styled, div, img, prelude::*, px,
};

use super::guide_ui::guide_anchor;
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::interaction::BoundsHandle;
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;
use rarebox_core::guide::GuideTarget;

impl RareBox {
    pub(crate) fn render_screen(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_harmony() {
            return self.render_screen_harmony(cx);
        }
        let weak = cx.entity().downgrade();
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let Some(snapshot) = self.snapshot.clone() else {
            return page_scroll(
                Page::Screen,
                &self.scroll_handles[Page::Screen as usize],
                device_required(
                    cx,
                    weak,
                    "连接手表后控制屏幕",
                    "截图、录屏以及分辨率调整都需要一台已授权的 Wear OS 设备。",
                ),
            )
            .into_any_element();
        };

        let colors = *cx.theme().colors();
        let recording = self
            .activity
            .as_ref()
            .map(|activity| activity.label.contains("录制"))
            .unwrap_or(false);

        let screenshot = self.last_screenshot.clone();
        let recording_path = self.last_recording.clone();

        let preview: AnyElement = if let Some(path) = screenshot.clone() {
            div()
                .w_full()
                .h(px(300.))
                .rounded(px(18.))
                .overflow_hidden()
                .bg(colors.surface_container)
                .border_1()
                .border_color(colors.outline_variant.opacity(0.5))
                .child(img(path).w_full().h_full().object_fit(ObjectFit::Contain))
                .into_any_element()
        } else {
            ui::empty_block(
                cx,
                "screenshot_monitor",
                "还没有截图",
                "点击「截取屏幕」通过 adb exec-out screencap 抓取当前表盘，并自动保存到本机。",
                None,
            )
            .into_any_element()
        };

        let shot_buttons = div()
            .mt(px(14.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(ui::action_button(
                cx,
                "screen-shot",
                "截取屏幕",
                Some("screenshot_monitor"),
                ActionKind::Filled,
                true,
                cx.listener(|app, _, _, cx| app.take_screenshot(cx)),
            ))
            .child(ui::action_button(
                cx,
                "screen-record",
                "录制 10 秒",
                Some("videocam"),
                ActionKind::Tonal,
                !recording,
                cx.listener(|app, _, _, cx| app.record_screen(cx)),
            ))
            .child(div().flex_1())
            .when_some(screenshot, |el, path| {
                let open_path = path.clone();
                let reveal_path = path.clone();
                el.child(ui::action_button(
                    cx,
                    "screen-open-shot",
                    "打开截图",
                    Some("open_in_new"),
                    ActionKind::Text,
                    true,
                    move |_, _, cx| cx.open_with_system(&open_path),
                ))
                .child(ui::action_button(
                    cx,
                    "screen-reveal-shot",
                    "文件夹",
                    Some("folder_open"),
                    ActionKind::Text,
                    true,
                    move |_, _, cx| cx.reveal_path(reveal_path.parent().unwrap_or(&reveal_path)),
                ))
            });

        let preview_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "屏幕预览",
                Some("截图会保存到本地，不会写入手表相册"),
                None,
            ))
            .child(div().mt(px(14.)).child(preview))
            .child(shot_buttons)
            .when_some(
                self.last_screenshot.as_ref().map(|path| short_path(path)),
                |el, path| {
                    el.child(
                        div()
                            .mt(px(8.))
                            .text_size(px(10.5))
                            .font_family(MONO_FONT)
                            .text_color(colors.on_surface_variant.opacity(0.8))
                            .child(path),
                    )
                },
            )
            .when_some(recording_path, |el, path| {
                let open = ui::action_button(
                    cx,
                    "screen-open-record",
                    "打开录屏",
                    Some("open_in_new"),
                    ActionKind::Text,
                    true,
                    {
                        let path = path.clone();
                        move |_, _, cx| cx.open_with_system(&path)
                    },
                );
                let reveal = ui::action_button(
                    cx,
                    "screen-reveal-record",
                    "文件夹",
                    Some("folder_open"),
                    ActionKind::Text,
                    true,
                    {
                        let path = path.clone();
                        move |_, _, cx| cx.reveal_path(&path)
                    },
                );
                el.child(
                    div()
                        .mt(px(10.))
                        .p(px(10.))
                        .rounded(px(12.))
                        .bg(colors.surface_container)
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .child(
                            Icon::new(IconName::new("videocam"))
                                .size(px(17.))
                                .color(colors.primary),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(px(0.))
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(colors.on_surface_variant)
                                        .child("最近录屏"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .font_family(MONO_FONT)
                                        .text_color(colors.on_surface)
                                        .truncate()
                                        .child(short_path(&path)),
                                ),
                        )
                        .child(open)
                        .child(reveal),
                )
            });

        let preview_card = guide_anchor(
            &self.guide.anchors.handle(GuideTarget::ScreenScreenshot),
            preview_card,
        );

        let keys = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "按键控制",
                Some("向设备发送 input keyevent"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .child(key_button(
                                cx,
                                "key-back",
                                "返回",
                                "4",
                                "arrow_back",
                                weak.clone(),
                            ))
                            .child(key_button(
                                cx,
                                "key-home",
                                "主页",
                                "3",
                                "home",
                                weak.clone(),
                            ))
                            .child(key_button(
                                cx,
                                "key-power",
                                "电源",
                                "26",
                                "power_settings_new",
                                weak.clone(),
                            ))
                            .child(key_button(
                                cx,
                                "key-wake",
                                "唤醒",
                                "224",
                                "bolt",
                                weak.clone(),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .child(key_button(
                                cx,
                                "key-vol-up",
                                "音量 +",
                                "24",
                                "add",
                                weak.clone(),
                            ))
                            .child(key_button(
                                cx,
                                "key-vol-down",
                                "音量 -",
                                "25",
                                "remove",
                                weak.clone(),
                            ))
                            .child(key_button(
                                cx,
                                "key-recents",
                                "最近任务",
                                "187",
                                "apps",
                                weak.clone(),
                            ))
                            .child(key_button(
                                cx,
                                "key-menu",
                                "菜单",
                                "82",
                                "list",
                                weak.clone(),
                            )),
                    ),
            );

        let text_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "文本输入",
                Some("将键盘文本发送到当前输入框"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .items_end()
                    .gap(px(10.))
                    .child(div().flex_1().child(self.screen_text_field.clone()))
                    .child(ui::action_button(
                        cx,
                        "screen-send-text",
                        "发送",
                        Some("keyboard"),
                        ActionKind::Filled,
                        true,
                        cx.listener(|app, _, _, cx| app.send_text(cx)),
                    )),
            );

        let display = display_card(cx, weak.clone(), &snapshot);
        let controls = controls_card(
            cx,
            &snapshot,
            weak.clone(),
            self.screen_size_field.clone(),
            self.screen_density_field.clone(),
            self.guide.anchors.handle(GuideTarget::ScreenDensityRow),
            self.guide.anchors.handle(GuideTarget::ScreenApplyDensity),
            self.guide.anchors.handle(GuideTarget::ScreenResetDensity),
        );

        let left = div()
            .min_w(px(0.))
            .when(!mobile, |el| el.flex_1())
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(preview_card)
            .child(keys)
            .child(text_card);
        let right = div()
            .min_w(px(0.))
            .when(!mobile, |el| el.flex_1())
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(display)
            .child(controls);

        page_scroll(
            Page::Screen,
            &self.scroll_handles[Page::Screen as usize],
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(colors.on_surface_variant)
                        .child(format!(
                            "当前显示 {} · {} DPI · 旋转 {}",
                            snapshot.display.size(),
                            snapshot.display.density(),
                            rotation_label(snapshot.display.rotation)
                        )),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .gap(px(16.))
                        .when(!mobile, |el| el.items_start())
                        .when(mobile, |el| el.flex_col())
                        .child(left)
                        .child(right),
                ),
        )
        .into_any_element()
    }

    // ---------------------------------------------------------------------
    // 截图与录屏
    // ---------------------------------------------------------------------
}

fn key_button(
    cx: &App,
    id: &'static str,
    label: &'static str,
    code: &'static str,
    icon: &'static str,
    weak: gpui::WeakEntity<RareBox>,
) -> Div {
    div()
        .flex_1()
        .min_w(px(140.))
        .flex()
        .justify_center()
        .child(ui::action_button(
            cx,
            id,
            label,
            Some(icon),
            ActionKind::Outlined,
            true,
            move |_, _, cx| {
                let Ok(code) = code.parse::<u32>() else {
                    return;
                };
                if let Err(err) = weak.update(cx, |app, cx| app.send_key(code, label, cx)) {
                    eprintln!("RareBox Desktop: 发送按键失败: {err}");
                }
            },
        ))
}

fn display_card(cx: &App, weak: gpui::WeakEntity<RareBox>, snapshot: &DeviceSnapshot) -> Card {
    Card::new()
        .outlined()
        .p(px(20.))
        .child(ui::section_header(
            cx,
            "显示参数",
            Some("来自 wm size / wm density"),
            None,
        ))
        .child(
            div()
                .mt(px(16.))
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(
                    div()
                        .flex()
                        .gap(px(20.))
                        .child(
                            ui::key_value(
                                cx,
                                weak.clone(),
                                "当前分辨率",
                                snapshot.display.size().to_string(),
                                true,
                            )
                            .flex_1(),
                        )
                        .child(
                            ui::key_value(
                                cx,
                                weak.clone(),
                                "当前 DPI",
                                snapshot.display.density().to_string(),
                                true,
                            )
                            .flex_1(),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(20.))
                        .child(
                            ui::key_value(
                                cx,
                                weak.clone(),
                                "物理分辨率",
                                snapshot.display.physical_size.clone(),
                                true,
                            )
                            .flex_1(),
                        )
                        .child(
                            ui::key_value(
                                cx,
                                weak.clone(),
                                "物理 DPI",
                                snapshot.display.physical_density.clone(),
                                true,
                            )
                            .flex_1(),
                        ),
                )
                .when(
                    snapshot.display.override_size.is_some()
                        || snapshot.display.override_density.is_some(),
                    |el| {
                        el.child(ui::callout(
                            cx,
                            "tune",
                            "检测到显示覆盖值。修改后如果手表界面异常，点击「恢复物理值」即可。",
                        ))
                    },
                ),
        )
}

fn controls_card(
    cx: &App,
    snapshot: &DeviceSnapshot,
    weak: gpui::WeakEntity<RareBox>,
    size_field: Entity<TextFieldState>,
    density_field: Entity<TextFieldState>,
    density_anchor: BoundsHandle,
    apply_anchor: BoundsHandle,
    reset_anchor: BoundsHandle,
) -> Card {
    let colors = *cx.theme().colors();
    let rotation = snapshot.display.rotation;
    let rotation_chips = [0u8, 1, 2, 3].iter().map(|value| {
        let value = *value;
        let weak = weak.clone();
        ui::filter_chip(
            cx,
            ("rotation", value as usize),
            rotation_label(value),
            rotation == value,
            move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| app.set_rotation(value, cx)) {
                    eprintln!("RareBox Desktop: 旋转屏幕失败: {err}");
                }
            },
        )
    });

    let apply_size = {
        let weak = weak.clone();
        ui::action_button(
            cx,
            "display-apply-size",
            "应用",
            Some("check"),
            ActionKind::Filled,
            true,
            move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| app.apply_display_size(cx)) {
                    eprintln!("RareBox Desktop: 应用分辨率失败: {err}");
                }
            },
        )
    };

    Card::new()
        .outlined()
        .p(px(20.))
        .child(ui::section_header(
            cx,
            "分辨率与密度",
            Some("修改后会自动刷新设备信息"),
            None,
        ))
        .child(
            div()
                .mt(px(16.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_end()
                        .gap(px(10.))
                        .child(div().flex_1().min_w(px(0.)).child(size_field))
                        .child(apply_size)
                        .child(ui::action_button(
                            cx,
                            "display-reset-size",
                            "恢复物理值",
                            Some("restart_alt"),
                            ActionKind::Outlined,
                            true,
                            {
                                let weak = weak.clone();
                                move |_, _, cx| {
                                    if let Err(err) =
                                        weak.update(cx, |app, cx| app.reset_display_size(cx))
                                    {
                                        eprintln!("RareBox Desktop: 重置分辨率失败: {err}");
                                    }
                                }
                            },
                        )),
                )
                .child(guide_anchor(
                    &density_anchor,
                    div()
                        .flex()
                        .items_end()
                        .gap(px(10.))
                        .child(div().flex_1().min_w(px(0.)).child(density_field))
                        .child(guide_anchor(
                            &apply_anchor,
                            ui::action_button(
                                cx,
                                "display-apply-density",
                                "应用",
                                Some("check"),
                                ActionKind::Filled,
                                true,
                                {
                                    let weak = weak.clone();
                                    move |_, _, cx| {
                                        if let Err(err) =
                                            weak.update(cx, |app, cx| app.apply_density(cx))
                                        {
                                            eprintln!("RareBox Desktop: 应用 DPI 失败: {err}");
                                        }
                                    }
                                },
                            ),
                        ))
                        .child(guide_anchor(
                            &reset_anchor,
                            ui::action_button(
                                cx,
                                "display-reset-density",
                                "恢复物理值",
                                Some("restart_alt"),
                                ActionKind::Outlined,
                                true,
                                {
                                    let weak = weak.clone();
                                    move |_, _, cx| {
                                        if let Err(err) =
                                            weak.update(cx, |app, cx| app.reset_density(cx))
                                        {
                                            eprintln!("RareBox Desktop: 重置 DPI 失败: {err}");
                                        }
                                    }
                                },
                            ),
                        )),
                ))
            )
        .child(
            div()
                .mt(px(16.))
                .pt(px(14.))
                .border_t_1()
                .border_color(colors.outline_variant.opacity(0.4))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.on_surface)
                        .child("屏幕旋转"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .children(rotation_chips),
                )
                .child(ui::callout(
                    cx,
                    "info",
                    "旋转会关闭自动旋转（accelerometer_rotation）。重新开启自动旋转可执行 settings put system accelerometer_rotation 1。",
                )),
        )
}
