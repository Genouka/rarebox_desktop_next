//! logs 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::DISPLAY_LIMIT;
use super::shell_ui::{device_required, page_scroll};
use super::{LogFilter, Page, RareBox};
use crate::adb::{LogLevel, LogLine};
use crate::config::short_path;
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, FontWeight, IntoElement, ParentElement, Styled,
    div, prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

impl RareBox {
    pub(crate) fn render_logs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_harmony() {
            return self.render_harmony_logs(cx);
        }
        let weak = cx.entity().downgrade();
        if !self.has_device() {
            return page_scroll(
                Page::Logs,
                &self.scroll_handles[Page::Logs as usize],
                device_required(
                    cx,
                    weak,
                    "连接手表后查看日志",
                    "日志页使用 adb logcat -d 抓取最近 1500 行，并按级别和关键字在本地筛选。",
                ),
            )
            .into_any_element();
        }

        let colors = *cx.theme().colors();
        let query = self.logs_query.trim().to_ascii_lowercase();
        let filter = self.logs_filter;
        let all: Vec<LogLine> = self
            .logs
            .iter()
            .filter(|line| filter.accepts(line.level))
            .filter(|line| {
                query.is_empty()
                    || line.message.to_ascii_lowercase().contains(&query)
                    || line.tag.to_ascii_lowercase().contains(&query)
                    || line.raw.to_ascii_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        let total = all.len();
        let displayed: Vec<LogLine> = all.into_iter().rev().take(DISPLAY_LIMIT).collect();
        let shown = displayed.len();

        let filters = LogFilter::ALL.iter().map(|candidate| {
            let candidate = *candidate;
            ui::filter_chip(
                cx,
                ("log-level", candidate as usize),
                candidate.label(),
                filter == candidate,
                cx.listener(move |app, _, _, cx| {
                    app.logs_filter = candidate;
                    cx.notify();
                }),
            )
        });

        let loading = self
            .activity
            .as_ref()
            .map(|activity| activity.label.contains("Logcat"))
            .unwrap_or(false);

        let copy_all: Option<String> = if displayed.is_empty() {
            None
        } else {
            Some(
                displayed
                    .iter()
                    .rev()
                    .map(|line| line.raw.clone())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        };

        let export = ui::action_button(
            cx,
            "logs-export",
            "导出",
            Some("save"),
            ActionKind::Tonal,
            total > 0,
            cx.listener(|app, _, _, cx| app.export_logs(cx)),
        );

        let refresh_logs = ui::action_button(
            cx,
            "logs-refresh",
            "抓取 Logcat",
            Some("refresh"),
            ActionKind::Filled,
            !loading,
            cx.listener(|app, _, _, cx| app.load_logs(cx)),
        );
        let clear = ui::action_button(
            cx,
            "logs-clear",
            "清空",
            Some("delete"),
            ActionKind::Text,
            true,
            cx.listener(|app, _, _, cx| app.clear_logcat(cx)),
        );
        let mobile = cfg!(target_os = "android");

        let toolbar = Card::new()
            .outlined()
            .p(px(16.))
            .child(if mobile {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.logs_query_field.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(refresh_logs)
                            .child(export)
                            .child(clear),
                    )
            } else {
                div()
                    .flex()
                    .items_end()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.logs_query_field.clone()),
                    )
                    .child(refresh_logs)
                    .child(export)
                    .child(clear)
            })
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .children(filters)
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child(match (self.logs_at.as_ref(), total) {
                                (Some(time), count) => {
                                    format!("{count} / {} 条 · 抓取于 {time}", self.logs.len())
                                }
                                (None, count) => format!("{count} / {} 条", self.logs.len()),
                            }),
                    )
                    .when_some(copy_all, |el, text| {
                        el.child(ui::action_button(
                            cx,
                            "logs-copy",
                            "复制",
                            Some("content_copy"),
                            ActionKind::Text,
                            true,
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            },
                        ))
                    }),
            );

        let list: AnyElement = if displayed.is_empty() {
            let action = Some(
                ui::action_button(
                    cx,
                    "logs-empty-load",
                    "抓取 Logcat",
                    Some("subject"),
                    ActionKind::Tonal,
                    true,
                    cx.listener(|app, _, _, cx| app.load_logs(cx)),
                )
                .into_any_element(),
            );
            Card::new()
                .outlined()
                .p(px(20.))
                .child(ui::empty_block(
                    cx,
                    "subject",
                    if self.logs.is_empty() {
                        "还没有日志"
                    } else {
                        "没有匹配的日志"
                    },
                    if self.logs.is_empty() {
                        "从设备抓取最近 1500 行 Logcat，然后在这里按级别与关键字筛选。"
                    } else {
                        "试试切换日志级别，或修改搜索关键字。"
                    },
                    action,
                ))
                .into_any_element()
        } else {
            Card::new()
                .outlined()
                .p(px(6.))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .children(displayed.iter().enumerate().map(|(index, line)| {
                            log_row(cx, index, line, displayed.len())
                        })),
                )
                .when(shown < total, |el| {
                    el.child(
                        div()
                            .w_full()
                            .px(px(14.))
                            .py(px(8.))
                            .text_size(px(10.))
                            .text_color(colors.on_surface_variant)
                            .child(format!(
                                "仅显示最新 {shown} 条（共 {total} 条匹配）。缩小筛选范围可查看更早日志。"
                            )),
                    )
                })
                .into_any_element()
        };

        let last_export = self.last_export.clone().map(|path| {
            let open_path = path.clone();
            let reveal_path = path.clone();
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(10.))
                .p(px(12.))
                .rounded(px(12.))
                .bg(colors.surface_container)
                .child(
                    Icon::new(IconName::new("save"))
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
                                .child("最近导出的日志"),
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
                .child(ui::action_button(
                    cx,
                    "logs-open-export",
                    "打开",
                    Some("open_in_new"),
                    ActionKind::Text,
                    true,
                    move |_, _, cx| cx.open_with_system(&open_path),
                ))
                .child(ui::action_button(
                    cx,
                    "logs-reveal-export",
                    "文件夹",
                    Some("folder_open"),
                    ActionKind::Text,
                    true,
                    move |_, _, cx| cx.reveal_path(&reveal_path),
                ))
        });

        page_scroll(
            Page::Logs,
            &self.scroll_handles[Page::Logs as usize],
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(toolbar)
                .child(list)
                .when_some(last_export, |el, export| el.child(export)),
        )
        .into_any_element()
    }
}

