// Copyright (c) 2026 TNTyep520
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! 绑定 [`ScrollHandle`] 的竖向滚动条。
//!
//! 组件不参与父级布局：它以绝对定位请求一个零尺寸节点，在 prepaint 阶段按被跟踪滚动容器的
//! 窗口坐标计算滑块几何并插入拖拽热区，在 paint 阶段直接绘制滑块。把它放在滚动元素之后的
//! 同级位置即可，父级无需为它预留空间。
//!
//! ```ignore
//! div()
//!     .id("panel")
//!     .size_full()
//!     .overflow_y_scroll()
//!     .track_scroll(&handle)
//!     .child(content)
//!     // 同级且位于滚动元素之后，才能读到本帧的滚动几何。
//!     .child(Scrollbar::new("panel-scrollbar", &handle))
//! ```

use gpui::{
    App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Position, ScrollHandle, Style, Window, fill, point, px, size,
};
use std::{cell::RefCell, rc::Rc};

use crate::theme::ActiveTheme;

const DEFAULT_THICKNESS: Pixels = px(8.);
const DEFAULT_INSET: Pixels = px(4.);
const DEFAULT_MIN_THUMB_LENGTH: Pixels = px(36.);
/// 滑块左右额外的可点击宽度，避免必须精准命中细滑块。
const HITBOX_PADDING: Pixels = px(4.);

/// 按下位置相对滑块顶端的偏移，跨帧保存在元素状态里。
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct DragAnchor {
    grab: Pixels,
}

type DragState = Rc<RefCell<Option<DragAnchor>>>;

/// 由滚动容器几何推导出的轨道尺寸，prepaint 与拖拽事件共用同一套数学。
#[derive(Clone, Copy)]
struct Track {
    viewport: Bounds<Pixels>,
    thumb_height: Pixels,
    /// 滑块可移动的距离，即轨道长度减去滑块长度。
    travel: Pixels,
}

#[doc(hidden)]
pub struct ThumbGeometry {
    track: Track,
    bounds: Bounds<Pixels>,
    hitbox: Hitbox,
}

/// 绑定滚动容器的竖向滚动条；内容无需滚动时自动隐藏。
pub struct Scrollbar {
    id: ElementId,
    handle: ScrollHandle,
    thickness: Pixels,
    inset: Pixels,
    min_thumb_length: Pixels,
}

impl Scrollbar {
    pub fn new(id: impl Into<ElementId>, handle: &ScrollHandle) -> Self {
        Self {
            id: id.into(),
            handle: handle.clone(),
            thickness: DEFAULT_THICKNESS,
            inset: DEFAULT_INSET,
            min_thumb_length: DEFAULT_MIN_THUMB_LENGTH,
        }
    }

    /// 滑块宽度，默认 8px。
    pub fn thickness(mut self, thickness: impl Into<Pixels>) -> Self {
        self.thickness = thickness.into();
        self
    }

    /// 滑块距容器右边缘的距离，默认 4px。
    pub fn inset(mut self, inset: impl Into<Pixels>) -> Self {
        self.inset = inset.into();
        self
    }

    /// 滑块最短长度，默认 36px：内容极长时滑块不会短于该值。
    pub fn min_thumb_length(mut self, length: impl Into<Pixels>) -> Self {
        self.min_thumb_length = length.into();
        self
    }

    fn track(&self) -> Option<Track> {
        let viewport = self.handle.bounds();
        let max_offset = self.handle.max_offset().y;
        if max_offset <= px(0.5) || viewport.size.height <= px(0.) {
            return None;
        }

        let content_height = viewport.size.height + max_offset;
        let thumb_height = (viewport.size.height * (viewport.size.height / content_height))
            .max(self.min_thumb_length)
            .min(viewport.size.height);

        Some(Track {
            viewport,
            thumb_height,
            travel: viewport.size.height - thumb_height,
        })
    }

    /// 当前滚动进度对应的滑块顶端 y 坐标。
    fn thumb_top(&self, track: Track) -> Pixels {
        let max_offset = self.handle.max_offset().y;
        let progress = (-self.handle.offset().y / max_offset).clamp(0., 1.);
        track.viewport.origin.y + track.travel * progress
    }
}

impl IntoElement for Scrollbar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Scrollbar {
    type RequestLayoutState = DragState;
    type PrepaintState = Option<ThumbGeometry>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // 绝对定位的零尺寸节点：占位为零，绘制与命中都在 prepaint 里按滚动容器坐标决定。
        let drag = match id {
            Some(id) => window.with_element_state(id, |state: Option<DragState>, _| {
                let drag = state.unwrap_or_default();
                (drag.clone(), drag)
            }),
            None => DragState::default(),
        };

