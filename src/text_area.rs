use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, Style, TextAlign, TextRun, UTF16Selection, Window, actions, div, fill, point,
    prelude::*, px, relative, rgb, rgba,
};
use unicode_segmentation::*;

use crate::{
    text_input::{
        Backspace, Copy, Cut, Delete, End, Home, Left, Paste, Right, SelectAll, SelectLeft,
        SelectRight, ShowCharacterPalette,
    },
    theme::*,
};

actions!(text_area, [Up, Down, SelectUp, SelectDown, Enter]);

pub struct TextArea {
    pub focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    is_selecting: bool,
    pub disabled: bool,
    line_offsets: Vec<usize>,
    last_layouts: Vec<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    last_line_height: Pixels,
}

impl TextArea {
    pub fn new(context: &mut App, placeholder: &str, initial: Option<&str>) -> Entity<Self> {
        let placeholder: SharedString = placeholder.to_string().into();
        let content = initial.unwrap_or("").to_string();
        let length = content.len();
        context.new(|context| {
            let mut this = Self {
                focus_handle: context.focus_handle(),
                content,
                placeholder,
                selected_range: length..length,
                selection_reversed: false,
                marked_range: None,
                is_selecting: false,
                disabled: false,
                line_offsets: vec![0],
                last_layouts: Vec::new(),
                last_bounds: None,
                last_line_height: px(LINE_HEIGHT_EXTRA_SMALL),
            };
            this.recompute_line_offsets();
            this
        })
    }

    pub fn text(&self) -> String {
        self.content.clone()
    }

    pub fn set_content(&mut self, text: &str) {
        self.content = text.replace("\r\n", "\n").replace('\r', "");
        self.recompute_line_offsets();
        let length = self.content.len();
        self.selected_range = length..length;
        self.marked_range = None;
    }

    fn recompute_line_offsets(&mut self) {
        self.line_offsets.clear();
        self.line_offsets.push(0);
        for (index, byte) in self.content.bytes().enumerate() {
            if byte == b'\n' {
                self.line_offsets.push(index + 1);
            }
        }
    }

    fn num_lines(&self) -> usize {
        self.line_offsets.len()
    }

    fn line_start(&self, line: usize) -> usize {
        self.line_offsets[line]
    }

    fn line_text_end(&self, line: usize) -> usize {
        if line + 1 < self.line_offsets.len() {
            self.line_offsets[line + 1] - 1
        } else {
            self.content.len()
        }
    }

    fn line_full_end(&self, line: usize) -> usize {
        if line + 1 < self.line_offsets.len() {
            self.line_offsets[line + 1]
        } else {
            self.content.len()
        }
    }

    fn offset_to_line(&self, offset: usize) -> usize {
        self.line_offsets
            .partition_point(|&o| o <= offset)
            .saturating_sub(1)
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, context: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        context.notify();
    }

