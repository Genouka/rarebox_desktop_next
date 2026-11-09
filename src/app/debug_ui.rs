//! 调试工具页渲染（UI 层）：输入法切换与 AppOps 权限管理。
use super::ime_display;
use super::shell_ui::{device_required, page_scroll};
use super::{Confirm, ConfirmKind, Page, RareBox};
use crate::adb::{AppOp, AppOpMode, InputMethod};
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, ParentElement, Styled, WeakEntity, div,
    prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::overlay::{MenuItem, MenuState, show_menu};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

impl RareBox {
    pub(crate) fn render_debug(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        if !self.has_device() {
            return page_scroll(
                Page::Tools,
                &self.scroll_handles[Page::Tools as usize],
                device_required(
                    cx,
                    weak,
                    "连接手表后使用调试工具",
                    "输入法切换与 AppOps 权限管理都通过 adb 对一台已授权的设备执行。",
                ),
            )
            .into_any_element();
        }

        let ime_card = self.render_imes(cx);
        let appops_card = self.render_appops(cx);

        page_scroll(
            Page::Tools,
            &self.scroll_handles[Page::Tools as usize],
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(ui::callout(
                    cx,
                    "info",
                    "这里的操作会直接修改设备状态：切换输入法会影响当前键盘；AppOps 可以单独允许或拒绝某项权限，\
                     改动在重启后仍然保留，可随时重置。",
                ))
                .child(ime_card)
                .child(appops_card),
        )
        .into_any_element()
    }

    // ---------------------------------------------------------------------
    // 输入法
    // ---------------------------------------------------------------------

