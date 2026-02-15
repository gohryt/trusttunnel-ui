use gpui::{
    App, Div, Entity, FocusHandle, MouseButton, MouseUpEvent, Stateful, Window, div, prelude::*,
    px, rgb,
};

use crate::{text_input::TextInput, theme::*};

pub fn label(text: &str) -> Div {
    div()
        .px(px(PADDING_INPUT_HORIZONTAL))
        .text_size(px(TEXT_SIZE_SMALL))
        .text_color(rgb(TEXT_DIM))
        .child(text.to_string())
}

pub fn field(text: &str, input: &Entity<TextInput>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(GAP_EXTRA_SMALL))
        .w_full()
        .child(label(text))
        .child(input.clone())
}

pub fn credential_item(
    name: &str,
    active: bool,
    is_dragged: bool,
    disabled: bool,
    focus_handle: &FocusHandle,
) -> Div {
    let (background, text_color, border) = match (active, disabled) {
        (true, true) => (BORDER, TEXT_DIM, BORDER),
        (true, false) => (BUTTON_PRIMARY, TEXT_WHITE, BORDER_FOCUS),
        (false, _) => (
            INPUT_BACKGROUND,
            if disabled { TEXT_DIM } else { TEXT_PRIMARY },
            BORDER,
        ),
    };

    div()
        .track_focus(focus_handle)
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(px(ELEMENT_HEIGHT))
        .w_full()
        .px(px(PADDING_INPUT_HORIZONTAL))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(background))
        .text_color(rgb(text_color))
        .text_size(px(TEXT_SIZE_SMALL))
        .when(is_dragged, |element| element.opacity(0.5))
        .when(disabled, |element| element.cursor_default())
        .when(!disabled, |element| {
            element
                .cursor(if is_dragged {
                    gpui::CursorStyle::ClosedHand
                } else {
                    gpui::CursorStyle::OpenHand
                })
                .when(active, |element| {
                    element
                        .hover(|style| style.bg(rgb(BUTTON_DANGER_HOVER)))
                        .focus(|style| style.bg(rgb(BUTTON_DANGER_HOVER)))
                })
                .when(!active, |element| {
                    element
                        .hover(|style| style.border_color(rgb(BORDER_FOCUS)))
                        .focus(|style| style.border_color(rgb(BORDER_FOCUS)))
                })
        })
        .overflow_hidden()
        .child(name.to_string())
}

pub fn toggle(
    text: &str,
    value: bool,
    locked: bool,
    focus_handle: &FocusHandle,
    on_click: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let (background, border, dot_offset) = match (value, locked) {
        (true, true) => (BORDER, BORDER, px(TOGGLE_DOT_ON_OFFSET)),
        (true, false) => (BUTTON_PRIMARY, BORDER_FOCUS, px(TOGGLE_DOT_ON_OFFSET)),
        (false, _) => (INPUT_BACKGROUND, BORDER, px(TOGGLE_DOT_OFF_OFFSET)),
    };
    let dot_color = if locked { TEXT_DIM } else { TEXT_WHITE };

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .w_full()
        .child(label(text))
        .child(
            div()
                .track_focus(focus_handle)
                .flex()
                .items_center()
                .w(px(TOGGLE_WIDTH))
                .h(px(TOGGLE_HEIGHT))
                .rounded(px(TOGGLE_HEIGHT / 2.0))
                .bg(rgb(background))
                .border_1()
                .border_color(rgb(border))
                .when(!locked, |element| {
                    element
                        .cursor_pointer()
                        .when(value, |element| {
                            element
                                .hover(|style| style.bg(rgb(BUTTON_DANGER_HOVER)))
                                .focus(|style| style.bg(rgb(BUTTON_DANGER_HOVER)))
                        })
                        .when(!value, |element| {
                            element
                                .hover(|style| style.border_color(rgb(BORDER_FOCUS)))
                                .focus(|style| style.border_color(rgb(BORDER_FOCUS)))
                        })
                })
                .on_mouse_up(MouseButton::Left, move |event, window, context| {
                    if !locked {
                        on_click(event, window, context);
                    }
                })
                .child(
                    div()
                        .size(px(TOGGLE_DOT_SIZE))
                        .rounded(px(TOGGLE_DOT_SIZE / 2.0))
                        .bg(rgb(dot_color))
                        .ml(dot_offset),
                ),
        )
}

