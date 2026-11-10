//! 共享页面中的 hdc 界面：连接、概览、屏幕与应用。

use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, ObjectFit, ParentElement,
    StatefulInteractiveElement as _, Styled, WeakEntity, div, img, prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

use super::HdcInstallReport;
use super::guide_ui::guide_anchor;
use super::shell_ui::page_scroll;
use super::{Confirm, ConfirmKind, Page, RareBox};
use crate::config::short_path;
use crate::hdc::{
    HdcBundleInfo, HdcDevice, HdcKey, HdcPackage, HdcRebootTarget, HdcSnapshot, HdcSwipeDirection,
    HdcTap,
};
use crate::ui::{self, ActionKind, MONO_FONT};
use rarebox_core::guide::GuideTarget;

impl RareBox {
    /// 连接页的 HarmonyOS（hdc）部分：检测状态、设备、网络连接与端口转发。
    pub(crate) fn render_connect_harmony(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let ready = self.harmony.path.is_some();
        let version = self
            .harmony
            .version
            .clone()
            .unwrap_or_else(|| "—".to_string());
        let path = self
            .harmony
            .path
            .as_ref()
            .map(|path| short_path(path))
            .unwrap_or_else(|| "尚未检测到 hdc".to_string());
        let error = self.harmony.error.clone();
        let switch = self.transport_switch(cx);

        let trailing = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ui::action_button(
                cx,
                "harmony-restart-server",
                "重启服务",
                Some("restart_alt"),
                ActionKind::Text,
                true,
                cx.listener(|app, _, _, cx| app.open_hdc_server_restart(cx)),
            ))
            .child(ui::action_button(
                cx,
                "harmony-settings",
                "HDC 设置",
                Some("settings"),
                ActionKind::Text,
                true,
                cx.listener(|app, _, _, cx| app.set_page(Page::Settings, cx)),
            ))
            .child(ui::action_button(
                cx,
                "harmony-refresh",
                "重新检测",
                Some("refresh"),
                ActionKind::Tonal,
                true,
                cx.listener(|app, _, _, cx| app.refresh_hdc(cx)),
            ))
            .into_any_element();

        let header = Card::new()
            .outlined()
            .p(px(18.))
            .child(ui::section_header(
                cx,
                "HarmonyOS（hdc）",
                Some(if cfg!(any(target_os = "android", target_os = "ios")) {
                    "使用 hdc 直连鸿蒙设备，可在设置中指定 hdc 路径"
                } else {
                    "不依赖 adb：Windows 使用内置 hdc，也可指定外部路径"
                }),
                if mobile { None } else { Some(trailing) },
            ))
            .when(mobile, |el| {
                el.child(
                    div()
                        .mt(px(12.))
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(ui::action_button(
                            cx,
                            "harmony-restart-server-mobile",
                            "重启服务",
                            Some("restart_alt"),
                            ActionKind::Text,
                            true,
                            cx.listener(|app, _, _, cx| app.open_hdc_server_restart(cx)),
                        ))
                        .child(ui::action_button(
                            cx,
                            "harmony-settings-mobile",
                            "HDC 设置",
                            Some("settings"),
                            ActionKind::Text,
                            true,
                            cx.listener(|app, _, _, cx| app.set_page(Page::Settings, cx)),
                        ))
                        .child(ui::action_button(
                            cx,
                            "harmony-refresh-mobile",
                            "重新检测",
                            Some("refresh"),
                            ActionKind::Tonal,
                            true,
                            cx.listener(|app, _, _, cx| app.refresh_hdc(cx)),
                        )),
                )
            })
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .child(ui::status_dot(if ready {
                        colors.tertiary
                    } else {
                        colors.error
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.on_surface)
                                    .child(if ready {
                                        "hdc 已就绪"
                                    } else {
                                        "未找到 hdc"
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_family(MONO_FONT)
                                    .text_color(colors.on_surface_variant)
                                    .truncate()
                                    .child(format!("{path} · {version}")),
                            ),
                    ),
            )
            .when_some(error, |el, error| {
                el.child(div().mt(px(10.)).child(ui::callout(
                    cx,
                    "warning",
                    format!("上次检测失败：{error}"),
                )))
            });

        let header = guide_anchor(
            &self.guide.anchors.handle(GuideTarget::ConnectHdcCard),
            header,
        );

        let body: AnyElement = if ready {
            self.render_harmony_connect_cards(cx)
        } else {
            let actions = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::action_button(
                    cx,
                    "harmony-empty-refresh",
                    "重新检测",
                    Some("refresh"),
                    ActionKind::Tonal,
                    true,
                    cx.listener(|app, _, _, cx| app.refresh_hdc(cx)),
                ))
                .child(ui::action_button(
                    cx,
                    "harmony-empty-settings",
                    "打开设置",
                    Some("settings"),
                    ActionKind::Text,
                    true,
                    cx.listener(|app, _, _, cx| app.set_page(Page::Settings, cx)),
                ))
                .into_any_element();
            ui::empty_block(
                cx,
                "devices",
                "未检测到 hdc",
                if cfg!(any(target_os = "android", target_os = "ios")) {
                    "鸿蒙调试使用 HarmonyOS Device Connector（hdc）。Android 端已内置 hdc（libhdc.so），可通过 USB OTG 直连鸿蒙设备（系统弹窗允许 USB 权限），也可用 hdc tconn 无线连接；还能在设置中指定外部 hdc。"
                } else {
                    "鸿蒙调试使用 HarmonyOS Device Connector（hdc）。各平台都已内置 hdc；解包或加载失败时会回退到 DevEco Studio / OpenHarmony SDK 的 toolchains 目录，也可在设置中指定外部路径。"
                },
                Some(actions),
            )
            .into_any_element()
        };

