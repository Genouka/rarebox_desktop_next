//! overview 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::shell_ui::{device_required, page_scroll};
use super::{Page, RareBox};
use crate::adb::{DeviceSnapshot, RebootTarget, format_duration};
use crate::ui::MONO_FONT;
use crate::ui::{self, ActionKind};
use gpui::{
    AnyElement, App, ClickEvent, Div, FontWeight, Hsla, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::*, px, relative,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

impl RareBox {
    pub(crate) fn render_overview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_harmony() {
            return self.render_harmony_overview(cx);
        }
        let snapshot = self.snapshot.clone();
        let weak = cx.entity().downgrade();
        let adb_error = self.adb_error.clone();
        let refreshed = self.snapshot_at.clone();

        let content = if let Some(snapshot) = snapshot {
            overview_content(cx, &snapshot, refreshed, weak)
        } else {
            let mut empty =
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .child(Card::new().outlined().p(px(24.)).child(device_required(
                    cx,
                    weak,
                    "还没有连接手表",
                    if cfg!(any(target_os = "android", target_os = "ios")) {
                        "通过无线 ADB 连接同一网络中的手表，或用 USB OTG 线直连（系统弹窗允许 USB 权限）。"
                    } else {
                        "使用 USB 数据线连接 Wear OS 手表并开启 USB 调试，或前往设置使用无线调试。"
                    },
                )));
            if let Some(error) = adb_error {
                empty = empty.child(ui::callout(cx, "warning", format!("ADB 尚未就绪：{error}")));
            }
            empty
        };

        page_scroll(
            Page::Overview,
            &self.scroll_handles[Page::Overview as usize],
            content,
        )
        .into_any_element()
    }
}

