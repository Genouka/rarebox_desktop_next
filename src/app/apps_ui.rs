//! apps 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::shell_ui::{device_required, page_scroll};
use super::{Confirm, ConfirmKind, PackageScope, Page, RareBox, ToastKind};
use crate::adb::AppPackage;
use crate::config::short_path;
use crate::install::InstallToggle;
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, Entity, FontWeight, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, div, prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::overlay::{MenuItem, MenuState, show_menu};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

impl RareBox {
    pub(crate) fn render_apps(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_harmony() {
            let content = self.render_harmony_apps(cx);
            return page_scroll(
                Page::Apps,
                &self.scroll_handles[Page::Apps as usize],
                content,
            )
            .into_any_element();
        }
        let weak = cx.entity().downgrade();
        if !self.has_device() {
            return page_scroll(
                Page::Apps,
                &self.scroll_handles[Page::Apps as usize],
                device_required(
                    cx,
                    weak,
                    "连接手表后管理应用",
                    "应用列表来自 pm list packages。请先连接设备，再回到这里刷新。",
                ),
            )
            .into_any_element();
        }

        let colors = *cx.theme().colors();
        let query = self.package_query.trim().to_ascii_lowercase();
        let scope = self.package_scope;
        let packages: Vec<&AppPackage> = self
            .packages
            .iter()
            .filter(|package| match scope {
                PackageScope::User => !package.system,
                PackageScope::System => package.system,
                PackageScope::All => true,
            })
            .filter(|package| {
                query.is_empty()
                    || package.name.to_ascii_lowercase().contains(&query)
                    || package.label.to_ascii_lowercase().contains(&query)
            })
            .collect();

        let user_count = self
            .packages
            .iter()
            .filter(|package| !package.system)
            .count();
        let system_count = self.packages.len().saturating_sub(user_count);
        let total = self.packages.len();
        let showing = packages.len();

        let scopes: Vec<AnyElement> = PackageScope::ALL
            .iter()
            .map(|candidate| {
                let candidate = *candidate;
                ui::filter_chip(
                    cx,
                    ("scope", candidate as usize),
                    candidate.label(),
                    scope == candidate,
                    cx.listener(move |app, _, _, cx| {
                        app.package_scope = candidate;
                        cx.notify();
                    }),
                )
                .into_any_element()
            })
            .collect();
        let install_options = self.render_install_options(cx);

        let metadata_pending = self.package_metadata_for.is_some();
        let install_button = ui::action_button(
            cx,
            "apps-install",
            "安装 APK",
            Some("install_desktop"),
            ActionKind::Filled,
            true,
            cx.listener(|app, _, _, cx| app.pick_and_install(cx)),
        );
        let refresh_button = ui::icon_button(
            cx,
            "apps-refresh",
            "refresh",
            38.,
            colors.on_surface_variant,
            cx.listener(|app, _, _, cx| app.load_packages(cx)),
        );
        let toolbar = Card::new()
            .outlined()
            .p(px(16.))
            .child(if cfg!(target_os = "android") {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.package_query_field.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(install_button)
                            .child(refresh_button),
                    )
            } else {
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.package_query_field.clone()),
                    )
                    .child(install_button)
                    .child(refresh_button)
            })
            .child(install_options)
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .children(scopes)
                    .child(div().flex_1())
                    .when(metadata_pending, |el| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(10.5))
                                .text_color(colors.primary)
                                .child(Icon::new(IconName::new("hourglass_empty")).size(px(13.)).color(colors.primary))
                                .child("正在解析应用名称…"),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child(format!(
                                "共 {total} 个 · 用户 {user_count} · 系统 {system_count} · 当前显示 {showing}"
                            )),
                    ),
            );

        let list = if packages.is_empty() {
            let (title, description) = if total == 0 {
                ("还没有读取应用列表", "点击刷新按钮从手表读取已安装的应用。")
            } else {
                ("没有匹配的应用", "试试切换筛选范围，或修改搜索关键字。")
            };
            let action = if total == 0 {
                Some(
                    ui::action_button(
                        cx,
                        "apps-empty-load",
                        "读取应用列表",
                        Some("refresh"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.load_packages(cx)),
                    )
                    .into_any_element(),
                )
            } else {
                None
            };
            Card::new().outlined().p(px(20.)).child(ui::empty_block(
                cx,
                "apps",
                title,
                description,
                action,
            ))
        } else {
            Card::new()
                .outlined()
                .p(px(8.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .children(packages.iter().enumerate().map(|(index, package)| {
                            package_row(cx, index, package, packages.len(), cx.entity().downgrade())
                        })),
                )
        };

        let export = self.last_export.clone().map(|path| {
            let open = ui::action_button(
                cx,
                "apps-open-export",
                "打开 APK",
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
                "apps-reveal-export",
                "所在文件夹",
                Some("folder_open"),
                ActionKind::Text,
                true,
                {
                    let path = path.clone();
                    move |_, _, cx| cx.reveal_path(&path)
                },
            );
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(10.))
                .p(px(12.))
                .rounded(px(12.))
                .bg(colors.surface_container)
                .child(
                    Icon::new(IconName::new("download"))
                        .size(px(18.))
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
                                .text_size(px(11.))
                                .text_color(colors.on_surface_variant)
                                .child("最近导出的 APK"),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .font_family(MONO_FONT)
                                .text_color(colors.on_surface)
                                .truncate()
                                .child(short_path(&path)),
                        ),
                )
                .child(open)
                .child(reveal)
        });

        let install_panel = self.render_install_panel(cx);

        page_scroll(
            Page::Apps,
            &self.scroll_handles[Page::Apps as usize],
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(toolbar)
                .when_some(install_panel, |el, panel| el.child(panel))
                .child(list)
                .when_some(export, |el, export| el.child(export)),
        )
        .into_any_element()
    }

    /// 安装参数开关。参数会写回配置，下次启动仍然生效。
    fn render_install_options(&self, cx: &mut Context<Self>) -> Div {
        let colors = *cx.theme().colors();
        let options = self.config.install;
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
            .children(InstallToggle::ALL.into_iter().map(|toggle| {
                ui::toggle_chip(
                    cx,
                    ("install-toggle", toggle as usize),
                    toggle.chip_label(),
                    options.get(toggle),
                    cx.listener(move |app, _, _, cx| app.toggle_install_option(toggle, cx)),
                )
            }))
    }

    /// 安装参数与本次安装的确认/结果面板。
    ///
    /// 分包与批量安装都要先看清「将要安装哪些应用」，所以参数、命令预览和结果
    /// 都放在同一张卡片里，选择文件后立刻出现。
    fn render_install_panel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let colors = *cx.theme().colors();
        let options = self.config.install;

        if let Some(plan) = self.install_plan.clone() {
            let apk_total: usize = plan.groups.iter().map(|group| group.apk_count()).sum();
            let split_count = plan
                .groups
                .iter()
                .filter(|group| group.is_split_install())
                .count();
            let summary = format!(
                "{} 个应用 · {apk_total} 个 APK{}",
                plan.groups.len(),
                if split_count > 0 {
                    format!(" · 其中 {split_count} 个分包安装")
                } else {
                    String::new()
                }
            );

            let targets = plan.groups.iter().enumerate().map(|(index, group)| {
                let files: Vec<String> = group
                    .files()
                    .iter()
                    .map(|path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| short_path(path))
                    })
                    .collect();
                div()
                    .id(("install-target", index))
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .py(px(10.))
                    .child(
                        div()
                            .size(px(30.))
                            .flex_none()
                            .rounded(px(10.))
                            .bg(if group.is_split_install() {
                                colors.tertiary_container
                            } else {
                                colors.primary_container
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(IconName::new("android")).size(px(16.)).color(
                                if group.is_split_install() {
                                    colors.on_tertiary_container
                                } else {
                                    colors.on_primary_container
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.on_surface)
                                    .truncate()
                                    .child(group.title()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(ui::tag(
                                        group.kind_label(),
                                        colors.surface_container_high,
                                        colors.on_surface_variant,
                                    ))
                                    .when_some(group.version_code, |el, code| {
                                        el.child(ui::tag(
                                            format!("versionCode {code}"),
                                            colors.surface_container_high,
                                            colors.on_surface_variant,
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_family(MONO_FONT)
                                    .text_color(colors.on_surface_variant)
                                    .child(files.join(" · ")),
                            ),
                    )
            });

            let command_preview = plan
                .groups
                .first()
                .map(|group| {
                    let mut parts = vec![
                        if group.is_split_install() {
                            "install-multiple"
                        } else {
                            "install"
                        }
                        .to_string(),
                    ];
                    parts.extend(options.flags().into_iter().map(str::to_string));
                    parts.extend(group.files().iter().map(|path| {
                        path.file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| short_path(path))
                    }));
                    parts.join(" ")
                })
                .unwrap_or_default();

            let failures = plan.failures.clone();
            let can_install = !plan.groups.is_empty();
            // 预览只展示第一条命令；多应用时说明后面还有几条。
            let preview_suffix = if plan.groups.len() > 1 {
                format!("（共 {} 条命令）", plan.groups.len())
            } else {
                String::new()
            };
            let note = if !can_install {
                "没有可以安装的应用，请重新选择文件。"
            } else if split_count > 0 {
                "分包会用 install-multiple 一次提交，主 APK 排在最前。"
            } else {
                "参数改动会立即反映在命令预览里，并在下次启动时保留。"
            };
            return Some(
                Card::new()
                    .outlined()
                    .p(px(16.))
                    .child(ui::section_header(
                        cx,
                        "准备安装",
                        Some(summary),
                        Some(
                            ui::action_button(
                                cx,
                                "install-cancel",
                                "取消",
                                None,
                                ActionKind::Text,
                                true,
                                cx.listener(|app, _, _, cx| app.cancel_install(cx)),
                            )
                            .into_any_element(),
                        ),
                    ))
                    .child(
                        div()
                            .mt(px(12.))
                            .text_size(px(10.5))
                            .font_family(MONO_FONT)
                            .text_color(colors.on_surface_variant)
                            .truncate()
                            .child(format!("adb {command_preview}{preview_suffix}")),
                    )
                    .child(div().mt(px(6.)).flex().flex_col().children(targets))
                    .when(!failures.is_empty(), |el| {
                        el.child(div().mt(px(4.)).flex().flex_col().gap(px(4.)).children(
                            failures.iter().map(|failure| {
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(8.))
                                    .child(
                                        div().mt(px(1.)).child(
                                            Icon::new(IconName::new("error"))
                                                .size(px(14.))
                                                .color(colors.error),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .text_size(px(11.))
                                            .line_height(px(17.))
                                            .text_color(colors.on_surface_variant)
                                            .child(format!(
                                                "{}：{}",
                                                failure.title, failure.detail
                                            )),
                                    )
                            }),
                        ))
                    })
                    .child(
                        div()
                            .mt(px(14.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(10.5))
                                    .text_color(colors.on_surface_variant)
                                    .child(note),
                            )
                            .child(ui::action_button(
                                cx,
                                "install-start",
                                if plan.groups.len() > 1 {
                                    format!("安装 {} 个应用", plan.groups.len())
                                } else {
                                    "开始安装".to_string()
                                },
                                Some("install_desktop"),
                                ActionKind::Filled,
                                can_install,
                                cx.listener(|app, _, _, cx| app.run_install_plan(cx)),
                            )),
                    )
                    .into_any_element(),
            );
        }

        let batch = self.install_batch.clone()?;
        let (succeeded, failed) = (batch.succeeded(), batch.failed());
        let rows = batch.reports.iter().enumerate().map(|(index, report)| {
            let (icon, color) = if report.success {
                ("check_circle", colors.tertiary)
            } else {
                ("error", colors.error)
            };
            div()
                .id(("install-report", index))
                .flex()
                .items_start()
                .gap(px(9.))
                .py(px(8.))
                .child(
                    div()
                        .mt(px(1.))
                        .child(Icon::new(IconName::new(icon)).size(px(15.)).color(color)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(colors.on_surface)
                                .truncate()
                                .child(report.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .line_height(px(16.))
                                .text_color(colors.on_surface_variant)
                                .child(report.detail.clone()),
                        ),
                )
        });

        Some(
            Card::new()
                .outlined()
                .p(px(16.))
                .child(ui::section_header(
                    cx,
                    "上次安装结果",
                    Some(if failed == 0 {
                        format!("{succeeded} 个应用全部安装成功")
                    } else {
                        format!("{succeeded} 个成功 · {failed} 个失败")
                    }),
                    Some(
                        ui::action_button(
                            cx,
                            "install-report-close",
                            "关闭",
                            Some("close"),
                            ActionKind::Text,
                            true,
                            cx.listener(|app, _, _, cx| {
                                app.install_batch = None;
                                cx.notify();
                            }),
                        )
                        .into_any_element(),
                    ),
                ))
                .child(div().mt(px(8.)).flex().flex_col().children(rows))
                .into_any_element(),
        )
    }
}

fn package_row(
    cx: &App,
    index: usize,
    package: &AppPackage,
    total: usize,
    weak: gpui::WeakEntity<RareBox>,
) -> Div {
    let colors = *cx.theme().colors();
    let name = package.name.clone();
    let title = package.label_or_name().to_string();
    let version = package.version_label();
    let has_label = package.has_label();
    let system = package.system;
    let is_last = index + 1 == total;

    let launch_weak = weak.clone();
    let launch_name = name.clone();
    let launch = ui::action_button(
        cx,
        ("apps-launch", index),
        "启动",
        Some("play_arrow"),
        ActionKind::Tonal,
        true,
        move |_, _, cx| {
            if let Err(err) =
                launch_weak.update(cx, |app, cx| app.launch_package(launch_name.clone(), cx))
            {
                eprintln!("RareBox Desktop: 启动应用失败: {err}");
            }
        },
    );

    let stop_weak = weak.clone();
    let stop_name = name.clone();
    let stop = ui::action_button(
        cx,
        ("apps-stop", index),
        "停止",
        Some("stop_circle"),
        ActionKind::Text,
        true,
        move |_, _, cx| {
            if let Err(err) =
                stop_weak.update(cx, |app, cx| app.stop_package(stop_name.clone(), cx))
            {
                eprintln!("RareBox Desktop: 停止应用失败: {err}");
            }
        },
    );

    let more_weak = weak.clone();
    let more_name = name.clone();
    let more_display = title.clone();
    let more = {
        div()
            .id(("apps-more", index))
            .size(px(38.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(move |style| style.bg(colors.surface_container_high))
            .child(
                Icon::new(IconName::new("more_horiz"))
                    .size(px(19.))
                    .color(colors.on_surface_variant),
            )
            .on_click(move |event, window, cx| {
                let menu = build_package_menu(
                    more_weak.clone(),
                    more_name.clone(),
                    more_display.clone(),
                    event,
                    cx,
                );
                show_menu(
                    window,
                    cx,
                    menu,
                    ui::menu_anchor(window, event.position(), 188., 312.),
                );
            })
    };

    div()
        .w_full()
        .h(px(62.))
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(10.))
        .when(!is_last, |el| {
            el.border_b_1()
                .border_color(colors.outline_variant.opacity(0.35))
        })
        .child(
            div()
                .size(px(34.))
                .rounded(px(11.))
                .bg(if system {
                    colors.surface_container
                } else {
                    colors.primary_container
                })
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::new(if system { "android" } else { "apps" }))
                        .size(px(18.))
                        .color(if system {
                            colors.on_surface_variant
                        } else {
                            colors.on_primary_container
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
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.on_surface)
                        .truncate()
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .min_w(px(0.))
                        .child(div().flex_1().min_w(px(0.)).when(has_label, |el| {
                            el.text_size(px(10.5))
                                .font_family(MONO_FONT)
                                .text_color(colors.on_surface_variant)
                                .truncate()
                                .child(name)
                        }))
                        .children((!version.is_empty()).then(|| {
                            ui::tag(
                                version,
                                colors.surface_container_high,
                                colors.on_surface_variant,
                            )
                        }))
                        .child(ui::tag(
                            if system {
                                "系统应用"
                            } else {
                                "用户应用"
                            },
                            if system {
                                colors.surface_container
                            } else {
                                colors.secondary_container
                            },
                            if system {
                                colors.on_surface_variant
                            } else {
                                colors.on_secondary_container
                            },
                        )),
                ),
        )
        .child(launch)
        .when(!cfg!(target_os = "android"), |el| el.child(stop))
        .child(more)
}

fn build_package_menu(
    weak: gpui::WeakEntity<RareBox>,
    package: String,
    display: String,
    _event: &ClickEvent,
    cx: &mut App,
) -> Entity<MenuState> {
    let mut menu = MenuState::new();

    {
        let weak = weak.clone();
        let name = package.clone();
        menu = menu.item(
            MenuItem::new("启动应用")
                .icon(IconName::new("play_arrow"))
                .on_click(move |_, cx| {
                    if let Err(err) =
                        weak.update(cx, |app, cx| app.launch_package(name.clone(), cx))
                    {
                        eprintln!("RareBox Desktop: 启动应用失败: {err}");
                    }
                }),
        );
    }
    {
        let weak = weak.clone();
        let name = package.clone();
        menu = menu.item(
            MenuItem::new("停止应用")
                .icon(IconName::new("stop_circle"))
                .on_click(move |_, cx| {
                    if let Err(err) = weak.update(cx, |app, cx| app.stop_package(name.clone(), cx))
                    {
                        eprintln!("RareBox Desktop: 停止应用失败: {err}");
                    }
                }),
        );
    }
    {
        let weak = weak.clone();
        let name = package.clone();
        menu = menu.item(
            MenuItem::new("导出 APK")
                .icon(IconName::new("download"))
                .on_click(move |_, cx| {
                    if let Err(err) =
                        weak.update(cx, |app, cx| app.export_package(name.clone(), cx))
                    {
                        eprintln!("RareBox Desktop: 导出 APK 失败: {err}");
                    }
                }),
        );
    }
    {
        let weak = weak.clone();
        let name = package.clone();
        let display = display.clone();
        menu = menu.item(
            MenuItem::new("复制包名")
                .icon(IconName::new("content_copy"))
                .on_click(move |_, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(name.clone()));
                    if let Err(err) = weak.update(cx, |app, cx| {
                        app.toast(
                            cx,
                            ToastKind::Info,
                            format!("已复制 {display} 的包名 {name}"),
                        );
                    }) {
                        eprintln!("RareBox Desktop: 复制包名提示失败: {err}");
                    }
                }),
        );
    }
    {
        let weak = weak.clone();
        let name = package.clone();
        menu = menu.item(
            MenuItem::new("AppOps 权限")
                .icon(IconName::new("build"))
                .on_click(move |_, cx| {
                    if let Err(err) = weak.update(cx, |app, cx| {
                        app.set_page(Page::Tools, cx);
                        app.open_app_ops(name.clone(), cx);
                    }) {
                        eprintln!("RareBox Desktop: 打开 AppOps 失败: {err}");
                    }
                }),
        );
    }
    {
        let weak = weak.clone();
        let name = package.clone();
        let display = display.clone();
        menu = menu.item(
            MenuItem::new("清除应用数据")
                .icon(IconName::new("cleaning_services"))
                .on_click(move |_, cx| {
                    let name = name.clone();
                    let display = display.clone();
                    if let Err(err) = weak.update(cx, |app, cx| {
                        app.open_confirm(
                            Confirm {
                                title: "清除应用数据？".into(),
                                message: format!(
                                    "将清除 {display}（{name}）的全部本地数据与登录状态，应用本身会保留。此操作无法撤销。"
                                )
                                .into(),
                                confirm_label: "清除数据".into(),
                                kind: ConfirmKind::ClearData(name.clone()),
                            },
                            cx,
                        );
                    }) {
                        eprintln!("RareBox Desktop: 打开清除确认框失败: {err}");
                    }
                }),
        );
    }
    {
        let weak = weak.clone();
        let name = package.clone();
        let display = display.clone();
        menu = menu.item(
            MenuItem::new("卸载应用")
                .icon(IconName::new("delete"))
                .on_click(move |_, cx| {
                    let name = name.clone();
                    let display = display.clone();
                    if let Err(err) = weak.update(cx, |app, cx| {
                        app.open_confirm(
                            Confirm {
                                title: "卸载这个应用？".into(),
                                message: format!(
                                    "将从手表卸载 {display}（{name}）。系统应用可能拒绝卸载，具体取决于设备权限。"
                                )
                                .into(),
                                confirm_label: "卸载".into(),
                                kind: ConfirmKind::Uninstall(name.clone()),
                            },
                            cx,
                        );
                    }) {
                        eprintln!("RareBox Desktop: 打开卸载确认框失败: {err}");
                    }
                }),
        );
    }

    menu.build(cx)
}