        page_scroll(
            Page::Connect,
            &self.scroll_handles[Page::Connect as usize],
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(switch)
                .child(header)
                .child(body),
        )
        .into_any_element()
    }

    /// 概览页的 HarmonyOS（hdc）内容：设备信息与常用入口。
    pub(crate) fn render_harmony_overview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let weak = cx.entity().downgrade();
        let selected = self.harmony.selected.clone();
        let snapshot = self.harmony.snapshot.clone();
        let snapshot_at = self.harmony.snapshot_at.clone();
        let screenshot = self.harmony.screenshot.clone();

        if selected.is_none() {
            let actions = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::action_button(
                    cx,
                    "hdc-overview-connect",
                    "连接 HarmonyOS 设备",
                    Some("cable"),
                    ActionKind::Filled,
                    true,
                    cx.listener(|app, _, _, cx| app.set_page(Page::Connect, cx)),
                ))
                .child(ui::action_button(
                    cx,
                    "hdc-overview-refresh",
                    "重新检测",
                    Some("refresh"),
                    ActionKind::Outlined,
                    true,
                    cx.listener(|app, _, _, cx| app.refresh_hdc(cx)),
                ))
                .into_any_element();
            return page_scroll(
                Page::Overview,
                &self.scroll_handles[Page::Overview as usize],
                ui::empty_block(
                    cx,
                    "devices",
                    "还没有连接 HarmonyOS 设备",
                    "在连接页选择 HarmonyOS（hdc），通过 USB 授权设备或使用网络连接；设备就绪后这里会显示型号、电量、存储与运行时间。",
                    Some(actions),
                ),
            )
            .into_any_element();
        }

        let info_card: AnyElement = if let Some(snapshot) = snapshot {
            harmony_info_card(cx, &snapshot, snapshot_at, weak)
        } else {
            Card::new()
                .outlined()
                .p(px(20.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(LoadingIndicator::new("harmony-info-loading").size(px(16.)))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(colors.on_surface_variant)
                                .child("正在读取设备信息…"),
                        ),
                )
                .into_any_element()
        };

        let actions = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "快捷操作",
                Some("截图与电源操作都通过 hdc 完成"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::action_button(
                        cx,
                        "hdc-overview-shot",
                        "截取屏幕",
                        Some("screenshot_monitor"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.hdc_screenshot(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "hdc-overview-apps",
                        "管理应用",
                        Some("apps"),
                        ActionKind::Outlined,
                        true,
                        cx.listener(|app, _, _, cx| app.set_page(Page::Apps, cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "hdc-overview-files",
                        "浏览文件",
                        Some("folder_open"),
                        ActionKind::Outlined,
                        true,
                        cx.listener(|app, _, _, cx| app.set_page(Page::Files, cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "hdc-overview-terminal",
                        "打开终端",
                        Some("terminal"),
                        ActionKind::Outlined,
                        true,
                        cx.listener(|app, _, _, cx| app.set_page(Page::Terminal, cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "hdc-overview-reboot",
                        "重启",
                        Some("restart_alt"),
                        ActionKind::Outlined,
                        true,
                        cx.listener(|app, _, _, cx| {
                            app.open_hdc_reboot(HdcRebootTarget::System, cx)
                        }),
                    ))
                    .child(ui::action_button(
                        cx,
                        "hdc-overview-shutdown",
                        "关机",
                        Some("power_settings_new"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| {
                            app.open_hdc_reboot(HdcRebootTarget::Shutdown, cx)
                        }),
                    )),
            )
            .when_some(screenshot, |el, path| {
                el.child(
                    div()
                        .mt(px(14.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .size(px(72.))
                                .flex_none()
                                .rounded(px(12.))
                                .overflow_hidden()
                                .bg(colors.surface_container)
                                .child(
                                    img(path.clone())
                                        .w_full()
                                        .h_full()
                                        .object_fit(ObjectFit::Cover),
                                ),
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
                                        .child("最近截图"),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .font_family(MONO_FONT)
                                        .text_color(colors.on_surface)
                                        .truncate()
                                        .child(short_path(&path)),
                                ),
                        )
                        .child(ui::action_button(
                            cx,
                            "hdc-overview-shot-open",
                            "打开",
                            Some("open_in_new"),
                            ActionKind::Text,
                            true,
                            {
                                let path = path.clone();
                                move |_, _, cx| cx.open_with_system(&path)
                            },
                        ))
                        .child(ui::action_button(
                            cx,
                            "hdc-overview-shot-reveal",
                            "文件夹",
                            Some("folder_open"),
                            ActionKind::Text,
                            true,
                            move |_, _, cx| cx.reveal_path(&path),
                        )),
                )
            })
            .into_any_element();

        page_scroll(
            Page::Overview,
            &self.scroll_handles[Page::Overview as usize],
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(info_card)
                .child(actions),
        )
        .into_any_element()
    }

    /// 屏幕与显示页的 HarmonyOS（hdc）内容：截图与 uitest 控制。
    pub(crate) fn render_screen_harmony(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let weak = cx.entity().downgrade();
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let selected = self.harmony.selected.clone();
        let snapshot = self.harmony.snapshot.clone();
        let screenshot = self.harmony.screenshot.clone();

        if selected.is_none() {
            let actions = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::action_button(
                    cx,
                    "hdc-screen-connect",
                    "连接 HarmonyOS 设备",
                    Some("cable"),
                    ActionKind::Filled,
                    true,
                    cx.listener(|app, _, _, cx| app.set_page(Page::Connect, cx)),
                ))
                .into_any_element();
            return page_scroll(
                Page::Screen,
                &self.scroll_handles[Page::Screen as usize],
                ui::empty_block(
                    cx,
                    "screenshot_monitor",
                    "连接设备后截图与控制",
                    "HarmonyOS 设备通过 hdc 截图，并用 uitest uiInput 发送按键、点击与文本；录屏、分辨率与 DPI 调整仅支持 Android 设备。",
                    Some(actions),
                ),
            )
            .into_any_element();
        }

        let screen_rows = snapshot
            .as_ref()
            .map(|snapshot| {
                [
                    ("屏幕状态", snapshot.screen_label()),
                    ("设备标识", snapshot.key.clone()),
                    (
                        "截图方式",
                        "snapshot_display，失败时回退 uitest screenCap".to_string(),
                    ),
                ]
            })
            .unwrap_or_else(|| {
                [
                    ("屏幕状态", "正在读取…".to_string()),
                    ("设备标识", selected.clone().unwrap_or_default()),
                    ("截图方式", "hdc".to_string()),
                ]
            });

        let shot_button = || {
            ui::action_button(
                cx,
                "hdc-screen-shot",
                "截取屏幕",
                Some("screenshot_monitor"),
                ActionKind::Filled,
                true,
                cx.listener(|app, _, _, cx| app.hdc_screenshot(cx)),
            )
        };
        let capture = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "屏幕截图",
                Some("截图通过 hdc 直接保存到本机，不需要 adb"),
                if mobile {
                    None
                } else {
                    Some(shot_button().into_any_element())
                },
            ))
            .when(mobile, |el| {
                el.child(div().mt(px(12.)).child(shot_button()))
            })
            .child(div().mt(px(16.)).flex().flex_wrap().gap(px(14.)).children(
                screen_rows.into_iter().map(|(label, value)| {
                    div()
                        .w(px(300.))
                        .child(ui::key_value(cx, weak.clone(), label, value, true))
                }),
            ))
            .when_some(screenshot, |el, path| {
                el.child(
                    div()
                        .mt(px(16.))
                        .rounded(px(14.))
                        .overflow_hidden()
                        .bg(colors.surface_container)
                        .child(
                            img(path)
                                .w_full()
                                .max_h(px(420.))
                                .object_fit(ObjectFit::Contain),
                        ),
                )
            });

        let controls = self.render_harmony_control(cx);
        page_scroll(
            Page::Screen,
            &self.scroll_handles[Page::Screen as usize],
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(capture)
                .child(controls)
                .child(ui::callout(
                    cx,
                    "info",
                    "HarmonyOS 设备不提供录屏、分辨率与 DPI 调整入口；这些功能仅在 Android 工作区显示。",
                )),
        )
        .into_any_element()
    }

    fn render_harmony_connect_cards(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let devices = self.harmony.devices.clone();
        let selected = self.harmony.selected.clone();
        let weak = cx.entity().downgrade();

        let device_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "设备",
                Some(if devices.is_empty() {
                    "尚未发现 HarmonyOS 设备"
                } else {
                    "选择 RareBox 当前调试的设备"
                }),
                Some(
                    ui::action_button(
                        cx,
                        "harmony-device-refresh",
                        "刷新列表",
                        Some("refresh"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.refresh_hdc(cx)),
                    )
                    .into_any_element(),
                ),
            ))
            .child(guide_anchor(
                &self
                    .guide
                    .anchors
                    .handle(GuideTarget::ConnectDeviceList),
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when(devices.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.5))
                                .line_height(px(17.))
                                .text_color(colors.on_surface_variant)
                                .child("没有设备。请开启开发者模式与 USB 调试并授权本机，或使用下方网络连接。"),
                        )
                    })
                    .children(devices.iter().enumerate().map(|(index, device)| {
                        let current = selected.as_deref() == Some(device.key.as_str());
                        harmony_device_row(cx, index, device, current, weak.clone())
                    })),
            ),
        );

        let network_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "网络连接",
                Some("hdc tconn 连接无线调试设备"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.harmony_address_field.clone()),
                    )
                    .child(ui::action_button(
                        cx,
                        "harmony-connect",
                        "连接",
                        Some("link"),
                        ActionKind::Filled,
                        true,
                        cx.listener(|app, _, _, cx| app.hdc_connect(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "harmony-disconnect",
                        "断开",
                        Some("link_off"),
                        ActionKind::Outlined,
                        true,
                        cx.listener(|app, _, _, cx| app.hdc_disconnect(cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(10.5))
                    .line_height(px(16.))
                    .text_color(colors.on_surface_variant)
                    .child("地址格式为 IP:端口，只填 IP 时默认使用 5555。设备端需开启「开发者选项 → 无线调试」，并确保本机能访问该地址。"),
            );

        let wireless_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "无线调试",
                Some("设备先通过 USB 连接，再由 hdc tmode 开启 TCP 端口"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(140.))
                            .child(self.harmony_tmode_port_field.clone()),
                    )
                    .child(ui::action_button(
                        cx,
                        "harmony-tmode-on",
                        "开启无线调试",
                        Some("wifi"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| {
                            let port = app.harmony_tmode_port_field.read(cx).value().to_string();
                            app.enable_hdc_wireless(port, cx);
                        }),
                    ))
                    .child(ui::action_button(
                        cx,
                        "harmony-tmode-off",
                        "关闭端口",
                        Some("link_off"),
                        ActionKind::Outlined,
                        true,
                        cx.listener(|app, _, _, cx| app.disable_hdc_wireless(cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(10.5))
                    .line_height(px(16.))
                    .text_color(colors.on_surface_variant)
                    .child("开启需要一台已授权的 USB 设备；成功后设备地址会出现在上方，可直接点击连接。"),
            );

        let forwards = self.harmony.forwards.clone();
        let forward_reverse = self.harmony.forward_reverse;
        let forward_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "端口转发",
                Some("hdc fport / rport：把 PC 端口映射到设备，或反向映射"),
                Some(ui::action_button(
                    cx,
                    "harmony-forward-refresh",
                    "刷新",
                    Some("refresh"),
                    ActionKind::Text,
                    true,
                    cx.listener(|app, _, _, cx| app.load_hdc_forwards(cx)),
                )
                .into_any_element()),
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(130.))
                            .child(self.harmony_forward_local_field.clone()),
                    )
                    .child(
                        div()
                            .w(px(130.))
                            .child(self.harmony_forward_remote_field.clone()),
                    )
                    .child(ui::filter_chip(
                        cx,
                        "harmony-forward-direction",
                        if forward_reverse {
                            "反向 rport"
                        } else {
                            "正向 fport"
                        },
                        forward_reverse,
                        cx.listener(|app, _, _, cx| app.toggle_hdc_forward_direction(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "harmony-forward-add",
                        "添加",
                        Some("add"),
                        ActionKind::Filled,
                        true,
                        cx.listener(|app, _, _, cx| {
                            let local = app
                                .harmony_forward_local_field
                                .read(cx)
                                .value()
                                .to_string();
                            let remote = app
                                .harmony_forward_remote_field
                                .read(cx)
                                .value()
                                .to_string();
                            app.add_hdc_forward(local, remote, cx);
                        }),
                    )),
            )
            .when(forwards.is_empty(), |el| {
                el.child(
                    div()
                        .mt(px(12.))
                        .text_size(px(11.))
                        .text_color(colors.on_surface_variant)
                        .child("当前没有端口转发规则。"),
                )
            })
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(forwards.into_iter().enumerate().map(|(index, forward)| {
                        let label = if forward.reverse {
                            format!(
                                "{}  设备 :{} → PC :{}",
                                forward.direction_label(),
                                forward.remote().trim_start_matches("tcp:"),
                                forward.local().trim_start_matches("tcp:"),
                            )
                        } else {
                            format!(
                                "{}  PC :{} → 设备 :{}",
                                forward.direction_label(),
                                forward.local().trim_start_matches("tcp:"),
                                forward.remote().trim_start_matches("tcp:"),
                            )
                        };
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(px(11.))
                                    .font_family(MONO_FONT)
                                    .text_color(colors.on_surface)
                                    .child(label),
                            )
                            .child(ui::action_button(
                                cx,
                                ("harmony-forward-remove", index),
                                "移除",
                                Some("delete"),
                                ActionKind::Text,
                                true,
                                cx.listener(move |app, _, _, cx| {
                                    app.remove_hdc_forward(forward.clone(), cx)
                                }),
                            ))
                    })),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(10.5))
                    .line_height(px(16.))
                    .text_color(colors.on_surface_variant)
                    .child("正向：PC 端口 → 设备端口。反向：设备端口 → PC 端口。列表只包含当前所选设备的规则。"),
            );

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(device_card)
            .child(network_card)
            .child(wireless_card)
            .child(forward_card)
            .into_any_element()
    }

    /// 通过 `uitest uiInput` 与 `aa start -U` 完成基础远程操作。
    fn render_harmony_control(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let keys = [
            (HdcKey::Back, "arrow_back"),
            (HdcKey::Home, "home"),
            (HdcKey::Power, "power_settings_new"),
        ];
        let keys_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "按键与电源",
                Some("hdc shell uitest uiInput keyEvent 与 power-shell"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .children(keys.into_iter().enumerate().map(|(index, (key, icon))| {
                        ui::action_button(
                            cx,
                            ("harmony-key", index),
                            key.label(),
                            Some(icon),
                            ActionKind::Tonal,
                            true,
                            cx.listener(move |app, _, _, cx| app.hdc_send_key(key, cx)),
                        )
                    }))
                    .child(ui::action_button(
                        cx,
                        "harmony-wake",
                        "唤醒屏幕",
                        Some("light_mode"),
                        ActionKind::Outlined,
                        true,
                        cx.listener(|app, _, _, cx| app.hdc_wake_screen(cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(10.5))
                    .line_height(px(16.))
                    .text_color(colors.on_surface_variant)
                    .child("按键事件发送到当前前台界面；电源键会锁屏或唤醒设备。"),
            );

        let taps = [
            (HdcTap::Click, "touch_app"),
            (HdcTap::DoubleClick, "ads_click"),
            (HdcTap::LongClick, "pan_tool"),
        ];
        let tap_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "坐标点击与滑动",
                Some("坐标以设备物理像素为单位，左上角为 (0, 0)"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap(px(8.))
                    .child(div().w(px(92.)).child(self.harmony_control_x_field.clone()))
                    .child(div().w(px(92.)).child(self.harmony_control_y_field.clone()))
                    .children(taps.into_iter().enumerate().map(|(index, (tap, icon))| {
                        ui::action_button(
                            cx,
                            ("harmony-tap", index),
                            tap.label(),
                            Some(icon),
                            ActionKind::Tonal,
                            true,
                            cx.listener(move |app, _, _, cx| {
                                let x = app.harmony_control_x_field.read(cx).value().to_string();
                                let y = app.harmony_control_y_field.read(cx).value().to_string();
                                app.hdc_tap(x, y, tap, cx);
                            }),
                        )
                    })),
            )
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .children(HdcSwipeDirection::ALL.into_iter().enumerate().map(
                        |(index, direction)| {
                            let icon = match direction {
                                HdcSwipeDirection::Left => "arrow_back",
                                HdcSwipeDirection::Right => "arrow_forward",
                                HdcSwipeDirection::Up => "arrow_upward",
                                HdcSwipeDirection::Down => "arrow_downward",
                            };
                            ui::action_button(
                                cx,
                                ("harmony-fling", index),
                                direction.label(),
                                Some(icon),
                                ActionKind::Outlined,
                                true,
                                cx.listener(move |app, _, _, cx| app.hdc_fling(direction, cx)),
                            )
                        },
                    )),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(10.5))
                    .line_height(px(16.))
                    .text_color(colors.on_surface_variant)
                    .child("滑动会从屏幕中部向对应方向快速滑动，用于翻页或返回上一级。"),
            );

        let text_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "文本输入与深链",
                Some("先点击输入框坐标，再向设备注入文字；中文与特殊字符受系统 uitest 版本限制"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.harmony_control_text_field.clone()),
                    )
                    .child(ui::action_button(
                        cx,
                        "harmony-input-text",
                        "点击并输入",
                        Some("keyboard"),
                        ActionKind::Filled,
                        true,
                        cx.listener(|app, _, _, cx| {
                            let x = app.harmony_control_x_field.read(cx).value().to_string();
                            let y = app.harmony_control_y_field.read(cx).value().to_string();
                            let text =
                                app.harmony_control_text_field.read(cx).value().to_string();
                            app.hdc_input_text(x, y, text, cx);
                        }),
                    )),
            )
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.harmony_deep_link_field.clone()),
                    )
                    .child(ui::action_button(
                        cx,
                        "harmony-open-scheme",
                        "打开链接",
                        Some("open_in_new"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| {
                            let uri = app.harmony_deep_link_field.read(cx).value().to_string();
                            app.open_hdc_scheme(uri, cx);
                        }),
                    )),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(10.5))
                    .line_height(px(16.))
                    .text_color(colors.on_surface_variant)
                    .child("深链通过 aa start -U 打开，可用于拉起应用页面或系统设置；需要目标应用注册对应 Scheme。"),
            );

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(keys_card)
            .child(tap_card)
            .child(text_card)
            .into_any_element()
    }

    pub(crate) fn render_harmony_apps(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.harmony.selected.is_none() {
            return ui::empty_block(
                cx,
                "apps",
                "请先连接 HarmonyOS 设备",
                "在「连接」页选择 HarmonyOS（hdc）并连接设备后，这里会显示 bm dump 读取到的应用列表。Android 应用安装与导出仍在 Android 工作区。",
                None,
            )
            .into_any_element();
        }
        let colors = *cx.theme().colors();
        let query = self.harmony.query.trim().to_ascii_lowercase();
        let packages: Vec<HdcPackage> = self
            .harmony
            .packages
            .iter()
            .filter(|package| {
                query.is_empty()
                    || package.name.to_ascii_lowercase().contains(&query)
                    || package
                        .label
                        .as_deref()
                        .map(|label| label.to_ascii_lowercase().contains(&query))
                        .unwrap_or(false)
            })
            .cloned()
            .collect();
        let total = self.harmony.packages.len();
        let showing = packages.len();
        let loading = self.harmony.packages_for.is_some();
        let weak = cx.entity().downgrade();
        let reports = self.harmony.install_reports.clone();

        let toolbar = Card::new()
            .outlined()
            .p(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.harmony_query_field.clone()),
                    )
                    .child(ui::action_button(
                        cx,
                        "harmony-sign",
                        "签名安装",
                        Some("security"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.signing_pick_hap(cx)),
                    ))
                    .child(guide_anchor(
                        &self.guide.anchors.handle(GuideTarget::HarmonyAppsInstall),
                        ui::action_button(
                            cx,
                            "harmony-install",
                            "安装 HAP",
                            Some("install_desktop"),
                            ActionKind::Filled,
                            true,
                            cx.listener(|app, _, _, cx| app.pick_and_install_hap(cx)),
                        ),
                    ))
                    .child(ui::icon_button(
                        cx,
                        "harmony-apps-refresh",
                        "refresh",
                        38.,
                        colors.on_surface_variant,
                        cx.listener(|app, _, _, cx| app.load_hdc_packages(cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(10.))
                    .pt(px(10.))
                    .border_t_1()
                    .border_color(colors.outline_variant.opacity(0.4))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child("安装参数"),
                    )
                    .child(ui::toggle_chip(
                        cx,
                        "harmony-install-replace",
                        "覆盖安装 -r（默认）",
                        self.harmony.install_replace,
                        cx.listener(|app, _, _, cx| app.toggle_hdc_install_option(false, cx)),
                    ))
                    .child(ui::toggle_chip(
                        cx,
                        "harmony-install-shared",
                        "共享包 -s",
                        self.harmony.install_shared,
                        cx.listener(|app, _, _, cx| app.toggle_hdc_install_option(true, cx)),
                    ))
                    .child(div().flex_1())
                    .when(loading, |el| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(10.5))
                                .text_color(colors.primary)
                                .child(
                                    LoadingIndicator::new("harmony-packages-loading").size(px(13.)),
                                )
                                .child("正在读取应用列表…"),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child(format!("共 {total} 个 · 当前显示 {showing}")),
                    ),
            );

        let report_card: Option<AnyElement> = if reports.is_empty() {
            None
        } else {
            Some(harmony_reports_card(cx, reports))
        };

        let details_card: Option<AnyElement> = self
            .harmony
            .bundle_details
            .clone()
            .map(|info| harmony_bundle_card(cx, &info, weak.clone()));

        let details_loading = self.harmony.details_for.clone().map(|bundle| {
            div()
                .mt(px(12.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(10.5))
                .text_color(colors.primary)
                .child(LoadingIndicator::new("harmony-details-loading").size(px(13.)))
                .child(format!("正在读取 {bundle} 的详情…"))
                .into_any_element()
        });
        let list = if packages.is_empty() {
            let action = Some(
                ui::action_button(
                    cx,
                    "harmony-empty-load",
                    "读取应用列表",
                    Some("refresh"),
                    ActionKind::Tonal,
                    true,
                    cx.listener(|app, _, _, cx| app.load_hdc_packages(cx)),
                )
                .into_any_element(),
            );
            Card::new().outlined().p(px(20.)).child(ui::empty_block(
                cx,
                "apps",
                if total == 0 {
                    "还没有读取应用列表"
                } else {
                    "没有匹配的应用"
                },
                if total == 0 {
                    "点击刷新从设备读取 bm dump -a 的结果。"
                } else {
                    "试试修改搜索关键字。"
                },
                action,
            ))
        } else {
            Card::new().outlined().p(px(8.)).child(
                div().flex().flex_col().children(
                    packages.iter().enumerate().map(|(index, package)| {
                        harmony_package_row(cx, index, package, weak.clone())
                    }),
                ),
            )
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(toolbar)
            .when_some(report_card, |el, card| el.child(card))
            .when_some(details_loading, |el, loading| el.child(loading))
            .when_some(details_card, |el, card| el.child(card))
            .child(list)
            .into_any_element()
    }
}

