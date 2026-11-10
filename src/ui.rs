//! 可复用的界面片段。视觉参数全部来自 material3-gpui 主题令牌。

use crate::app::{RareBox, ToastKind};
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Div, ElementId, FontWeight, Hsla, IntoElement,
    ParentElement, Pixels, Point, SharedString, Stateful, StatefulInteractiveElement as _, Styled,
    WeakEntity, Window, anchored, deferred, div, point, prelude::*, px, size,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::prelude::*;
use material3_gpui::theme::ActiveTheme;

#[cfg(target_os = "windows")]
pub const MONO_FONT: &str = "Cascadia Mono";
#[cfg(target_os = "macos")]
pub const MONO_FONT: &str = "Menlo";
#[cfg(target_os = "android")]
pub const MONO_FONT: &str = "Droid Sans Mono";
#[cfg(target_os = "ios")]
pub const MONO_FONT: &str = "Menlo";
#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    target_os = "android"
)))]
pub const MONO_FONT: &str = "DejaVu Sans Mono";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Filled,
    Tonal,
    Outlined,
    Text,
    Danger,
}

/// 一个 Material 3 风格的操作按钮。
pub fn action_button(
    cx: &App,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: Option<&'static str>,
    kind: ActionKind,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    action_button_sized(cx, id, label, icon, kind, enabled, 40., 13., 17., on_click)
}

/// 与 [`action_button`] 相同，但允许指定高度、文字与图标尺寸。
///
/// 快速指引里的按钮需要比常规工具栏更大，便于不看小字的用户点按。
#[allow(clippy::too_many_arguments)]
pub fn action_button_sized(
    cx: &App,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: Option<&'static str>,
    kind: ActionKind,
    enabled: bool,
    height: f32,
    text_size: f32,
    icon_size: f32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let (background, foreground) = match kind {
        ActionKind::Filled => (colors.primary, colors.on_primary),
        ActionKind::Tonal => (colors.secondary_container, colors.on_secondary_container),
        ActionKind::Outlined => (colors.surface.opacity(0.0), colors.primary),
        ActionKind::Text => (colors.surface.opacity(0.0), colors.primary),
        ActionKind::Danger => (colors.error_container, colors.on_error_container),
    };

    let hover_background = match kind {
        ActionKind::Filled | ActionKind::Danger => background.opacity(0.82),
        ActionKind::Tonal => colors.secondary_container.opacity(0.7),
        ActionKind::Outlined | ActionKind::Text => colors.primary.opacity(0.08),
    };
    let pressed_background = match kind {
        ActionKind::Filled | ActionKind::Danger => background.opacity(0.7),
        ActionKind::Tonal => colors.secondary_container.opacity(0.55),
        ActionKind::Outlined | ActionKind::Text => colors.primary.opacity(0.14),
    };

    let mut button = div()
        .id(id)
        .h(px(height))
        .px(px(if height >= 44. { 18. } else { 16. }))
        .rounded_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .bg(background)
        .text_color(foreground)
        .text_size(px(text_size))
        .font_weight(FontWeight::MEDIUM)
        .border_1()
        .border_color(if kind == ActionKind::Outlined {
            colors.outline_variant
        } else {
            colors.surface.opacity(0.0)
        });

    if enabled {
        button = button
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .active(move |style| style.bg(pressed_background))
            .on_click(on_click);
    } else {
        button = button.opacity(0.45);
    }

    if let Some(icon) = icon {
        button = button.child(
            Icon::new(IconName::new(icon))
                .size(px(icon_size))
                .color(foreground),
        );
    }

    button.child(label.into())
}

/// 仅图标按钮，用于工具栏和列表操作。
pub fn icon_button(
    _cx: &App,
    id: impl Into<ElementId>,
    icon: &'static str,
    size: f32,
    color: Hsla,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .size(px(size))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |style| style.bg(color.opacity(0.09)))
        .active(move |style| style.bg(color.opacity(0.16)))
        .child(
            Icon::new(IconName::new(icon))
                .size(px(size * 0.5))
                .color(color),
        )
        .on_click(on_click)
}

/// 铺满窗口的模态层：半透明遮罩 + 居中卡片，绘制在所有普通元素之上。
pub fn modal_overlay(
    window: &Window,
    id: &'static str,
    scrim: Hsla,
    card: impl IntoElement,
) -> impl IntoElement {
    let viewport = window.viewport_size();
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id(id)
                .occlude()
                .w(viewport.width)
                .h(viewport.height)
                .bg(scrim)
                .flex()
                .items_center()
                .justify_center()
                .child(card),
        ),
    )
    .with_priority(100)
}

