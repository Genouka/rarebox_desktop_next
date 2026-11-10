//! files 页面渲染（UI 层）：只读状态并绑定交互，不直接执行命令。

use super::shell_ui::{device_required, page_scroll};
use super::{Page, RareBox};
use crate::adb::FileEntry;
use crate::config::short_path;
use crate::ui::{self, ActionKind, MONO_FONT};
use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, div, prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

use super::guide_ui::guide_anchor;
use rarebox_core::guide::GuideTarget;

impl RareBox {
    pub(crate) fn render_files(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.is_harmony() {
            return self.render_harmony_files(cx);
        }
        let weak = cx.entity().downgrade();
        if !self.has_device() {
            return page_scroll(
                Page::Files,
                &self.scroll_handles[Page::Files as usize],
                device_required(
                    cx,
                    weak,
                    "连接手表后浏览文件",
                    "文件页通过 adb shell ls 浏览设备目录，并使用 adb pull / push 传输文件。",
                ),
            )
            .into_any_element();
        }

        let colors = *cx.theme().colors();
        let entries = self.files_entries.clone();
        let path = self.files_path.clone();

        let up = ui::icon_button(
            cx,
            "files-up",
            "arrow_upward",
            38.,
            colors.on_surface_variant,
            cx.listener(|app, _, _, cx| app.go_parent_files(cx)),
        );
        let refresh = ui::icon_button(
            cx,
            "files-refresh",
            "refresh",
            38.,
            colors.on_surface_variant,
            cx.listener(|app, _, _, cx| app.list_files(cx)),
        );

        let go = ui::action_button(
            cx,
            "files-go",
            "前往",
            Some("arrow_forward"),
            ActionKind::Tonal,
            true,
            cx.listener(|app, _, _, cx| {
                let path = app.files_path_field.read(cx).value().to_string();
                app.navigate_files(path, cx);
            }),
        );
        let push_anchor = self.guide.anchors.handle(GuideTarget::FilesUpload);
        let push = guide_anchor(
            &push_anchor,
            ui::action_button(
                cx,
                "files-push",
                "上传文件",
                Some("upload"),
                ActionKind::Filled,
                true,
                cx.listener(|app, _, _, cx| app.pick_and_push(cx)),
            ),
        );
        let mobile = cfg!(any(target_os = "android", target_os = "ios"));

        let toolbar = Card::new()
            .outlined()
            .p(px(16.))
            .child(if mobile {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(self.files_path_field.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(up)
                            .child(go)
                            .child(refresh)
                            .child(push),
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
                            .child(self.files_path_field.clone()),
                    )
                    .child(go)
                    .child(refresh)
                    .child(push)
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
                            ("/sdcard", "内部存储"),
                            ("/sdcard/Download", "下载"),
                            ("/sdcard/DCIM", "相册"),
                            ("/data/local/tmp", "临时目录"),
                        ]
                        .into_iter()
                        .map(|(target, label)| {
                            let target_owned = target.to_string();
                            let weak = weak.clone();
                            ui::filter_chip(
                                cx,
                                ("quick-dir", target.len()),
                                label,
                                path == target,
                                move |_, _, cx| {
                                    if let Err(err) = weak.update(cx, |app, cx| {
                                        app.navigate_files(target_owned.clone(), cx);
                                    }) {
                                        eprintln!("RareBox Desktop: 打开快捷目录失败: {err}");
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
                "点击刷新从设备读取当前目录；如果提示权限不足，请尝试 /sdcard 或 /data/local/tmp。",
                Some(
                    ui::action_button(
                        cx,
                        "files-empty-refresh",
                        "读取当前目录",
                        Some("refresh"),
                        ActionKind::Tonal,
                        true,
                        cx.listener(|app, _, _, cx| app.list_files(cx)),
                    )
                    .into_any_element(),
                ),
            ))
        } else {
            let rows = entries.iter().enumerate().map(|(index, entry)| {
                file_row(cx, index, entry, entries.len(), cx.entity().downgrade())
            });
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
                        .when(!cfg!(any(target_os = "android", target_os = "ios")), |el| {
                            el.child(div().w(px(86.)).text_right().child("大小"))
                                .child(div().w(px(120.)).text_right().child("修改时间"))
                        })
                        .child(div().w(px(80.)).text_right().child("操作")),
                )
                .child(div().flex().flex_col().children(rows))
        };

        let list = guide_anchor(&self.guide.anchors.handle(GuideTarget::FilesList), list);

        let export = self.last_export.clone().map(|path| {
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
                    "files-open-export",
                    "打开",
                    Some("open_in_new"),
                    ActionKind::Text,
                    true,
                    move |_, _, cx| cx.open_with_system(&open_path),
                ))
                .child(ui::action_button(
                    cx,
                    "files-reveal-export",
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
                                .child(format!("{path} · {} 个条目", self.files_entries.len())),
                        ),
                )
                .child(list)
                .when_some(export, |el, export| el.child(export)),
        )
        .into_any_element()
    }
}

fn file_row(
    cx: &App,
    index: usize,
    entry: &FileEntry,
    total: usize,
    weak: gpui::WeakEntity<RareBox>,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let mobile = cfg!(any(target_os = "android", target_os = "ios"));
    let is_last = index + 1 == total;
    let name = entry.name.clone();
    let is_dir = entry.is_dir;

    let mut row = div()
        .id(("files-row", index))
        .w_full()
        .h(px(50.))
        .px(px(14.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(8.))
        .when(!is_last, |el| {
            el.border_b_1()
                .border_color(colors.outline_variant.opacity(0.3))
        })
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
                .id(("files-open-dir", index))
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
                .id(("files-pull", index))
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
                    let path = app.files_path.clone();
                    app.pull_file(format!("{path}/{name}"), cx);
                }) {
                    eprintln!("RareBox Desktop: 拉取文件失败: {err}");
                }
            });
            button.into_any_element()
        }));

    if is_dir {
        let weak = weak.clone();
        let current = name.clone();
        row = row.on_click(move |_, _, cx| {
            if let Err(err) = weak.update(cx, |app, cx| {
                let path = app.files_path.clone();
                let next = format!("{}/{}", path.trim_end_matches('/'), current);
                app.navigate_files(next, cx);
            }) {
                eprintln!("RareBox Desktop: 打开目录失败: {err}");
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
    } else if lower.ends_with(".apk") {
        "android"
    } else if lower.ends_with(".log") || lower.ends_with(".txt") {
        "subject"
    } else {
        "description"
    }
}