fn log_row(cx: &App, index: usize, line: &LogLine, total: usize) -> Div {
    let colors = *cx.theme().colors();
    let (background, foreground) = level_colors(cx, line.level);
    let is_last = index + 1 == total;
    let tag = if line.tag.is_empty() {
        "system".to_string()
    } else {
        line.tag.clone()
    };

    div()
        .w_full()
        .px(px(10.))
        .py(px(7.))
        .flex()
        .items_start()
        .gap(px(9.))
        .rounded(px(8.))
        .when(!is_last, |el| {
            el.border_b_1()
                .border_color(colors.outline_variant.opacity(0.22))
        })
        .hover(move |style| style.bg(colors.surface_container))
        .child(
            div()
                .w(px(22.))
                .h(px(18.))
                .flex_none()
                .rounded(px(5.))
                .bg(background)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(foreground)
                .child(line.level.label()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(9.5))
                                .font_family(MONO_FONT)
                                .text_color(colors.on_surface_variant.opacity(0.7))
                                .child(if line.time.is_empty() {
                                    "—".to_string()
                                } else {
                                    line.time.clone()
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .font_family(MONO_FONT)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(colors.on_surface_variant)
                                .child(tag),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .font_family(MONO_FONT)
                        .text_color(if line.level == LogLevel::Unknown {
                            colors.on_surface_variant
                        } else {
                            colors.on_surface
                        })
                        .child(if line.message.is_empty() {
                            line.raw.clone()
                        } else {
                            line.message.clone()
                        }),
                ),
        )
}

fn level_colors(cx: &App, level: LogLevel) -> (gpui::Hsla, gpui::Hsla) {
    let colors = *cx.theme().colors();
    match level {
        LogLevel::Verbose => (colors.surface_container_high, colors.on_surface_variant),
        LogLevel::Debug => (colors.secondary_container, colors.on_secondary_container),
        LogLevel::Info => (colors.primary_container, colors.on_primary_container),
        LogLevel::Warn => (colors.tertiary_container, colors.on_tertiary_container),
        LogLevel::Error | LogLevel::Fatal => (colors.error_container, colors.on_error_container),
        LogLevel::Unknown => (colors.surface_container, colors.on_surface_variant),
    }
}