fn harmony_device_row(
    cx: &App,
    index: usize,
    device: &HdcDevice,
    current: bool,
    weak: WeakEntity<RareBox>,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let key = device.key.clone();
    let hover = colors.surface_container_high;
    div()
        .id(("harmony-device", index))
        .w_full()
        .px(px(14.))
        .py(px(10.))
        .rounded(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .bg(if current {
            colors.secondary_container
        } else {
            colors.surface_container_low
        })
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(move |_, _, cx| {
            if let Err(err) = weak.update(cx, |app, cx| app.select_hdc_device(key.clone(), cx)) {
                eprintln!("RareBox Desktop: 切换鸿蒙设备失败: {err}");
            }
        })
        .child(
            div()
                .size(px(32.))
                .flex_none()
                .rounded(px(10.))
                .bg(colors.primary_container)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::new(device.connection.icon()))
                        .size(px(17.))
                        .color(colors.on_primary_container),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if current {
                            colors.on_secondary_container
                        } else {
                            colors.on_surface
                        })
                        .truncate()
                        .child(device.key.clone()),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(colors.on_surface_variant)
                        .child(format!(
                            "{} 连接 · {}",
                            device.connection.label(),
                            device.state.label()
                        )),
                ),
        )
        .when(current, |el| {
            el.child(ui::tag(
                "当前设备",
                colors.tertiary_container,
                colors.on_tertiary_container,
            ))
        })
        .child(
            Icon::new(IconName::new("chevron_right"))
                .size(px(16.))
                .color(colors.on_surface_variant),
        )
}

