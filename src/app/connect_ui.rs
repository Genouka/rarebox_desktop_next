//! connect 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::shell_ui::page_scroll;
use super::{Page, RareBox};
use crate::adb::DeviceConnection;
use rarebox_core::state::DeviceKind;
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, App, Context, ElementId, FontWeight, IntoElement, ParentElement, SharedString,
    Styled, div, prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

use super::guide_ui::guide_anchor;
use rarebox_core::guide::GuideTarget;
impl RareBox {
    pub(crate) fn render_connect(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_harmony() {
            return self.render_connect_harmony(cx);
        }
        let colors = *cx.theme().colors();
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));
        let adb_ready = self.adb_path.is_some();
        let adb_error = self.adb_error.clone();
        let devices = self.devices.clone();
        let online = devices.iter().filter(|device| device.is_online()).count();
        let selected = self.selected_serial.clone();
        let ip_hint = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.ip.clone());
        let weak = cx.entity().downgrade();
        let adb_path = self
            .adb_path
            .as_ref()
            .map(|path| crate::config::short_path(path))
            .unwrap_or_else(|| "尚未检测到 ADB".to_string());
        let adb_version = self.adb_version.clone().unwrap_or_else(|| "—".to_string());

        // 能力矩阵永远在顶部：先看图块知道手表支持哪种调试，再动手。
        let switch = self.transport_switch(cx);
        let mut content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(switch)
            .child(self.support_card(cx, weak.clone(), adb_ready, adb_error.clone()));
        if online == 0 {
            content = content.child(self.scan_card(cx, weak.clone()));
        }

        let device_anchor = self.guide.anchors.handle(GuideTarget::ConnectDeviceList);
        let device_card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "设备",
                Some(if online == 0 {
                    "尚未发现可用的手表"
                } else {
                    "选择 RareBox 当前操作的设备"
                }),
                Some(
                    ui::action_button(
                        cx,
                        "connect-refresh",
                        "刷新列表",
                        Some("refresh"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.refresh_devices(cx)),
                    )
                    .into_any_element(),
                ),
            ))
            .child(guide_anchor(
                &device_anchor,
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
                                .child(if cfg!(any(target_os = "android", target_os = "ios")) {
                                    "还没有任何设备。Android 端请使用下方「无线调试」连接手表。"
                                } else {
                                    "还没有任何设备。先按上方步骤完成 USB 连接，或在下方用无线调试连接。"
                                }),
                        )
                    })
                    .children(devices.iter().enumerate().map(|(index, device)| {
                        let current = selected.as_deref() == Some(device.serial.as_str());
                        let serial = device.serial.clone();
                        let weak = weak.clone();
                        device_row(cx, index, device, current, weak, serial)
                    })),
            ),
        );
        let pair_button = || {
            ui::action_button(
                cx,
                "connect-pair",
                "配对",
                Some("security"),
                ActionKind::Tonal,
                true,
                cx.listener(|app, _, _, cx| app.wireless_pair(cx)),
            )
        };
        let pair_anchor = self.guide.anchors.handle(GuideTarget::ConnectPairCard);
        // 教程进行中强制展开，遮罩才能高亮到真实控件。
        let manual_open = self.manual_open || self.guide.session.is_some();
        let wireless = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "无线调试",
                Some("先配对，再用连接地址接入"),
                None,
            ))
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Icon::new(IconName::new("wifi"))
                            .size(px(18.))
                            .color(colors.primary),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.on_surface)
                            .child("连接已配对设备"),
                    )
                    .child(div().flex_1())
                    .when_some(ip_hint.clone(), |el, ip| {
                        el.child(ui::action_button(
                            cx,
                            "connect-fill-ip",
                            "填入设备 IP",
                            Some("lan"),
                            ActionKind::Text,
                            true,
                            {
                                let address = format!("{ip}:5555");
                                cx.listener(move |app, _, _, cx| {
                                    app.set_wireless_address(address.clone(), cx);
                                })
                            },
                        ))
                    }),
            )
            .child({
                let connect_btn = ui::action_button(
                    cx,
                    "connect-wireless-connect",
                    "连接",
                    Some("link"),
                    ActionKind::Filled,
                    true,
                    cx.listener(|app, _, _, cx| app.connect_smart(cx)),
                );
                let disconnect_btn = ui::action_button(
                    cx,
                    "connect-wireless-disconnect",
                    "断开",
                    Some("link_off"),
                    ActionKind::Outlined,
                    true,
                    cx.listener(|app, _, _, cx| app.wireless_disconnect(cx)),
                );
                if mobile {
                    div()
                        .mt(px(10.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(div().w_full().child(self.wireless_field.clone()))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(connect_btn)
                                .child(disconnect_btn),
                        )
                } else {
                    div()
                        .mt(px(10.))
                        .flex()
                        .items_end()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .child(self.wireless_field.clone()),
                        )
                        .child(connect_btn)
                        .child(disconnect_btn)
                }
            })
            .child(
                div()
                    .mt(px(8.))
                    .child(ui::action_button(
                        cx,
                        "connect-tcpip",
                        "开启设备 TCP/IP 5555",
                        Some("settings_remote"),
                        ActionKind::Text,
                        self.has_device(),
                        cx.listener(|app, _, _, cx| app.enable_tcpip(cx)),
                    )),
            )
            .child(guide_anchor(
                &pair_anchor,
                div()
                    .mt(px(10.))
                    .pt(px(10.))
                    .border_t_1()
                    .border_color(colors.outline_variant.opacity(0.4))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(ui::action_button(
                        cx,
                        "connect-manual-toggle",
                        if manual_open {
                            "收起配对码"
                        } else {
                            "用配对码配对"
                        },
                        Some("security"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.toggle_manual(cx)),
                    ))
                    .when(manual_open, |el| {
                        el.child(if mobile {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(div().w_full().child(self.pair_field.clone()))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .child(self.pair_code_field.clone()),
                                        )
                                        .child(pair_button()),
                                )
                        } else {
                            div()
                                .flex()
                                .items_end()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .child(self.pair_field.clone()),
                                )
                                .child(
                                    div()
                                        .w(px(140.))
                                        .flex_none()
                                        .child(self.pair_code_field.clone()),
                                )
                                .child(pair_button())
                        })
                        .child(div().mt(px(8.)).child(ui::callout(
                            cx,
                            "info",
                            "配对地址在手表「无线调试 → 使用配对码配对设备」里。",
                        )))
                    }),
            ));

        let adb = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "ADB",
                Some("连接失败时先确认 ADB 可用"),
                Some(
                    ui::action_button(
                        cx,
                        "connect-adb-settings",
                        "打开设置",
                        Some("settings"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.set_page(Page::Settings, cx)),
                    )
                    .into_any_element(),
                ),
            ))
            .child(
                div()
                    .mt(px(14.))
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
                    ),
            )
            .when_some(adb_error, |el, error| {
                el.child(div().mt(px(10.)).child(ui::callout(
                    cx,
                    "warning",
                    format!("上次检测失败：{error}"),
                )))
            });

        let driver_anchor = self.guide.anchors.handle(GuideTarget::ConnectDriverCard);
        page_scroll(
            Page::Connect,
            &self.scroll_handles[Page::Connect as usize],
            content
                .child(
                    div()
                        .flex()
                        .gap(px(16.))
                        .when(!mobile, |el| el.items_start())
                        .when(mobile, |el| el.flex_col())
                        .child(
                            div()
                                .min_w(px(0.))
                                .when(!mobile, |el| el.flex_1())
                                .child(device_card),
                        )
                        .child(
                            div()
                                .min_w(px(0.))
                                .when(!mobile, |el| el.flex_1())
                                .child(adb),
                        ),
                )
                .child(wireless)
                .when(cfg!(windows), |el| {
                    // 驱动卡默认收起，只有 USB 路径需要时才展开，减少无关文字。
                    let driver_open = self.driver_open;
                    let driver_toggle = ui::action_button(
                        cx,
                        "connect-driver-toggle",
                        if driver_open {
                            "收起 USB 驱动"
                        } else {
                            "USB 驱动"
                        },
                        Some("usb"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.toggle_driver(cx)),
                    );
                    el.child(guide_anchor(
                        &driver_anchor,
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(driver_toggle)
                            .when(driver_open, |el| el.child(driver_card(cx, weak.clone()))),
                    ))
                }),
        )
        .into_any_element()
    }
}