    fn render_imes(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let weak = cx.entity().downgrade();
        let loaded = self.ime_loaded_for.is_some();
        let imes = self.imes.clone();

        let refresh = ui::icon_button(
            cx,
            "tools-ime-refresh",
            "refresh",
            38.,
            colors.on_surface_variant,
            cx.listener(|app, _, _, cx| app.load_imes(cx)),
        );
        let open_settings = ui::action_button(
            cx,
            "tools-ime-settings",
            "系统输入法设置",
            Some("settings"),
            ActionKind::Text,
            true,
            cx.listener(|app, _, _, cx| app.open_ime_settings(cx)),
        );

        let mut card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "输入法",
                Some("通过 ime 命令启用、停用或切换当前输入法"),
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(open_settings)
                        .child(refresh)
                        .into_any_element(),
                ),
            ));

        if !loaded {
            card = card.child(
                div().mt(px(14.)).child(ui::empty_block(
                    cx,
                    "keyboard",
                    "还没有读取输入法",
                    "点击右上角刷新按钮，从设备读取已安装的输入法。",
                    None,
                )),
            );
        } else if imes.is_empty() {
            card = card.child(
                div().mt(px(14.)).child(ui::empty_block(
                    cx,
                    "keyboard",
                    "设备上没有可用输入法",
                    "系统可能需要先安装或启用一个键盘应用。",
                    None,
                )),
            );
        } else {
            let mut sorted: Vec<&InputMethod> = imes.iter().collect();
            sorted.sort_by_key(|ime| (!ime.active, !ime.enabled, ime.label.clone()));
            let total = sorted.len();
            let rows: Vec<Div> = sorted
                .iter()
                .enumerate()
                .map(|(index, ime)| ime_row(cx, index, ime, total, weak.clone()))
                .collect();
            card = card.child(
                div()
                    .mt(px(14.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(colors.outline_variant.opacity(0.45))
                    .overflow_hidden()
                    .child(div().flex().flex_col().children(rows)),
            );
        }

        card.into_any_element()
    }

    // ---------------------------------------------------------------------
    // AppOps
    // ---------------------------------------------------------------------

    fn render_appops(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = *cx.theme().colors();
        let weak = cx.entity().downgrade();
        let package = self.appops_package.trim().to_string();
        let loaded = !package.is_empty()
            && self.appops_loaded_for.as_deref() == Some(package.as_str());
        let ops = self.appops_ops.clone();

        let read = ui::action_button(
            cx,
            "tools-appops-read",
            "读取",
            Some("refresh"),
            ActionKind::Filled,
            !package.is_empty(),
            cx.listener(|app, _, _, cx| app.load_app_ops(cx)),
        );

        let package_row = if cfg!(any(target_os = "android", target_os = "ios")) {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(self.appops_package_field.clone())
                .child(div().flex().items_center().gap(px(8.)).child(read))
        } else {
            div()
                .flex()
                .items_end()
                .gap(px(10.))
                .child(div().flex_1().child(self.appops_package_field.clone()))
                .child(read)
        };

        let mut card = Card::new()
            .outlined()
            .p(px(20.))
            .child(ui::section_header(
                cx,
                "AppOps",
                Some("按包名查看和调整 appops 执行模式"),
                None,
            ))
            .child(div().mt(px(14.)).child(package_row));

        if loaded {
            let count = ops.len();
            let reset = ui::action_button(
                cx,
                "tools-appops-reset-all",
                "重置全部",
                Some("restart_alt"),
                ActionKind::Danger,
                count > 0,
                {
                    let package = package.clone();
                    let weak = weak.clone();
                    move |_, _, cx| {
                        let package = package.clone();
                        if let Err(err) = weak.update(cx, |app, cx| {
                            app.open_confirm(
                                Confirm {
                                    title: "重置这个应用的 AppOps？".into(),
                                    message: format!(
                                        "将把 {package} 上所有调整过的 AppOps 恢复为默认模式。\
                                         应用之后会重新按系统默认权限规则运行。"
                                    )
                                    .into(),
                                    confirm_label: "重置".into(),
                                    kind: ConfirmKind::AppOpsReset(package.clone()),
                                },
                                cx,
                            );
                        }) {
                            eprintln!("RareBox Desktop: 打开 AppOps 重置确认失败: {err}");
                        }
                    }
                },
            );

            card = card.child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .truncate()
                            .child(format!("{package} · 已调整 {count} 项")),
                    )
                    .child(reset),
            );

            if ops.is_empty() {
                card = card.child(div().mt(px(8.)).child(ui::callout(
                    cx,
                    "check_circle",
                    "这个应用的所有 AppOps 都是默认模式，没有需要显示或重置的条目。",
                )));
            } else {
                let mut sorted: Vec<&AppOp> = ops.iter().collect();
                sorted.sort_by(|left, right| left.name.cmp(&right.name));
                let total = sorted.len();
                let rows: Vec<Div> = sorted
                    .iter()
                    .enumerate()
                    .map(|(index, op)| {
                        appop_row(
                            cx,
                            index,
                            op,
                            total,
                            package.clone(),
                            weak.clone(),
                        )
                    })
                    .collect();
                card = card.child(
                    div()
                        .mt(px(12.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(colors.outline_variant.opacity(0.45))
                        .overflow_hidden()
                        .child(div().flex().flex_col().children(rows)),
                );
            }
        } else {
            card = card.child(div().mt(px(12.)).child(ui::callout(
                cx,
                "help",
                "输入包名后点击「读取」。只显示已经被显式调整过的 op；要设置其他 op，\
                 使用下方的自定义操作。",
            )));
        }

        card.child(self.render_appop_custom(cx)).into_any_element()
    }

    fn render_appop_custom(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        let mode = self.appops_mode;
        let chips: Vec<AnyElement> = AppOpMode::ALL
            .iter()
            .map(|candidate| {
                let candidate = *candidate;
                ui::filter_chip(
                    cx,
                    ("appop-mode", candidate as usize),
                    candidate.label(),
                    mode == candidate,
                    cx.listener(move |app, _, _, cx| {
                        app.appops_mode = candidate;
                        cx.notify();
                    }),
                )
                .into_any_element()
            })
            .collect();

        let apply = ui::action_button(
            cx,
            "tools-appops-apply",
            "应用",
            Some("save"),
            ActionKind::Filled,
            !self.appops_op_input.trim().is_empty(),
            cx.listener(|app, _, _, cx| app.apply_custom_app_op(cx)),
        );

        div()
            .mt(px(18.))
            .pt(px(14.))
            .border_t_1()
            .border_color(colors.outline_variant.opacity(0.45))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.on_surface)
                    .child("自定义操作"),
            )
            .child(
                div()
                    .text_size(px(10.5))
                    .text_color(colors.on_surface_variant)
                    .child(
                        "输入 op 名称并选择模式，例如 CAMERA、RECORD_AUDIO、READ_CLIPBOARD、\
                         ACTIVATE_VPN。名称区分大小写。",
                    ),
            )
            .child(
                if cfg!(any(target_os = "android", target_os = "ios")) {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(self.appops_op_field.clone())
                        .child(div().flex().items_center().gap(px(8.)).child(apply))
                } else {
                    div()
                        .flex()
                        .items_end()
                        .gap(px(10.))
                        .child(div().flex_1().child(self.appops_op_field.clone()))
                        .child(apply)
                },
            )
            .child(div().flex().flex_wrap().gap(px(6.)).children(chips))
    }
}