fn harmony_info_card(
    cx: &App,
    snapshot: &HdcSnapshot,
    at: Option<String>,
    weak: WeakEntity<RareBox>,
) -> AnyElement {
    let subtitle = match at {
        Some(at) => format!("读取于 {at}"),
        None => "来自 param get / hidumper / bm get".to_string(),
    };
    let refresh = {
        let weak = weak.clone();
        ui::action_button(
            cx,
            "harmony-info-refresh",
            "刷新",
            Some("refresh"),
            ActionKind::Text,
            true,
            move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| app.load_hdc_snapshot(cx)) {
                    eprintln!("RareBox Desktop: 刷新鸿蒙设备信息失败: {err}");
                }
            },
        )
    };

    let battery = match snapshot.battery_percent() {
        Some(value) => format!("{value:.0}% · {}", snapshot.battery_status_label()),
        None => format!("— · {}", snapshot.battery_status_label()),
    };
    let storage = match snapshot.storage_percent() {
        Some(percent) => format!("{}（{percent:.0}%）", snapshot.storage_used_label()),
        None => snapshot.storage_used_label(),
    };
    let memory = match snapshot.memory_percent() {
        Some(percent) => format!("{}（{percent:.0}%）", snapshot.memory_used_label()),
        None => snapshot.memory_used_label(),
    };
    let rows: [(&'static str, String, bool); 14] = [
        ("型号", non_empty(&snapshot.model), false),
        ("设备名称", snapshot.device_name_label().to_string(), false),
        ("品牌", non_empty(&snapshot.brand), false),
        ("系统版本", non_empty(&snapshot.os_version), true),
        (
            "API / ABI",
            format!(
                "{} · {}",
                non_empty(&snapshot.api_version),
                non_empty(&snapshot.abi)
            ),
            true,
        ),
        ("UDID", non_empty(&snapshot.udid), true),
        (
            "IP 地址",
            snapshot.ip.clone().unwrap_or_else(|| "—".to_string()),
            true,
        ),
        ("屏幕", snapshot.screen_label(), false),
        ("电池温度", snapshot.temperature_label(), false),
        ("Wi-Fi", snapshot.wifi_label().to_string(), false),
        ("电量", battery, false),
        ("存储（/data）", storage, false),
        ("内存", memory, false),
        ("运行时间", snapshot.uptime_label(), false),
    ];

    Card::new()
        .outlined()
        .p(px(20.))
        .child(ui::section_header(
            cx,
            "设备信息",
            Some(subtitle),
            Some(refresh.into_any_element()),
        ))
        .child(
            div()
                .mt(px(16.))
                .flex()
                .flex_wrap()
                .gap(px(14.))
                .children(rows.into_iter().map(|(label, value, mono)| {
                    div()
                        .w(px(300.))
                        .child(ui::key_value(cx, weak.clone(), label, value, mono))
                })),
        )
        .child(div().mt(px(12.)).child(ui::callout(
            cx,
            "info",
            format!(
                "设备标识：{}。设备信息全部来自 hdc shell 的系统命令，不包含模拟数据。",
                snapshot.key
            ),
        )))
        .into_any_element()
}

