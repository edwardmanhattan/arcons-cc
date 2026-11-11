//! The icons a pane's chrome draws, taken from Rune's catalogue (#111).
//!
//! Rune has an icon set — `rune::Icon` and `El::icon` — and a glyph is only a button when a reader can
//! recognise it. These three bodies are copied verbatim from Rune's catalogue at the pinned rev
//! (`themes/opencode/src/icons.rs`, generated from `@opencode/ui`), because the catalogue is a theme
//! crate rather than part of the `rune` library this wall depends on. Copying the geometry keeps the
//! shape Rune draws; the meaning is also in words where there is room (the filter panel's title, and the
//! buttons' help text), which is the rule `AGENTS.md` states.
//!
//! **Not** a hand-drawn icon: a path invented here would be a picture nobody else can name, and the
//! ticket's whole point is that the operator can tell the filter from the maximise.

use rune::Icon;

/// The filter toggle: three sliders, Rune's `outline-sliders`.
pub static FILTER: Icon = Icon {
    name: "outline-sliders",
    view: 16.0,
    body: r#"<path d="M11.7779 4.66675H14.4446M11.7779 4.66675C11.7779 5.77132 10.8825 6.66675 9.77789 6.66675C8.67332 6.66675 7.77789 5.77132 7.77789 4.66675M11.7779 4.66675C11.7779 3.56218 10.8825 2.66675 9.77789 2.66675C8.67332 2.66675 7.77789 3.56218 7.77789 4.66675M1.55566 4.66675H7.77789M4.22233 11.3334H1.55566M4.22233 11.3334C4.22233 12.438 5.11776 13.3334 6.22233 13.3334C7.3269 13.3334 8.22233 12.438 8.22233 11.3334M4.22233 11.3334C4.22233 10.2288 5.11776 9.33341 6.22233 9.33341C7.3269 9.33341 8.22233 10.2288 8.22233 11.3334M14.4446 11.3334H8.22233" stroke="currentColor"/>"#,
};

/// The maximise toggle: Rune's `outline-expand`, arrows to the corners.
pub static MAXIMISE: Icon = Icon {
    name: "outline-expand",
    view: 24.0,
    body: r#"<g transform="scale(1.2)" stroke="currentColor" stroke-width="1.25"><path d="M4.58301 10.4163V15.4163H9.58301M10.4163 4.58301H15.4163V9.58301" stroke="currentColor" stroke-linecap="square"/></g>"#,
};

/// The same toggle once the pane is full size: Rune's `collapse`, arrows back towards the tiling. The
/// icon says which way the press will go, which is the difference between a toggle and a trap.
pub static RESTORE: Icon = Icon {
    name: "collapse",
    view: 16.0,
    body: r#"<path d="M8 1V6M11 3L8 6L5 3" stroke="currentColor"/><path d="M8 15V10M11 13L8 10L5 13" stroke="currentColor"/><path d="M4 8H6" stroke="currentColor"/><path d="M7 8H9" stroke="currentColor"/><path d="M10 8H12" stroke="currentColor"/>"#,
};