fn ime_row(
    cx: &mut Context<RareBox>,
    index: usize,
    ime: &InputMethod,
    total: usize,
    weak: WeakEntity<RareBox>,
) -> Div {
    let colors = *cx.theme().colors();
    let is_last = index + 1 == total;
    let title = ime
        .label
        .clone()
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| ime_display(&ime.id));
    let id = ime.id.clone();

    let mut actions = div().flex_none().flex().items_center().gap(px(6.));
    if !ime.active {
        let activate_weak = weak.clone();
        let activate_id = id.clone();
        actions = actions.child(ui::action_button(
            cx,
            ("ime-activate", index),
            "激活",
            Some("keyboard"),
            ActionKind::Filled,
            true,
            move |_, _, cx| {
                if let Err(err) = activate_weak
                    .update(cx, |app, cx| app.activate_ime(activate_id.clone(), cx))
                {
                    eprintln!("RareBox Desktop: 激活输入法失败: {err}");
                }
            },
        ));
    }
    if ime.enabled && !ime.active {
        let disable_weak = weak.clone();
        let disable_id = id.clone();
        actions = actions.child(ui::action_button(
            cx,
            ("ime-disable", index),
            "停用",
            None,
            ActionKind::Text,
            true,
            move |_, _, cx| {
                if let Err(err) = disable_weak.update(cx, |app, cx| {
                    app.set_ime_enabled(disable_id.clone(), false, cx)
                }) {
                    eprintln!("RareBox Desktop: 停用输入法失败: {err}");
                }
            },
        ));
    } else if !ime.enabled {
        let enable_weak = weak.clone();
        let enable_id = id.clone();
        actions = actions.child(ui::action_button(
            cx,
            ("ime-enable", index),
            "启用",
            None,
            ActionKind::Outlined,
            true,
            move |_, _, cx| {
                if let Err(err) = enable_weak.update(cx, |app, cx| {
                    app.set_ime_enabled(enable_id.clone(), true, cx)
                }) {
                    eprintln!("RareBox Desktop: 启用输入法失败: {err}");
                }
            },
        ));
    }

    let status = if ime.active {
        Some(ui::tag(
            "当前",
            colors.primary_container,
            colors.on_primary_container,
        ))
    } else if ime.enabled {
        Some(ui::tag(
            "已启用",
            colors.secondary_container,
            colors.on_secondary_container,
        ))
    } else {
        Some(ui::tag(
            "未启用",
            colors.surface_container_high,
            colors.on_surface_variant,
        ))
    };

    div()
        .w_full()
        .px(px(12.))
        .py(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .when(!is_last, |el| {
            el.border_b_1()
                .border_color(colors.outline_variant.opacity(0.35))
        })
        .child(
            div()
                .flex_none()
                .size(px(34.))
                .rounded(px(11.))
                .bg(if ime.active {
                    colors.primary_container
                } else {
                    colors.surface_container
                })
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::new("keyboard"))
                        .size(px(18.))
                        .color(if ime.active {
                            colors.on_primary_container
                        } else {
                            colors.on_surface_variant
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(3.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(colors.on_surface)
                                .truncate()
                                .child(title),
                        )
                        .when_some(status, |el, status| el.child(status)),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_family(MONO_FONT)
                        .text_color(colors.on_surface_variant.opacity(0.85))
                        .truncate()
                        .child(ime.id.clone()),
                ),
        )
        .child(actions)
}

