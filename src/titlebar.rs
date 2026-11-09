//! 自定义窗口标题栏，和 MD3 主题保持一致的视觉语言。

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, ClickEvent, FontWeight, Hsla, IntoElement, MouseButton, ParentElement, Rgba,
    StatefulInteractiveElement as _, Styled, Window, WindowControlArea, div, prelude::*, px,
};
use material3_gpui::icon::{Icon, IconName};
use material3_gpui::theme::ActiveTheme;

const HEIGHT: f32 = 48.0;
const BUTTON_SIZE: f32 = 44.0;
const MAC_TRAFFIC_LIGHT_INSET: f32 = 48.0;

const CLOSE_RED: Rgba = Rgba {
    r: 210.0 / 255.0,
    g: 58.0 / 255.0,
    b: 58.0 / 255.0,
    a: 1.0,
};

/// 绘制标题栏。`device_count` 用于右上角的连接状态。
pub fn render(device_count: usize, cx: &App) -> impl IntoElement + use<> {
    let colors = *cx.theme().colors();
    let is_windows = cfg!(target_os = "windows");
    let is_mac = cfg!(target_os = "macos");
    let is_linux = !is_windows && !is_mac;
    let dragging = Rc::new(Cell::new(false));

    let bar = div()
        .id("rarebox-titlebar")
        .relative()
        .h(px(HEIGHT))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .px(px(12.))
        .gap(px(10.))
        .bg(colors.surface_container_low)
        .border_b_1()
        .border_color(colors.outline_variant.opacity(0.6))
        .when(is_windows, |el| {
            el.window_control_area(WindowControlArea::Drag)
        })
        .when(is_linux, |el| {
            let down = dragging.clone();
            let up = dragging.clone();
            let mv = dragging.clone();
            el.on_mouse_down(MouseButton::Left, move |_, _, _| down.set(true))
                .on_mouse_up(MouseButton::Left, move |_, _, _| up.set(false))
                .on_mouse_move(move |_, window, _| {
                    if mv.get() {
                        mv.set(false);
                        window.start_window_move();
                    }
                })
                .on_click(move |event, window, _| {
                    if event.click_count() == 2 {
                        window.zoom_window();
                    }
                })
        });

    let bar = bar.when(is_mac, |el| {
        el.child(div().flex_none().w(px(MAC_TRAFFIC_LIGHT_INSET)))
    });

    let logo = div()
        .flex_none()
        .size(px(30.))
        .rounded(px(9.))
        .bg(colors.primary)
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::new(IconName::new("watch"))
                .size(px(18.))
                .color(colors.on_primary),
        );

    let brand = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.on_surface)
                .child("RareBox Desktop"),
        )
        .child(
            div()
                .px(px(6.))
                .py(px(1.))
                .rounded(px(5.))
                .bg(colors.surface_container_highest)
                .text_size(px(9.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.on_surface_variant)
                .child(crate::update::APP_VERSION),
        );

    let status_color = if device_count > 0 {
        colors.tertiary
    } else {
        colors.on_surface_variant.opacity(0.5)
    };
    let status_text = if device_count > 0 {
        format!("{device_count} 台设备已连接")
    } else {
        "等待设备".to_string()
    };
    let status = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(7.))
        .px(px(10.))
        .h(px(24.))
        .rounded_full()
        .bg(colors.surface_container)
        .child(div().size(px(6.)).rounded_full().bg(status_color))
        .child(
            div()
                .text_size(px(11.))
                .text_color(colors.on_surface_variant)
                .child(status_text),
        );

    let spacer = div().flex_1().min_w(px(8.));

    let controls = if is_mac {
        div()
    } else {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(window_button(
                "titlebar-minimize",
                WindowControlArea::Min,
                IconName::new("remove"),
                colors.on_surface_variant,
                false,
                move |_event, window, _cx| window.minimize_window(),
                is_windows,
            ))
            .child(window_button(
                "titlebar-maximize",
                WindowControlArea::Max,
                IconName::new("crop_free"),
                colors.on_surface_variant,
                false,
                move |_event, window, _cx| window.zoom_window(),
                is_windows,
            ))
            .child(window_button(
                "titlebar-close",
                WindowControlArea::Close,
                IconName::new("close"),
                CLOSE_RED.into(),
                true,
                move |_event, window, _cx| window.remove_window(),
                is_windows,
            ))
    };

    bar.child(logo)
        .child(brand)
        .child(spacer)
        .child(status)
        .child(controls)
}

#[allow(clippy::too_many_arguments)]
fn window_button(
    id: &'static str,
    area: WindowControlArea,
    icon: IconName,
    icon_color: Hsla,
    danger: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    windows_native: bool,
) -> impl IntoElement {
    let hover_color = if danger { CLOSE_RED.into() } else { icon_color };

    let base = div()
        .id(id)
        .size(px(BUTTON_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .occlude()
        .hover(move |style| style.bg(hover_color.opacity(0.10)))
        .active(move |style| style.bg(hover_color.opacity(0.18)));

    let base = if windows_native {
        base.window_control_area(area)
    } else {
        base.on_click(move |event, window, cx| {
            cx.stop_propagation();
            on_click(event, window, cx)
        })
    };

    base.child(Icon::new(icon).size(px(18.)).color(icon_color))
}