/// 能力矩阵：三张图块分别表示无线调试 / WLAN 调试 / USB 调试。
///
/// 状态来自快照里的调试读数；没连过设备时统一显示「未检测」并保留教程入口，
/// 不做没有依据的「不支持」判断。
impl RareBox {
    fn support_card(
        &self,
        cx: &App,
        weak: gpui::WeakEntity<RareBox>,
        adb_ready: bool,
        adb_error: Option<String>,
    ) -> Card {
        let colors = *cx.theme().colors();
        let support = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.debug_support());
        let recommended = support.as_ref().and_then(|support| support.recommended());

        let tile = |key: &'static str,
                    label: &'static str,
                    icon: &'static str,
                    capability: Option<&rarebox_core::connect_support::Capability>,
                    guide: &'static str| {
            let (dot, status) = match capability {
                Some(capability) if capability.enabled => (colors.tertiary, "已开启"),
                Some(capability) if capability.supported == Some(false) => {
                    (colors.error, capability.hint)
                }
                Some(capability) if capability.supported == Some(true) => {
                    (colors.primary, "支持，未开启")
                }
                _ => (colors.outline_variant, "未检测"),
            };
            let is_recommended = recommended.map(|transport| transport.guide_id()) == Some(guide);
            let weak = weak.clone();
            div()
                .id(ElementId::Name(format!("connect-tile-{key}").into()))
                .flex_1()
                .min_w(px(120.))
                .p(px(12.))
                .rounded(px(14.))
                .bg(if is_recommended {
                    colors.secondary_container
                } else {
                    colors.surface_container_low
                })
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .hover(move |style| style.bg(colors.surface_container_high))
                .on_click(move |_, _, cx| {
                    if let Err(err) = weak.update(cx, |app, cx| app.guide_start(guide, cx)) {
                        eprintln!("RareBox Desktop: 打开连接教程失败: {err}");
                    }
                })
                .child(
                    Icon::new(IconName::new(icon))
                        .size(px(26.))
                        .color(colors.primary),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.on_surface)
                        .child(label),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(ui::status_dot(dot))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(colors.on_surface_variant)
                                .child(status),
                        ),
                )
                .when(is_recommended, |el| {
                    el.child(
                        div()
                            .text_size(px(9.5))
                            .text_color(colors.on_secondary_container)
                            .child("推荐"),
                    )
                })
        };

        let wireless = support.as_ref().map(|support| &support.wireless);
        let wlan = support.as_ref().map(|support| &support.wlan);
        let usb = support.as_ref().map(|support| &support.usb);
        let tutorial_id = self.guide_connect_target();
        let guide_weak = weak.clone();
        let refresh_weak = weak.clone();

        Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "连接方式",
                Some(if support.is_some() {
                    "按手表能力标注；点图块看图文教程"
                } else {
                    "连上后自动识别，也可先看教程"
                }),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(tile(
                        "wireless",
                        "无线调试",
                        "tabler-wifi",
                        wireless,
                        "connect_wireless",
                    ))
                    .child(tile(
                        "wlan",
                        "WLAN 调试",
                        "wifi_tethering",
                        wlan,
                        "connect_wlan",
                    ))
                    .child(tile("usb", "USB 调试", "tabler-usb", usb, "connect_usb")),
            )
            .when_some(adb_error, |el, error| {
                el.child(div().mt(px(12.)).child(ui::callout(
                    cx,
                    "warning",
                    format!("ADB 未就绪：{error}"),
                )))
            })
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::action_button(
                        cx,
                        "connect-support-refresh",
                        "重新识别",
                        Some("refresh"),
                        ActionKind::Filled,
                        true,
                        move |_, _, cx| {
                            if let Err(err) =
                                refresh_weak.update(cx, |app, cx| app.refresh_all(cx))
                            {
                                eprintln!("RareBox Desktop: 重新识别失败: {err}");
                            }
                        },
                    ))
                    .child(ui::action_button(
                        cx,
                        "connect-support-guide",
                        "图文教程",
                        Some("help"),
                        ActionKind::Tonal,
                        true,
                        move |_, _, cx| {
                            if let Err(err) = guide_weak.update(cx, |app, cx| {
                                app.guide_start(tutorial_id, cx);
                            }) {
                                eprintln!("RareBox Desktop: 打开连接教程失败: {err}");
                            }
                        },
                    ))
                    .when(!adb_ready, |el| {
                        el.child(
                            div()
                                .text_size(px(10.5))
                                .text_color(colors.on_surface_variant)
                                .child("ADB 未找到"),
                        )
                    }),
            )
    }

    /// 盲扫面板：找不到设备时按网段/端口扫一遍，结果可一键连接。
    fn scan_card(&self, cx: &App, weak: gpui::WeakEntity<RareBox>) -> Card {
        let colors = *cx.theme().colors();
        let scan = &self.scan;
        let progress = scan.progress();
        let weak_start = weak.clone();
        let weak_deep = weak.clone();
        let weak_cancel = weak.clone();
        let weak_clear = weak.clone();

        let hits = scan.hits.iter().map(|hit| {
            let address = hit.address.clone();
            let weak = weak.clone();
            let kind = match hit.kind {
                Some(DeviceKind::Android) => "ADB",
                Some(DeviceKind::Harmony) => "hdc",
                None => "端口",
            };
            div()
                .id(ElementId::Name(format!("scan-hit-{}", hit.address).into()))
                .w_full()
                .rounded(px(10.))
                .bg(colors.surface_container_low)
                .px(px(12.))
                .py(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(colors.primary)
                        .child(kind),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(12.))
                        .font_family(MONO_FONT)
                        .truncate()
                        .child(address.clone()),
                )
                .child(ui::action_button(
                    cx,
                    ElementId::Name(format!("scan-connect-{}", hit.address).into()),
                    "连接",
                    None,
                    ActionKind::Filled,
                    hit.kind.is_some(),
                    move |_, _, cx| {
                        let address = address.clone();
                        if let Err(err) =
                            weak.update(cx, |app, cx| app.scan_connect(address, cx))
                        {
                            eprintln!("RareBox Desktop: 扫描结果连接失败: {err}");
                        }
                    },
                ))
                .into_any_element()
        });

        Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "找不到手表？",
                Some("扫本机网段，或填手表的 IP"),
                None,
            ))
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w(px(0.)).child(self.scan_field.clone()))
                    .child(ui::action_button(
                        cx,
                        "connect-scan",
                        if scan.running { "扫描中…" } else { "扫描" },
                        Some("search"),
                        ActionKind::Filled,
                        !scan.running,
                        move |_, _, cx| {
                            if let Err(err) =
                                weak_start.update(cx, |app, cx| app.start_scan(false, cx))
                            {
                                eprintln!("RareBox Desktop: 启动扫描失败: {err}");
                            }
                        },
                    )),
            )
            .when(scan.running || scan.total > 0, |el| {
                el.child(
                    div()
                        .mt(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .h(px(6.))
                                .w_full()
                                .rounded_full()
                                .bg(colors.surface_container_highest)
                                .child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .bg(ui::progress_color(cx, progress))
                                        .w(px((progress * 360.).max(2.))),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(colors.on_surface_variant)
                                .child(format!("{} · {}/{}", scan.scope, scan.done, scan.total)),
                        ),
                )
            })
            .children(hits)
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(ui::action_button(
                        cx,
                        "connect-scan-deep",
                        "深度扫描",
                        None,
                        ActionKind::Text,
                        !scan.running,
                        move |_, _, cx| {
                            if let Err(err) =
                                weak_deep.update(cx, |app, cx| app.start_scan(true, cx))
                            {
                                eprintln!("RareBox Desktop: 深度扫描失败: {err}");
                            }
                        },
                    ))
                    .when(scan.running, |el| {
                        el.child(ui::action_button(
                            cx,
                            "connect-scan-cancel",
                            "取消",
                            None,
                            ActionKind::Text,
                            true,
                            move |_, _, cx| {
                                if let Err(err) =
                                    weak_cancel.update(cx, |app, cx| app.cancel_scan(cx))
                                {
                                    eprintln!("RareBox Desktop: 取消扫描失败: {err}");
                                }
                            },
                        ))
                    })
                    .when(!scan.running && !scan.hits.is_empty(), |el| {
                        el.child(ui::action_button(
                            cx,
                            "connect-scan-clear",
                            "清空结果",
                            None,
                            ActionKind::Text,
                            true,
                            move |_, _, cx| {
                                if let Err(err) =
                                    weak_clear.update(cx, |app, cx| app.clear_scan(cx))
                                {
                                    eprintln!("RareBox Desktop: 清空扫描结果失败: {err}");
                                }
                            },
                        ))
                    }),
            )
    }
}