fn overview_content(
    cx: &mut Context<RareBox>,
    snapshot: &DeviceSnapshot,
    refreshed: Option<String>,
    weak: gpui::WeakEntity<RareBox>,
) -> Div {
    let colors = *cx.theme().colors();
    let typography = *cx.theme().typography();
    let mobile = cfg!(any(target_os = "android", target_os = "ios"));

    let title = snapshot.title();
    let wearable = snapshot.is_wearable();
    let battery = snapshot.battery_level;
    let rotation = match snapshot.display.rotation {
        1 => "90°",
        2 => "180°",
        3 => "270°",
        _ => "0°",
    };

    let screenshot = ui::action_button(
        cx,
        "overview-screenshot",
        "截取屏幕",
        Some("screenshot_monitor"),
        ActionKind::Filled,
        true,
        cx.listener(|app, _, _, cx| app.take_screenshot(cx)),
    );
    let refresh = ui::action_button(
        cx,
        "overview-refresh",
        "刷新",
        Some("refresh"),
        ActionKind::Outlined,
        true,
        cx.listener(|app, _, _, cx| app.refresh_devices(cx)),
    );
    let open_apps = ui::action_button(
        cx,
        "overview-apps",
        "管理应用",
        Some("apps"),
        ActionKind::Tonal,
        true,
        cx.listener(|app, _, _, cx| app.set_page(Page::Apps, cx)),
    );

    let hero = div()
        .relative()
        .w_full()
        .overflow_hidden()
        .rounded(px(24.))
        .bg(colors.primary_container)
        .p(px(26.))
        .flex()
        .items_center()
        .gap(px(20.))
        .child(
            div()
                .absolute()
                .right(px(-30.))
                .top(px(-70.))
                .size(px(240.))
                .rounded_full()
                .bg(colors.on_primary_container.opacity(0.05)),
        )
        .child(
            div()
                .absolute()
                .right(px(90.))
                .bottom(px(-90.))
                .size(px(190.))
                .rounded_full()
                .bg(colors.on_primary_container.opacity(0.04)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .child(
                            div()
                                .px(px(9.))
                                .h(px(24.))
                                .rounded_full()
                                .bg(colors.on_primary_container.opacity(0.09))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(ui::status_dot(colors.tertiary))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(colors.on_primary_container)
                                        .child(if wearable {
                                            "Wear OS 手表已连接"
                                        } else {
                                            "Android 设备已连接"
                                        }),
                                ),
                        )
                        .child(
                            ui::copyable(
                                div(),
                                weak.clone(),
                                "overview-hero-android",
                                format!("Android {}", snapshot.android),
                            )
                            .child(ui::tag(
                                format!("Android {}", snapshot.android),
                                colors.on_primary_container.opacity(0.09),
                                colors.on_primary_container,
                            )),
                        ),
                )
                .child(
                    ui::copyable(div(), weak.clone(), "overview-hero-title", title.clone()).child(
                        typography
                            .headline_large
                            .apply(div())
                            .text_color(colors.on_primary_container)
                            .truncate()
                            .child(title),
                    ),
                )
                .child(
                    ui::copyable(
                        div()
                            .text_size(px(11.5))
                            .font_family(MONO_FONT)
                            .text_color(colors.on_primary_container.opacity(0.72)),
                        weak.clone(),
                        "overview-hero-serial",
                        snapshot.serial.clone(),
                    )
                    .child(snapshot.serial.clone()),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(screenshot)
                        .child(refresh)
                        .child(open_apps),
                ),
        )
        .when(!mobile, |el| {
            el.child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .pr(px(16.))
                    .child(
                        div()
                            .size(px(112.))
                            .rounded_full()
                            .bg(colors.on_primary_container.opacity(0.08))
                            .border_1()
                            .border_color(colors.on_primary_container.opacity(0.12))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::new("watch"))
                                    .size(px(54.))
                                    .color(colors.on_primary_container),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.on_primary_container)
                            .child(
                                battery
                                    .map(|level| format!("{level}%"))
                                    .unwrap_or_else(|| "—".to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_primary_container.opacity(0.72))
                            .child(snapshot.battery_status_label()),
                    ),
            )
        });

    let battery_fraction = snapshot.battery_percent();
    let memory_fraction = snapshot.memory_percent();
    let storage_fraction = if snapshot.storage.percent > 0 {
        Some(f32::from(snapshot.storage.percent) / 100.0)
    } else {
        None
    };

    let stats = div()
        .w_full()
        .flex()
        .flex_wrap()
        .gap(px(14.))
        .child(stat_card(
            cx,
            weak.clone(),
            "overview-stat-battery",
            "battery_full",
            "电量",
            battery
                .map(|level| format!("{level}%"))
                .unwrap_or_else(|| "—".to_string()),
            snapshot
                .battery_temperature
                .map(|temp| format!("{temp:.1} °C · {}", snapshot.battery_status_label()))
                .unwrap_or_else(|| snapshot.battery_status_label().to_string()),
            battery_fraction,
            false,
        ))
        .child(stat_card(
            cx,
            weak.clone(),
            "overview-stat-storage",
            "storage",
            "存储空间",
            if snapshot.storage.size.is_empty() {
                "—".to_string()
            } else {
                format!("{}%", snapshot.storage.percent)
            },
            if snapshot.storage.size.is_empty() {
                "无法读取 /data".to_string()
            } else {
                format!("{} / {}", snapshot.storage.used, snapshot.storage.size)
            },
            storage_fraction,
            true,
        ))
        .child(stat_card(
            cx,
            weak.clone(),
            "overview-stat-memory",
            "memory",
            "运行内存",
            memory_fraction
                .map(|fraction| format!("{:.0}%", fraction * 100.0))
                .unwrap_or_else(|| "—".to_string()),
            snapshot.memory_used_label(),
            memory_fraction,
            true,
        ))
        .child(stat_card(
            cx,
            weak.clone(),
            "overview-stat-uptime",
            "hourglass_empty",
            "运行时间",
            snapshot
                .uptime_seconds
                .map(|seconds| {
                    if seconds >= 3600 {
                        format!("{} 小时", seconds / 3600)
                    } else {
                        format!("{} 分钟", seconds / 60)
                    }
                })
                .unwrap_or_else(|| "—".to_string()),
            snapshot
                .load
                .as_ref()
                .map(|load| format!("负载 {load}"))
                .unwrap_or_else(|| "负载 —".to_string()),
            None,
            false,
        ));

    let details = Card::new()
        .outlined()
        .p(px(20.))
        .when(!mobile, |el| el.flex_1())
        .min_w(px(0.))
        .child(ui::section_header(
            cx,
            "设备信息",
            Some("来自 getprop 与系统服务"),
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
                        .child(ui::key_value(cx, weak.clone(), "型号", snapshot.model.clone(), false).flex_1())
                        .child(
                            ui::key_value(cx, weak.clone(), "厂商", snapshot.manufacturer.clone(), false)
                                .flex_1(),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(20.))
                        .child(
                            ui::key_value(cx, weak.clone(), "Android", snapshot.android.clone(), false).flex_1(),
                        )
                        .child(ui::key_value(cx, weak.clone(), "SDK", snapshot.sdk.clone(), true).flex_1()),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(20.))
                        .child(
                            ui::key_value(cx, weak.clone(), "Build ID", snapshot.build_id.clone(), true).flex_1(),
                        )
                        .child(
                            ui::key_value(cx, weak.clone(), "安全补丁", snapshot.patch.clone(), true).flex_1(),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(20.))
                        .child(ui::key_value(cx, weak.clone(), "ABI", snapshot.abi.clone(), true).flex_1())
                        .child(
                            ui::key_value(
                                cx,
                                weak.clone(),
                                "无线地址",
                                snapshot
                                    .ip
                                    .clone()
                                    .unwrap_or_else(|| "未连接 Wi-Fi".to_string()),
                                true,
                            )
                            .flex_1(),
                        ),
                )
                .child(ui::callout(
                    cx,
                    "info",
                    snapshot
                        .load
                        .as_ref()
                        .map(|load| {
                            format!(
                                "系统负载 {load} · 已运行 {} · 电池 {} mV",
                                format_duration(snapshot.uptime_seconds.unwrap_or(0)),
                                snapshot
                                    .battery_voltage
                                    .map(|voltage| voltage.to_string())
                                    .unwrap_or_else(|| "—".to_string())
                            )
                        })
                        .unwrap_or_else(|| "ADB 连接稳定，可以开始调试。".to_string()),
                )),
        );

    let display = Card::new()
        .outlined()
        .p(px(20.))
        .flex_1()
        .min_w(px(0.))
        .child(ui::section_header(
            cx,
            "屏幕与显示",
            Some("当前显示参数"),
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
                            ui::key_value(cx, weak.clone(), "分辨率", snapshot.display.size().to_string(), true)
                                .flex_1(),
                        )
                        .child(
                            ui::key_value(cx, weak.clone(), "DPI", snapshot.display.density().to_string(), true)
                                .flex_1(),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(20.))
                        .child(ui::key_value(cx, weak.clone(), "旋转", rotation, false).flex_1())
                        .child(
                            ui::key_value(
                                cx,
                                weak.clone(),
                                "物理分辨率",
                                snapshot.display.physical_size.clone(),
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
                            "当前使用了分辨率或 DPI 覆盖值，可在「屏幕与显示」页一键重置。",
                        ))
                    },
                )
                .child(div().mt(px(2.)).flex().child(ui::action_button(
                    cx,
                    "overview-screen",
                    "打开显示控制",
                    Some("tune"),
                    ActionKind::Tonal,
                    true,
                    cx.listener(|app, _, _, cx| app.set_page(Page::Screen, cx)),
                ))),
        );

    let quick = Card::new()
        .outlined()
        .p(px(20.))
        .flex_1()
        .min_w(px(0.))
        .child(ui::section_header(
            cx,
            "快捷操作",
            Some("常用功能一步直达"),
            None,
        ))
        .child(
            div()
                .mt(px(12.))
                .flex()
                .flex_col()
                .child(quick_row(
                    cx,
                    "overview-quick-apps",
                    "apps",
                    "应用管理",
                    "安装、启动、清理与导出应用",
                    colors.primary,
                    colors.primary_container,
                    cx.listener(|app, _, _, cx| app.set_page(Page::Apps, cx)),
                ))
                .child(quick_row(
                    cx,
                    "overview-quick-files",
                    "folder_open",
                    "文件传输",
                    "浏览 /sdcard 并导入导出文件",
                    colors.secondary,
                    colors.secondary_container,
                    cx.listener(|app, _, _, cx| app.set_page(Page::Files, cx)),
                ))
                .child(quick_row(
                    cx,
                    "overview-quick-terminal",
                    "terminal",
                    "ADB 终端",
                    "直接执行 shell 命令查看结果",
                    colors.tertiary,
                    colors.tertiary_container,
                    cx.listener(|app, _, _, cx| app.set_page(Page::Terminal, cx)),
                ))
                .child(quick_row(
                    cx,
                    "overview-quick-logs",
                    "subject",
                    "日志查看器",
                    "抓取 Logcat 并定位异常",
                    colors.error,
                    colors.error_container,
                    cx.listener(|app, _, _, cx| app.set_page(Page::Logs, cx)),
                )),
        )
        .child(
            div()
                .mt(px(8.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(12.))
                .bg(colors.surface_container)
                .flex()
                .items_center()
                .gap(px(9.))
                .child(
                    Icon::new(IconName::new("wifi_tethering"))
                        .size(px(18.))
                        .color(colors.on_surface_variant),
                )
                .child({
                    let text = snapshot
                        .ip
                        .as_ref()
                        .map(|ip| format!("设备 Wi-Fi 地址：{ip}"))
                        .unwrap_or_else(|| "可在设置中通过无线 ADB 连接手表".to_string());
                    ui::copyable(
                        div()
                            .flex_1()
                            .text_size(px(11.5))
                            .line_height(px(17.))
                            .text_color(colors.on_surface_variant),
                        weak.clone(),
                        "overview-wireless-address",
                        text.clone(),
                    )
                    .child(text)
                })
                .child(
                    div()
                        .id("overview-wireless-link")
                        .text_size(px(11.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.primary)
                        .cursor_pointer()
                        .on_click(cx.listener(|app, _, _, cx| app.set_page(Page::Settings, cx)))
                        .child("配置"),
                ),
        );

    let power = Card::new()
        .outlined()
        .p(px(20.))
        .child(ui::section_header(
            cx,
            "电源与重启",
            Some("通过 adb reboot 直接进入对应模式"),
            None,
        ))
        .child(
            div().mt(px(14.)).flex().flex_wrap().gap(px(10.)).children(
                RebootTarget::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, target)| {
                        let kind = if target == RebootTarget::System {
                            ActionKind::Filled
                        } else {
                            ActionKind::Outlined
                        };
                        ui::action_button(
                            cx,
                            ("overview-reboot", index),
                            target.menu_label(),
                            Some(target.icon()),
                            kind,
                            true,
                            cx.listener(move |app, _, _, cx| app.request_reboot(target, cx)),
                        )
                    }),
            ),
        )
        .child(
            div()
                .mt(px(12.))
                .text_size(px(10.5))
                .line_height(px(16.))
                .text_color(colors.on_surface_variant)
                .child(
                    "Bootloader / Fastbootd / EDL 下 ADB 不可用，需要 fastboot 或长按电源键退出。",
                ),
        );

    let updated = refreshed
        .map(|time| format!("上次刷新 {time}"))
        .unwrap_or_else(|| "尚未刷新".to_string());

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(hero)
        .child(stats)
        .child(
            div()
                .flex()
                .gap(px(16.))
                .when(!mobile, |el| el.items_start())
                .when(mobile, |el| el.flex_col())
                .child(details)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .min_w(px(0.))
                        .when(!mobile, |el| el.flex_1())
                        .child(display)
                        .child(quick),
                ),
        )
        .child(power)
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(10.5))
                .text_color(colors.on_surface_variant.opacity(0.75))
                .child(
                    ui::copyable(div(), weak.clone(), "overview-updated", updated.clone())
                        .child(updated),
                )
                .child(
                    ui::copyable(
                        div(),
                        weak.clone(),
                        "overview-footer-serial",
                        snapshot.serial.clone(),
                    )
                    .child(format!("序列号 {}", snapshot.serial)),
                )
        )
}

