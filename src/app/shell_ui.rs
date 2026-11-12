//! 窗口骨架渲染（UI 层）：侧栏、顶栏、页面调度、弹窗与 toast。

use super::guide_ui::ui_page_to_guide;
use super::{KindMode, LegalTab, Page, RareBox, ToastKind};
use crate::adb::Device;
use crate::update;
use crate::{theme, titlebar, ui};
use gpui::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, MouseButton, ParentElement, Render,
    ScrollHandle, StatefulInteractiveElement as _, Styled, WeakEntity, Window, div, prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::overlay::{MenuItem, MenuState, show_menu};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;
use rarebox_core::guide::GuideTarget;
use ui::{ActionKind, MONO_FONT};

impl RareBox {
    /// 协议选择：自动识别（推荐）/ Android / HarmonyOS。
    ///
    /// 用户通常分不清手表跑的是哪个系统，默认自动：哪边探测到在线设备就用哪边。
    pub(crate) fn transport_switch(&self, cx: &mut Context<Self>) -> Div {
        let harmony = self.is_harmony();
        let mode = self.kind_mode;
        let resolved = if harmony { "HarmonyOS" } else { "Android" };
        let anchor = self
            .guide
            .anchors
            .handle(GuideTarget::ConnectTransportSwitch);
        div()
            .relative()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(8.))
            .child(ui::filter_chip(
                cx,
                "transport-auto",
                format!("自动 · {resolved}"),
                mode == KindMode::Auto,
                cx.listener(|app, _, _, cx| app.set_kind_mode(KindMode::Auto, cx)),
            ))
            .child(ui::filter_chip(
                cx,
                "transport-android",
                "Android",
                mode == KindMode::Android,
                cx.listener(|app, _, _, cx| app.set_kind_mode(KindMode::Android, cx)),
            ))
            .child(ui::filter_chip(
                cx,
                "transport-harmony",
                "HarmonyOS",
                mode == KindMode::Harmony,
                cx.listener(|app, _, _, cx| app.set_kind_mode(KindMode::Harmony, cx)),
            ))
            .child(anchor.capture_element())
    }

    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let typography = *cx.theme().typography();
        let current_page = self.page;
        let harmony = self.is_harmony();

        let nav = Page::ALL
            .iter()
            .filter(move |page| match **page {
                Page::Remote => !harmony,
                Page::Signing => harmony,
                Page::Tools => !harmony,
                _ => true,
            })
            .map(|page| {
                let page = *page;
                let selected = page == current_page;
                let anchor = (page != Page::Guide).then(|| {
                    self.guide
                        .anchors
                        .handle(GuideTarget::Nav(ui_page_to_guide(page)))
                });
                let (background, foreground) = if selected {
                    (colors.secondary_container, colors.on_secondary_container)
                } else {
                    (colors.surface_container_low, colors.on_surface_variant)
                };
                let hover = colors.surface_container_high;
                div()
                    .id(("nav-item", page as usize))
                    .relative()
                    .h(px(42.))
                    .w_full()
                    .px(px(12.))
                    .rounded(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .bg(background)
                    .text_color(foreground)
                    .cursor_pointer()
                    .when(!selected, |el| {
                        el.hover(move |style| style.bg(hover))
                            .active(move |style| style.bg(colors.surface_container_highest))
                    })
                    .child(
                        Icon::new(IconName::new(page.icon()))
                            .size(px(20.))
                            .color(foreground),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(if selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .child(page.label()),
                    )
                    .when(selected, |el| {
                        el.child(div().flex_1()).child(
                            div()
                                .size(px(5.))
                                .rounded_full()
                                .bg(colors.on_secondary_container),
                        )
                    })
                    .when_some(anchor, |el, anchor| el.child(anchor.capture_element()))
                    .on_click(cx.listener(move |app, _, _, cx| app.set_page(page, cx)))
            });

        let adb_ready = self.adb_path.is_some();
        let adb_text = if adb_ready {
            self.adb_version
                .clone()
                .unwrap_or_else(|| "ADB 已就绪".to_string())
        } else {
            "ADB 未找到".to_string()
        };
        let hdc_ready = self.harmony.path.is_some();
        let hdc_path = self
            .harmony
            .path
            .as_ref()
            .map(|path| crate::config::short_path(path))
            .unwrap_or_else(|| {
                if self.is_harmony() {
                    "在设置中指定 hdc 路径".to_string()
                } else {
                    "点击切换到 HarmonyOS 工作区".to_string()
                }
            });
        // 更新弹窗被关闭后，左下角保留一个胶囊提醒，直到用户再次打开或更新完成。
        let update_pill = if self.update.dismissed {
            self.update
                .status
                .as_ref()
                .filter(|status| status.update_available)
                .and_then(|status| status.latest_version.clone())
        } else {
            None
        };

        div()
            .w(px(232.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .id("sidebar-scroll")
            .when(cfg!(any(target_os = "android", target_os = "ios")), |el| {
                el.overflow_y_scroll().restrict_scroll_to_axis()
            })
            .bg(colors.surface_container_low)
            .border_r_1()
            .border_color(colors.outline_variant.opacity(0.55))
            .child(
                div().px(px(16.)).pt(px(18.)).pb(px(8.)).child(
                    typography
                        .label_small
                        .apply(div())
                        .text_color(colors.on_surface_variant)
                        .child("工作台"),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(12.))
                    .children(nav),
            )
            .child(div().flex_1())
            .child(
                div()
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when_some(update_pill, |el, version| {
                        el.child(
                            div()
                                .id("sidebar-update-pill")
                                .px(px(12.))
                                .py(px(9.))
                                .rounded_full()
                                .bg(colors.primary_container)
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .cursor_pointer()
                                .hover(move |style| {
                                    style.bg(colors.primary_container.opacity(0.82))
                                })
                                .on_click(cx.listener(|app, _, _, cx| app.open_update_dialog(cx)))
                                .child(
                                    Icon::new(IconName::new("system_update"))
                                        .size(px(16.))
                                        .color(colors.on_primary_container),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(colors.on_primary_container)
                                        .truncate()
                                        .child(format!("发现新版本 {version}")),
                                )
                                .child(
                                    Icon::new(IconName::new("chevron_right"))
                                        .size(px(15.))
                                        .color(colors.on_primary_container),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id("sidebar-theme")
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(14.))
                            .bg(colors.surface_container)
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                Icon::new(IconName::new(if self.config.dark {
                                    "dark_mode"
                                } else {
                                    "light_mode"
                                }))
                                .size(px(18.))
                                .color(colors.primary),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(colors.on_surface)
                                            .child(if self.config.dark {
                                                "深色模式"
                                            } else {
                                                "浅色模式"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .text_color(colors.on_surface_variant)
                                            .child(theme::accent_name(self.config.seed)),
                                    ),
                            )
                            .child(ui::mini_switch(
                                cx,
                                self.config.dark,
                                cx.listener(|app, _, window, cx| app.toggle_dark(window, cx)),
                            )),
                    )
                    .child(
                        div()
                            .id("sidebar-adb")
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(14.))
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .cursor_pointer()
                            .hover(move |style| style.bg(colors.surface_container))
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.set_page(Page::Settings, cx);
                            }))
                            .child(ui::status_dot(if adb_ready {
                                colors.tertiary
                            } else {
                                colors.error
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(11.5))
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
                                            .text_color(colors.on_surface_variant)
                                            .truncate()
                                            .child(adb_text),
                                    ),
                            )
                            .child(
                                Icon::new(IconName::new("chevron_right"))
                                    .size(px(15.))
                                    .color(colors.on_surface_variant),
                            ),
                    )
                    .child(
                        div()
                            .id("sidebar-hdc")
                            .px(px(12.))
                            .py(px(10.))
                            .rounded(px(14.))
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .cursor_pointer()
                            .hover(move |style| style.bg(colors.surface_container))
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.open_harmony_connect(cx);
                            }))
                            .child(ui::status_dot(if hdc_ready {
                                colors.tertiary
                            } else {
                                colors.error
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(colors.on_surface)
                                            .child(if hdc_ready {
                                                "HDC 已就绪"
                                            } else {
                                                "HDC 未找到"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(colors.on_surface_variant)
                                            .truncate()
                                            .child(hdc_path),
                                    ),
                            )
                            .child(
                                Icon::new(IconName::new("chevron_right"))
                                    .size(px(15.))
                                    .color(colors.on_surface_variant),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_topbar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        let typography = *cx.theme().typography();
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let page = self.page;
        let harmony_mode = self.is_harmony();
        let current = self.current_device().cloned();
        let harmony_current = self.harmony_device().cloned();
        let busy = self.activity.clone();
        let weak = cx.entity().downgrade();
        let devices = self.devices.clone();
        let selected_serial = self.selected_serial.clone();
        let harmony_devices = self.harmony.devices.clone();
        let harmony_selected = self.harmony.selected.clone();
        let harmony_selected_menu = harmony_selected.clone();
        let harmony_name = self
            .harmony
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.title())
            .or_else(|| harmony_selected.clone())
            .unwrap_or_else(|| "未连接 HarmonyOS 设备".to_string());
        let harmony_detail = self
            .harmony
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.identity())
            .or_else(|| harmony_selected.clone())
            .unwrap_or_else(|| "连接设备以开始调试".to_string());

        let device_name = if harmony_mode {
            harmony_name
        } else {
            current
                .as_ref()
                .map(Device::display_name)
                .unwrap_or_else(|| "未连接设备".to_string())
        };
        let device_detail = if harmony_mode {
            harmony_detail
        } else {
            current
                .as_ref()
                .map(|device| device.serial.clone())
                .unwrap_or_else(|| "连接手表以开始调试".to_string())
        };
        let state_color = if harmony_mode {
            harmony_current
                .as_ref()
                .filter(|device| device.is_online())
                .map(|_| colors.tertiary)
                .unwrap_or(colors.on_surface_variant.opacity(0.45))
        } else {
            current
                .as_ref()
                .filter(|device| device.is_online())
                .map(|_| colors.tertiary)
                .unwrap_or(colors.on_surface_variant.opacity(0.45))
        };
        let state_label = if harmony_mode {
            harmony_current
                .as_ref()
                .map(|device| device.state.label())
                .unwrap_or("未连接")
        } else {
            current
                .as_ref()
                .map(|device| device.state_label())
                .unwrap_or("未连接")
        };

        let device_anchor = self.guide.anchors.handle(GuideTarget::TopbarDevice);
        let selector = div()
            .id("device-selector")
            .relative()
            .h(px(46.))
            .when(!mobile, |el| el.flex_none())
            .when(mobile, |el| el.flex_1().min_w_0())
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .rounded(px(14.))
            .border_1()
            .border_color(colors.outline_variant)
            .bg(colors.surface_container_low)
            .cursor_pointer()
            .hover(move |style| style.bg(colors.surface_container))
            .active(move |style| style.bg(colors.surface_container_high))
            .child(
                div()
                    .size(px(32.))
                    .rounded(px(10.))
                    .bg(colors.primary_container)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::new(if harmony_mode {
                            "devices"
                        } else {
                            current
                                .as_ref()
                                .map(|device| device.connection.icon())
                                .unwrap_or("watch")
                        }))
                        .size(px(18.))
                        .color(colors.on_primary_container),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .max_w(px(if mobile { 140. } else { 200. }))
                    .min_w(px(if mobile { 72. } else { 108. }))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.on_surface)
                            .truncate()
                            .child(device_name),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_family(MONO_FONT)
                            .text_color(colors.on_surface_variant)
                            .truncate()
                            .child(device_detail),
                    ),
            )
            .when(!mobile, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .child(ui::status_dot(state_color))
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(colors.on_surface_variant)
                                .child(state_label),
                        ),
                )
                .child(
                    Icon::new(IconName::new("keyboard_arrow_down"))
                        .size(px(18.))
                        .color(colors.on_surface_variant),
                )
            })
            .child(device_anchor.capture_element())
            .on_click(move |event, window, cx| {
                let mut menu = MenuState::new();
                let mut item_count = 0;
                {
                    let weak = weak.clone();
                    menu = menu.item(
                        MenuItem::new("刷新 Android 设备（ADB）")
                            .icon(IconName::new("refresh"))
                            .on_click(move |_, cx| {
                                if let Err(err) = weak.update(cx, |app, cx| app.refresh_devices(cx))
                                {
                                    eprintln!("RareBox Desktop: 刷新设备失败: {err}");
                                }
                            }),
                    );
                    item_count += 1;
                }
                {
                    let weak = weak.clone();
                    menu = menu.item(
                        MenuItem::new("刷新 HarmonyOS 设备（hdc）")
                            .icon(IconName::new("refresh"))
                            .on_click(move |_, cx| {
                                if let Err(err) = weak.update(cx, |app, cx| app.refresh_hdc(cx)) {
                                    eprintln!("RareBox Desktop: 刷新鸿蒙设备失败: {err}");
                                }
                            }),
                    );
                    item_count += 1;
                }
                if devices.is_empty() && harmony_devices.is_empty() {
                    menu = menu.item(
                        MenuItem::new("未发现设备")
                            .icon(IconName::new("usb_off"))
                            .on_click(|_, _| {}),
                    );
                    item_count += 1;
                }
                if !devices.is_empty() {
                    menu = menu.item(MenuItem::new("Android（ADB）").icon(IconName::new("watch")));
                    item_count += 1;
                    for device in devices.iter() {
                        let serial = device.serial.clone();
                        let weak = weak.clone();
                        let checked = selected_serial.as_deref() == Some(device.serial.as_str());
                        let label = format!(
                            "{} · {}{}",
                            device.display_name(),
                            device.state_label(),
                            if checked { "（当前）" } else { "" }
                        );
                        menu = menu.item(
                            MenuItem::new(label)
                                .icon(IconName::new(device.connection.icon()))
                                .on_click(move |_, cx| {
                                    if let Err(err) = weak.update(cx, |app, cx| {
                                        app.select_device(serial.clone(), cx);
                                    }) {
                                        eprintln!("RareBox Desktop: 切换设备失败: {err}");
                                    }
                                }),
                        );
                        item_count += 1;
                    }
                }
                if !harmony_devices.is_empty() {
                    menu = menu.item(
                        MenuItem::new("HarmonyOS / OpenHarmony（hdc）")
                            .icon(IconName::new("devices")),
                    );
                    item_count += 1;
                    for device in harmony_devices.iter() {
                        let key = device.key.clone();
                        let weak = weak.clone();
                        let checked = harmony_selected_menu.as_deref() == Some(device.key.as_str());
                        let label = format!(
                            "{} · {}{}",
                            device.display_name(),
                            device.connection.label(),
                            if checked { "（当前）" } else { "" }
                        );
                        menu = menu.item(
                            MenuItem::new(label)
                                .icon(IconName::new(device.connection.icon()))
                                .on_click(move |_, cx| {
                                    if let Err(err) = weak.update(cx, |app, cx| {
                                        app.select_hdc_device(key.clone(), cx);
                                    }) {
                                        eprintln!("RareBox Desktop: 切换鸿蒙设备失败: {err}");
                                    }
                                }),
                        );
                        item_count += 1;
                    }
                }
                let menu = menu.build(cx);
                let menu_height = item_count as f32 * 48. + 16.;
                show_menu(
                    window,
                    cx,
                    menu,
                    ui::menu_anchor(window, event.position(), 320., menu_height),
                );
            });

        let refresh = if harmony_mode {
            ui::icon_button(
                cx,
                "topbar-refresh-hdc",
                "refresh",
                38.,
                colors.on_surface_variant,
                cx.listener(|app, _, _, cx| app.refresh_hdc(cx)),
            )
            .into_any_element()
        } else {
            ui::icon_button(
                cx,
                "topbar-refresh",
                "refresh",
                38.,
                colors.on_surface_variant,
                cx.listener(|app, _, _, cx| app.refresh_devices(cx)),
            )
            .into_any_element()
        };

        let announcements = ui::icon_button(
            cx,
            "topbar-announcements",
            "notifications",
            38.,
            colors.on_surface_variant,
            cx.listener(|app, _, _, cx| app.open_announcements(cx)),
        );

        let reboot_weak = cx.entity().downgrade();
        let reboot = if mobile {
            let weak = cx.entity().downgrade();
            let harmony = harmony_mode;
            ui::icon_button(
                cx,
                "topbar-reboot-mobile",
                "restart_alt",
                38.,
                colors.on_surface_variant,
                move |event, window, cx| {
                    if let Err(err) = weak.update(cx, |app, cx| {
                        if harmony {
                            app.open_hdc_reboot_menu(event.position(), window, cx);
                        } else {
                            app.open_reboot_menu(event.position(), window, cx);
                        }
                    }) {
                        eprintln!("RareBox Desktop: 打开重启菜单失败: {err}");
                    }
                },
            )
            .into_any_element()
        } else if harmony_mode {
            let weak = cx.entity().downgrade();
            ui::action_button(
                cx,
                "topbar-hdc-reboot",
                "重启",
                Some("restart_alt"),
                ActionKind::Tonal,
                harmony_selected.is_some(),
                move |event, window, cx| {
                    if let Err(err) = weak.update(cx, |app, cx| {
                        app.open_hdc_reboot_menu(event.position(), window, cx);
                    }) {
                        eprintln!("RareBox Desktop: 打开鸿蒙重启菜单失败: {err}");
                    }
                },
            )
            .into_any_element()
        } else {
            ui::action_button(
                cx,
                "topbar-reboot",
                "重启",
                Some("restart_alt"),
                ActionKind::Tonal,
                current.as_ref().is_some_and(Device::is_online),
                move |event, window, cx| {
                    if let Err(err) = reboot_weak.update(cx, |app, cx| {
                        app.open_reboot_menu(event.position(), window, cx);
                    }) {
                        eprintln!("RareBox Desktop: 打开重启菜单失败: {err}");
                    }
                },
            )
            .into_any_element()
        };

        div()
            .h(px(if mobile { 60. } else { 76. }))
            .flex_none()
            .px(px(if mobile { 10. } else { 28. }))
            .flex()
            .items_center()
            .gap(px(if mobile { 6. } else { 14. }))
            .bg(colors.surface)
            .border_b_1()
            .border_color(colors.outline_variant.opacity(0.45))
            .when(!mobile, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            typography
                                .headline_small
                                .apply(div())
                                .text_color(colors.on_surface)
                                .child(page.label()),
                        )
                        .child(
                            typography
                                .body_small
                                .apply(div())
                                .text_color(colors.on_surface_variant)
                                .truncate()
                                .child(page.subtitle()),
                        ),
                )
            })
            .when_some(busy, |el, busy| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .h(px(32.))
                        .rounded_full()
                        .bg(colors.surface_container)
                        .child(LoadingIndicator::new("topbar-busy").size(px(15.)))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(colors.on_surface_variant)
                                .child(busy.label),
                        ),
                )
            })
            .child(selector)
            .when(!mobile, |el| el.child(announcements))
            .child(refresh)
            .child(reboot)
    }

    pub(crate) fn render_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match self.page {
            Page::Guide => self.render_guide(cx),
            Page::Overview => self.render_overview(cx),
            Page::Connect => self.render_connect(cx),
            Page::Apps => self.render_apps(cx),
            Page::Screen => self.render_screen(cx),
            Page::Remote => self.render_remote(cx),
            Page::Files => self.render_files(cx),
            Page::Terminal => self.render_terminal(cx),
            Page::Logs => self.render_logs(cx),
            Page::Tools => self.render_debug(cx),
            Page::Signing => self.render_signing(cx),
            Page::Settings => self.render_settings(cx),
        }
    }

    pub(crate) fn render_toasts(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        div()
            .absolute()
            .right(px(24.))
            .bottom(px(24.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(self.toasts.iter().map(|toast| {
                let id = toast.id;
                let (accent, icon) = match toast.kind {
                    ToastKind::Info => (colors.primary, "info"),
                    ToastKind::Success => (colors.tertiary, "check_circle"),
                    ToastKind::Error => (colors.error, "error"),
                };
                div()
                    .id(("toast", id as usize))
                    .occlude()
                    .max_w(px(420.))
                    .px(px(14.))
                    .py(px(11.))
                    .rounded(px(14.))
                    .bg(colors.inverse_surface)
                    .border_1()
                    .border_color(colors.outline_variant.opacity(0.25))
                    .flex()
                    .items_start()
                    .gap(px(9.))
                    .child(
                        div()
                            .mt(px(1.))
                            .child(Icon::new(IconName::new(icon)).size(px(17.)).color(accent)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(colors.inverse_on_surface)
                            .child(toast.message.clone()),
                    )
                    .child(
                        div()
                            .id(("toast-close", id as usize))
                            .size(px(20.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(move |style| style.bg(colors.inverse_on_surface.opacity(0.1)))
                            .child(
                                Icon::new(IconName::new("close"))
                                    .size(px(13.))
                                    .color(colors.inverse_on_surface.opacity(0.7)),
                            )
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.toasts.retain(|toast| toast.id != id);
                                cx.notify();
                            })),
                    )
            }))
    }

    pub(crate) fn render_confirm_dialog(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        div().when_some(self.confirm.clone(), |el, confirm| {
            let cancel = ui::action_button(
                cx,
                "confirm-cancel",
                "取消",
                None,
                ActionKind::Text,
                true,
                cx.listener(|app, _, _, cx| {
                    app.confirm = None;
                    cx.notify();
                }),
            );
            let kind = confirm.kind.clone();
            let weak = cx.entity().downgrade();
            let accept = ui::action_button(
                cx,
                "confirm-accept",
                confirm.confirm_label.clone(),
                Some("check"),
                ActionKind::Danger,
                true,
                move |_, _, cx| {
                    if let Err(err) = weak.update(cx, |app, cx| {
                        app.confirm = None;
                        app.execute_confirm(kind.clone(), cx);
                    }) {
                        eprintln!("RareBox Desktop: 执行确认操作失败: {err}");
                    }
                },
            );
            let weak = cx.entity().downgrade();
            el.child(
                Dialog::new("rarebox-confirm")
                    .icon(IconName::new("warning"))
                    .title(confirm.title.clone())
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(colors.on_surface_variant)
                            .child(confirm.message.clone()),
                    )
                    .action(cancel)
                    .action(accept)
                    .on_dismiss(move |_, cx| {
                        if let Err(err) = weak.update(cx, |app, cx| {
                            app.confirm = None;
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 关闭确认框失败: {err}");
                        }
                    }),
            )
        })
    }

    pub(crate) fn render_legal_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = *cx.theme().colors();
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let viewport = window.viewport_size();
        div().when_some(self.legal.clone(), |el, modal| {
            let body = match modal.tab {
                LegalTab::Eula => modal.eula.clone(),
                LegalTab::Privacy => modal.privacy.clone(),
            };
            let eula = ui::action_button(
                cx,
                "legal-tab-eula",
                "最终用户许可协议",
                None,
                if modal.tab == LegalTab::Eula {
                    ActionKind::Tonal
                } else {
                    ActionKind::Text
                },
                true,
                cx.listener(move |app, _, _, cx| app.set_legal_tab(LegalTab::Eula, cx)),
            );
            let privacy = ui::action_button(
                cx,
                "legal-tab-privacy",
                "隐私政策",
                None,
                if modal.tab == LegalTab::Privacy {
                    ActionKind::Tonal
                } else {
                    ActionKind::Text
                },
                true,
                cx.listener(move |app, _, _, cx| app.set_legal_tab(LegalTab::Privacy, cx)),
            );
            let accept = ui::action_button(
                cx,
                "legal-accept",
                "同意并继续",
                Some("check"),
                ActionKind::Filled,
                true,
                cx.listener(|app, _, _, cx| app.accept_legal(cx)),
            );
            let decline = ui::action_button(
                cx,
                "legal-decline",
                "拒绝并退出",
                None,
                ActionKind::Text,
                true,
                cx.listener(|app, _, _, cx| app.decline_legal(cx)),
            );
            let card = div()
                .w(px(720.))
                .max_w(viewport.width - px(48.))
                .h(px(640.))
                .max_h(viewport.height - px(96.))
                .rounded(px(16.))
                .bg(colors.surface_container_high)
                .shadow_lg()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(20.))
                        .py(px(14.))
                        .border_b(px(1.))
                        .border_color(colors.outline_variant)
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight::BOLD)
                                .child("请阅读并同意以下条款"),
                        )
                        .child(
                            div()
                                .when(modal.loading, |d| {
                                    d.child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(colors.outline)
                                            .child("正在获取最新条款…"),
                                    )
                                })
                                .when_some(modal.error.clone(), |d, msg| {
                                    d.child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(colors.error)
                                            .child(msg),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .px(px(20.))
                        .pt(px(12.))
                        .child(eula)
                        .child(privacy),
                )
                .child(
                    div()
                        .id("legal-body")
                        .flex_1()
                        .overflow_y_scroll()
                        .restrict_scroll_to_axis()
                        .min_h_0()
                        .mx(px(20.))
                        .my(px(12.))
                        .p(px(16.))
                        .rounded(px(10.))
                        .bg(colors.surface_container)
                        .border(px(1.))
                        .border_color(colors.outline_variant)
                        .text_size(px(13.))
                        .child(body),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .px(px(20.))
                        .py(px(14.))
                        .border_t(px(1.))
                        .border_color(colors.outline_variant)
                        .when(!mobile, |el| {
                            el.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(colors.outline)
                                    .child("不同意将无法使用本软件。"),
                            )
                        })
                        .child(div().flex().gap(px(10.)).child(decline).child(accept)),
                );

            el.child(ui::modal_overlay(
                window,
                "legal-scrim",
                colors.scrim.opacity(0.6),
                card,
            ))
        })
    }

    /// 更新弹窗：有更新时展示版本对比、更新说明与下载入口。
    fn render_update_dialog(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        let open = self.update.dialog;
        let status = self.update.status.clone();
        div().when(open, |el| {
            el.when_some(status, |el, status| {
                let forced = status.forced();
                let current = status
                    .current_version
                    .clone()
                    .unwrap_or_else(|| update::APP_VERSION.to_string());
                let latest = status
                    .latest_version
                    .clone()
                    .unwrap_or_else(|| "未知".to_string());
                let notes = status
                    .notes_text()
                    .unwrap_or("此版本没有提供更新说明。")
                    .to_string();
                let mut meta = Vec::new();
                if let Some(published) = status.published_at.as_deref() {
                    meta.push(format!("发布于 {published}"));
                }
                if status.file_size > 0 {
                    meta.push(update::format_size(status.file_size));
                }
                let meta = meta.join(" · ");
                let download = ui::action_button(
                    cx,
                    "update-download",
                    if forced {
                        "立即更新"
                    } else {
                        "下载更新"
                    },
                    Some("download"),
                    ActionKind::Filled,
                    status.download_url.is_some(),
                    cx.listener(|app, _, _, cx| app.download_update(cx)),
                );

                let mut dialog = Dialog::new("rarebox-update")
                    .icon(IconName::new(if forced {
                        "warning"
                    } else {
                        "system_update"
                    }))
                    .title(if forced {
                        "需要更新"
                    } else {
                        "发现新版本"
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(10.))
                            .child(version_chip(cx, &current, false))
                            .child(
                                Icon::new(IconName::new("arrow_upward"))
                                    .size(px(18.))
                                    .color(colors.on_surface_variant),
                            )
                            .child(version_chip(cx, &latest, true)),
                    );
                if !meta.is_empty() {
                    dialog = dialog.child(
                        div()
                            .text_size(px(11.5))
                            .text_color(colors.on_surface_variant)
                            .child(meta),
                    );
                }
                dialog = dialog.child(
                    div()
                        .id("update-notes")
                        .max_h(px(240.))
                        .overflow_y_scroll()
                        .restrict_scroll_to_axis()
                        .p(px(12.))
                        .rounded(px(10.))
                        .bg(colors.surface_container)
                        .text_size(px(12.5))
                        .line_height(px(19.))
                        .text_color(colors.on_surface_variant)
                        .child(notes),
                );
                if forced {
                    dialog = dialog.child(ui::callout(
                        cx,
                        "warning",
                        "此版本为强制更新，请更新后继续使用。",
                    ));
                }
                dialog = dialog.action(download);

                if !forced {
                    let later = ui::action_button(
                        cx,
                        "update-later",
                        "稍后",
                        None,
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.close_update_dialog(cx)),
                    );
                    let weak = cx.entity().downgrade();
                    dialog = dialog.action(later).on_dismiss(move |_, cx| {
                        if let Err(err) = weak.update(cx, |app, cx| app.close_update_dialog(cx)) {
                            eprintln!("RareBox Desktop: 关闭更新弹窗失败: {err}");
                        }
                    });
                }

                el.child(dialog)
            })
        })
    }

    /// 公告弹窗：左侧列表可查看以往公告，右侧显示正文。
    fn render_announcements_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = *cx.theme().colors();
        let viewport = window.viewport_size();
        let Some(modal) = self.announcements.clone() else {
            return div().into_any_element();
        };
        let selected = modal
            .selected
            .and_then(|id| modal.items.iter().find(|item| item.id == id))
            .or_else(|| modal.items.first())
            .cloned();

        let list = div()
            .id("announcement-list")
            .w(px(248.))
            .flex_none()
            .overflow_y_scroll()
            .restrict_scroll_to_axis()
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .border_r_1()
            .border_color(colors.outline_variant)
            .children(modal.items.iter().map(|item| {
                let id = item.id;
                let active = selected.as_ref().is_some_and(|selected| selected.id == id);
                let hover = colors.surface_container_high;
                div()
                    .id(("announcement-item", id as usize))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(10.))
                    .bg(if active {
                        colors.secondary_container
                    } else {
                        colors.surface_container_high.opacity(0.0)
                    })
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .on_click(cx.listener(move |app, _, _, cx| app.select_announcement(id, cx)))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .text_color(if active {
                                colors.on_secondary_container
                            } else {
                                colors.on_surface
                            })
                            .truncate()
                            .child(item.title.clone()),
                    )
                    .child(
                        div()
                            .mt(px(3.))
                            .text_size(px(10.))
                            .text_color(colors.on_surface_variant)
                            .child(format!("{} · {}", item.level_label(), item.date())),
                    )
            }));

        let detail = match selected {
            Some(item) => {
                let (level_bg, level_fg) = match item.level.as_str() {
                    "warning" => (colors.tertiary_container, colors.on_tertiary_container),
                    "critical" => (colors.error_container, colors.on_error_container),
                    _ => (colors.primary_container, colors.on_primary_container),
                };
                div()
                    .id("announcement-body")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .restrict_scroll_to_axis()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.on_surface)
                            .child(item.title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(ui::tag(item.level_label(), level_bg, level_fg))
                            .when(!item.date().is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(colors.on_surface_variant)
                                        .child(item.date().to_string()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(13.))
                            .line_height(px(21.))
                            .text_color(colors.on_surface_variant)
                            .child(item.body.clone()),
                    )
            }
            None => div()
                .id("announcement-empty")
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(if modal.loading {
                    LoadingIndicator::new("announcement-loading")
                        .size(px(20.))
                        .into_any_element()
                } else {
                    Icon::new(IconName::new("notifications"))
                        .size(px(26.))
                        .color(colors.outline)
                        .into_any_element()
                })
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(colors.on_surface_variant)
                        .child(if modal.loading {
                            "正在获取公告…"
                        } else {
                            "暂无公告"
                        }),
                ),
        };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .px(px(20.))
            .py(px(14.))
            .border_b_1()
            .border_color(colors.outline_variant)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .child("公告"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.on_surface_variant)
                            .child("最新公告与以往公告"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(ui::icon_button(
                        cx,
                        "announcement-refresh",
                        "refresh",
                        34.,
                        colors.on_surface_variant,
                        cx.listener(|app, _, _, cx| app.refresh_announcements(cx)),
                    ))
                    .child(ui::icon_button(
                        cx,
                        "announcement-close",
                        "close",
                        34.,
                        colors.on_surface_variant,
                        cx.listener(|app, _, _, cx| app.close_announcements(cx)),
                    )),
            );

        let card = div()
            .w(px(760.))
            .max_w(viewport.width - px(48.))
            .h(px(600.))
            .max_h(viewport.height - px(96.))
            .rounded(px(16.))
            .bg(colors.surface_container_high)
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .when_some(modal.error.clone(), |el, error| {
                el.child(
                    div()
                        .px(px(20.))
                        .pt(px(12.))
                        .child(ui::callout(cx, "error", error)),
                )
            })
            .child(div().flex().flex_1().min_h_0().child(list).child(detail));

        ui::modal_overlay(
            window,
            "announcement-scrim",
            colors.scrim.opacity(0.6),
            card,
        )
        .into_any_element()
    }

    /// Android 顶栏：桌面标题栏在手机上会挤占空间，这里只保留菜单、标题与连接状态。
    fn render_mobile_appbar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        let connected = self.devices.len() + self.harmony.devices.len();
        let (status_color, status_label) = if connected > 0 {
            (colors.primary, format!("{connected} 台设备"))
        } else {
            (colors.outline, "未连接".to_string())
        };
        let weak = cx.entity().downgrade();

        div()
            .h(px(52.))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .px(px(8.))
            .gap(px(6.))
            .bg(colors.surface_container_low)
            .border_b_1()
            .border_color(colors.outline_variant.opacity(0.6))
            .child(ui::icon_button(
                cx,
                "mobile-sidebar-toggle",
                "list",
                40.,
                colors.on_surface_variant,
                move |_, _, cx| {
                    if let Err(err) = weak.update(cx, |app, cx| {
                        app.sidebar_open = !app.sidebar_open;
                        cx.notify();
                    }) {
                        eprintln!("RareBox Desktop: 切换侧栏失败: {err}");
                    }
                },
            ))
            .child(
                div()
                    .flex_none()
                    .size(px(28.))
                    .rounded(px(8.))
                    .bg(colors.primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::new("watch"))
                            .size(px(16.))
                            .color(colors.on_primary),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.page.label()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child(self.page.subtitle()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(div().size(px(7.)).rounded_full().bg(status_color))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.on_surface_variant)
                            .child(status_label),
                    ),
            )
    }
}

