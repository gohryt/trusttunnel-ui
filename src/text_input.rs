use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, Style, TextAlign, TextRun, UTF16Selection, Window, actions, div, fill, point,
    prelude::*, px, relative, rgb, rgba,
};
use unicode_segmentation::*;

use crate::theme::*;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
    ]
);

pub struct TextInput {
    pub focus_handle: FocusHandle,
    pub content: SharedString,
    pub placeholder: SharedString,
    pub selected_range: Range<usize>,
    pub selection_reversed: bool,
    pub marked_range: Option<Range<usize>>,
    pub last_layout: Option<ShapedLine>,
    pub last_bounds: Option<Bounds<Pixels>>,
    pub is_selecting: bool,
    pub is_password: bool,
    pub disabled: bool,
}

impl TextInput {
    pub fn new(
        context: &mut App,
        placeholder: &str,
        is_password: bool,
        initial: Option<&str>,
    ) -> Entity<Self> {
        let placeholder: SharedString = placeholder.to_string().into();
        let content: SharedString = initial.unwrap_or("").to_string().into();
        let length = content.len();
        context.new(|context| Self {
            focus_handle: context.focus_handle(),
            content,
            placeholder,
            selected_range: length..length,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            is_password,
            disabled: false,
        })
    }

    pub fn text(&self) -> String {
        self.content.to_string()
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

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, context: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), context);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, context: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), context);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, context: &mut Context<Self>) {
        self.move_to(0, context);
        self.select_to(self.content.len(), context);
    }

    fn home(&mut self, _: &Home, _: &mut Window, context: &mut Context<Self>) {
        self.move_to(0, context);
    }

    fn end(&mut self, _: &End, _: &mut Window, context: &mut Context<Self>) {
        self.move_to(self.content.len(), context);
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

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
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

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, context: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if let Some(text) = context.read_from_clipboard().and_then(|item| item.text()) {
            let clean = text.replace(['\n', '\r'], "");
            self.replace_text_in_range(None, &clean, window, context);
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

    fn move_to(&mut self, offset: usize, context: &mut Context<Self>) {
        self.selected_range = offset..offset;
        context.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn to_display_offset(&self, offset: usize) -> usize {
        if self.is_password {
            self.content[..offset].graphemes(true).count() * '•'.len_utf8()
        } else {
            offset
        }
    }

    fn display_offset_to_content(&self, display_offset: usize) -> usize {
        if self.is_password {
            let bullet_len = '•'.len_utf8();
            let grapheme_idx = display_offset / bullet_len;
            self.content
                .grapheme_indices(true)
                .nth(grapheme_idx)
                .map(|(i, _)| i)
                .unwrap_or(self.content.len())
        } else {
            display_offset
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        let display_idx = line.closest_index_for_x(position.x - bounds.left());
        self.display_offset_to_content(display_idx)
    }

    fn select_to(&mut self, offset: usize, context: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        context.notify();
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
}

impl EntityInputHandler for TextInput {
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

        let clean: String = new_text
            .chars()
            .filter(|character| *character != '\n' && *character != '\r')
            .collect();

        self.content =
            (self.content[0..range.start].to_owned() + &clean + &self.content[range.end..]).into();
        self.selected_range = range.start + clean.len()..range.start + clean.len();
        self.marked_range.take();
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

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();

        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }

        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());

        context.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = self.to_display_offset(range.start);
        let end = self.to_display_offset(range.end);
        Some(Bounds::from_corners(
            point(bounds.left() + last_layout.x_for_index(start), bounds.top()),
            point(
                bounds.left() + last_layout.x_for_index(end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: gpui::Point<Pixels>,
        _window: &mut Window,
        _context: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&position)?;
        let last_layout = self.last_layout.as_ref()?;
        let display_index = last_layout.index_for_x(position.x - line_point.x)?;
        let utf8_index = self.display_offset_to_content(display_index);
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
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
    ) -> Self::PrepaintState {
        let input = self.input.read(context);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let is_password = input.is_password;
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), rgba(INPUT_PLACEHOLDER).into())
        } else if is_password {
            let bullets: String = content.graphemes(true).map(|_| '•').collect();
            (SharedString::from(bullets), style.color)
        } else {
            (content.clone(), style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            if !is_password {
                vec![
                    TextRun {
                        len: marked_range.start,
                        ..run.clone()
                    },
                    TextRun {
                        len: marked_range.end - marked_range.start,
                        underline: Some(gpui::UnderlineStyle {
                            color: Some(run.color),
                            thickness: px(1.0),
                            wavy: false,
                        }),
                        ..run.clone()
                    },
                    TextRun {
                        len: display_text.len() - marked_range.end,
                        ..run
                    },
                ]
                .into_iter()
                .filter(|run| run.len > 0)
                .collect()
            } else {
                vec![run]
            }
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let bullet_len = '•'.len_utf8();
        let to_display = |offset: usize| -> usize {
            if is_password {
                content[..offset].graphemes(true).count() * bullet_len
            } else {
                offset
            }
        };

        let display_cursor = to_display(cursor);
        let cursor_pos = line.x_for_index(display_cursor);

        let (selection, cursor_quad) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        gpui::size(px(CURSOR_WIDTH), bounds.bottom() - bounds.top()),
                    ),
                    rgb(BORDER_FOCUS),
                )),
            )
        } else {
            let sel_start = to_display(selected_range.start);
            let sel_end = to_display(selected_range.end);
            (
                Some(fill(
                    Bounds::from_corners(
                        point(bounds.left() + line.x_for_index(sel_start), bounds.top()),
                        point(bounds.left() + line.x_for_index(sel_end), bounds.bottom()),
                    ),
                    rgba(SELECTION),
                )),
                None,
            )
        };

        PrepaintState {
            line: Some(line),
            cursor: cursor_quad,
            selection,
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
        let focus_handle = self.input.read(context).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            context,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().unwrap();
        line.paint(
            bounds.origin,
            window.line_height(),
            TextAlign::Left,
            None,
            window,
            context,
        )
        .unwrap();
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.input.update(context, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.disabled;
        let focused = !disabled && self.focus_handle.is_focused(window);
        let border = if focused { BORDER_FOCUS } else { BORDER };
        let text_color = if disabled { TEXT_DIM } else { TEXT_PRIMARY };

        div()
            .flex()
            .key_context("TextInput")
            .when(!disabled, |element| {
                element.track_focus(&self.focus_handle(context))
            })
            .when(!disabled, |element| element.cursor(CursorStyle::IBeam))
            .on_action(context.listener(Self::backspace))
            .on_action(context.listener(Self::delete))
            .on_action(context.listener(Self::left))
            .on_action(context.listener(Self::right))
            .on_action(context.listener(Self::select_left))
            .on_action(context.listener(Self::select_right))
            .on_action(context.listener(Self::select_all))
            .on_action(context.listener(Self::home))
            .on_action(context.listener(Self::end))
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
            .text_size(px(TEXT_SIZE_MEDIUM))
            .line_height(px(LINE_HEIGHT_MEDIUM))
            .child(
                div()
                    .h(px(ELEMENT_HEIGHT))
                    .w_full()
                    .px(px(PADDING_INPUT_HORIZONTAL))
                    .py(px(PADDING_INPUT_VERTICAL))
                    .bg(rgb(INPUT_BACKGROUND))
                    .border_1()
                    .border_color(rgb(border))
                    .rounded(px(RADIUS))
                    .child(TextElement {
                        input: context.entity().clone(),
                    }),
            )
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
