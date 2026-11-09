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
// 参考 https://github.com/androidx/androidx/blob/androidx-main/compose/material3/material3/src/commonMain/kotlin/androidx/compose/material3/TextField.kt
// 参考 https://github.com/androidx/androidx/blob/androidx-main/compose/material3/material3/src/commonMain/kotlin/androidx/compose/material3/OutlinedTextField.kt

use std::ops::Range;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    App, AppContext as _, Bounds, ClipboardItem, Context, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, Font, FontFeatures, FontStyle,
    GlobalElementId, Hsla, InteractiveElement as _, IntoElement, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, ParentElement as _, Pixels, Point,
    Render, ShapedLine, SharedString, Style, Styled, TextRun, UTF16Selection, UnderlineStyle,
    Window, div, fill, point, prelude::FluentBuilder as _, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::icon::{Icon, IconName};
use crate::motion::{Animatable, AnimatedComponent, AnimationDriver, MotionRole, lerp_color};
use crate::theme::ActiveTheme;

type ChangeHandler = Rc<dyn Fn(&str, &mut Window, &mut App) + 'static>;
type SubmitHandler = Rc<dyn Fn(&str, &mut Window, &mut App) + 'static>;

pub struct TextField {
    id: ElementId,
    label: SharedString,
    value: SharedString,
    helper: Option<SharedString>,
    error: Option<SharedString>,
    leading_icon: Option<IconName>,
    disabled: bool,
    outlined: bool,
    password: bool,
    plain: bool,
    on_change: Option<ChangeHandler>,
    on_submit: Option<SubmitHandler>,
}

pub struct TextFieldState {
    id: ElementId,
    label: SharedString,
    helper: Option<SharedString>,
    error: Option<SharedString>,
    leading_icon: Option<IconName>,
    disabled: bool,
    outlined: bool,
    password: bool,
    plain: bool,
    value: String,

    /// Selection expressed as UTF-8 byte offsets into `value`.
    selected_range: Range<usize>,
    selection_reversed: bool,
    /// Active IME composition range, also expressed as UTF-8 byte offsets.
    marked_range: Option<Range<usize>>,
    is_selecting: bool,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
    focus: FocusHandle,

    focus_progress: Animatable,
    on_change: Option<ChangeHandler>,
    on_submit: Option<SubmitHandler>,
    driver: AnimationDriver,
}

impl TextField {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value: SharedString::default(),
            helper: None,
            error: None,
            leading_icon: None,
            disabled: false,
            outlined: false,
            password: false,
            plain: false,
            on_change: None,
            on_submit: None,
        }
    }

    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = value.into();
        self
    }

    pub fn helper(mut self, helper: impl Into<SharedString>) -> Self {
        self.helper = Some(helper.into());
        self
    }

    pub fn error(mut self, error: impl Into<SharedString>) -> Self {
        self.error = Some(error.into());
        self
    }

    pub fn leading_icon(mut self, icon: IconName) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn enabled(self, enabled: bool) -> Self {
        self.disabled(!enabled)
    }

    pub fn outlined(mut self) -> Self {
        self.outlined = true;
        self
    }

    pub fn password(mut self, password: bool) -> Self {
        self.password = password;
        self
    }

    pub fn plain(mut self, plain: bool) -> Self {
        self.plain = plain;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn on_value_change(self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_change(handler)
    }

    pub fn on_submit(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Rc::new(handler));
        self
    }

    pub fn build(self, cx: &mut App) -> Entity<TextFieldState> {
        let focus = cx.focus_handle();
        let value = self.value.to_string();
        let selection = value.len()..value.len();
        cx.new(|_| TextFieldState {
            id: self.id,
            label: self.label,
            helper: self.helper,
            error: self.error,
            leading_icon: self.leading_icon,
            disabled: self.disabled,
            outlined: self.outlined,
            password: self.password,
            plain: self.plain,
            value,
            selected_range: selection,
            selection_reversed: false,
            marked_range: None,
            is_selecting: false,
            last_layout: None,
            last_bounds: None,
            scroll_x: px(0.),
            focus,
            focus_progress: Animatable::new(0.0, 1.0e-3),
            on_change: self.on_change,
            on_submit: self.on_submit,
            driver: AnimationDriver::default(),
        })
    }
}

