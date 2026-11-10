//! remote 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::shell_ui::{device_required, page_scroll};
use super::{KEYCODE_APP_SWITCH, KEYCODE_BACK, KEYCODE_HOME, KEYCODE_POWER};
use super::{Page, RareBox, RemotePhase};
use crate::ui::{self, ActionKind};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, SharedString, Styled, div, img, prelude::*, px,
};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

use super::guide_ui::guide_anchor;
use rarebox_core::guide::GuideTarget;

impl RareBox {
    pub(crate) fn render_remote(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_harmony() {
            return page_scroll(
                Page::Remote,
                &self.scroll_handles[Page::Remote as usize],
                ui::empty_block(
                    cx,
                    "settings_remote",
                    "远程投屏仅支持 Android 设备",
                    "实时镜像依赖 scrcpy，HarmonyOS 设备请使用「屏幕与显示」页的截图与 uitest 控制。",
                    Some(
                        ui::action_button(
                            cx,
                            "remote-hdc-screen",
                            "打开屏幕与显示",
                            Some("screenshot_monitor"),
                            ActionKind::Tonal,
                            true,
                            cx.listener(|app, _, _, cx| app.set_page(Page::Screen, cx)),
                        )
                        .into_any_element(),
                    ),
                ),
            )
            .into_any_element();
        }
        if !self.has_device() {
            return page_scroll(
                Page::Remote,
                &self.scroll_handles[Page::Remote as usize],
                device_required(
                    cx,
                    cx.entity().downgrade(),
                    "连接手表后开始远程控制",
                    "实时镜像依赖设备上的 scrcpy-server，请先通过无线调试连接 Wear OS 手表。",
                ),
            )
            .into_any_element();
        }

        let colors = *cx.theme().colors();
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let phase = self.remote_phase;
        let running = phase == RemotePhase::Running;
        let frame = self.remote_frame.clone();
        let size = self.remote_size;
        let error = self.remote_error.clone();
        let text_len = self.remote_text_input.chars().count();

        let (status_color, status_text) = match phase {
            RemotePhase::Running => (colors.tertiary, "投屏中"),
            RemotePhase::Starting => (colors.primary, "正在连接"),
            RemotePhase::Failed => (colors.error, "启动失败"),
            RemotePhase::Idle => (colors.on_surface_variant, "未启动"),
        };
        let status_chip = || {
            div()
                .h(px(28.))
                .px(px(11.))
                .rounded_full()
                .bg(colors.surface_container)
                .flex()
                .items_center()
                .gap(px(6.))
                .child(ui::status_dot(status_color))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.on_surface_variant)
                        .child(status_text),
                )
        };

        let start_label = if phase == RemotePhase::Failed {
            "重新开始"
        } else if phase == RemotePhase::Starting {
            "正在启动…"
        } else {
            "启动远程控制"
        };
        let start_button = || {
            ui::action_button(
                cx,
                "remote-start",
                start_label,
                Some(if phase == RemotePhase::Failed {
                    "refresh"
                } else {
                    "play_arrow"
                }),
                ActionKind::Filled,
                !running && phase != RemotePhase::Starting,
                cx.listener(|app, _, _, cx| app.start_remote(cx)),
            )
        };
        let stop_button = || {
            ui::action_button(
                cx,
                "remote-stop",
                "停止",
                Some("stop_circle"),
                ActionKind::Outlined,
                running,
                cx.listener(|app, _, _, cx| app.stop_remote(cx)),
            )
        };
        let restart_button = || {
            ui::action_button(
                cx,
                "remote-restart",
                "重新启动",
                Some("refresh"),
                ActionKind::Text,
                running,
                cx.listener(|app, _, _, cx| app.restart_remote(cx)),
            )
        };

        let trailing = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(status_chip())
            .when(!running, |el| el.child(start_button()))
            .when(running, |el| {
                el.child(restart_button()).child(stop_button())
            })
            .into_any_element();
        let mobile_actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(status_chip())
            .when(!running, |el| el.child(start_button()))
            .when(running, |el| {
                el.child(restart_button()).child(stop_button())
            });

        let placeholder: AnyElement = {
            let description: SharedString = match phase {
                RemotePhase::Starting => {
                    "正在上传内置 scrcpy-server 并等待视频流，通常需要几秒钟。".into()
                }
                RemotePhase::Failed => error
                    .clone()
                    .unwrap_or_else(|| "scrcpy 会话启动失败，请检查设备连接后重试。".to_string())
                    .into(),
                RemotePhase::Running => "scrcpy-server 已连接，正在等待 H.264 视频流。".into(),
                RemotePhase::Idle => {
                    "点击「启动远程控制」后，画面会直接显示在这里，不会打开独立的 scrcpy 窗口。"
                        .into()
                }
            };
            let (icon, title): (&'static str, &'static str) = match phase {
                RemotePhase::Starting => ("settings_remote", "正在建立远程会话"),
                RemotePhase::Failed => ("error", "远程控制未启动"),
                RemotePhase::Running => ("settings_remote", "等待第一帧画面"),
                RemotePhase::Idle => ("settings_remote", "在应用内镜像手表"),
            };
            let mut block = div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .child(ui::empty_block(cx, icon, title, description, None));
            if phase == RemotePhase::Starting {
                block = block.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(LoadingIndicator::new("remote-loading").size(px(20.)))
                        .child(
                            div()
                                .text_size(px(11.5))
                                .text_color(colors.on_surface_variant)
                                .child("正在准备 scrcpy-server"),
                        ),
                );
            }
            block.into_any_element()
        };

        let viewport_side = if mobile { 300.0 } else { 460.0 };
        let scale = if size.0 > 0 && size.1 > 0 {
            (viewport_side / f32::from(size.0)).min(viewport_side / f32::from(size.1))
        } else {
            1.0
        };
        let (display_width, display_height) =
            (f32::from(size.0) * scale, f32::from(size.1) * scale);

        let mut viewport = div()
            .id("remote-viewport")
            .relative()
            .w(px(viewport_side))
            .h(px(viewport_side))
            .flex_shrink_0()
            .rounded(px(20.))
            .overflow_hidden()
            .border_1()
            .border_color(colors.outline_variant.opacity(0.5))
            .bg(colors.surface_container_lowest)
            .when(frame.is_some(), |el| el.bg(gpui::black()))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer();

        if let Some(frame) = frame {
            viewport = viewport.child(
                img(frame)
                    .w(px(display_width))
                    .h(px(display_height))
                    .flex_shrink_0(),
            );
        } else {
            viewport = viewport.child(placeholder);
        }

        viewport = viewport
            .child(self.remote_bounds.capture_element())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|app, event: &MouseDownEvent, _, cx| {
                    app.remote_pointer_down(event.position, cx);
                }),
            )
            .on_mouse_move(cx.listener(|app, event: &MouseMoveEvent, _, cx| {
                app.remote_pointer_move(event.position, event.pressed_button, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|app, event: &MouseUpEvent, _, cx| {
                    app.remote_pointer_up(event.position, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|app, event: &MouseUpEvent, _, cx| {
                    app.remote_pointer_up(event.position, cx);
                }),
            );

        let key_weak = cx.entity().downgrade();
        let key_button = |id: &'static str,
                          label: &'static str,
                          icon: &'static str,
                          keycode: u32|
         -> AnyElement {
            let weak = key_weak.clone();
            ui::action_button(
                cx,
                id,
                label,
                Some(icon),
                ActionKind::Outlined,
                running,
                move |_, _, cx| {
                    if let Err(err) = weak.update(cx, |app, cx| {
                        app.send_remote_key(keycode, cx);
                    }) {
                        eprintln!("RareBox Desktop: 发送远程按键失败: {err}");
                    }
                },
            )
            .into_any_element()
        };

        let key_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(key_button(
                "remote-back",
                "返回",
                "arrow_back",
                KEYCODE_BACK,
            ))
            .child(key_button("remote-home", "主页", "home", KEYCODE_HOME))
            .child(key_button(
                "remote-recents",
                "最近任务",
                "apps",
                KEYCODE_APP_SWITCH,
            ))
            .child(key_button(
                "remote-power",
                "电源",
                "power_settings_new",
                KEYCODE_POWER,
            ));

        let text_send = ui::action_button(
            cx,
            "remote-send-text",
            "发送",
            Some("keyboard"),
            ActionKind::Tonal,
            running && text_len > 0,
            cx.listener(|app, _, _, cx| app.send_remote_text(cx)),
        );
        let text_row = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(self.remote_text_field.clone()),
            )
            .child(text_send);

        let size_text = if size.0 > 0 && size.1 > 0 {
            format!("{} × {}", size.0, size.1)
        } else {
            "等待视频元数据".to_string()
        };
        let server_text = format!(
            "H.264 · 内置 server {}",
            fadb_scrcpy::server::SCRCPY_VERSION
        );
        let detail = |label: &'static str, value: String, mono: bool| {
            let mut value_el = div()
                .w_full()
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.on_surface)
                .child(value);
            if mono {
                value_el = value_el.font_family(ui::MONO_FONT);
            }
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(colors.on_surface_variant)
                        .child(label),
                )
                .child(value_el)
        };
        let panel_label = |text: &'static str| {
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.on_surface)
                .child(text)
        };

        let side_panel = div()
            .min_w(px(0.))
            .when(!mobile, |el| el.flex_1())
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(detail("采集分辨率", size_text, true))
                    .child(detail("视频编码", server_text, false)),
            )
            .child(panel_label("设备按键"))
            .child(key_row)
            .child(panel_label("文字输入"))
            .child(text_row)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(colors.on_surface_variant.opacity(0.85))
                    .child(if running {
                        "点击画面发送触控，按住并拖动可模拟滑动手势。"
                    } else {
                        "启动后可使用鼠标点击、拖动，或发送设备按键与文字。"
                    }),
            )
            .child(ui::callout(
                cx,
                "info",
                "画面由内置 scrcpy-server 采集，H.264 在应用内解码，无需在本机安装 scrcpy。",
            ));

        let content = div().w_full().flex().flex_col().gap(px(16.)).child(
            Card::new()
                .outlined()
                .p(px(20.))
                .child(ui::section_header(
                    cx,
                    "实时投屏",
                    Some("镜像、触控与按键都通过内置 scrcpy-server 完成"),
                    if mobile { None } else { Some(trailing) },
                ))
                .when(mobile, |el| {
                    el.child(div().mt(px(12.)).child(mobile_actions))
                })
                .child(
                    div()
                        .mt(px(16.))
                        .flex()
                        .gap(px(18.))
                        .when(!mobile, |el| el.items_start())
                        .when(mobile, |el| el.flex_col())
                        .child(viewport)
                        .child(side_panel),
                ),
        );

        let content = guide_anchor(
            &self.guide.anchors.handle(GuideTarget::RemoteStart),
            content,
        );

        page_scroll(
            Page::Remote,
            &self.scroll_handles[Page::Remote as usize],
            content,
        )
        .into_any_element()
    }
}
