//! terminal 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::shell_ui::{device_required, page_scroll};
use super::{Page, RareBox, TerminalEntry, ToastKind};
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, App, ClipboardItem, Context, FontWeight, IntoElement, ParentElement, Styled, div,
    px,
};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

impl RareBox {
    pub(crate) fn render_terminal(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_harmony() {
            return self.render_harmony_terminal(cx);
        }
        let weak = cx.entity().downgrade();
        if !self.has_device() {
            return page_scroll(
                Page::Terminal,
                &self.scroll_handles[Page::Terminal as usize],
                device_required(
                    cx,
                    weak,
                    "连接手表后使用终端",
                    "终端会在所选设备上执行 adb shell 命令。对于 logcat 等长驻命令，请使用日志页。",
                ),
            )
            .into_any_element();
        }

        let colors = *cx.theme().colors();
        let history = self.terminal_history.clone();
        let count = history.len();

        let quick_commands = [
            ("getprop ro.build.version.release", "Android 版本"),
            ("dumpsys battery", "电池信息"),
            ("wm size", "屏幕分辨率"),
            ("df -h /data", "存储空间"),
            ("pm list packages -3", "用户应用"),
            ("settings get system user_rotation", "屏幕旋转"),
        ];

        let toolbar = Card::new()
            .outlined()
            .p(px(18.))
            .child(ui::section_header(
                cx,
                "执行命令",
                Some("结果会保留在下方，按回车或点击执行"),
                None,
            ))
            .child(
                div()
                    .mt(px(14.))
                    .flex()
                    .items_end()
                    .gap(px(10.))
                    .child(div().flex_1().min_w(px(0.)).child(self.terminal_field.clone()))
                    .child(ui::action_button(
                        cx,
                        "terminal-run",
                        "执行",
                        Some("play_arrow"),
                        ActionKind::Filled,
                        true,
                        cx.listener(|app, _, _, cx| app.submit_terminal(cx)),
                    )),
            )
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
                            .child("快捷命令"),
                    )
                    .children(quick_commands.into_iter().enumerate().map(
                        |(index, (command, label))| {
                            let weak = weak.clone();
                            let command = command.to_string();
                            ui::filter_chip(
                                cx,
                                ("terminal-quick", index),
                                label,
                                false,
                                move |_, _, cx| {
                                    if let Err(err) = weak.update(cx, |app, cx| {
                                        app.run_quick_command(command.clone(), cx);
                                    }) {
                                        eprintln!("RareBox Desktop: 执行快捷命令失败: {err}");
                                    }
                                },
                            )
                        },
                    )),
            )
            .child(div().mt(px(12.)).child(ui::callout(
                cx,
                "warning",
                "终端执行的是阻塞式命令。logcat、top 等不会自动退出的命令会一直占用任务，请改用「日志查看器」或追加 -n 1。",
            )));

        let list: AnyElement = if history.is_empty() {
            ui::empty_block(
                cx,
                "terminal",
                "还没有执行过命令",
                "输入任意 adb shell 命令，或点击上方的快捷命令开始。每条命令的结果都会保留在这里。",
                None,
            )
            .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .children(history.iter().enumerate().map(|(index, entry)| {
                    terminal_entry(cx, index, entry, cx.entity().downgrade())
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
                            "terminal-clear",
                            "清空记录",
                            Some("delete"),
                            ActionKind::Text,
                            count > 0,
                            cx.listener(|app, _, _, cx| {
                                app.terminal_history.clear();
                                app.toast(cx, ToastKind::Info, "终端记录已清空");
                                cx.notify();
                            }),
                        )),
                )
                .child(list),
        )
        .into_any_element()
    }
}

fn terminal_entry(
    cx: &App,
    index: usize,
    entry: &TerminalEntry,
    weak: gpui::WeakEntity<RareBox>,
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
                    ("terminal-copy", index),
                    "content_copy",
                    30.,
                    colors.on_surface_variant,
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_output.clone()));
                        if let Err(err) = copy_output_weak.update(cx, |app, cx| {
                            app.toast(cx, ToastKind::Info, "已复制命令输出");
                        }) {
                            eprintln!("RareBox Desktop: 复制输出提示失败: {err}");
                        }
                    },
                ))
                .child(ui::icon_button(
                    cx,
                    ("terminal-copy-command", index),
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