impl Render for RareBox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = *cx.theme().colors();
        let drawer_weak = cx.entity().downgrade();
        let sidebar_open = self.sidebar_open;
        self.drop_remote_frames(window, cx);
        let overlay = material3_gpui::overlay::host(window, cx);
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let titlebar: AnyElement = if mobile {
            self.render_mobile_appbar(cx).into_any_element()
        } else {
            titlebar::render(self.devices.len() + self.harmony.devices.len(), cx).into_any_element()
        };
        let sidebar = self.render_sidebar(cx);
        let topbar = self.render_topbar(cx);
        let page = self.render_page(cx);
        let guide = self.render_guide_overlay(window, cx);
        let toasts = self.render_toasts(cx);
        let dialog = self.render_confirm_dialog(cx);
        let update_dialog = self.render_update_dialog(cx);
        let announcements = self.render_announcements_modal(window, cx);
        let legal = self.render_legal_modal(window, cx);

        let (safe_top, safe_bottom, safe_left, safe_right) = if mobile {
            mobile_safe_area()
        } else {
            (0., 0., 0., 0.)
        };

        let (body, drawer) = if mobile {
            let drawer = if sidebar_open {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .bg(gpui::rgba(0x00000059))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                if let Err(err) = drawer_weak.update(cx, |app, cx| {
                                    app.sidebar_open = false;
                                    cx.notify();
                                }) {
                                    eprintln!("RareBox Desktop: 关闭侧栏失败: {err}");
                                }
                            }),
                    )
                    .child(
                        div()
                            .h_full()
                            .flex_none()
                            .pt(px(safe_top))
                            .pb(px(safe_bottom))
                            .pl(px(safe_left))
                            .pr(px(safe_right))
                            .child(sidebar),
                    )
                    .into_any_element()
            } else {
                div().into_any_element()
            };
            (
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(topbar)
                    .child(page),
                drawer,
            )
        } else {
            (
                div().flex_1().min_h_0().flex().child(sidebar).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(topbar)
                        .child(page),
                ),
                div().into_any_element(),
            )
        };

        div()
            .relative()
            .size_full()
            .bg(colors.surface)
            .text_color(colors.on_surface)
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .pt(px(safe_top))
                    .pb(px(safe_bottom))
                    .pl(px(safe_left))
                    .pr(px(safe_right))
                    .child(titlebar)
                    .child(body),
            )
            .child(drawer)
            .child(guide)
            .child(toasts)
            .child(dialog)
            .child(update_dialog)
            .child(announcements)
            .child(legal)
            .child(overlay)
    }
}