pub struct OutlinedTextField(TextField);

impl OutlinedTextField {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self(TextField::new(id, label).outlined())
    }

    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.0 = self.0.value(value);
        self
    }

    pub fn helper(mut self, helper: impl Into<SharedString>) -> Self {
        self.0 = self.0.helper(helper);
        self
    }

    pub fn error(mut self, error: impl Into<SharedString>) -> Self {
        self.0 = self.0.error(error);
        self
    }

    pub fn leading_icon(mut self, icon: IconName) -> Self {
        self.0 = self.0.leading_icon(icon);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.0 = self.0.enabled(enabled);
        self
    }

    pub fn on_value_change(
        mut self,
        handler: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.0 = self.0.on_value_change(handler);
        self
    }

    pub fn on_submit(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.0 = self.0.on_submit(handler);
        self
    }

    pub fn build(self, cx: &mut App) -> Entity<TextFieldState> {
        self.0.build(cx)
    }
}

impl TextFieldState {
    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, value: &str, cx: &mut Context<Self>) {
        self.value = value.to_string();
        self.selected_range = self.value.len()..self.value.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.is_selecting = false;
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    fn display_text(&self) -> String {
        if self.password {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        }
    }

    fn display_offset(&self, value_offset: usize) -> usize {
        if !self.password {
            return floor_char_boundary(&self.value, value_offset);
        }
        let offset = floor_char_boundary(&self.value, value_offset);
        self.value[..offset].chars().count() * '•'.len_utf8()
    }

    fn value_offset(&self, display_offset: usize) -> usize {
        if !self.password {
            return floor_char_boundary(&self.value, display_offset);
        }
        let display = self.display_text();
        let offset = floor_char_boundary(&display, display_offset);
        let chars = display[..offset].chars().count();
        self.value
            .char_indices()
            .nth(chars)
            .map(|(index, _)| index)
            .unwrap_or(self.value.len())
    }

    fn display_range(&self, range: &Range<usize>) -> Range<usize> {
        let start = self.display_offset(range.start);
        let end = self.display_offset(range.end);
        start.min(end)..start.max(end)
    }

    fn clamp_value_range(&self, range: Range<usize>) -> Range<usize> {
        let start = floor_char_boundary(&self.value, range.start);
        let end = floor_char_boundary(&self.value, range.end);
        start.min(end)..start.max(end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        range_from_utf16_in(&self.value, range)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        range_to_utf16_in(&self.value, range)
    }

    fn sync_focus(&mut self, focused: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !focused {
            self.is_selecting = false;
            self.marked_range = None;
        }
        let target = if focused { 1.0 } else { 0.0 };
        if (self.focus_progress.target() - target).abs() > f64::EPSILON {
            let spec = *cx.theme().motion().spec(MotionRole::FastEffects);
            self.focus_progress
                .animate_to(target, &spec, Instant::now());
            if self.focus_progress.is_running() {
                self.schedule_next(window, cx);
            }
        }
    }

    fn notify_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handler) = self.on_change.clone() {
            handler(&self.value, window, cx);
        }
        cx.notify();
    }

    fn replace_range_utf8(
        &mut self,
        range: Range<usize>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.clamp_value_range(range);
        let text = sanitize_single_line(text);
        if range.is_empty() && text.is_empty() {
            return;
        }
        let start = range.start;
        self.value.replace_range(range, &text);
        let cursor = start + text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.notify_change(window, cx);
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = floor_char_boundary(&self.value, offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = floor_char_boundary(&self.value, offset);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.marked_range = None;
        cx.notify();
    }

    fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.selected_range = self.clamp_value_range(range);
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.select_range(0..self.value.len(), cx);
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.value
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.value
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.value.len())
    }

    fn previous_word_boundary(&self, offset: usize) -> usize {
        let mut index = floor_char_boundary(&self.value, offset);
        while index > 0 {
            let previous = self.previous_boundary(index);
            let whitespace = self.value[previous..index].chars().all(char::is_whitespace);
            if !whitespace {
                break;
            }
            index = previous;
        }
        while index > 0 {
            let previous = self.previous_boundary(index);
            let whitespace = self.value[previous..index].chars().all(char::is_whitespace);
            if whitespace {
                break;
            }
            index = previous;
        }
        index
    }

    fn next_word_boundary(&self, offset: usize) -> usize {
        let mut index = floor_char_boundary(&self.value, offset);
        while index < self.value.len() {
            let next = self.next_boundary(index);
            let whitespace = self.value[index..next].chars().all(char::is_whitespace);
            if !whitespace {
                break;
            }
            index = next;
        }
        while index < self.value.len() {
            let next = self.next_boundary(index);
            let whitespace = self.value[index..next].chars().all(char::is_whitespace);
            if whitespace {
                break;
            }
            index = next;
        }
        index
    }

    fn word_range_at(&self, offset: usize) -> Range<usize> {
        let offset = floor_char_boundary(&self.value, offset);
        let mut start = offset;
        while start > 0 {
            let previous = self.previous_boundary(start);
            let whitespace = self.value[previous..start].chars().all(char::is_whitespace);
            if whitespace {
                break;
            }
            start = previous;
        }
        let mut end = offset;
        while end < self.value.len() {
            let next = self.next_boundary(end);
            let whitespace = self.value[end..next].chars().all(char::is_whitespace);
            if whitespace {
                break;
            }
            end = next;
        }
        start..end
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds.as_ref() else {
            return 0;
        };
        let Some(line) = self.last_layout.as_ref() else {
            return 0;
        };
        let display_index = line.closest_index_for_x(position.x - bounds.left() + self.scroll_x);
        self.value_offset(display_index)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus, cx);
        let offset = self.index_for_mouse_position(event.position);
        self.is_selecting = true;
        match event.click_count {
            3.. => self.select_all(cx),
            2 => {
                let word = self.word_range_at(offset);
                self.select_range(word, cx);
            }
            _ if event.modifiers.shift => self.select_to(offset, cx),
            _ => self.move_to(offset, cx),
        }
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled || !self.is_selecting {
            return;
        }
        let offset = self.index_for_mouse_position(event.position);
        self.select_to(offset, cx);
    }

    fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.is_selecting = false;
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        let text = self.value[self.selected_range.clone()].to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn cut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        self.copy(cx);
        self.replace_range_utf8(self.selected_range.clone(), "", window, cx);
    }

    fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_range_utf8(self.selected_range.clone(), &text, window, cx);
        }
    }

    fn backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.replace_range_utf8(self.selected_range.clone(), "", window, cx);
            return;
        }
        let cursor = self.cursor_offset();
        let previous = self.previous_boundary(cursor);
        self.replace_range_utf8(previous..cursor, "", window, cx);
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.replace_range_utf8(self.selected_range.clone(), "", window, cx);
            return;
        }
        let cursor = self.cursor_offset();
        let next = self.next_boundary(cursor);
        self.replace_range_utf8(cursor..next, "", window, cx);
    }

    fn backspace_word(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.replace_range_utf8(self.selected_range.clone(), "", window, cx);
            return;
        }
        let cursor = self.cursor_offset();
        let previous = self.previous_word_boundary(cursor);
        self.replace_range_utf8(previous..cursor, "", window, cx);
    }

    fn delete_word(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.replace_range_utf8(self.selected_range.clone(), "", window, cx);
            return;
        }
        let cursor = self.cursor_offset();
        let next = self.next_word_boundary(cursor);
        self.replace_range_utf8(cursor..next, "", window, cx);
    }

    fn handle_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let shift = modifiers.shift;
        let secondary = modifiers.secondary() && !modifiers.alt;
        let key = keystroke.key.as_str();

        if secondary {
            match key {
                "a" => {
                    self.select_all(cx);
                    return true;
                }
                "c" | "insert" => {
                    self.copy(cx);
                    return true;
                }
                "x" => {
                    self.cut(window, cx);
                    return true;
                }
                "v" => {
                    self.paste(window, cx);
                    return true;
                }
                "left" => {
                    let target = self.previous_word_boundary(self.cursor_offset());
                    if shift {
                        self.select_to(target, cx);
                    } else {
                        self.move_to(target, cx);
                    }
                    return true;
                }
                "right" => {
                    let target = self.next_word_boundary(self.cursor_offset());
                    if shift {
                        self.select_to(target, cx);
                    } else {
                        self.move_to(target, cx);
                    }
                    return true;
                }
                "backspace" => {
                    self.backspace_word(window, cx);
                    return true;
                }
                "delete" => {
                    self.delete_word(window, cx);
                    return true;
                }
                "home" => {
                    if shift {
                        self.select_to(0, cx);
                    } else {
                        self.move_to(0, cx);
                    }
                    return true;
                }
                "end" => {
                    let end = self.value.len();
                    if shift {
                        self.select_to(end, cx);
                    } else {
                        self.move_to(end, cx);
                    }
                    return true;
                }
                _ => {}
            }
        }

        if shift && key == "insert" {
            self.paste(window, cx);
            return true;
        }

        match key {
            "enter" => {
                if self.marked_range.is_some() {
                    return false;
                }
                if let Some(handler) = self.on_submit.clone() {
                    let value = self.value.clone();
                    handler(&value, window, cx);
                }
                true
            }
            "backspace" => {
                self.backspace(window, cx);
                true
            }
            "delete" => {
                self.delete(window, cx);
                true
            }
            "left" => {
                let target = if self.selected_range.is_empty() {
                    self.previous_boundary(self.cursor_offset())
                } else {
                    self.selected_range.start
                };
                if shift {
                    self.select_to(target, cx);
                } else {
                    self.move_to(target, cx);
                }
                true
            }
            "right" => {
                let target = if self.selected_range.is_empty() {
                    self.next_boundary(self.cursor_offset())
                } else {
                    self.selected_range.end
                };
                if shift {
                    self.select_to(target, cx);
                } else {
                    self.move_to(target, cx);
                }
                true
            }
            "home" => {
                if shift {
                    self.select_to(0, cx);
                } else {
                    self.move_to(0, cx);
                }
                true
            }
            "end" => {
                let end = self.value.len();
                if shift {
                    self.select_to(end, cx);
                } else {
                    self.move_to(end, cx);
                }
                true
            }
            _ => false,
        }
    }
}