/// 计算浮层菜单的锚点，避免在窗口右缘或底部被裁切。
pub fn menu_anchor(
    window: &Window,
    position: Point<Pixels>,
    width: f32,
    height: f32,
) -> Bounds<Pixels> {
    let viewport = window.viewport_size();
    let margin = px(8.);
    let gap = px(4.);
    let width = px(width);
    let height = px(height);
    let max_x = (viewport.width - width - margin).max(margin);
    let x = position.x.min(max_x).max(margin);
    let below = position.y + gap;
    let y = if below + height + margin > viewport.height {
        (position.y - height - gap).max(margin)
    } else {
        below
    };

    Bounds {
        origin: point(x, y),
        size: size(px(0.), px(0.)),
    }
}

/// 过滤胶囊按钮。
pub fn filter_chip(
    cx: &App,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let (background, foreground, border) = if selected {
        (
            colors.secondary_container,
            colors.on_secondary_container,
            colors.secondary_container,
        )
    } else {
        (
            colors.surface.alpha(0.0),
            colors.on_surface_variant,
            colors.outline_variant,
        )
    };

    let chip = div()
        .id(id)
        .h(px(30.))
        .px(px(13.))
        .rounded_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(5.))
        .bg(background)
        .border_1()
        .border_color(border)
        .text_color(foreground)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(selected, |el| {
            el.child(
                Icon::new(IconName::new("check"))
                    .size(px(13.))
                    .color(foreground),
            )
        })
        .child(label.into());

    if selected {
        chip
    } else {
        chip.hover(move |style| style.bg(colors.surface_container_high))
            .active(move |style| style.bg(colors.surface_container_highest))
            .on_click(on_click)
    }
}

/// 可以切换的选项胶囊（与过滤胶囊的区别是选中后仍可点击取消）。
pub fn toggle_chip(
    cx: &App,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let (background, foreground, border) = if selected {
        (
            colors.secondary_container,
            colors.on_secondary_container,
            colors.secondary_container,
        )
    } else {
        (
            colors.surface.alpha(0.0),
            colors.on_surface_variant,
            colors.outline_variant,
        )
    };

    div()
        .id(id)
        .h(px(30.))
        .px(px(12.))
        .rounded_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(5.))
        .bg(background)
        .border_1()
        .border_color(border)
        .text_color(foreground)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |style| style.bg(colors.surface_container_high))
        .active(move |style| style.bg(colors.surface_container_highest))
        .when(selected, |el| {
            el.child(
                Icon::new(IconName::new("check"))
                    .size(px(13.))
                    .color(foreground),
            )
        })
        .child(label.into())
        .on_click(on_click)
}

/// 小组件：圆角标签。
pub fn tag(label: impl Into<SharedString>, background: Hsla, foreground: Hsla) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(8.))
        .h(px(20.))
        .rounded(px(6.))
        .bg(background)
        .text_color(foreground)
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .flex()
        .items_center()
        .child(label.into())
}

/// 带边框的 Material 3 面板。
pub fn panel() -> Card {
    Card::new().outlined().p(px(20.))
}

/// 面板标题区。
pub fn section_header(
    cx: &App,
    title: impl Into<SharedString>,
    subtitle: Option<impl Into<SharedString>>,
    trailing: Option<AnyElement>,
) -> Div {
    let colors = *cx.theme().colors();
    let typography = *cx.theme().typography();
    let mut header = div().w_full().flex().items_center().gap(px(12.)).child(
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(
                typography
                    .title_medium
                    .apply(div())
                    .text_color(colors.on_surface)
                    .child(title.into()),
            )
            .when_some(subtitle, |el, subtitle| {
                el.child(
                    typography
                        .body_small
                        .apply(div())
                        .text_color(colors.on_surface_variant)
                        .child(subtitle.into()),
                )
            }),
    );
    if let Some(trailing) = trailing {
        header = header.child(trailing);
    }
    header
}

