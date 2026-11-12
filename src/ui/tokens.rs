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

/// A hull's symbol, in points. **Twenty-four, and measured** (`docs/research/nato-symbology.md`): the
/// frame takes the outside of the box and the icon's reach inside it is 0.578 of the box for the
/// unknown-affiliation quatrefoil, so at the old 12 pt marker the icon was 6.9 pt across with a 0.86 pt
/// stroke — a dot with a halo, which is worse than the plain glyph it replaced because it costs four
/// more vertices to say the same "something is here". It is also the touch target, so nothing moves.
pub const SYMBOL: f32 = 24.0;
/// The frame's stroke as a fraction of the symbol's box, floored by the shape's own minimum. A literal
/// is chunky at one size and invisible at another; `0.072 × 24 = 1.73 pt`.
pub const SYMBOL_STROKE: f32 = 0.072;
/// A marker's hit target: at least 24 pt, because a touch target is a decision about fingers.
pub const MARKER_TARGET: f32 = 24.0;
/// A tactical object's mark: the box its artwork is drawn in, and the size of the generic diamond a
/// missing picture falls back to (#110). The same 24 pt as a hull's symbol — a buoy is read from the
/// same distance across the room, and it is not a hit target.
pub const OBJECT_MARK: f32 = 24.0;
/// An installation's mark (#109): the box its artwork — or the generic facility shape — is drawn in.
/// The same 24 pt as a hull and an object, because a base is read from the same distance across the room.
pub const INSTALLATION_MARK: f32 = 24.0;
/// The card's edge. Stronger than a panel hairline: it has to hold its shape over light water.
pub const CARD_BORDER: f32 = 1.0;

/// The gap between a card's rows: tighter than a panel's, because a card is read in one glance.
pub const CARD_GAP: f32 = 4.0;

/// The little tick that shows a measured heading.
pub const HEADING_TICK: f32 = 3.0;

/// The smallest a pane may be dragged to, in points. Below this a pane is a sliver with a hull in it,
/// and a tiling engine that lets one pane eat another is one the operator cannot get back out of.
pub const MIN_PANE_PT: f32 = 160.0;
/// The width of the border between two panes: the grip a drag starts on, and the width `rune-kit`'s
/// own `SplitView` uses, so the wall moves like the rest of Rune's shell.
pub const PANE_GRIP: f32 = 6.0;
/// The share of a pane, from each edge, that reads as "drop against this edge" rather than "drop on
/// this pane". A third: the middle of a pane is a big enough target to swap, and the edges are wide
/// enough to hit with a hand on a wall (#70).
pub const PANE_EDGE_BAND: f32 = 0.3;
/// A pane's handle: the mark you grab to move a pane, and its size in points. Small enough not to
/// cover the water, large enough to find without aiming.
pub const PANE_HANDLE: (f32, f32) = (28.0, 4.0);
/// A pane's chrome buttons: the filter and maximise toggles in its top-right corner (#111). A square the
/// size of a touch target — the same 24 pt rule as a marker (`MARKER_TARGET`) — because a control found
/// by aiming is a control an operator on a wall does not use.
pub const PANE_BUTTON: f32 = 24.0;
/// The filter panel (#112): a **set** of sizes, because the panel is a projection of a vocabulary the
/// backend owns and the vocabulary's length is not ours to decide. A width, one row's height, the head
/// above the first row, and the height past which the panel scrolls instead of growing over the water.
pub const FILTER_WIDTH: f32 = 216.0;
pub const FILTER_ROW: f32 = 26.0;
/// Everything in the panel that is not a row: the padding above and below, the title's line at
/// `TITLE`, and the gap between the title and the rows. Derived from the tokens it is made of, so a
/// padding change cannot silently start clipping the last layer row behind the scroll.
pub const FILTER_HEAD: f32 = PAD * 2.0 + 20.0 + GAP;
/// The most of the picture the panel may cover. Past this it scrolls inside itself, because a panel
/// that grew past the pane would hide the water it is filtering.
pub const FILTER_MAX_H: f32 = 320.0;
/// The indent of a category row under its domain: the vocabulary is flat on the wire and grouped here.
pub const FILTER_INDENT: f32 = 16.0;
/// The **minimum** height of the panel's scrolling part. It is zero on purpose: a scroll viewport inside
/// a flex column needs `min_h(0)` or it grows to its content and never scrolls. A named zero, because a
/// size belongs in this file (`tests/scaffold.rs`) even when the size is "no floor".
pub const FILTER_SCROLL_MIN_H: f32 = 0.0;

