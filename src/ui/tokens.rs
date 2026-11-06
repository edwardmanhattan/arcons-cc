//! The wall's palette and metrics — **the only file allowed to name a colour or a size.**
//!
//! Two reasons this is one file rather than literals at the call sites:
//!
//! - `docs/verification.md` rule 3: assert against tokens, not literals. A capture assertion that
//!   says `#22D3EE` outlives the palette and then lies quietly; `tokens::accent()` does not.
//! - A wall display is read from across a room, at night. Contrast and size are a decision made
//!   once, not per element.
//!
//! `tests/scaffold.rs` fails if a colour is constructed anywhere else in `src/`.

use rune::Color;
use rune::oklch;
use rune::oklcha;
use rune::tok::TextStyle;

// ---------------------------------------------------------------------------
// Metrics, in points.
// ---------------------------------------------------------------------------

/// The default window: 1280×720 is a capture the tests can reason about, and it fits a 1080p wall.
pub const WINDOW_SIZE: (f32, f32) = (1280.0, 720.0);

/// Below this the picture stops being readable at wall distance, so the window refuses to shrink.
pub const MIN_WINDOW: (f32, f32) = (640.0, 360.0);

pub const PAD: f32 = 16.0;
pub const GAP: f32 = 8.0;
pub const HAIRLINE: f32 = 1.0;
pub const PANEL_RADIUS: f32 = 10.0;

/// A marker glyph's diameter, in points.
pub const MARKER: f32 = 12.0;
/// A marker's hit target: at least 24 pt, because a touch target is a decision about fingers.
pub const MARKER_TARGET: f32 = 24.0;
/// The little tick that shows a measured heading.
pub const HEADING_TICK: f32 = 3.0;

// ---------------------------------------------------------------------------
// Type.
// ---------------------------------------------------------------------------

/// The wordmark: letterspaced, uppercase, quiet.
pub const WORDMARK: TextStyle = TextStyle::new(13.0, 600.0);
/// The one heading the placeholder panel has.
pub const TITLE: TextStyle = TextStyle::new(15.0, 600.0);
/// Everything that is not a heading or a wordmark.
pub const LABEL: TextStyle = TextStyle::new(12.0, 400.0);

// ---------------------------------------------------------------------------
// Colour. OKLCH, like Rune's own tokens: perceptually even, and the wall is a dark room.
// ---------------------------------------------------------------------------

/// The window background: near-black, one step of blue so it reads as a console, not as an absence.
pub fn bg() -> Color {
    oklch(0.145, 0.012, 250.0)
}

/// A panel sitting on the background.
pub fn panel() -> Color {
    oklch(0.19, 0.014, 250.0)
}

/// A hairline: white at low alpha, which is what a border on dark actually is.
pub fn hairline() -> Color {
    oklcha(1.0, 0.0, 0.0, 0.06)
}

/// Primary text.
pub fn ink() -> Color {
    oklch(0.94, 0.004, 250.0)
}

/// Secondary text: labels, units, hints.
pub fn ink_dim() -> Color {
    oklch(0.66, 0.008, 250.0)
}

/// The one accent: the wordmark today, and the "live" glyph (#6) once there is one. Never
/// decoration — if it is on screen it means "this is happening now".
pub fn accent() -> Color {
    oklch(0.78, 0.11, 220.0)
}
