use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, Context, CursorStyle, ElementId, Entity,
    FocusHandle, Focusable, GlobalElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Size, Style, TextAlign,
    TextRun, Window, WrappedLine, div, fill, point, prelude::*, px, rgb, rgba,
};

use crate::{
    text_input::{Copy, SelectAll},
    theme::*,
};

pub struct LogPanel {
    focus_handle: FocusHandle,
    content: String,
    line_offsets: Vec<usize>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    is_selecting: bool,
    last_layouts: Vec<WrappedLine>,
    last_origin: Point<Pixels>,
    last_line_height: Pixels,
}

impl LogPanel {
    pub fn new(context: &mut Context<Self>) -> Self {
        Self {
            focus_handle: context.focus_handle(),
            content: String::new(),
            line_offsets: vec![0],
            selected_range: 0..0,
            selection_reversed: false,
            is_selecting: false,
            last_layouts: Vec::new(),
            last_origin: point(px(0.), px(0.)),
            last_line_height: px(LINE_HEIGHT_EXTRA_SMALL),
        }
    }

    pub fn set_lines(&mut self, lines: &[String]) -> bool {
        let new_content = lines.join("\n");
        if new_content == self.content {
            return false;
        }
        self.content = new_content;
        self.line_offsets.clear();
        let mut offset = 0;
        for (line_index, line) in lines.iter().enumerate() {
            self.line_offsets.push(offset);
            offset += line.len();
            if line_index + 1 < lines.len() {
                offset += 1;
            }
        }
        if self.line_offsets.is_empty() {
            self.line_offsets.push(0);
        }
        self.selected_range = 0..0;
        true
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() || self.last_layouts.is_empty() {
            return 0;
        }

        let origin = self.last_origin;
        let line_height = self.last_line_height;
        let layout_count = self.last_layouts.len();

        let mut y = origin.y;
        for (line_index, line) in self.last_layouts.iter().enumerate() {
            if line_index >= self.line_offsets.len() {
                break;
            }

            let visual_rows = line.wrap_boundaries().len() + 1;
            let total_height = line_height * visual_rows as f32;

            if position.y < y + total_height || line_index == layout_count - 1 {
                let line_start = self.line_offsets[line_index];
                let line_text_end = if line_index + 1 < self.line_offsets.len() {
                    self.line_offsets[line_index + 1] - 1
                } else {
                    self.content.len()
                };
                let line_len = line_text_end - line_start;

                let relative = point(position.x - origin.x, position.y - y);
                let local_index = match line.closest_index_for_position(relative, line_height) {
                    Ok(index) | Err(index) => index,
                };

                return line_start + local_index.min(line_len);
            }

            y += total_height;
        }

        self.content.len()
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
        if self.selected_range.start > self.selected_range.end {
            self.selected_range = self.selected_range.end..self.selected_range.start;
            self.selection_reversed = !self.selection_reversed;
        }
        context.notify();
    }

    fn on_select_all(&mut self, _: &SelectAll, _: &mut Window, context: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        context.notify();
    }

    fn on_copy(&mut self, _: &Copy, _: &mut Window, context: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            context.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
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
}

struct LayoutData {
    lines: Vec<WrappedLine>,
    line_height: Pixels,
}

struct LogPanelElement {
    panel: Entity<LogPanel>,
}

struct LogPanelPrepaintState {
    selections: Vec<PaintQuad>,
}

impl IntoElement for LogPanelElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for LogPanelElement {
    type RequestLayoutState = Rc<RefCell<Option<LayoutData>>>;
    type PrepaintState = LogPanelPrepaintState;

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
        let shared: Rc<RefCell<Option<LayoutData>>> = Rc::new(RefCell::new(None));
        let panel = self.panel.read(context);
        let content: SharedString = panel.content.clone().into();
        let content_len = content.len();

        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        let run = TextRun {
            len: content_len,
            font: text_style.font(),
            color: text_style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let shared_clone = shared.clone();
        let layout_id = window.request_measured_layout(
            Style::default(),
            move |known_dimensions, available_space, window, _context| {
                let wrap_width = known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(x) => Some(x),
                    _ => None,
                });

                if content.is_empty() {
                    shared_clone.borrow_mut().replace(LayoutData {
                        lines: Vec::new(),
                        line_height,
                    });
                    return Size {
                        width: wrap_width.unwrap_or(px(0.)),
                        height: line_height,
                    };
                }

                let lines = window
                    .text_system()
                    .shape_text(
                        content.clone(),
                        font_size,
                        std::slice::from_ref(&run),
                        wrap_width,
                        None,
                    )
                    .unwrap_or_default();

                let mut size = Size::<Pixels>::default();
                for line in &lines {
                    let line_size = line.size(line_height);
                    size.height += line_size.height;
                    size.width = size.width.max(line_size.width);
                }
                if size.height == px(0.) {
                    size.height = line_height;
                }
                size.width = wrap_width.unwrap_or(size.width);

                shared_clone.borrow_mut().replace(LayoutData {
                    lines: lines.into_vec(),
                    line_height,
                });

                size
            },
        );