/// The filter panel's box for `rows` rows: its one width, and a height that stops at `FILTER_MAX_H`.
pub fn filter_size(rows: usize) -> (f32, f32) {
    (
        FILTER_WIDTH,
        (FILTER_HEAD + FILTER_ROW * rows as f32).min(FILTER_MAX_H),
    )
}
/// A pane menu's own size, in points. A context menu is a **surface with a known box** on this wall, not
/// an intrinsic one: the drag of a menu's corner is not a gesture the operator has, and a known box is
/// what lets the menu be clamped inside the pane it was asked on without measuring the text first (#77).
/// A pane's name: the plate's height, and the width after which the name is cut short — a cap, not a size,
/// so a long name never runs across the pane it names (#83).
pub const NAME_H: f32 = 22.0;
pub const NAME_MAX_W: f32 = 220.0;
/// How many characters a name may show. Past this it is truncated with an ellipsis that is drawn, rather
/// than clipped by the plate at whatever glyph happens to fit.
pub const NAME_MAX_CHARS: usize = 28;

/// The pane menu's width, and the height of **one row**. The surface is as tall as it has items (#81):
/// a menu `size()`d to one row clips the second one, which is the kind of wrong no golden notices until
/// someone opens it on a wall.
pub const MENU_WIDTH: f32 = 216.0;
/// The settings sheet: wide enough for a sentence and its switch, and as tall as the settings it has.
pub const SHEET_SIZE: (f32, f32) = (360.0, 132.0);
pub const MENU_ROW: f32 = 36.0;
/// The size of a menu with `rows` items, the only place a menu's height is computed.
pub fn menu_size(rows: usize) -> (f32, f32) {
    (MENU_WIDTH, MENU_ROW * (rows.max(1) as f32))
}
/// How much a lifted pane fades while it is being dragged: enough to read as "in the air", not so
/// much that the hulls inside it cannot be seen.
pub const PANE_LIFTED: f32 = 0.55;

/// A tree pane's node (#116): the plate a post is drawn on. Wide enough for a box's name and the post
/// that runs it at `LABEL`. Its **height is computed per post** — a plate is as tall as what it says,
/// which is what keeps a two-line post from wearing a four-line plate's empty space.
pub const TREE_NODE_W: f32 = 184.0;
/// One line of a post's plate. `LABEL` at 12 pt draws 20 pt tall, measured from the capture; a layout
/// that guessed 15 made every connector meet a plate in the wrong place.
pub const TREE_LINE: f32 = 20.0;
/// The space between two levels of the tree — where a connector's elbow runs — and between two
/// sibling subtrees. The row gap is also the elbow's own room: a connector that turned at the child's
/// edge would be a bracket, not a line.
pub const TREE_ROW_GAP: f32 = 24.0;
pub const TREE_COL_GAP: f32 = 16.0;
/// **The newsroom pane's metrics** (#136): the tab strip, one story's row, the footer, and the room the
/// pane's chrome takes at the top. A tab is a control found by a hand, so it is the same 24 pt floor as a
/// pane's buttons; a row is three lines at `LABEL`, measured from a capture rather than guessed.
pub const NEWS_TOP: f32 = 36.0;
pub const NEWS_PAD: f32 = 12.0;
pub const NEWS_TAB_H: f32 = 24.0;
pub const NEWS_ROW_H: f32 = 58.0;
pub const NEWS_ROW_GAP: f32 = 6.0;
pub const NEWS_FOOTER_H: f32 = 24.0;
/// The height floor of the list's scroll viewport. Zero on purpose, for the reason
/// [`FILTER_SCROLL_MIN_H`] gives: a scroll viewport in a flex column grows to its content without it.
pub const NEWS_SCROLL_MIN_H: f32 = 0.0;
/// How many characters of a headline a row shows. A pane name is 28 because a name is short by
/// construction; a headline is not, and a row is one line.
pub const NEWS_TITLE_MAX_CHARS: usize = 96;

