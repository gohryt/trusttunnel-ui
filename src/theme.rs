// Factory.ai-inspired light theme
//
// Element design follows 90s utilitarian spirit with modern usability:
// — Outlined inputs (no fill, just borders on the surface)
// — Selected states use subtle darkened bg + stronger borders, not accent fills
// — Dark filled button for primary CTA, accent reserved for focus/toggles/badges
// — Minimal, flat ghost buttons with no background fill

// Backgrounds — two-tier: #eeeeee (base) and #fafafa (elevated surface)
pub const SURFACE: u32 = 0xeeeeee;
pub const TITLEBAR_BACKGROUND: u32 = 0xfafafa;
pub const INPUT_BACKGROUND: u32 = 0xeeeeee; // matches SURFACE — outlined-only inputs

// Borders
pub const BORDER: u32 = 0xccc9c7; // neutral-200, default border and disabled bg
pub const BORDER_STRONG: u32 = 0x4d4947; // neutral-700, active/selected item borders
pub const BORDER_FOCUS: u32 = 0xef6f2e; // orange accent, focus rings only

// Text
pub const TEXT_PRIMARY: u32 = 0x020202;
pub const TEXT_DIM: u32 = 0x5c5855; // neutral-600, labels and secondary text
pub const TEXT_WHITE: u32 = 0xffffff; // on colored/dark surfaces
pub const INPUT_PLACEHOLDER: u32 = 0xa49d9a66; // neutral-400 + alpha
pub const LOG_TEXT: u32 = 0x4d4947; // neutral-700
pub const LOG_PLACEHOLDER: u32 = 0xb8b3b0; // neutral-300

// Active/selected state — subtle darkened bg like factory.ai tabs
pub const ACTIVE_BACKGROUND: u32 = 0xd6d3d2; // neutral-100
pub const ACTIVE_HOVER: u32 = 0xccc9c7; // neutral-200, hover on active

// Primary filled button — dark, like factory.ai LOG IN
pub const BUTTON_FILLED: u32 = 0x020202;
pub const BUTTON_FILLED_HOVER: u32 = 0x4d4947; // neutral-700, clearly visible lightening

// Accent — reserved for toggles, focus borders, badges
pub const BUTTON_PRIMARY: u32 = 0xef6f2e;
pub const BUTTON_HOVER: u32 = 0xd15010; // visible darkening from #ef6f2e

// Danger
pub const BUTTON_DANGER: u32 = 0xd93050;
pub const BUTTON_DANGER_HOVER: u32 = 0xb8283e;

// Status indicators — strong colors readable on light backgrounds
pub const COLOR_RED: u32 = 0xd93050;
pub const COLOR_YELLOW: u32 = 0xc47a10;

// Selection highlight
pub const SELECTION: u32 = 0xef6f2e40;

// Typography
pub const TEXT_SIZE_MEDIUM: f32 = 13.0;
pub const TEXT_SIZE_SMALL: f32 = 12.0;
pub const TEXT_SIZE_EXTRA_SMALL: f32 = 10.0;

pub const LINE_HEIGHT_MEDIUM: f32 = 18.0;
pub const LINE_HEIGHT_EXTRA_SMALL: f32 = 14.0;

// Element sizing
pub const ELEMENT_HEIGHT: f32 = 32.0;
pub const TITLEBAR_HEIGHT: f32 = 32.0;
pub const TEXTAREA_HEIGHT: f32 = 80.0;

// Radius
pub const RADIUS: f32 = 4.0;
pub const CURSOR_WIDTH: f32 = 2.0;

// Spacing
pub const GAP_EXTRA_SMALL: f32 = 4.0;
pub const GAP_SMALL: f32 = 8.0;
pub const GAP_MEDIUM: f32 = 12.0;

// Padding
pub const PADDING_INPUT_HORIZONTAL: f32 = 10.0;
pub const PADDING_INPUT_VERTICAL: f32 = 6.0;

pub const PADDING_COLUMN: f32 = 20.0;
pub const PADDING_COLUMN_TOP: f32 = 8.0;
pub const PADDING_LOG: f32 = 8.0;

// Layout
pub const WINDOW_WIDTH: f32 = 960.0;
pub const WINDOW_HEIGHT: f32 = 640.0;
pub const LEFT_COLUMN_WIDTH: f32 = 380.0;
pub const CREDENTIAL_BUTTON_WIDTH: f32 = 80.0;

// Toggle
pub const TOGGLE_WIDTH: f32 = 34.0;
pub const TOGGLE_HEIGHT: f32 = 18.0;
pub const TOGGLE_DOT_SIZE: f32 = 14.0;
pub const TOGGLE_DOT_ON_OFFSET: f32 = 18.0;
pub const TOGGLE_DOT_OFF_OFFSET: f32 = 2.0;