    fn select_to(&mut self, offset: usize, context: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selected_range = self.selected_range.end..self.selected_range.start;
            self.selection_reversed = !self.selection_reversed;
        }
        context.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn left(&mut self, _: &Left, _: &mut Window, context: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), context);
        } else {
            self.move_to(self.selected_range.start, context);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, context: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), context);
        } else {
            self.move_to(self.selected_range.end, context);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, context: &mut Context<Self>) {
        let offset = self.cursor_offset();
        let line = self.offset_to_line(offset);
        if line == 0 {
            self.move_to(0, context);
            return;
        }
        let column = offset - self.line_start(line);
        let prev_line = line - 1;
        let previous_line_length = self.line_text_end(prev_line) - self.line_start(prev_line);
        self.move_to(
            self.line_start(prev_line) + column.min(previous_line_length),
            context,
        );
    }

    fn down(&mut self, _: &Down, _: &mut Window, context: &mut Context<Self>) {
        let offset = self.cursor_offset();
        let line = self.offset_to_line(offset);
        if line + 1 >= self.num_lines() {
            self.move_to(self.content.len(), context);
            return;
        }
        let column = offset - self.line_start(line);
        let next_line = line + 1;
        let next_line_length = self.line_text_end(next_line) - self.line_start(next_line);
        self.move_to(
            self.line_start(next_line) + column.min(next_line_length),
            context,
        );
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, context: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), context);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, context: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), context);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, context: &mut Context<Self>) {
        let offset = self.cursor_offset();
        let line = self.offset_to_line(offset);
        if line == 0 {
            self.select_to(0, context);
            return;
        }
        let column = offset - self.line_start(line);
        let prev_line = line - 1;
        let previous_line_length = self.line_text_end(prev_line) - self.line_start(prev_line);
        self.select_to(
            self.line_start(prev_line) + column.min(previous_line_length),
            context,
        );
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, context: &mut Context<Self>) {
        let offset = self.cursor_offset();
        let line = self.offset_to_line(offset);
        if line + 1 >= self.num_lines() {
            self.select_to(self.content.len(), context);
            return;
        }
        let column = offset - self.line_start(line);
        let next_line = line + 1;
        let next_line_length = self.line_text_end(next_line) - self.line_start(next_line);
        self.select_to(
            self.line_start(next_line) + column.min(next_line_length),
            context,
        );
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, context: &mut Context<Self>) {
        self.move_to(0, context);
        self.select_to(self.content.len(), context);
    }

    fn home(&mut self, _: &Home, _: &mut Window, context: &mut Context<Self>) {
        let line = self.offset_to_line(self.cursor_offset());
        self.move_to(self.line_start(line), context);
    }

    fn end(&mut self, _: &End, _: &mut Window, context: &mut Context<Self>) {
        let line = self.offset_to_line(self.cursor_offset());
        self.move_to(self.line_text_end(line), context);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, context: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), context);
        }
        self.replace_text_in_range(None, "", window, context);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, context: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), context);
        }
        self.replace_text_in_range(None, "", window, context);
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, context: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.replace_text_in_range(None, "\n", window, context);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, context: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(text) = context.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, context);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, context: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            context.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, context: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if !self.selected_range.is_empty() {
            context.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, context);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() || self.last_layouts.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds.as_ref() else {
            return 0;
        };
        let line_height = self.last_line_height;
        let line_count = self.last_layouts.len();

        let line_index = if position.y < bounds.top() {
            0
        } else {
            let mut index = 0;
            let mut y = bounds.top() + line_height;
            while index < line_count - 1 && position.y >= y {
                index += 1;
                y += line_height;
            }
            index
        };

        let line_start = self.line_start(line_index);
        let line_text_length = self.line_text_end(line_index) - line_start;
        let relative_x = position.x - bounds.left();
        let display_index = self.last_layouts[line_index].closest_index_for_x(relative_x);
        line_start + display_index.min(line_text_length)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        context: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), context);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), context);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        context: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), context);
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for character in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += character.len_utf16();
            utf8_offset += character.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for character in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += character.len_utf8();
            utf16_offset += character.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _context: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        context: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        let clean = new_text.replace("\r\n", "\n").replace('\r', "");
        self.content = self.content[..range.start].to_owned() + &clean + &self.content[range.end..];
        self.selected_range = range.start + clean.len()..range.start + clean.len();
        self.marked_range.take();
        self.recompute_line_offsets();
        context.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        let clean = new_text.replace("\r\n", "\n").replace('\r', "");
        self.content = self.content[..range.start].to_owned() + &clean + &self.content[range.end..];

        if !clean.is_empty() {
            self.marked_range = Some(range.start..range.start + clean.len());
        } else {
            self.marked_range = None;
        }

        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + clean.len()..range.start + clean.len());

        self.recompute_line_offsets();
        context.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let line = self.offset_to_line(range.start);
        let layout = self.last_layouts.get(line)?;
        let line_height = self.last_line_height;

        let column_start = range.start - self.line_start(line);
        let column_end = if self.offset_to_line(range.end) == line {
            range.end - self.line_start(line)
        } else {
            self.line_text_end(line) - self.line_start(line)
        };

        let x_start = layout.x_for_index(column_start);
        let x_end = layout.x_for_index(column_end);
        let y = bounds.top() + line_height * line as f32;

        Some(Bounds::from_corners(
            point(bounds.left() + x_start, y),
            point(bounds.left() + x_end, y + line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _window: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<usize> {
        self.last_bounds?;
        let offset = self.index_for_mouse_position(position);
        Some(self.offset_to_utf16(offset))
    }
}

struct TextAreaElement {
    area: Entity<TextArea>,
}

struct TextAreaPrepaintState {
    lines: Vec<ShapedLine>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for TextAreaElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = TextAreaPrepaintState;

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
        context: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let area = self.area.read(context);
        let line_count = area.num_lines().max(1);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (window.line_height() * line_count as f32).into();
        (window.request_layout(style, [], context), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        context: &mut App,
    ) -> TextAreaPrepaintState {
        let area = self.area.read(context);
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let selection = area.selected_range.clone();
        let cursor_offset = area.cursor_offset();
        let content_empty = area.content.is_empty();

        if content_empty {
            let placeholder = area.placeholder.clone();
            let run = TextRun {
                len: placeholder.len(),
                font: text_style.font(),
                color: rgba(INPUT_PLACEHOLDER).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs: Vec<TextRun> = vec![run].into_iter().filter(|run| run.len > 0).collect();
            let shaped = window
                .text_system()
                .shape_line(placeholder, font_size, &runs, None);

            let cursor = Some(fill(
                Bounds::new(
                    point(bounds.left(), bounds.top()),
                    gpui::size(px(CURSOR_WIDTH), line_height),
                ),
                rgb(BORDER_FOCUS),
            ));

            return TextAreaPrepaintState {
                lines: vec![shaped],
                cursor,
                selections: Vec::new(),
            };
        }

        let line_count = area.num_lines();
        let mut shaped_lines = Vec::with_capacity(line_count);
        let mut selections = Vec::new();

        for line_index in 0..line_count {
            let line_start = area.line_start(line_index);
            let line_text_end = area.line_text_end(line_index);
            let line_full_end = area.line_full_end(line_index);
            let line_text_len = line_text_end - line_start;

            let text: SharedString = area.content[line_start..line_text_end].to_string().into();
            let run = TextRun {
                len: text.len(),
                font: text_style.font(),
                color: text_style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs: Vec<TextRun> = vec![run].into_iter().filter(|run| run.len > 0).collect();
            let shaped = window
                .text_system()
                .shape_line(text, font_size, &runs, None);

            if !selection.is_empty()
                && selection.start < line_full_end
                && selection.end > line_start
            {
                let local_start = selection
                    .start
                    .saturating_sub(line_start)
                    .min(line_text_len);
                let local_end = selection.end.saturating_sub(line_start).min(line_text_len);
                let x_start = shaped.x_for_index(local_start);
                let x_end = shaped.x_for_index(local_end);
                let line_y = bounds.top() + line_height * line_index as f32;
                let right = if selection.end > line_text_end {
                    bounds.right()
                } else {
                    bounds.left() + x_end
                };
                selections.push(fill(
                    Bounds::from_corners(
                        point(bounds.left() + x_start, line_y),
                        point(right, line_y + line_height),
                    ),
                    rgba(SELECTION),
                ));
            }

            shaped_lines.push(shaped);
        }

        let cursor = if selection.is_empty() {
            let cursor_line = area.offset_to_line(cursor_offset);
            let cursor_column = cursor_offset - area.line_start(cursor_line);
            let x = if let Some(layout) = shaped_lines.get(cursor_line) {
                layout.x_for_index(cursor_column)
            } else {
                px(0.)
            };
            let y = bounds.top() + line_height * cursor_line as f32;
            Some(fill(
                Bounds::new(
                    point(bounds.left() + x, y),
                    gpui::size(px(CURSOR_WIDTH), line_height),
                ),
                rgb(BORDER_FOCUS),
            ))
        } else {
            None
        };

        TextAreaPrepaintState {
            lines: shaped_lines,
            cursor,
            selections,
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
        context: &mut App,
    ) {
        let focus_handle = self.area.read(context).focus_handle.clone();
        let line_height = window.line_height();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.area.clone()),
            context,
        );
        let selection_quads = std::mem::take(&mut prepaint.selections);
        for quad in selection_quads {
            window.paint_quad(quad);
        }
        let cursor = prepaint.cursor.take();
        for (line_index, line) in prepaint.lines.iter().enumerate() {
            let origin = point(
                bounds.left(),
                bounds.top() + line_height * line_index as f32,
            );
            if let Err(error) =
                line.paint(origin, line_height, TextAlign::Left, None, window, context)
            {
                log::warn!("[text_area] failed to paint line: {error}");
            }
        }
        if focus_handle.is_focused(window)
            && let Some(cursor) = cursor
        {
            window.paint_quad(cursor);
        }
        let lines = std::mem::take(&mut prepaint.lines);
        self.area.update(context, |area, _| {
            area.last_layouts = lines;
            area.last_bounds = Some(bounds);
            area.last_line_height = line_height;
        });
    }
}

impl Render for TextArea {
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.disabled;
        let focused = !disabled && self.focus_handle.is_focused(window);
        let border = if focused { BORDER_FOCUS } else { BORDER };
        let text_color = if disabled { TEXT_DIM } else { TEXT_PRIMARY };

        div()
            .key_context("TextArea")
            .when(!disabled, |element| {
                element.track_focus(&self.focus_handle(context))
            })
            .when(!disabled, |element| element.cursor(CursorStyle::IBeam))
            .on_action(context.listener(Self::backspace))
            .on_action(context.listener(Self::delete))
            .on_action(context.listener(Self::left))
            .on_action(context.listener(Self::right))
            .on_action(context.listener(Self::up))
            .on_action(context.listener(Self::down))
            .on_action(context.listener(Self::select_left))
            .on_action(context.listener(Self::select_right))
            .on_action(context.listener(Self::select_up))
            .on_action(context.listener(Self::select_down))
            .on_action(context.listener(Self::select_all))
            .on_action(context.listener(Self::home))
            .on_action(context.listener(Self::end))
            .on_action(context.listener(Self::enter))
            .on_action(context.listener(Self::show_character_palette))
            .on_action(context.listener(Self::paste))
            .on_action(context.listener(Self::cut))
            .on_action(context.listener(Self::copy))
            .when(!disabled, |element| {
                element
                    .on_mouse_down(MouseButton::Left, context.listener(Self::on_mouse_down))
                    .on_mouse_up(MouseButton::Left, context.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, context.listener(Self::on_mouse_up))
                    .on_mouse_move(context.listener(Self::on_mouse_move))
            })
            .text_color(rgb(text_color))
            .text_size(px(TEXT_SIZE_EXTRA_SMALL))
            .line_height(px(LINE_HEIGHT_EXTRA_SMALL))
            .child(
                div()
                    .id("textarea-scroll")
                    .w_full()
                    .h(px(TEXTAREA_HEIGHT))
                    .overflow_y_scroll()
                    .px(px(PADDING_INPUT_HORIZONTAL))
                    .py(px(PADDING_INPUT_VERTICAL))
                    .bg(rgb(INPUT_BACKGROUND))
                    .border_1()
                    .border_color(rgb(border))
                    .rounded(px(RADIUS))
                    .child(TextAreaElement {
                        area: context.entity().clone(),
                    }),
            )
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