/// The tree's own margin inside its pane, and the room the pane's chrome takes at the top: the tree
/// starts below the title plate and the two corner buttons rather than under them.
pub const TREE_PAD: f32 = 12.0;
pub const TREE_TOP: f32 = 40.0;
/// A connector's stroke, and the size of the icon a post or its level carries.
pub const TREE_LINK_W: f32 = 1.5;
pub const TREE_ICON: f32 = 14.0;

/// **The tree pane's zoom limits** (#127). They are not the map's: a map's zoom is a ground resolution
/// and a tree's is a text size, so the numbers come from the label instead of from metres per pixel.
///
/// The hand may zoom out to `TREE_ZOOM_MIN`; the automatic fit on open may go as low as this too, because
/// the operator's complaint is a narrow pane that *cuts* the forest. At the recorded structure's 784 pt
/// forest in a 320 pt pane the fit is ≈0.38, where a 12 pt label draws at ≈4.6 pt — small, and exactly why
/// the pane is a canvas: the shape is the overview and a hand zooms in for the names.
pub const TREE_ZOOM_MIN: f32 = 0.35;
pub const TREE_ZOOM_MAX: f32 = 3.0;
/// Below this zoom the secondary lines (`position_title`, the holder, the level) are dropped and only
/// names draw: at an overview scale, four lines a post is a grey block, and the shape is what is read.
pub const TREE_SECONDARY_MIN: f32 = 0.6;

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
/// The card's border: brighter than a hairline, still not a line you read rather than the content.
pub fn card_edge() -> Color {
    oklcha(0.5, 0.02, 250.0, 0.42)
}

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

/// The handle's ink: a neutral, a shade below the focus edge so the mark reads as something to grab
/// rather than as a border.
pub fn handle_ink() -> Color {
    oklch(0.46, 0.008, 250.0)
}

/// The gap a dragged pane will land in: the panel's own colour, one step up, so an empty slot reads as
/// a slot and not as a hole in the wall (#70).
pub fn gap_fill() -> Color {
    oklch(0.235, 0.02, 220.0)
}

/// No paint at all. A token, so that "this surface has no fill of its own" is a decision recorded beside
/// the colour it would otherwise have had — and so `tests/scaffold.rs` can keep its promise that nothing
/// outside this file names a colour.
pub fn clear() -> Color {
    Color::TRANSPARENT
}

/// What is behind a modal sheet: the wall, dimmed but still visible, because the operator is choosing a
/// preference *about* the picture and hiding it would hide what they are choosing for (#82).
pub fn scrim() -> Color {
    oklcha(0.05, 0.0, 250.0, 0.72)
}

/// A pane's name plate (#83): the panel's colour lifted a step, so a label reads as chrome sitting on the
/// picture rather than as something the map drew. Neutral, like the handle and the focus ring — the accent
/// means "this is happening now" and a name is not news.
pub fn name_plate() -> Color {
    oklch(0.24, 0.018, 250.0)
}

/// The focused pane's edge: a **neutral** line, not the accent (#68).
///
/// The accent means one thing on this wall — *this is happening now* — and a hull drawn in it must not
/// compete with a border for the eye. So the focused pane is marked the way a frame is marked in the
/// rest of the interface: a brighter hairline, no hue.
///
/// **Opaque on purpose.** A translucent ring renders as a blend over whatever is behind it, so an
/// assertion written against the token would compare a colour that is never on screen; this one is
/// exactly the pixels the wall draws.
pub fn focus_edge() -> Color {
    oklch(0.72, 0.008, 250.0)
}

/// **A facility's ink** (#109): a warm neutral, so a base reads as furniture on the water and never as
/// the accent that means "this is happening now". **Opaque on purpose**, for the reason [`focus_edge`]
/// records: an assertion written against a translucent token would compare a blend the wall never draws.
pub fn facility_ink() -> Color {
    oklch(0.74, 0.055, 82.0)
}

/// The connectors between posts in a chain of command (#116): a neutral line, a step above the
/// hairline so a run of them reads as a structure rather than as a border.
///
/// **Opaque on purpose**, for the reason [`focus_edge`] records: a translucent stroke is a blend, and
/// an assertion written against this token would then compare a colour the wall never draws.
pub fn tree_link() -> Color {
    oklch(0.5, 0.01, 250.0)
}