fn non_empty(value: &str) -> String {
    if value.trim().is_empty() {
        "—".to_string()
    } else {
        value.to_string()
    }
}

fn harmony_reports_card(cx: &App, reports: Vec<HdcInstallReport>) -> AnyElement {
    let colors = *cx.theme().colors();
    let succeeded = reports.iter().filter(|report| report.success).count();
    Card::new()
        .outlined()
        .p(px(16.))
        .child(ui::section_header(
            cx,
            "上次安装结果",
            Some(format!(
                "{succeeded} 个成功 · {} 个失败",
                reports.len().saturating_sub(succeeded)
            )),
            None,
        ))
        .child(
            div()
                .mt(px(8.))
                .flex()
                .flex_col()
                .children(reports.into_iter().map(|report| {
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .py(px(6.))
                        .child(
                            div().mt(px(1.)).child(
                                Icon::new(IconName::new(if report.success {
                                    "check_circle"
                                } else {
                                    "error"
                                }))
                                .size(px(16.))
                                .color(if report.success {
                                    colors.tertiary
                                } else {
                                    colors.error
                                }),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(px(0.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(colors.on_surface)
                                        .truncate()
                                        .child(report.title),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .line_height(px(16.))
                                        .text_color(colors.on_surface_variant)
                                        .child(report.detail),
                                ),
                        )
                })),
        )
        .into_any_element()
}

fn harmony_bundle_card(
    cx: &mut Context<RareBox>,
    info: &HdcBundleInfo,
    weak: WeakEntity<RareBox>,
) -> AnyElement {
    let colors = *cx.theme().colors();
    let bundle = info.bundle.clone();
    let version = match info.version_code {
        Some(code) => format!("{}（{code}）", non_empty(&info.version_name)),
        None => non_empty(&info.version_name),
    };
    let rows = [
        ("Bundle", non_empty(&info.bundle), true),
        ("版本", version, true),
        ("厂商", non_empty(&info.vendor), false),
        (
            "主 Ability",
            info.main_ability.clone().unwrap_or_else(|| "—".to_string()),
            true,
        ),
        ("代码路径", non_empty(&info.code_path), true),
        (
            "可调试",
            if info.debug { "是" } else { "否" }.to_string(),
            false,
        ),
    ];
    let abilities = summary_list(&info.abilities, 30);
    let permissions = summary_list(&info.permissions, 40);

    let close = {
        let weak = weak.clone();
        ui::icon_button(
            cx,
            "harmony-details-close",
            "close",
            32.,
            colors.on_surface_variant,
            move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| app.close_hdc_bundle_details(cx)) {
                    eprintln!("RareBox Desktop: 关闭鸿蒙应用详情失败: {err}");
                }
            },
        )
    };
    let launch = {
        let weak = weak.clone();
        let target = bundle.clone();
        ui::action_button(
            cx,
            "harmony-details-launch",
            "启动",
            Some("play_arrow"),
            ActionKind::Filled,
            true,
            move |_, _, cx| {
                if let Err(err) =
                    weak.update(cx, |app, cx| app.launch_hdc_package(target.clone(), cx))
                {
                    eprintln!("RareBox Desktop: 启动鸿蒙应用失败: {err}");
                }
            },
        )
    };
    let clear_cache = {
        let weak = weak.clone();
        let target = bundle.clone();
        ui::action_button(
            cx,
            "harmony-details-cache",
            "清缓存",
            Some("cleaning_services"),
            ActionKind::Outlined,
            true,
            move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| {
                    app.open_hdc_clear_app_data(target.clone(), true, cx)
                }) {
                    eprintln!("RareBox Desktop: 清除鸿蒙应用缓存失败: {err}");
                }
            },
        )
    };
    let clear_data = {
        let weak = weak.clone();
        let target = bundle.clone();
        ui::action_button(
            cx,
            "harmony-details-data",
            "清除数据",
            Some("delete_sweep"),
            ActionKind::Text,
            true,
            move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| {
                    app.open_hdc_clear_app_data(target.clone(), false, cx)
                }) {
                    eprintln!("RareBox Desktop: 清除鸿蒙应用数据失败: {err}");
                }
            },
        )
    };

    Card::new()
        .outlined()
        .p(px(20.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w(px(0.)).child(ui::section_header(
                    cx,
                    "应用详情",
                    Some("来自 bm dump -n"),
                    None,
                )))
                .child(close),
        )
        .child(
            div()
                .mt(px(14.))
                .flex()
                .flex_wrap()
                .gap(px(14.))
                .children(rows.into_iter().map(|(label, value, mono)| {
                    div()
                        .w(px(300.))
                        .child(ui::key_value(cx, weak.clone(), label, value, mono))
                })),
        )
        .child(
            div()
                .mt(px(12.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.on_surface_variant)
                        .child(format!("Abilities（{}）", info.abilities.len())),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .line_height(px(17.))
                        .font_family(MONO_FONT)
                        .text_color(colors.on_surface)
                        .child(abilities),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.on_surface_variant)
                        .child(format!("权限（{}）", info.permissions.len())),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .line_height(px(17.))
                        .font_family(MONO_FONT)
                        .text_color(colors.on_surface)
                        .child(permissions),
                ),
        )
        .child(
            div()
                .mt(px(14.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(launch)
                .child(clear_cache)
                .child(clear_data),
        )
        .into_any_element()
}

fn summary_list(items: &[String], limit: usize) -> String {
    if items.is_empty() {
        return "—".to_string();
    }
    let shown = items
        .iter()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .join("、");
    if items.len() > limit {
        format!("{shown} … 另有 {} 项", items.len() - limit)
    } else {
        shown
    }
}

fn harmony_package_row(
    cx: &App,
    index: usize,
    package: &HdcPackage,
    weak: WeakEntity<RareBox>,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let name = package.name.clone();

    let launch = {
        let weak = weak.clone();
        let target = name.clone();
        ui::action_button(
            cx,
            ("harmony-launch", index),
            "启动",
            Some("play_arrow"),
            ActionKind::Tonal,
            true,
            move |_, _, cx| {
                if let Err(err) =
                    weak.update(cx, |app, cx| app.launch_hdc_package(target.clone(), cx))
                {
                    eprintln!("RareBox Desktop: 启动鸿蒙应用失败: {err}");
                }
            },
        )
    };
    let stop = {
        let weak = weak.clone();
        let target = name.clone();
        ui::action_button(
            cx,
            ("harmony-stop", index),
            "停止",
            Some("stop_circle"),
            ActionKind::Text,
            true,
            move |_, _, cx| {
                if let Err(err) =
                    weak.update(cx, |app, cx| app.stop_hdc_package(target.clone(), cx))
                {
                    eprintln!("RareBox Desktop: 停止鸿蒙应用失败: {err}");
                }
            },
        )
    };
    let uninstall = {
        let weak = weak.clone();
        let target = name.clone();
        ui::action_button(
            cx,
            ("harmony-uninstall", index),
            "卸载",
            Some("delete"),
            ActionKind::Text,
            true,
            move |_, _, cx| {
                let target = target.clone();
                if let Err(err) = weak.update(cx, |app, cx| {
                    app.open_confirm(
                        Confirm {
                            title: "卸载这个 HarmonyOS 应用？".into(),
                            message: format!(
                                "将执行 hdc uninstall {target}。应用数据会一并删除，此操作无法撤销。"
                            )
                            .into(),
                            confirm_label: "卸载".into(),
                            kind: ConfirmKind::HdcUninstall(target.clone()),
                        },
                        cx,
                    );
                }) {
                    eprintln!("RareBox Desktop: 打开鸿蒙卸载确认失败: {err}");
                }
            },
        )
    };
    let copy = {
        let weak = weak.clone();
        let target = name.clone();
        ui::icon_button(
            cx,
            ("harmony-copy", index),
            "content_copy",
            32.,
            colors.on_surface_variant,
            move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| app.copy_hdc_bundle(target.clone(), cx))
                {
                    eprintln!("RareBox Desktop: 复制 Bundle 名失败: {err}");
                }
            },
        )
    };
    let details = {
        let weak = weak.clone();
        let target = name.clone();
        ui::action_button(
            cx,
            ("harmony-details", index),
            "详情",
            Some("info"),
            ActionKind::Text,
            true,
            move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| {
                    app.load_hdc_bundle_details(target.clone(), cx)
                }) {
                    eprintln!("RareBox Desktop: 读取鸿蒙应用详情失败: {err}");
                }
            },
        )
    };

    div()
        .id(("harmony-package", index))
        .w_full()
        .px(px(14.))
        .py(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .border_b_1()
        .border_color(colors.outline_variant.opacity(0.3))
        .child(
            div()
                .size(px(30.))
                .flex_none()
                .rounded(px(9.))
                .bg(colors.primary_container)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::new("apps"))
                        .size(px(16.))
                        .color(colors.on_primary_container),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.on_surface)
                        .truncate()
                        .child(package.display_name()),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(colors.on_surface_variant)
                        .truncate()
                        .child("HarmonyOS 应用"),
                ),
        )
        .child(ui::tag(
            "HarmonyOS",
            colors.tertiary_container,
            colors.on_tertiary_container,
        ))
        .child(details)
        .child(copy)
        .child(launch)
        .child(stop)
        .child(uninstall)
}
