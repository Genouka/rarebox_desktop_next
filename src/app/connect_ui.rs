//! connect 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::shell_ui::page_scroll;
use super::{Page, RareBox};
use crate::adb::DeviceConnection;
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

impl RareBox {
    pub(crate) fn render_connect(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_harmony() {
            return self.render_connect_harmony(cx);
        }
        let colors = *cx.theme().colors();
        let mobile = cfg!(target_os = "android");
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

        // 没有在线设备时，把引导放在最前面。
        let switch = self.transport_switch(cx);
        let mut content = div().w_full().flex().flex_col().gap(px(16.)).child(switch);
        if online == 0 {
            content = content.child(guide_card(cx, weak.clone(), adb_ready, adb_error.clone()));
        }

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
            .child(
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
                                .child(if cfg!(target_os = "android") {
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
                    cx.listener(|app, _, _, cx| app.wireless_connect(cx)),
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
            .child(
                div()
                    .mt(px(16.))
                    .pt(px(16.))
                    .border_t_1()
                    .border_color(colors.outline_variant.opacity(0.4))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.on_surface)
                            .child("首次配对"),
                    )
                    .child(if mobile {
                        div()
                            .mt(px(10.))
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
                            .mt(px(10.))
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
                    .child(div().mt(px(10.)).child(ui::callout(
                        cx,
                        "info",
                        "在手表「开发者选项 → 无线调试」中开启配对，输入手表显示的配对地址与六位配对码。配对成功后，用上方连接地址连接。",
                    ))),
            );

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
                .when(cfg!(windows), |el| el.child(driver_card(cx, weak.clone()))),
        )
        .into_any_element()
    }
}

/// 首次连接引导：按真实状态标注每一步是否已完成。
fn guide_card(
    cx: &App,
    weak: gpui::WeakEntity<RareBox>,
    adb_ready: bool,
    adb_error: Option<String>,
) -> Card {
    let colors = *cx.theme().colors();
    let step = |index: usize, done: bool, title: &'static str, detail: &'static str| {
        let (dot, text) = if done {
            (colors.tertiary, colors.on_surface)
        } else {
            (colors.outline_variant, colors.on_surface_variant)
        };
        div()
            .flex()
            .items_start()
            .gap(px(10.))
            .child(
                div()
                    .size(px(20.))
                    .flex_none()
                    .rounded_full()
                    .bg(if done {
                        colors.tertiary_container
                    } else {
                        colors.surface_container_high
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if done { dot } else { text })
                            .child(if done {
                                "✓".to_string()
                            } else {
                                format!("{}", index + 1)
                            }),
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
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(text)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .line_height(px(16.))
                            .text_color(colors.on_surface_variant)
                            .child(detail),
                    ),
            )
    };

    Card::new()
        .outlined()
        .p(px(20.))
        .child(ui::section_header(
            cx,
            "连接一台新设备",
            Some("RareBox 通过 ADB 与手表通信，完成下面三步即可"),
            None,
        ))
        .child(
            div()
                .mt(px(16.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(step(
                    0,
                    adb_ready,
                    "准备 ADB",
                    if cfg!(target_os = "android") {
                        "应用已内置 adb，无需另外安装；也可以在「设置」中指定外部 adb 路径。"
                    } else {
                        "RareBox 会自动查找 platform-tools；找不到时可在「设置」指定 adb.exe。"
                    },
                ))
                .child(step(
                    1,
                    false,
                    if cfg!(target_os = "android") {
                        "在手表上开启无线调试"
                    } else {
                        "用 USB 数据线连接手表"
                    },
                    if cfg!(target_os = "android") {
                        "在手表「设置 → 系统 → 开发者选项 → 无线调试」中开启配对，然后使用下方的无线调试卡片连接。"
                    } else {
                        "手表上进入「设置 → 系统 → 开发者选项」，打开「USB 调试」，并在弹窗中点「允许」。"
                    },
                ))
                .child(step(
                    2,
                    false,
                    "确认设备出现在下方列表",
                    if cfg!(target_os = "android") {
                        "配对并连接后设备会出现在下方列表；若显示「离线」，请检查地址、端口与网络。"
                    } else {
                        "首次授权后设备状态会变成「已连接」；如果显示「未授权」，请在手表上重新确认授权。"
                    },
                )),
        )
        .when_some(adb_error, |el, error| {
            el.child(div().mt(px(12.)).child(ui::callout(
                cx,
                "warning",
                format!("ADB 尚未就绪：{error}"),
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
                    "connect-guide-refresh",
                    "重新检测",
                    Some("refresh"),
                    ActionKind::Filled,
                    true,
                    {
                        let weak = weak.clone();
                        move |_, _, cx| {
                            if let Err(err) = weak.update(cx, |app, cx| app.refresh_devices(cx)) {
                                eprintln!("RareBox Desktop: 重新检测设备失败: {err}");
                            }
                        }
                    },
                ))
                .child(ui::action_button(
                    cx,
                    "connect-guide-settings",
                    "ADB 设置",
                    Some("settings"),
                    ActionKind::Outlined,
                    true,
                    {
                        let weak = weak.clone();
                        move |_, _, cx| {
                            if let Err(err) =
                                weak.update(cx, |app, cx| app.set_page(Page::Settings, cx))
                            {
                                eprintln!("RareBox Desktop: 打开设置失败: {err}");
                            }
                        }
                    },
                )),
        )
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