/// 页面统一使用的滚动容器。
///
/// 滚动条与滚动区是兄弟节点且排在其后：`Scrollbar` 在 prepaint 阶段读取被跟踪容器的
/// 窗口坐标，只有同帧、同层且位于其后才能拿到正确的滚动几何。
pub(crate) fn page_scroll(
    page: Page,
    handle: &ScrollHandle,
    content: impl IntoElement,
) -> impl IntoElement {
    let id = ("page-scroll", page as usize);
    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .relative()
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .id(id)
                .flex_1()
                .min_h_0()
                .w_full()
                // 换页时 id 不同，滚动位置按页独立保存。
                .overflow_y_scroll()
                .restrict_scroll_to_axis()
                .track_scroll(handle)
                .child(
                    div().w_full().flex().justify_center().child(
                        div()
                            .w_full()
                            .max_w(px(1120.))
                            .px(px(28.))
                            .py(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(18.))
                            .child(content),
                    ),
                ),
        )
        .child(Scrollbar::new(("page-scrollbar", page as usize), handle))
}

/// 通用空状态页面：找不到设备时，除了重试还要能把用户带到连接页。
pub(crate) fn device_required(
    cx: &App,
    weak: WeakEntity<RareBox>,
    title: &'static str,
    description: &'static str,
) -> Div {
    let refresh = ui::action_button(
        cx,
        "device-required-refresh",
        "重新检测设备",
        Some("refresh"),
        ActionKind::Outlined,
        true,
        {
            let weak = weak.clone();
            move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| app.refresh_devices(cx)) {
                    eprintln!("RareBox Desktop: 重新检测设备失败: {err}");
                }
            }
        },
    );
    let connect = ui::action_button(
        cx,
        "device-required-connect",
        "连接新设备",
        Some("cable"),
        ActionKind::Filled,
        true,
        move |_, _, cx| {
            if let Err(err) = weak.update(cx, |app, cx| app.set_page(Page::Connect, cx)) {
                eprintln!("RareBox Desktop: 打开连接页失败: {err}");
            }
        },
    );
    ui::empty_block(
        cx,
        "watch",
        title,
        description,
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(connect)
                .child(refresh)
                .into_any_element(),
        ),
    )
}

/// 更新弹窗中的版本号徽标。
fn version_chip(cx: &App, version: &str, latest: bool) -> Div {
    let colors = *cx.theme().colors();
    div()
        .px(px(12.))
        .py(px(6.))
        .rounded_full()
        .bg(if latest {
            colors.primary_container
        } else {
            colors.surface_container_highest
        })
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if latest {
            colors.on_primary_container
        } else {
            colors.on_surface_variant
        })
        .child(format!("v{version}"))
}

/// Android 根布局的安全区留白；桌面端无系统栏。
#[cfg(target_os = "android")]
fn mobile_safe_area() -> (f32, f32, f32, f32) {
    crate::android::safe_area_insets()
}

#[cfg(not(target_os = "android"))]
fn mobile_safe_area() -> (f32, f32, f32, f32) {
    (0., 0., 0., 0.)
}