        let mut style = Style::default();
        style.position = Position::Absolute;
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, drag)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _drag: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        let track = self.track()?;

        let top = self.thumb_top(track);
        let left = track.viewport.origin.x + track.viewport.size.width - self.inset - self.thickness;
        let bounds = Bounds {
            origin: point(left, top),
            size: size(self.thickness, track.thumb_height),
        };
        let hitbox = window.insert_hitbox(
            Bounds {
                origin: point(left - HITBOX_PADDING, top),
                size: size(
                    self.thickness + HITBOX_PADDING * 2.,
                    track.thumb_height,
                ),
            },
            HitboxBehavior::BlockMouseExceptScroll,
        );

        Some(ThumbGeometry {
            track,
            bounds,
            hitbox,
        })
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        drag: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(thumb) = prepaint.as_ref() else {
            // 没有可滚动内容：清掉残留的拖拽状态，防止下一帧继续拖动。
            drag.borrow_mut().take();
            return;
        };

        let dragging = drag.borrow().is_some();
        let color: Hsla = cx
            .theme()
            .colors()
            .on_surface_variant
            .opacity(if dragging { 0.72 } else { 0.45 });
        let radius = (self.thickness * 0.5).min(thumb.bounds.size.height * 0.5);
        window.paint_quad(fill(thumb.bounds, color).corner_radii(radius));

        let view = window.current_view();
        let handle = self.handle.clone();
        let track = thumb.track;

        let hitbox = thumb.hitbox.clone();
        let thumb_top = thumb.bounds.origin.y;
        if hitbox.is_hovered(window) {
            window.set_cursor_style(gpui::CursorStyle::ResizeRow, &hitbox);
        }
        let down_drag = drag.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && hitbox.is_hovered(window)
            {
                *down_drag.borrow_mut() = Some(DragAnchor {
                    grab: event.position.y - thumb_top,
                });
                cx.notify(view);
            }
        });

        let move_drag = drag.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            // 鼠标在窗口外松开时收不到 MouseUp，这里用按键状态兜底结束拖拽。
            if event.pressed_button.is_none() {
                move_drag.borrow_mut().take();
                cx.notify(view);
                return;
            }
            let Some(anchor) = *move_drag.borrow() else {
                return;
            };
            if track.travel <= px(0.) || track.thumb_height <= px(0.) {
                return;
            }

            let last_top = track.viewport.origin.y + track.travel;
            let top = (event.position.y - anchor.grab).clamp(track.viewport.origin.y, last_top);
            let progress = (top - track.viewport.origin.y) / track.travel;
            handle.set_offset(point(
                handle.offset().x,
                -(handle.max_offset().y * progress),
            ));
            cx.notify(view);
        });

        let up_drag = drag.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble && up_drag.borrow_mut().take().is_some() {
                cx.notify(view);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// `ScrollHandle` 的几何只在 prepaint 里由真实布局写入，这里用同名结构复刻那套数学，
    /// 以便在没有窗口的情况下验证滑块长度与位置。
    fn track_for(viewport: f32, max_offset: f32, min_thumb: f32) -> Option<Track> {
        if max_offset <= 0.5 || viewport <= 0. {
            return None;
        }
        let content_height = viewport + max_offset;
        let thumb_height = (viewport * (viewport / content_height))
            .max(min_thumb)
            .min(viewport);
        Some(Track {
            viewport: Bounds {
                origin: point(px(10.), px(20.)),
                size: size(px(300.), px(viewport)),
            },
            thumb_height: px(thumb_height),
            travel: px(viewport - thumb_height),
        })
    }

    fn thumb_top_for(track: Track, max_offset: f32, offset_y: f32) -> Pixels {
        let progress = (-offset_y / max_offset).clamp(0., 1.) as f32;
        track.viewport.origin.y + track.travel * progress
    }

    #[test]
    fn hides_when_content_fits() {
        // 内容没有溢出：不画滑块，避免对完全可见的列表造成视觉噪声。
        assert!(track_for(400., 0., 36.).is_none());
        // 阈值内的浮点误差也应视为不可滚动。
        assert!(track_for(400., 0.4, 36.).is_none());
        assert!(track_for(0., 400., 36.).is_none());
    }

    #[test]
    fn thumb_length_tracks_overflow() {
        let track = track_for(400., 400., 36.).unwrap();
        // 内容总高 800，可见 400 → 滑块占一半。
        assert_eq!(track.viewport.size.height, px(400.));
        assert_eq!(track.thumb_height, px(200.));
        assert_eq!(track.travel, px(200.));
    }

    #[test]
    fn tiny_thumb_clamps_to_minimum_length() {
        // 内容 40000px 时按比例只有 4px，必须撑到最小长度以便抓取。
        let track = track_for(400., 39_600., 36.).unwrap();
        assert_eq!(track.thumb_height, px(36.));
        assert_eq!(track.travel, px(364.));
    }

    #[test]
    fn thumb_top_follows_scroll_progress() {
        let track = track_for(400., 400., 36.).unwrap();
        // 顶部：滑块贴着视口顶部。
        assert_eq!(thumb_top_for(track, 400., 0.), px(20.));
        // 滚到最底部：滑块贴底。
        assert_eq!(thumb_top_for(track, 400., -400.), px(220.));
        // 越界的偏移量被 clamp，滑块不会跑出轨道。
        assert_eq!(thumb_top_for(track, 400., -1_000.), px(220.));
        assert_eq!(thumb_top_for(track, 400., 120.), px(20.));
    }

    #[test]
    fn drag_state_is_shared_between_frames() {
        // 热区与事件回调共享同一个 RefCell，拖拽状态必须跨帧可见。
        let drag: DragState = Rc::new(RefCell::new(None));
        let clone = drag.clone();
        assert!(clone.borrow().is_none());
        *drag.borrow_mut() = Some(DragAnchor { grab: px(6.) });
        assert!(clone.borrow().is_some());
        let lifted = clone.borrow_mut().take();
        assert!(lifted.is_some());
        // 松开后清空，不会在下一帧继续拖动。
        assert!(drag.borrow().is_none());
        let _ = Cell::new(0);
    }
}
