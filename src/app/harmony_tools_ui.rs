//! harmony_tools 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::shell_ui::page_scroll;
use super::{LogFilter, Page, RareBox, TerminalEntry, ToastKind};
use crate::adb::{FileEntry, LogLevel};
use crate::config::short_path;
use crate::hdc::HdcLogLine;
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, App, ClipboardItem, Context, FontWeight, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, WeakEntity, div, prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

const LOG_DISPLAY_LIMIT: usize = 400;

impl RareBox {
    pub(crate) fn render_harmony_files(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.harmony.selected.is_none() {
            return ui::empty_block(
                cx,
                "devices",
                "请先选择设备",
                "在「连接」页选择 HarmonyOS（hdc）并连接设备后，再浏览文件。",
                None,
            )
            .into_any_element();
        }
        let colors = *cx.theme().colors();
        let entries = self.harmony.files.clone();
        let path = self.harmony.files_path.clone();
        let weak = cx.entity().downgrade();

        let up = ui::icon_button(
            cx,
            "harmony-files-up",
            "arrow_upward",
            38.,
            colors.on_surface_variant,
            cx.listener(|app, _, _, cx| app.go_parent_hdc_files(cx)),
        );
        let refresh = ui::icon_button(
            cx,
            "harmony-files-refresh",
            "refresh",
            38.,
            colors.on_surface_variant,
            cx.listener(|app, _, _, cx| app.load_hdc_files(cx)),
        );

        let toolbar = Card::new()
            .outlined()
            .p(px(16.))
            .child(if cfg!(target_os = "android") {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.harmony_files_field.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(up)
                            .child(ui::action_button(
                                cx,
                                "harmony-files-go",
                                "前往",
                                Some("arrow_forward"),
                                ActionKind::Tonal,
                                true,
                                cx.listener(|app, _, _, cx| {
                                    let path = app.harmony_files_field.read(cx).value().to_string();
                                    app.navigate_hdc_files(path, cx);
                                }),
                            ))
                            .child(refresh)
                            .child(ui::action_button(
                                cx,
                                "harmony-files-push",
                                "上传文件",
                                Some("upload"),
                                ActionKind::Filled,
                                true,
                                cx.listener(|app, _, _, cx| app.pick_and_push_hdc(cx)),
                            )),
                    )
            } else {
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(up)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.harmony_files_field.clone()),
                    )
                    .child(ui::action_button(
                        cx,
                        "harmony-files-go",
                        "前往",
                        Some("arrow_forward"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| {
                            let path = app.harmony_files_field.read(cx).value().to_string();
                            app.navigate_hdc_files(path, cx);
                        }),
                    ))
                    .child(refresh)
                    .child(ui::action_button(
                        cx,
                        "harmony-files-push",
                        "上传文件",
                        Some("upload"),
                        ActionKind::Filled,
                        true,
                        cx.listener(|app, _, _, cx| app.pick_and_push_hdc(cx)),
                    ))
            })
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child("快捷位置"),
                    )
                    .children(
                        [
                            ("/data/local/tmp", "临时目录"),
                            ("/storage/media/100/local/files", "用户文件"),
                            ("/data/app/el2/100/base", "应用数据"),
                            ("/data/log/hilog", "HiLog 日志"),
                            ("/data/log/faultlog/faultlogger", "Crash 日志"),
                            ("/system", "系统分区"),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, (target, label))| {
                            let target_owned = target.to_string();
                            let weak = weak.clone();
                            ui::filter_chip(
                                cx,
                                ("harmony-quick-dir", index),
                                label,
                                path == target,
                                move |_, _, cx| {
                                    if let Err(err) = weak.update(cx, |app, cx| {
                                        app.navigate_hdc_files(target_owned.clone(), cx);
                                    }) {
                                        eprintln!("RareBox Desktop: 打开鸿蒙快捷目录失败: {err}");
                                    }
                                },
                            )
                        }),
                    ),
            );

        let list = if entries.is_empty() {
            Card::new().outlined().p(px(20.)).child(ui::empty_block(
                cx,
                "folder_open",
                "目录为空或尚未读取",
                "点击刷新从设备读取当前目录；如果提示权限不足，请尝试 /data/local/tmp。",
                Some(
                    ui::action_button(
                        cx,
                        "harmony-files-empty-refresh",
                        "读取当前目录",
                        Some("refresh"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.load_hdc_files(cx)),
                    )
                    .into_any_element(),
                ),
            ))
        } else {
            let rows = entries
                .iter()
                .enumerate()
                .map(|(index, entry)| harmony_file_row(cx, index, entry, weak.clone()));
            Card::new()
                .outlined()
                .p(px(8.))
                .child(
                    div()
                        .h(px(34.))
                        .px(px(14.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .text_size(px(10.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.on_surface_variant)
                        .child(div().w(px(24.)))
                        .child(div().flex_1().child("名称"))
                        .when(!cfg!(target_os = "android"), |el| {
                            el.child(div().w(px(86.)).text_right().child("大小"))
                                .child(div().w(px(120.)).text_right().child("修改时间"))
                        })
                        .child(div().w(px(80.)).text_right().child("操作")),
                )
                .child(div().flex().flex_col().children(rows))
        };

        let export = self.harmony.last_file.clone().map(|path| {
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
                                .child("最近接收的文件"),
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
                    "harmony-files-open-export",
                    "打开",
                    Some("open_in_new"),
                    ActionKind::Text,
                    true,
                    move |_, _, cx| cx.open_with_system(&open_path),
                ))
                .child(ui::action_button(
                    cx,
                    "harmony-files-reveal-export",
                    "文件夹",
                    Some("folder_open"),
                    ActionKind::Text,
                    true,
                    move |_, _, cx| cx.reveal_path(&reveal_path),
                ))
        });

        page_scroll(
            Page::Files,
            &self.scroll_handles[Page::Files as usize],
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(toolbar)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(10.5))
                        .text_color(colors.on_surface_variant)
                        .child(Icon::new(IconName::new("folder")).size(px(14.)))
                        .child(
                            div()
                                .font_family(MONO_FONT)
                                .child(format!("{path} · {} 个条目", self.harmony.files.len())),
                        ),
                )
                .child(list)
                .when_some(export, |el, export| el.child(export)),
        )
        .into_any_element()
    }

    pub(crate) fn render_harmony_terminal(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.harmony.selected.is_none() {
            return ui::empty_block(
                cx,
                "devices",
                "请先选择设备",
                "在「连接」页选择 HarmonyOS（hdc）并连接设备后，再执行 shell 命令。",
                None,
            )
            .into_any_element();
        }
        let colors = *cx.theme().colors();
        let history = self.harmony.terminal.clone();
        let count = history.len();
        let weak = cx.entity().downgrade();

        let quick_commands = [
            ("bm dump -a", "应用列表"),
            ("jpid", "调试进程"),
            ("param get const.product.model", "设备型号"),
            ("hidumper -s PowerManagerService -a -s", "电源信息"),
            ("hidumper -s RenderService -a screen", "屏幕信息"),
            ("hidumper -s 3302 -a -i", "电池信息"),
            ("hilog -x", "抓取日志"),
            ("df -h /data", "存储空间"),
            ("ifconfig", "网络地址"),
            ("ls -la /data/local/tmp", "临时目录"),
        ];

        let toolbar = Card::new()
            .outlined()
            .p(px(18.))
            .child(ui::section_header(
                cx,
                "执行命令",
                Some("命令通过 hdc shell 在设备上执行，结果保留在下方"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .items_end()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.harmony_terminal_field.clone()),
                    )
                    .child(ui::action_button(
                        cx,
                        "harmony-terminal-run",
                        "执行",
                        Some("play_arrow"),
                        ActionKind::Filled,
                        true,
                        cx.listener(|app, _, _, cx| app.submit_hdc_terminal(cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(12.))
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child("快捷命令"),
                    )
                    .children(quick_commands.into_iter().enumerate().map(
                        |(index, (command, label))| {
                            let weak = weak.clone();
                            let command = command.to_string();
                            ui::filter_chip(
                                cx,
                                ("harmony-terminal-quick", index),
                                label,
                                false,
                                move |_, _, cx| {
                                    if let Err(err) = weak.update(cx, |app, cx| {
                                        app.run_hdc_quick_command(command.clone(), cx);
                                    }) {
                                        eprintln!("RareBox Desktop: 执行鸿蒙快捷命令失败: {err}");
                                    }
                                },
                            )
                        },
                    )),
            )
            .child(div().mt(px(12.)).child(ui::callout(
                cx,
                "warning",
                "终端执行的是阻塞式命令。hilog 等不会自动退出的命令请改用「日志查看器」页。",
            )));

        let list: AnyElement = if history.is_empty() {
            ui::empty_block(
                cx,
                "terminal",
                "还没有执行过命令",
                "输入任意 hdc shell 命令，或点击上方的快捷命令开始。",
                None,
            )
            .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .children(history.iter().enumerate().map(|(index, entry)| {
                    harmony_terminal_entry(cx, index, entry, cx.entity().downgrade())
                }))
                .into_any_element()
        };

        page_scroll(
            Page::Terminal,
            &self.scroll_handles[Page::Terminal as usize],
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(toolbar)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(colors.on_surface)
                                        .child("命令记录"),
                                )
                                .child(ui::tag(
                                    format!("{count}"),
                                    colors.surface_container_high,
                                    colors.on_surface_variant,
                                )),
                        )
                        .child(ui::action_button(
                            cx,
                            "harmony-terminal-clear",
                            "清空记录",
                            Some("delete"),
                            ActionKind::Text,
                            count > 0,
                            cx.listener(|app, _, _, cx| {
                                app.harmony.terminal.clear();
                                app.toast(cx, ToastKind::Info, "终端记录已清空");
                                cx.notify();
                            }),
                        )),
                )
                .child(list),
        )
        .into_any_element()
    }

    pub(crate) fn render_harmony_logs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.harmony.selected.is_none() {
            return ui::empty_block(
                cx,
                "devices",
                "请先选择设备",
                "在「连接」页选择 HarmonyOS（hdc）并连接设备后，再抓取 HiLog。",
                None,
            )
            .into_any_element();
        }
        let colors = *cx.theme().colors();
        let filter = self.harmony.logs_filter;
        let query = self.harmony.logs_query.trim().to_ascii_lowercase();
        let mut filtered: Vec<HdcLogLine> = self
            .harmony
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
        let total = filtered.len();
        filtered.reverse();
        filtered.truncate(LOG_DISPLAY_LIMIT);
        let shown = filtered.len();
        let loading = self
            .activity
            .as_ref()
            .map(|activity| activity.label.contains("HiLog"))
            .unwrap_or(false);
        let weak = cx.entity().downgrade();

        let filters = LogFilter::ALL.iter().map(|candidate| {
            let candidate = *candidate;
            ui::filter_chip(
                cx,
                ("harmony-log-level", candidate as usize),
                candidate.label(),
                filter == candidate,
                cx.listener(move |app, _, _, cx| {
                    app.harmony.logs_filter = candidate;
                    cx.notify();
                }),
            )
        });

        let toolbar = Card::new()
            .outlined()
            .p(px(16.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(self.harmony_logs_field.clone()),
                    )
                    .child(ui::action_button(
                        cx,
                        "harmony-logs-refresh",
                        "抓取 HiLog",
                        Some("refresh"),
                        ActionKind::Filled,
                        !loading,
                        cx.listener(|app, _, _, cx| app.load_hdc_logs(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "harmony-logs-export",
                        "导出",
                        Some("save"),
                        ActionKind::Tonal,
                        total > 0,
                        cx.listener(|app, _, _, cx| app.export_hdc_logs(cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(10.))
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap(px(8.))
                    .child(div().w(px(220.)).child(self.harmony_logs_tag_field.clone()))
                    .child(ui::action_button(
                        cx,
                        "harmony-logs-clear",
                        "清空缓冲区",
                        Some("delete_sweep"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.open_hdc_clear_logs(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "harmony-faultlog-export",
                        "导出 Crash 日志",
                        Some("bug_report"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.export_hdc_faultlog(cx)),
                    ))
                    .child(ui::action_button(
                        cx,
                        "harmony-hilog-files-export",
                        "导出持久化日志",
                        Some("folder_zip"),
                        ActionKind::Text,
                        true,
                        cx.listener(|app, _, _, cx| app.export_hdc_hilog_files(cx)),
                    )),
            )
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .children(filters)
                    .child(div().flex_1())
                    .when(loading, |el| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(10.5))
                                .text_color(colors.primary)
                                .child(LoadingIndicator::new("harmony-logs-loading").size(px(13.)))
                                .child("正在抓取…"),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(colors.on_surface_variant)
                            .child(match self.harmony.logs_at.clone() {
                                Some(at) => {
                                    format!("共 {total} 行 · 显示最近 {shown} 行 · 抓取于 {at}")
                                }
                                None => "尚未抓取".to_string(),
                            }),
                    ),
            )
            .child(div().mt(px(10.)).child(ui::callout(
                cx,
                "info",
                "使用 hilog -x 非阻塞读取设备日志缓冲区；可选 -L 级别与 -T 标签过滤，关键字筛选在本地进行。清空缓冲区会删除设备上现有日志。",
            )));

        let list: AnyElement = if filtered.is_empty() {
            ui::empty_block(
                cx,
                "subject",
                "没有日志",
                "点击「抓取 HiLog」读取设备日志缓冲区；如果设备刚重启，缓冲区可能为空。",
                Some(
                    ui::action_button(
                        cx,
                        "harmony-logs-empty-refresh",
                        "抓取 HiLog",
                        Some("refresh"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.load_hdc_logs(cx)),
                    )
                    .into_any_element(),
                ),
            )
            .into_any_element()
        } else {
            Card::new()
                .outlined()
                .p(px(8.))
                .child(
                    div().flex().flex_col().children(
                        filtered
                            .iter()
                            .enumerate()
                            .map(|(index, line)| harmony_log_row(cx, index, line, weak.clone())),
                    ),
                )
                .into_any_element()
        };

        let export = self.harmony.last_log_export.clone().map(|path| {
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
                    Icon::new(IconName::new("subject"))
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
                    "harmony-logs-open-export",
                    "打开",
                    Some("open_in_new"),
                    ActionKind::Text,
                    true,
                    move |_, _, cx| cx.open_with_system(&open_path),
                ))
                .child(ui::action_button(
                    cx,
                    "harmony-logs-reveal-export",
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
                .when_some(export, |el, export| el.child(export)),
        )
        .into_any_element()
    }
}

fn harmony_file_row(
    cx: &App,
    index: usize,
    entry: &FileEntry,
    weak: WeakEntity<RareBox>,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let mobile = cfg!(target_os = "android");
    let name = entry.name.clone();
    let is_dir = entry.is_dir;

    let mut row = div()
        .id(("harmony-files-row", index))
        .w_full()
        .h(px(50.))
        .px(px(14.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(8.))
        .border_b_1()
        .border_color(colors.outline_variant.opacity(0.3))
        .hover(move |style| style.bg(colors.surface_container))
        .child(
            div()
                .w(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::new(file_icon(entry)))
                        .size(px(18.))
                        .color(if is_dir {
                            colors.primary
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
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(if is_dir {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(colors.on_surface)
                        .truncate()
                        .child(name.clone()),
                )
                .child(
                    div()
                        .text_size(px(9.5))
                        .font_family(MONO_FONT)
                        .text_color(colors.on_surface_variant.opacity(0.75))
                        .child(entry.permissions.clone()),
                ),
        )
        .when(!mobile, |el| {
            el.child(
                div()
                    .w(px(86.))
                    .text_size(px(10.5))
                    .text_color(colors.on_surface_variant)
                    .text_right()
                    .child(if is_dir {
                        "—".to_string()
                    } else {
                        entry.size_label.clone()
                    }),
            )
            .child(
                div()
                    .w(px(120.))
                    .text_size(px(10.))
                    .text_color(colors.on_surface_variant)
                    .text_right()
                    .truncate()
                    .child(entry.modified.clone()),
            )
        })
        .child(div().w(px(80.)).flex().justify_end().child(if is_dir {
            div()
                .id(("harmony-files-open-dir", index))
                .h(px(30.))
                .px(px(10.))
                .rounded_full()
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.primary)
                .cursor_pointer()
                .hover(move |style| style.bg(colors.primary.opacity(0.08)))
                .child("打开")
                .child(
                    Icon::new(IconName::new("chevron_right"))
                        .size(px(14.))
                        .color(colors.primary),
                )
                .into_any_element()
        } else {
            let weak = weak.clone();
            let name = name.clone();
            let mut button = div()
                .id(("harmony-files-pull", index))
                .h(px(30.))
                .px(px(10.))
                .rounded_full()
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.primary)
                .cursor_pointer()
                .hover(move |style| style.bg(colors.primary.opacity(0.08)))
                .child(
                    Icon::new(IconName::new("download"))
                        .size(px(13.))
                        .color(colors.primary),
                )
                .child("拉取");
            button = button.on_click(move |_, _, cx| {
                if let Err(err) = weak.update(cx, |app, cx| {
                    let path = app.harmony.files_path.clone();
                    app.pull_hdc_file(format!("{path}/{name}"), cx);
                }) {
                    eprintln!("RareBox Desktop: 拉取鸿蒙文件失败: {err}");
                }
            });
            button.into_any_element()
        }));

    if is_dir {
        let weak = weak.clone();
        let current = name.clone();
        row = row.on_click(move |_, _, cx| {
            if let Err(err) = weak.update(cx, |app, cx| {
                let path = app.harmony.files_path.clone();
                let next = format!("{}/{}", path.trim_end_matches('/'), current);
                app.navigate_hdc_files(next, cx);
            }) {
                eprintln!("RareBox Desktop: 打开鸿蒙目录失败: {err}");
            }
        });
    }

    row
}

fn file_icon(entry: &FileEntry) -> &'static str {
    if entry.is_dir {
        return "folder";
    }
    let lower = entry.name.to_ascii_lowercase();
    if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image"
    } else if lower.ends_with(".mp4") || lower.ends_with(".mov") {
        "videocam"
    } else if lower.ends_with(".hap") || lower.ends_with(".hsp") || lower.ends_with(".app") {
        "apps"
    } else if lower.ends_with(".log") || lower.ends_with(".txt") {
        "subject"
    } else {
        "description"
    }
}

fn harmony_terminal_entry(
    cx: &App,
    index: usize,
    entry: &TerminalEntry,
    weak: WeakEntity<RareBox>,
) -> AnyElement {
    let colors = *cx.theme().colors();
    let copy_output = entry.output.clone();
    let copy_command = entry.command.clone();
    let success = entry.success;
    let copy_output_weak = weak.clone();

    Card::new()
        .outlined()
        .p(px(14.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(9.))
                .child(ui::status_dot(if success {
                    colors.tertiary
                } else {
                    colors.error
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .font_family(MONO_FONT)
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.on_surface)
                        .truncate()
                        .child(format!("$ {}", entry.command)),
                )
                .child(ui::tag(
                    if success { "成功" } else { "失败" },
                    if success {
                        colors.tertiary_container
                    } else {
                        colors.error_container
                    },
                    if success {
                        colors.on_tertiary_container
                    } else {
                        colors.on_error_container
                    },
                ))
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(colors.on_surface_variant)
                        .child(format!("{} · {} ms", entry.time, entry.duration_ms)),
                )
                .child(ui::icon_button(
                    cx,
                    ("harmony-terminal-copy", index),
                    "content_copy",
                    30.,
                    colors.on_surface_variant,
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_output.clone()));
                        if let Err(err) = copy_output_weak.update(cx, |app, cx| {
                            app.toast(cx, ToastKind::Info, "已复制命令输出");
                        }) {
                            eprintln!("RareBox Desktop: 复制鸿蒙输出提示失败: {err}");
                        }
                    },
                ))
                .child(ui::icon_button(
                    cx,
                    ("harmony-terminal-copy-command", index),
                    "data_object",
                    30.,
                    colors.on_surface_variant,
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_command.clone()));
                    },
                )),
        )
        .child(
            div()
                .mt(px(10.))
                .p(px(12.))
                .rounded(px(10.))
                .bg(colors.surface_container_low)
                .border_1()
                .border_color(colors.outline_variant.opacity(0.35))
                .font_family(MONO_FONT)
                .text_size(px(11.))
                .line_height(px(17.))
                .text_color(if success {
                    colors.on_surface
                } else {
                    colors.error
                })
                .child(entry.output.clone()),
        )
        .into_any_element()
}