/// USB 驱动卡片：仅 Windows 有意义。显示驱动是否已安装，并提供安装/卸载按钮。
///
/// 安装/卸载需要管理员权限，应用本身无需提权；点击按钮后真正写驱动存储的步骤会
/// 由 `adb_driver` 模块通过 UAC 单独提权。非 Windows 平台不渲染本卡片。
fn driver_card(cx: &App, weak: gpui::WeakEntity<RareBox>) -> Card {
    let colors = *cx.theme().colors();
    let installed = crate::adb_driver::is_installed();
    let elevated = crate::adb_driver::is_elevated();

    Card::new()
        .outlined()
        .p(px(20.))
        .child(ui::section_header(
            cx,
            "USB 驱动",
            Some("设备无法被 ADB 发现时安装"),
            None,
        ))
        .child(
            div()
                .mt(px(14.))
                .flex()
                .items_center()
                .gap(px(9.))
                .child(ui::status_dot(if installed {
                    colors.tertiary
                } else {
                    colors.outline_variant
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
                                .child(if installed {
                                    "驱动已安装"
                                } else {
                                    "驱动未安装"
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(colors.on_surface_variant)
                                .child(if installed {
                                    "手表已可被识别为 Android ADB Interface"
                                } else {
                                    "连接后设备未出现在列表时，先安装此驱动"
                                }),
                        ),
                ),
        )
        .when(!elevated, |el| {
            el.child(div().mt(px(12.)).child(ui::callout(
                cx,
                "info",
                "安装/卸载需要管理员权限，点击后会弹出 Windows 用户账户控制（UAC）请求，请选择「是」。",
            )))
        })
        .child(
            div()
                .mt(px(14.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    ui::action_button(
                        cx,
                        "connect-driver-install",
                        "安装驱动",
                        Some("usb"),
                        ActionKind::Filled,
                        !installed,
                        {
                            let weak = weak.clone();
                            move |_, _, cx| {
                                if let Err(err) =
                                    weak.update(cx, |app, cx| app.install_driver(cx))
                                {
                                    eprintln!("RareBox Desktop: 安装驱动失败: {err}");
                                }
                            }
                        },
                    )
                    .into_any_element(),
                )
                .child(
                    ui::action_button(
                        cx,
                        "connect-driver-uninstall",
                        "卸载驱动",
                        Some("delete"),
                        ActionKind::Outlined,
                        installed,
                        {
                            let weak = weak.clone();
                            move |_, _, cx| {
                                if let Err(err) =
                                    weak.update(cx, |app, cx| app.uninstall_driver(cx))
                                {
                                    eprintln!("RareBox Desktop: 卸载驱动失败: {err}");
                                }
                            }
                        },
                    )
                    .into_any_element(),
                ),
        )
}

fn device_row(
    cx: &App,
    index: usize,
    device: &crate::adb::Device,
    current: bool,
    weak: gpui::WeakEntity<RareBox>,
    serial: String,
) -> AnyElement {
    let colors = *cx.theme().colors();
    let online = device.is_online();
    let wireless = device.connection == DeviceConnection::Wireless;

    let mut actions = div().flex().items_center().gap(px(6.)).flex_none();
    if online && !current {
        let select_weak = weak.clone();
        let select_serial = serial.clone();
        actions = actions.child(ui::action_button(
            cx,
            ("connect-select", index),
            "使用",
            Some("check_circle"),
            ActionKind::Tonal,
            true,
            move |_, _, cx| {
                if let Err(err) = select_weak.update(cx, |app, cx| {
                    app.select_device(select_serial.clone(), cx);
                }) {
                    eprintln!("RareBox Desktop: 切换设备失败: {err}");
                }
            },
        ));
    }
    if wireless && online {
        let drop_weak = weak.clone();
        let drop_serial = serial.clone();
        actions = actions.child(ui::action_button(
            cx,
            ("connect-drop", index),
            "断开",
            Some("link_off"),
            ActionKind::Text,
            true,
            move |_, _, cx| {
                if let Err(err) = drop_weak.update(cx, |app, cx| {
                    app.disconnect_device(drop_serial.clone(), cx);
                }) {
                    eprintln!("RareBox Desktop: 断开设备失败: {err}");
                }
            },
        ));
    }

    let (state_background, state_foreground) = if online {
        (colors.tertiary_container, colors.on_tertiary_container)
    } else {
        (colors.error_container, colors.on_error_container)
    };

    div()
        .id(("connect-device", index))
        .w_full()
        .px(px(12.))
        .py(px(10.))
        .rounded(px(12.))
        .border_1()
        .border_color(if current {
            colors.primary.opacity(0.55)
        } else {
            colors.outline_variant.opacity(0.45)
        })
        .bg(if current {
            colors.primary_container.opacity(0.35)
        } else {
            colors.surface_container_low
        })
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            div()
                .size(px(32.))
                .flex_none()
                .rounded(px(10.))
                .bg(colors.surface_container)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::new(device.connection.icon()))
                        .size(px(17.))
                        .color(colors.on_surface_variant),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .min_w(px(0.))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(colors.on_surface)
                                .truncate()
                                .child(SharedString::from(device.display_name())),
                        )
                        .children(current.then(|| {
                            ui::tag(
                                "当前设备",
                                colors.primary_container,
                                colors.on_primary_container,
                            )
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .min_w(px(0.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(10.))
                                .font_family(MONO_FONT)
                                .text_color(colors.on_surface_variant)
                                .truncate()
                                .child(SharedString::from(device.serial.clone())),
                        )
                        .child(ui::tag(
                            device.state_label(),
                            state_background,
                            state_foreground,
                        ))
                        .child(ui::tag(
                            device.connection.label(),
                            colors.surface_container_high,
                            colors.on_surface_variant,
                        )),
                ),
        )
        .child(actions)
        .into_any_element()
}