pub fn selector_option(text: &str, active: bool, locked: bool, focus_handle: &FocusHandle) -> Div {
    let (background, text_color, border) = match (active, locked) {
        (true, true) => (BORDER, TEXT_DIM, BORDER),
        (true, false) => (BUTTON_PRIMARY, TEXT_WHITE, BORDER_FOCUS),
        (false, _) => (INPUT_BACKGROUND, TEXT_DIM, BORDER),
    };

    div()
        .track_focus(focus_handle)
        .flex()
        .flex_1()
        .items_center()
        .px(px(PADDING_INPUT_HORIZONTAL))
        .h(px(ELEMENT_HEIGHT))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(background))
        .text_color(rgb(text_color))
        .text_size(px(TEXT_SIZE_SMALL))
        .when(!locked, |element| {
            element
                .cursor_pointer()
                .when(active, |element| {
                    element
                        .hover(|style| style.bg(rgb(BUTTON_DANGER_HOVER)))
                        .focus(|style| style.bg(rgb(BUTTON_DANGER_HOVER)))
                })
                .when(!active, |element| {
                    element
                        .hover(|style| style.border_color(rgb(BORDER_FOCUS)))
                        .focus(|style| style.border_color(rgb(BORDER_FOCUS)))
                })
        })
        .child(text.to_string())
}

pub fn selector(text: &str, options_row: Div) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(GAP_EXTRA_SMALL))
        .w_full()
        .child(label(text))
        .child(options_row)
}

pub fn selector_row() -> Div {
    div().flex().flex_row().w_full().gap(px(GAP_SMALL))
}

pub fn button_action(
    text: &str,
    background: u32,
    hover_background: u32,
    disabled: bool,
    focus_handle: &FocusHandle,
) -> Div {
    let (background_color, text_color) = if disabled {
        (BORDER, TEXT_DIM)
    } else {
        (background, TEXT_WHITE)
    };

    div()
        .track_focus(focus_handle)
        .flex()
        .items_center()
        .px(px(PADDING_INPUT_HORIZONTAL))
        .h(px(ELEMENT_HEIGHT))
        .w_full()
        .bg(rgb(background_color))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(gpui::transparent_black())
        .when(!disabled, |element| {
            element
                .cursor_pointer()
                .hover(move |style| style.bg(rgb(hover_background)))
                .focus(|style| {
                    style
                        .border_color(rgb(BORDER_FOCUS))
                        .bg(rgb(BUTTON_DANGER_HOVER))
                })
        })
        .text_color(rgb(text_color))
        .text_size(px(TEXT_SIZE_MEDIUM))
        .child(text.to_string())
}

pub fn button_ghost(text: &str, disabled: bool, focus_handle: &FocusHandle) -> Div {
    div()
        .track_focus(focus_handle)
        .flex()
        .flex_shrink_0()
        .items_center()
        .px(px(PADDING_INPUT_HORIZONTAL))
        .h(px(ELEMENT_HEIGHT))
        .w_full()
        .bg(rgb(INPUT_BACKGROUND))
        .border_1()
        .border_color(rgb(BORDER))
        .rounded(px(RADIUS))
        .when(!disabled, |element| element.cursor_pointer())
        .text_color(rgb(if disabled { TEXT_DIM } else { TEXT_PRIMARY }))
        .text_size(px(TEXT_SIZE_MEDIUM))
        .when(!disabled, |element| {
            element
                .hover(|style| style.border_color(rgb(BORDER_FOCUS)))
                .focus(|style| style.border_color(rgb(BORDER_FOCUS)))
        })
        .child(text.to_string())
}

pub fn status_dot(color: u32) -> Div {
    div()
        .size(px(STATUS_DOT_SIZE))
        .rounded(px(STATUS_DOT_SIZE / 2.0))
        .bg(rgb(color))
}

pub fn status_label(text: String, color: u32) -> Div {
    div()
        .text_size(px(TEXT_SIZE_MEDIUM))
        .text_color(rgb(color))
        .child(text)
}

pub fn status_detail(text: String) -> Div {
    div()
        .text_size(px(TEXT_SIZE_EXTRA_SMALL))
        .text_color(rgb(TEXT_DIM))
        .child(text)
}

pub fn titlebar_title(text: &str) -> Div {
    div()
        .flex()
        .flex_1()
        .h_full()
        .items_center()
        .pl(px(PADDING_COLUMN + PADDING_INPUT_HORIZONTAL))
        .text_size(px(TEXT_SIZE_SMALL))
        .text_color(rgb(TEXT_DIM))
        .child(text.to_string())
}

pub fn titlebar_close() -> Stateful<Div> {
    div()
        .id("titlebar-close")
        .flex()
        .items_center()
        .px(px(PADDING_COLUMN + PADDING_INPUT_HORIZONTAL))
        .h(px(TITLEBAR_HEIGHT))
        .text_size(px(TEXT_SIZE_SMALL))
        .text_color(rgb(TEXT_DIM))
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(rgb(BUTTON_DANGER_HOVER))
                .text_color(rgb(TEXT_WHITE))
        })
        .child("Exit")
}

pub fn log_container() -> Stateful<Div> {
    div()
        .id("log-scroll")
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .rounded(px(RADIUS))
        .bg(rgb(LOG_BACKGROUND))
        .border_1()
        .border_color(rgb(BORDER))
        .p(px(PADDING_LOG))
        .overflow_scroll()
}