fn appop_row(
    cx: &mut Context<RareBox>,
    index: usize,
    op: &AppOp,
    total: usize,
    package: String,
    weak: WeakEntity<RareBox>,
) -> Div {
    let colors = *cx.theme().colors();
    let is_last = index + 1 == total;
    let (background, foreground) = appop_mode_tag_colors(cx, op.mode);
    let name = op.name.clone();


    let mut menu_button = div()
        .id(("appop-more", index))
        .h(px(32.))
        .px(px(10.))
        .rounded_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .cursor_pointer()
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(colors.primary)
        .hover(move |style| style.bg(colors.primary.opacity(0.08)))
        .child(
            Icon::new(IconName::new("edit"))
                .size(px(14.))
                .color(colors.primary),
        )
        .child("调整");

    {
        let weak = weak.clone();
        let package = package.clone();
        let name = name.clone();
        menu_button = menu_button.on_click(move |event, window, cx| {
            let menu = build_appop_menu(weak.clone(), package.clone(), name.clone(), cx);
            show_menu(
                window,
                cx,
                menu,
                ui::menu_anchor(window, event.position(), 200., 336.),
            );
        });
    }

    div()
        .w_full()
        .px(px(12.))
        .py(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .when(!is_last, |el| {
            el.border_b_1()
                .border_color(colors.outline_variant.opacity(0.35))
        })
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .gap(px(3.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_family(MONO_FONT)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(colors.on_surface)
                                .truncate()
                                .child(name),
                        )
                        .child(ui::tag(op.mode.label(), background, foreground)),
                )
                .when_some(op.detail.clone(), |el, detail| {
                    el.child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant.opacity(0.85))
                            .truncate()
                            .child(detail),
                    )
                }),
        )
        .child(menu_button)
}

fn build_appop_menu(
    weak: WeakEntity<RareBox>,
    package: String,
    op: String,
    cx: &mut App,
) -> gpui::Entity<MenuState> {
    let mut menu = MenuState::new();
    for mode in AppOpMode::ALL {
        let weak = weak.clone();
        let package = package.clone();
        let op = op.clone();
        menu = menu.item(
            MenuItem::new(mode.label())
                .icon(IconName::new(match mode {
                    AppOpMode::Allowed => "check_circle",
                    AppOpMode::Denied => "close",
                    AppOpMode::Ignored => "folder_off",
                    AppOpMode::Foreground => "sensors",
                    AppOpMode::Default => "restart_alt",
                }))
                .on_click(move |_, cx| {
                    if let Err(err) = weak
                        .update(cx, |app, cx| {
                            app.set_app_op(package.clone(), op.clone(), mode, cx)
                        })
                    {
                        eprintln!("RareBox Desktop: 设置 AppOps 失败: {err}");
                    }
                }),
        );
    }
    menu.build(cx)
}

fn appop_mode_tag_colors(cx: &App, mode: AppOpMode) -> (gpui::Hsla, gpui::Hsla) {
    let colors = *cx.theme().colors();
    match mode {
        AppOpMode::Allowed => (colors.tertiary_container, colors.on_tertiary_container),
        AppOpMode::Foreground => (colors.secondary_container, colors.on_secondary_container),
        AppOpMode::Ignored => (colors.surface_container_high, colors.on_surface_variant),
        AppOpMode::Denied => (colors.error_container, colors.on_error_container),
        AppOpMode::Default => (colors.surface_container_high, colors.on_surface_variant),
    }
}