/// 空状态区块。
pub fn empty_block(
    cx: &App,
    icon: &'static str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    action: Option<AnyElement>,
) -> Div {
    let colors = *cx.theme().colors();
    let typography = *cx.theme().typography();
    let mut block = div()
        .w_full()
        .py(px(38.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .child(
            div()
                .size(px(64.))
                .rounded_full()
                .bg(colors.surface_container_high)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(IconName::new(icon))
                        .size(px(28.))
                        .color(colors.on_surface_variant),
                ),
        )
        .child(
            typography
                .title_small
                .apply(div())
                .text_color(colors.on_surface)
                .child(title.into()),
        )
        .child(
            typography
                .body_small
                .apply(div())
                .text_color(colors.on_surface_variant)
                .text_center()
                .max_w(px(440.))
                .child(description.into()),
        );
    if let Some(action) = action {
        block = block.child(div().pt(px(8.)).child(action));
    }
    block
}

/// 让只读字段支持复制：桌面单击、触屏长按（触摸长按以 aux-click 事件派发）。
pub fn copyable(
    target: Div,
    weak: WeakEntity<RareBox>,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> Stateful<Div> {
    let text: SharedString = text.into();
    let click_text = text.clone();
    let click_weak = weak.clone();
    target
        .id(id)
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(click_text.to_string()));
            if let Err(err) = click_weak.update(cx, |app, cx| {
                app.toast(cx, ToastKind::Info, format!("已复制：{click_text}"));
            }) {
                eprintln!("RareBox Desktop: 复制提示失败: {err}");
            }
        })
        .on_aux_click(move |_, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
            if let Err(err) = weak.update(cx, |app, cx| {
                app.toast(cx, ToastKind::Info, format!("已复制：{text}"))
            }) {
                eprintln!("RareBox Desktop: 复制提示失败: {err}");
            }
        })
}

/// 两列详情中的单行键值，点击（触屏长按）即可复制值。
pub fn key_value(
    cx: &App,
    weak: WeakEntity<RareBox>,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    mono: bool,
) -> Stateful<Div> {
    let colors = *cx.theme().colors();
    let typography = *cx.theme().typography();
    let label: SharedString = label.into();
    let value: SharedString = value.into();
    let mut value_el = div()
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(colors.on_surface)
        .truncate()
        .child(value.clone());
    if mono {
        value_el = value_el.font_family(MONO_FONT);
    }
    let id = ElementId::Name(format!("copy-kv-{label}-{value}").into());
    let target = div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .min_w_0()
        .rounded(px(8.))
        .px(px(6.))
        .py(px(4.))
        .mx(px(-6.))
        .my(px(-4.))
        .hover(move |style| style.bg(colors.surface_container))
        .child(
            typography
                .label_small
                .apply(div())
                .text_color(colors.on_surface_variant)
                .child(label),
        )
        .child(value_el);
    copyable(target, weak, id, value)
}

/// 状态点。
pub fn status_dot(color: Hsla) -> Div {
    div().size(px(8.)).rounded_full().bg(color)
}

/// 自定义开关（用于侧栏快速切换主题）。
pub fn mini_switch(
    cx: &App,
    checked: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let colors = *cx.theme().colors();
    let knob = if checked { 18.0 } else { 5.0 };
    div()
        .id("mini-switch")
        .w(px(40.))
        .h(px(22.))
        .flex_none()
        .rounded_full()
        .bg(if checked {
            colors.primary
        } else {
            colors.surface_variant
        })
        .relative()
        .cursor_pointer()
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(knob))
                .size(px(18.))
                .rounded_full()
                .bg(if checked {
                    colors.on_primary
                } else {
                    colors.outline
                }),
        )
        .on_click(on_click)
}

/// 小提示条。
pub fn callout(cx: &App, icon: &'static str, text: impl Into<SharedString>) -> Div {
    let colors = *cx.theme().colors();
    div()
        .w_full()
        .flex()
        .items_start()
        .gap(px(10.))
        .p(px(12.))
        .rounded(px(12.))
        .bg(colors.surface_container_high)
        .child(
            div().mt(px(1.)).child(
                Icon::new(IconName::new(icon))
                    .size(px(17.))
                    .color(colors.primary),
            ),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(colors.on_surface_variant)
                .child(text.into()),
        )
}

/// 进度条颜色是否处于警示区间。
pub fn progress_color(cx: &App, fraction: f32) -> Hsla {
    let colors = *cx.theme().colors();
    if fraction >= 0.9 {
        colors.error
    } else if fraction >= 0.75 {
        colors.tertiary
    } else {
        colors.primary
    }
}