        (layout_id, shared)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        shared: &mut Self::RequestLayoutState,
        _window: &mut Window,
        context: &mut App,
    ) -> LogPanelPrepaintState {
        let data_ref = shared.borrow();
        let Some(ref data) = *data_ref else {
            return LogPanelPrepaintState {
                selections: Vec::new(),
            };
        };

        let panel = self.panel.read(context);
        let selected_range = panel.selected_range.clone();
        let line_offsets = &panel.line_offsets;
        let line_height = data.line_height;
        let num_lines = line_offsets.len();

        let mut selections = Vec::new();

        if !selected_range.is_empty() {
            let mut y = bounds.top();
            for (line_index, line) in data.lines.iter().enumerate() {
                if line_index >= num_lines {
                    break;
                }

                let line_start = line_offsets[line_index];
                let line_text_end = if line_index + 1 < num_lines {
                    line_offsets[line_index + 1] - 1
                } else {
                    panel.content.len()
                };
                let line_full_end = if line_index + 1 < num_lines {
                    line_offsets[line_index + 1]
                } else {
                    panel.content.len()
                };
                let line_text_len = line_text_end - line_start;
                let visual_rows = line.wrap_boundaries().len() + 1;
                let total_height = line_height * visual_rows as f32;

                if selected_range.start < line_full_end && selected_range.end > line_start {
                    let local_start = selected_range
                        .start
                        .saturating_sub(line_start)
                        .min(line_text_len);
                    let local_end = selected_range
                        .end
                        .saturating_sub(line_start)
                        .min(line_text_len);
                    let extends_past_end = selected_range.end > line_text_end;

                    let start_pos = line
                        .position_for_index(local_start, line_height)
                        .unwrap_or(point(px(0.), px(0.)));
                    let end_pos = line
                        .position_for_index(local_end, line_height)
                        .unwrap_or(point(px(0.), px(0.)));

                    let start_row = (start_pos.y / line_height) as usize;
                    let end_row = (end_pos.y / line_height) as usize;

                    if start_row == end_row {
                        let right = if extends_past_end && end_row == visual_rows - 1 {
                            bounds.right()
                        } else {
                            bounds.left() + end_pos.x
                        };
                        selections.push(fill(
                            Bounds::from_corners(
                                point(bounds.left() + start_pos.x, y + start_pos.y),
                                point(right, y + start_pos.y + line_height),
                            ),
                            rgba(SELECTION),
                        ));
                    } else {
                        selections.push(fill(
                            Bounds::from_corners(
                                point(bounds.left() + start_pos.x, y + start_pos.y),
                                point(bounds.right(), y + start_pos.y + line_height),
                            ),
                            rgba(SELECTION),
                        ));
                        for row in (start_row + 1)..end_row {
                            let row_y = y + line_height * row as f32;
                            selections.push(fill(
                                Bounds::from_corners(
                                    point(bounds.left(), row_y),
                                    point(bounds.right(), row_y + line_height),
                                ),
                                rgba(SELECTION),
                            ));
                        }
                        let right = if extends_past_end && end_row == visual_rows - 1 {
                            bounds.right()
                        } else {
                            bounds.left() + end_pos.x
                        };
                        selections.push(fill(
                            Bounds::from_corners(
                                point(bounds.left(), y + end_pos.y),
                                point(right, y + end_pos.y + line_height),
                            ),
                            rgba(SELECTION),
                        ));
                    }
                }

                y += total_height;
            }
        }

        LogPanelPrepaintState { selections }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        shared: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        context: &mut App,
    ) {
        let selections = std::mem::take(&mut prepaint.selections);
        for quad in selections {
            window.paint_quad(quad);
        }

        let mut data = shared.borrow_mut().take();
        if let Some(ref data) = data {
            let line_height = data.line_height;
            let mut y = bounds.top();
            for line in &data.lines {
                let origin = point(bounds.left(), y);
                if let Err(error) =
                    line.paint(origin, line_height, TextAlign::Left, None, window, context)
                {
                    log::warn!("[log_panel] failed to paint log line: {error}");
                }
                let visual_rows = line.wrap_boundaries().len() + 1;
                y += line_height * visual_rows as f32;
            }
        }

        if let Some(data) = data.take() {
            self.panel.update(context, |panel, _| {
                panel.last_layouts = data.lines;
                panel.last_origin = bounds.origin;
                panel.last_line_height = data.line_height;
            });
        }
    }
}

impl Render for LogPanel {
    fn render(&mut self, _window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("LogPanel")
            .track_focus(&self.focus_handle(context))
            .cursor(CursorStyle::IBeam)
            .on_action(context.listener(Self::on_select_all))
            .on_action(context.listener(Self::on_copy))
            .on_mouse_down(MouseButton::Left, context.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, context.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, context.listener(Self::on_mouse_up))
            .on_mouse_move(context.listener(Self::on_mouse_move))
            .text_color(rgb(LOG_TEXT))
            .text_size(px(TEXT_SIZE_EXTRA_SMALL))
            .line_height(px(LINE_HEIGHT_EXTRA_SMALL))
            .when(self.content.is_empty(), |element| {
                element.child(
                    div()
                        .text_color(rgb(LOG_PLACEHOLDER))
                        .child("No log output yet…"),
                )
            })
            .when(!self.content.is_empty(), |element| {
                element.child(LogPanelElement {
                    panel: context.entity().clone(),
                })
            })
    }
}

impl Focusable for LogPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