fn harmony_log_row(
    cx: &App,
    index: usize,
    line: &HdcLogLine,
    _weak: WeakEntity<RareBox>,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let color = match line.level {
        LogLevel::Warn => colors.tertiary,
        LogLevel::Error | LogLevel::Fatal => colors.error,
        LogLevel::Info => colors.primary,
        _ => colors.on_surface_variant,
    };
    div()
        .id(("harmony-log", index))
        .w_full()
        .px(px(10.))
        .py(px(3.))
        .flex()
        .items_start()
        .gap(px(8.))
        .rounded(px(6.))
        .hover(move |style| style.bg(colors.surface_container))
        .child(
            div()
                .w(px(132.))
                .flex_none()
                .text_size(px(9.5))
                .font_family(MONO_FONT)
                .text_color(colors.on_surface_variant)
                .truncate()
                .child(line.time.clone()),
        )
        .child(
            div()
                .w(px(20.))
                .flex_none()
                .text_size(px(10.))
                .font_family(MONO_FONT)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(line.level.label()),
        )
        .child(
            div()
                .w(px(170.))
                .flex_none()
                .text_size(px(10.))
                .font_family(MONO_FONT)
                .text_color(colors.on_surface_variant)
                .truncate()
                .child(line.tag.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(10.5))
                .font_family(MONO_FONT)
                .text_color(colors.on_surface)
                .truncate()
                .child(line.message.clone()),
        )
}