fn stat_card(
    cx: &App,
    weak: gpui::WeakEntity<RareBox>,
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    value: String,
    detail: String,
    fraction: Option<f32>,
    high_is_bad: bool,
) -> impl IntoElement {
    let copy_value = value.clone();
    let colors = *cx.theme().colors();
    let mut card = ui::panel()
        .w_full()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(30.))
                        .rounded(px(10.))
                        .bg(colors.surface_container)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Icon::new(IconName::new(icon))
                                .size(px(17.))
                                .color(colors.on_surface_variant),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.on_surface_variant)
                        .child(label),
                ),
        )
        .child(
            div()
                .mt(px(12.))
                .text_size(px(24.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.on_surface)
                .child(value),
        )
        .child(
            div()
                .mt(px(2.))
                .text_size(px(10.5))
                .text_color(colors.on_surface_variant)
                .truncate()
                .child(detail),
        );
    if let Some(fraction) = fraction {
        card = card.child(div().mt(px(12.)).child(meter(cx, fraction, high_is_bad)));
    } else {
        card = card.child(div().mt(px(12.)).h(px(6.)));
    }
    ui::copyable(div().flex_1().min_w(px(150.)), weak, id, copy_value).child(card)
}

fn meter(cx: &App, fraction: f32, high_is_bad: bool) -> Div {
    let colors = *cx.theme().colors();
    let fraction = fraction.clamp(0.0, 1.0);
    let color = if high_is_bad {
        ui::progress_color(cx, fraction)
    } else {
        colors.primary
    };
    div()
        .w_full()
        .h(px(6.))
        .rounded_full()
        .bg(colors.surface_container_highest)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(fraction))
                .rounded_full()
                .bg(color),
        )
}

#[allow(clippy::too_many_arguments)]
fn quick_row(
    cx: &App,
    id: &'static str,
    icon: &'static str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    accent: Hsla,
    accent_container: Hsla,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let title: SharedString = title.into();
    let description: SharedString = description.into();
    div()
        .id(id)
        .w_full()
        .px(px(10.))
        .py(px(9.))
        .rounded(px(12.))
        .flex()
        .items_center()
        .gap(px(11.))
        .cursor_pointer()
        .hover(move |style| style.bg(colors.surface_container))
        .child(
            div()
                .size(px(34.))
                .rounded(px(11.))
                .bg(accent_container)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(IconName::new(icon)).size(px(18.)).color(accent)),
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
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(colors.on_surface_variant)
                        .truncate()
                        .child(description),
                ),
        )
        .child(
            Icon::new(IconName::new("chevron_right"))
                .size(px(16.))
                .color(colors.on_surface_variant),
        )
        .on_click(on_click)
}
