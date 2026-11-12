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

/// **A row that opens a second surface** (#140): Rune's `chevron-right`, from the same catalogue and the
/// same pinned rev. The waterfall's trigger wears it, so "there is more behind this row" is a shape and
/// not only a hope.
pub static CHEVRON_RIGHT: Icon = Icon {
    name: "chevron-right",
    view: 20.0,
    body: r#"<path d="M8 15L13 10L8 5" stroke="currentColor" stroke-linecap="square"/>"#,
};

/// **A workspace**, from Rune's catalogue (`workspace`, #138): the glyph on each workspace chip.
///
/// Copied verbatim from the pinned rev like the pane icons, because the catalogue is a theme crate
/// rather than part of the `rune` library — a path invented here would be a picture nobody can name.
pub static WORKSPACE: Icon = Icon {
    name: "workspace",
    view: 16.0,
    body: r#"<path d="M2 10.668V14.0013H10.6667M13.9974 10.6667V2H2.66406M13.9974 10.668V14.0013H10.6641M2 10V2H5.33333" stroke="currentColor" stroke-miterlimit="10" stroke-linecap="square"/><path d="M10.6693 10.6654V5.33203H5.33594V10.6654H10.6693Z" fill="currentColor"/>"#,
};

/// **A new workspace** (`workspace-new`, #138): the `+` after the last chip.
pub static WORKSPACE_NEW: Icon = Icon {
    name: "workspace-new",
    view: 16.0,
    body: r#"<path d="M2 10.7578V14.0011H5.24324M13.9991 5.24324V2H10.7559M13.9991 10.7578V14.0011H10.7559M2 5.24324V2H5.24324" stroke="currentColor" stroke-miterlimit="10" stroke-linecap="square"/><path d="M8 4.5V11.5M4.5 8H11.5" stroke="currentColor" stroke-linejoin="round"/>"#,
};

/// A post that runs a branch: Rune's `branch`, from the same catalogue and the same pinned rev. A
/// chain of command draws it only when the wire names it — the recorded structure sends `""` for
/// every post, which is no icon and not a default one (#116).
pub static BRANCH: Icon = Icon {
    name: "branch",
    view: 16.0,
    body: r#"<path d="M5.118 5.686V10.314M5.118 5.686C5.97 5.686 6.661 4.995 6.661 4.143C6.661 3.291 5.97 2.6 5.118 2.6C4.266 2.6 3.575 3.291 3.575 4.143C3.575 4.995 4.266 5.686 5.118 5.686ZM5.118 10.314C4.266 10.314 3.575 11.005 3.575 11.857C3.575 12.709 4.266 13.4 5.118 13.4C5.97 13.4 6.661 12.709 6.661 11.857M5.118 10.314C5.97 10.314 6.661 11.005 6.661 11.857M10.882 5.686C11.734 5.686 12.425 4.995 12.425 4.143C12.425 3.291 11.734 2.6 10.882 2.6C10.03 2.6 9.339 3.291 9.339 4.143C9.339 4.995 10.03 5.686 10.882 5.686ZM10.882 5.686V9.457C10.882 10.783 9.807 11.857 8.482 11.857H6.661" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// A post that holds a command: Rune's `shield`.
pub static SHIELD: Icon = Icon {
    name: "shield",
    view: 20.0,
    body: r#"<path d="M7.49935 9.3737L9.16602 11.0404L12.4994 7.70703M9.99935 2.08203L17.0827 4.3737V9.92565C17.0827 14.0694 13.3327 16.2487 9.99935 18.047C6.66602 16.2487 2.91602 14.0694 2.91602 9.92565V4.3737L9.99935 2.08203Z" stroke="currentColor" stroke-linecap="square"/>"#,
};

/// The echelon mark: Rune's `chevron-down`, a level below the post above it. Drawn only when the post
/// states a level; a post with none is not a post at level zero (`INV-01`).
pub static ECHELON: Icon = Icon {
    name: "chevron-down",
    view: 16.0,
    body: r#"<path d="M5 6.5L8 9.5L11 6.5" stroke="currentColor"/>"#,
};

/// The catalogue icon a post's `icon` names, or `None` when the post carries none this wall can draw.
///
/// The wire's `icon` is *"a symbol name from a tactical palette, not an uploaded image"*, and the
/// recorded structure sends `""` for every post. Absence, an empty name and a name the catalogue does
/// not carry all draw **no icon**: the wall's old "no icon set" rule becomes "an icon comes from
/// Rune's catalogue", never a glyph invented here (`#116`).
pub fn post_icon(name: &str) -> Option<&'static Icon> {
    match name.trim() {
        "branch" => Some(&BRANCH),
        "shield" => Some(&SHIELD),
        _ => None,
    }
}