impl EntityInputHandler for TextFieldState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.value[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.replace_range_utf8(range, new_text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let range = self.clamp_value_range(range);
        let new_text = sanitize_single_line(new_text);
        let start = range.start;
        let new_selection = new_selected_range_utf16
            .as_ref()
            .map(|range| range_from_utf16_in(&new_text, range))
            .unwrap_or(new_text.len()..new_text.len());

        self.value.replace_range(range, &new_text);
        self.marked_range = (!new_text.is_empty()).then_some(start..start + new_text.len());
        self.selected_range = start + new_selection.start..start + new_selection.end;
        self.selection_reversed = false;
        self.notify_change(window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.display_range(&self.range_from_utf16(&range_utf16));
        let (start, end) = match self.last_layout.as_ref() {
            Some(line) => (
                bounds.left() + line.x_for_index(range.start) - self.scroll_x,
                bounds.left() + line.x_for_index(range.end) - self.scroll_x,
            ),
            None => (bounds.left(), bounds.left()),
        };
        Some(Bounds::from_corners(
            point(start, bounds.top()),
            point(end.max(start), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let local = point.relative_to(&bounds.origin);
        let display_index = match self.last_layout.as_ref() {
            Some(line) => line.closest_index_for_x(local.x + self.scroll_x),
            None => 0,
        };
        let value_index = self.value_offset(display_index);
        Some(utf8_to_utf16_index(&self.value, value_index))
    }
}

impl Focusable for TextFieldState {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl AnimatedComponent for TextFieldState {
    fn step(&mut self, now: Instant) -> bool {
        self.focus_progress.tick(now)
    }

    fn driver_mut(&mut self) -> &mut AnimationDriver {
        &mut self.driver
    }
}

impl Render for TextFieldState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_progress.is_running() {
            self.schedule_next(window, cx);
        }

        let focused = !self.disabled && self.focus.is_focused(window);
        self.sync_focus(focused, window, cx);

        let theme = cx.theme();
        let colors = theme.colors();
        let tokens = theme.component().text_field;
        let p = self.focus_progress.value() as f32;

        let has_error = self.error.is_some();
        let style = TextFieldStyle::resolve_for_variant(
            theme.token_set(),
            has_error,
            self.disabled,
            self.outlined,
        );
        let accent = if has_error {
            colors.error
        } else {
            colors.primary
        };

        let border_color = if self.disabled || has_error {
            style.border_color
        } else {
            lerp_color(style.border_color, accent, p)
        };

        let floating = !self.plain && (p > 0.5 || !self.value.is_empty());
        let label_color = if focused {
            style.focused_label_color
        } else {
            style.label_color
        };
        let label_style = theme.typography();
        let gap = style.supporting_gap;

        let icon_size = style.icon_size;
        let min_h = style.min_height;
        let text_style = label_style.body_large;

        let entity = cx.entity();
        let key_entity = entity.clone();

        let container = div()
            .id(self.id.clone())
            .min_h(min_h)
            .w_full()
            .relative()
            .flex()
            .flex_col()
            .justify_center()
            .rounded(theme.shapes().medium)
            .when(!self.plain && !self.outlined, |el| {
                el.bg(style.container_color)
            })
            .when(!self.plain && self.outlined, |el| {
                el.border_1()
                    .border_color(border_color)
                    .when(focused && !self.disabled, |el| {
                        el.border_2()
                            .border_color(if has_error { colors.error } else { accent })
                    })
            })
            .when(!self.disabled, |el| {
                el.cursor_text()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            })
            .on_key_down(move |event, window, cx| {
                let mut handled = false;
                key_entity.update(cx, |state, cx| {
                    handled = state.handle_key(event, window, cx);
                });
                if handled {
                    cx.stop_propagation();
                }
            })
            .track_focus(&self.focus);

        let text_element = TextFieldTextElement {
            field: entity,
            font: Font {
                family: theme.font_family().clone(),
                fallbacks: None,
                features: FontFeatures::default(),
                weight: text_style.weight,
                style: FontStyle::Normal,
            },
            font_size: text_style.size,
            line_height: text_style.line_height,
            text_color: style.text_color,
            accent,
            selection_color: accent.opacity(0.28),
            disabled: self.disabled,
        };

        let row = div()
            .flex()
            .items_center()
            .gap(px(tokens.horizontal_padding * 0.5))
            .px(px(tokens.horizontal_padding))
            .py(px(tokens.top_padding));
        let row = row
            .when_some(self.leading_icon.clone(), |el, icon| {
                el.child(Icon::new(icon).size(icon_size).color(style.icon_color))
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .min_h(px(tokens.min_height
                        - tokens.top_padding
                        - tokens.bottom_padding))
                    .flex()
                    .items_center()
                    .child(text_element)
                    .when(self.value.is_empty() && !floating, |el| {
                        el.child(
                            div()
                                .absolute()
                                .left_0()
                                .text_size(text_style.size)
                                .text_color(style.placeholder_color)
                                .child(self.label.clone()),
                        )
                    }),
            );

        let inside_label = floating && !self.outlined;
        let label_el = if floating && self.outlined {
            div()
                .absolute()
                .top(px(-8.))
                .left(px(tokens.horizontal_padding))
                .px(px(4.))
                .bg(colors.surface)
                .text_size(label_style.label_small.size)
                .text_color(label_color)
                .child(self.label.clone())
        } else {
            div()
        };

        let supporting = if let Some(err) = &self.error {
            div()
                .text_size(label_style.body_small.size)
                .text_color(colors.error)
                .child(err.clone())
        } else if let Some(helper) = &self.helper {
            div()
                .text_size(label_style.body_small.size)
                .text_color(colors.on_surface_variant)
                .child(helper.clone())
        } else {
            div()
        };

        container
            .when(inside_label, |el| {
                el.child(
                    label_style
                        .label_small
                        .apply(div())
                        .pl(px(tokens.horizontal_padding))
                        .pt(px(4.))
                        .text_color(label_color)
                        .child(self.label.clone()),
                )
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(label_el)
                    .child(row),
            )
            .when(
                !self.plain && (self.error.is_some() || self.helper.is_some()),
                |el| {
                    el.child(
                        div()
                            .px(px(tokens.horizontal_padding))
                            .mt(gap)
                            .child(supporting),
                    )
                },
            )
    }
}

struct TextFieldPrepaint {
    line: Option<ShapedLine>,
    selection: Option<PaintQuad>,
    caret: Option<PaintQuad>,
    scroll_x: Pixels,
}

struct TextFieldTextElement {
    field: Entity<TextFieldState>,
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
    text_color: Hsla,
    accent: Hsla,
    selection_color: Hsla,
    disabled: bool,
}

impl IntoElement for TextFieldTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextFieldTextElement {
    type RequestLayoutState = ();
    type PrepaintState = TextFieldPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = self.line_height.into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (display, selected_range, selection_reversed, marked_range, focused, mut scroll_x) = {
            let state = self.field.read(cx);
            let display = state.display_text();
            let selected = state.display_range(&state.selected_range);
            let marked = state
                .marked_range
                .as_ref()
                .map(|range| state.display_range(range));
            (
                display,
                selected,
                state.selection_reversed,
                marked,
                state.focus.is_focused(window),
                state.scroll_x,
            )
        };

        let base_run = TextRun {
            len: display.len(),
            font: self.font.clone(),
            color: self.text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = text_runs(&display, marked_range, &base_run, self.text_color);
        let line = if display.is_empty() {
            None
        } else {
            let text: SharedString = display.into();
            Some(
                window
                    .text_system()
                    .shape_line(text, self.font_size, &runs, None),
            )
        };

        // Keep the caret inside the visible area for single-line values that
        // overflow the field. The caret is anchored to the active selection
        // edge so following the insertion point looks continuous while typing.
        let line_width = line.as_ref().map(|line| line.width).unwrap_or(px(0.));
        let viewport = bounds.size.width;
        if line_width <= viewport {
            scroll_x = px(0.);
        } else {
            let max_scroll = (line_width - viewport).max(px(0.));
            let anchor = if selection_reversed {
                selected_range.start
            } else {
                selected_range.end
            };
            let anchor_x = line
                .as_ref()
                .map(|line| line.x_for_index(anchor))
                .unwrap_or(px(0.));
            if anchor_x < scroll_x {
                scroll_x = anchor_x;
            } else if anchor_x + px(2.) - scroll_x > viewport {
                scroll_x = (anchor_x + px(2.) - viewport).min(max_scroll);
            }
            scroll_x = scroll_x.max(px(0.)).min(max_scroll);
        }
        self.field.update(cx, |state, _| state.scroll_x = scroll_x);

        let selection = line.as_ref().and_then(|line| {
            if selected_range.is_empty() {
                None
            } else {
                let left = bounds.left() + line.x_for_index(selected_range.start) - scroll_x;
                let right = bounds.left() + line.x_for_index(selected_range.end) - scroll_x;
                Some(fill(
                    Bounds::from_corners(
                        point(left, bounds.top() + px(2.)),
                        point(right, bounds.bottom() - px(2.)),
                    ),
                    self.selection_color,
                ))
            }
        });

        let caret = if focused && selected_range.is_empty() {
            let x = line
                .as_ref()
                .map(|line| line.x_for_index(selected_range.end))
                .unwrap_or(px(0.));
            Some(fill(
                Bounds::new(
                    point(bounds.left() + x - scroll_x, bounds.top() + px(2.)),
                    size(px(2.), (bounds.size.height - px(4.)).max(px(1.))),
                ),
                self.accent,
            ))
        } else {
            None
        };

        TextFieldPrepaint {
            line,
            selection,
            caret,
            scroll_x,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.field.read(cx).focus.clone();
        if !self.disabled {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.field.clone()),
                cx,
            );
        }

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }

        if let Some(line) = prepaint.line.as_ref() {
            let origin = point(bounds.left() - prepaint.scroll_x, bounds.top());
            let _ = line.paint(
                origin,
                self.line_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            );
        }

        if focus_handle.is_focused(window)
            && let Some(caret) = prepaint.caret.take()
        {
            window.paint_quad(caret);
        }

        let line = prepaint.line.take();
        self.field.update(cx, |state, _cx| {
            state.last_layout = line;
            state.last_bounds = Some(bounds);
        });
    }
}

fn text_runs(
    display: &str,
    marked: Option<Range<usize>>,
    base_run: &TextRun,
    underline_color: Hsla,
) -> Vec<TextRun> {
    let Some(marked) = marked else {
        return if display.is_empty() {
            Vec::new()
        } else {
            vec![base_run.clone()]
        };
    };
    let start = floor_char_boundary(display, marked.start);
    let end = floor_char_boundary(display, marked.end);
    if start >= end {
        return vec![base_run.clone()];
    }

    let underline = UnderlineStyle {
        color: Some(underline_color),
        thickness: px(1.),
        wavy: false,
    };
    let mut runs = Vec::new();
    let mut cursor = 0;
    if cursor < start {
        runs.push(TextRun {
            len: start - cursor,
            ..base_run.clone()
        });
        cursor = start;
    }
    if cursor < end {
        runs.push(TextRun {
            len: end - cursor,
            underline: Some(underline),
            ..base_run.clone()
        });
        cursor = end;
    }
    if cursor < display.len() {
        runs.push(TextRun {
            len: display.len() - cursor,
            ..base_run.clone()
        });
    }
    runs
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn utf16_to_utf8_index(text: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    for (index, character) in text.char_indices() {
        if utf16_offset >= offset {
            return index;
        }
        utf16_offset += character.len_utf16();
        if utf16_offset > offset {
            return index;
        }
    }
    text.len()
}

fn utf8_to_utf16_index(text: &str, offset: usize) -> usize {
    let offset = floor_char_boundary(text, offset);
    text[..offset].encode_utf16().count()
}

fn range_from_utf16_in(text: &str, range: &Range<usize>) -> Range<usize> {
    let start = utf16_to_utf8_index(text, range.start);
    let end = utf16_to_utf8_index(text, range.end);
    start.min(end)..start.max(end)
}

fn range_to_utf16_in(text: &str, range: &Range<usize>) -> Range<usize> {
    let start = utf8_to_utf16_index(text, range.start);
    let end = utf8_to_utf16_index(text, range.end);
    start.min(end)..start.max(end)
}

fn sanitize_single_line(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\r' | '\n' => ' ',
            character => character,
        })
        .collect()
}

pub use appearance::TextFieldStyle;

mod appearance {
    use crate::theme::TokenSet;
    use gpui::{Hsla, Pixels, px};

    #[derive(Clone, Copy, Debug)]
    pub struct TextFieldStyle {
        pub container_color: Hsla,

        pub border_color: Hsla,

        pub accent: Hsla,

        pub label_color: Hsla,

        pub focused_label_color: Hsla,

        pub text_color: Hsla,

        pub placeholder_color: Hsla,

        pub helper_color: Hsla,

        pub error_color: Hsla,

        pub icon_color: Hsla,

        pub disabled_container_color: Hsla,

        pub min_height: Pixels,

        pub corner_radius: Pixels,

        pub horizontal_padding: Pixels,

        pub vertical_padding: Pixels,

        pub supporting_gap: Pixels,

        pub icon_size: Pixels,

        pub text: crate::theme::TypeStyle,

        pub floating_label: crate::theme::TypeStyle,

        pub supporting_text: crate::theme::TypeStyle,
    }
    impl TextFieldStyle {
        pub fn resolve(tokens: &TokenSet, error: bool, disabled: bool) -> Self {
            Self::resolve_for_variant(tokens, error, disabled, false)
        }

        pub fn resolve_for_variant(
            tokens: &TokenSet,
            error: bool,
            disabled: bool,
            outlined: bool,
        ) -> Self {
            let colors = &tokens.colors;
            let state = &tokens.state_layer;
            let field = &tokens.component.text_field;
            let accent = if error { colors.error } else { colors.primary };
            Self {
                container_color: if disabled {
                    colors.on_surface.opacity(state.disabled_container / 6.0)
                } else if outlined {
                    colors.surface
                } else {
                    colors.surface_container
                },
                border_color: if disabled {
                    colors.disabled_container(state)
                } else if error {
                    colors.error
                } else {
                    colors.outline
                },
                accent,
                label_color: if disabled {
                    colors.disabled_content(state)
                } else if error {
                    colors.error
                } else {
                    colors.on_surface_variant
                },
                focused_label_color: if error {
                    colors.error
                } else {
                    colors.on_surface_variant
                },
                text_color: if disabled {
                    colors.disabled_content(state)
                } else {
                    colors.on_surface
                },
                placeholder_color: if disabled {
                    colors.disabled_content(state)
                } else {
                    colors.on_surface_variant
                },
                helper_color: colors.on_surface_variant,
                error_color: colors.error,
                icon_color: if disabled {
                    colors.disabled_content(state)
                } else {
                    colors.on_surface_variant
                },
                disabled_container_color: colors.on_surface.opacity(state.disabled_container / 6.0),
                min_height: px(field.min_height),
                corner_radius: tokens.shapes.extra_small,
                horizontal_padding: px(field.horizontal_padding),
                vertical_padding: px(field.top_padding),
                supporting_gap: px(field.supporting_gap),
                icon_size: px(field.icon_size),
                text: tokens.typography.body_large,
                floating_label: tokens.typography.label_small,
                supporting_text: tokens.typography.body_small,
            }
        }
    }
}

pub struct SecureTextField {
    id: ElementId,
    label: SharedString,
    value: SharedString,
    enabled: bool,
    on_value_change: Option<ChangeHandler>,
}
impl SecureTextField {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value: SharedString::default(),
            enabled: true,
            on_value_change: None,
        }
    }
    pub fn value(mut self, v: impl Into<SharedString>) -> Self {
        self.value = v.into();
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn on_value_change(
        mut self,
        handler: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_value_change = Some(Rc::new(handler));
        self
    }
}
impl SecureTextField {
    pub fn build(self, cx: &mut App) -> Entity<TextFieldState> {
        let mut field = TextField::new(self.id, self.label)
            .password(true)
            .value(self.value)
            .enabled(self.enabled);
        if let Some(handler) = self.on_value_change {
            field = field.on_value_change(move |value, window, cx| handler(value, window, cx));
        }
        field.build(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_ranges_round_trip_for_supplementary_characters() {
        // “😀” 占 4 字节 UTF-8，但在 UTF-16 里是两个 code unit。
        let text = "a😀b";
        assert_eq!(range_to_utf16_in(text, &(1..5)), 1..3);
        assert_eq!(range_from_utf16_in(text, &(1..3)), 1..5);
        // IME 落在代理对中间时收敛到字符边界，不能切出半个字符。
        assert_eq!(range_from_utf16_in(text, &(2..2)), 1..1);
        assert_eq!(utf16_to_utf8_index(text, 99), text.len());
    }

    #[test]
    fn ranges_clamp_to_char_boundaries() {
        let text = "中文abc";
        // 每个汉字占 3 字节：2 落在「中」中间，4 落在「文」中间。
        assert_eq!(floor_char_boundary(text, 2), 0);
        assert_eq!(floor_char_boundary(text, 4), 3);
        // 字符边界原样返回，不会被多退一格。
        assert_eq!(floor_char_boundary(text, 3), 3);
        assert_eq!(floor_char_boundary(text, 999), text.len());
        // 反向区间被规范化，避免出现 start > end。
        // UTF-16 的 2 对应「中文」之后（UTF-8 第 6 字节），5 对应文末。
        assert_eq!(range_from_utf16_in(text, &(5..2)), 6..9);
    }

    #[test]
    fn single_line_sanitizes_newlines() {
        assert_eq!(sanitize_single_line("a\r\nb"), "a  b");
        assert_eq!(sanitize_single_line("no breaks"), "no breaks");
    }
}
