//! **The projection** — what a client may read (#181, `ADR-0005` decision 7).
//!
//! Every intent answers "did it work". This is the other half: a small, versioned snapshot of the wall's
//! **control** state — the workspaces, the panes in them, which is on screen, which has the keys, what is full
//! size, what each pane is looking at.
//!
//! **What is deliberately not here**, because a client that reads it starts to be a renderer:
//!
//! - the fleet, the fixes, the ages — a client that knows where the hulls are has the picture;
//! - the cards, the story text, the chain of command — the wall's *content*, which is what it was built to show;
//! - the frames and the pixels — `INV-04` is a claim about this process's drawing surface.
//!
//! **Absence is not zero** in the projection too (`INV-01`): a pane that has no camera says `null` rather than
//! `0, 0, 0`; a workspace with no panes is `empty: true` and still in the list, because it is still on the wall
//! with its name (#141); a pane with no card says `selected: null`.
//!
//! The shape is filled by `ui::projection::of`, from `State`, in one function, so the projection cannot drift
//! from the wall without the wall having changed.

use serde::{Deserialize, Serialize};

use crate::config::layout::Axis;
use crate::verb::ViewId;

/// The wall, as a controller sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wall {
    /// **The projection's version** (#221): a counter that only goes up, one step per change to what a client reads.
    ///
    /// `Scene::seq`'s sibling (#201), and the reason a client can be told a wall moved without being told how: a
    /// number it did not expect means it missed one and should **read again**, never reconstruct. A publish the wall
    /// sent carries this number, so a client holding a projection can put the two in order.
    pub seq: u64,
    /// The workspaces, in chip order.
    pub workspaces: Vec<WorkspaceView>,
    /// Which workspace is on screen, as an index into `workspaces`.
    pub active: usize,
    /// The pane with the keys. `null` when the active workspace has no pane — which is a wall #141 allows.
    pub focused: Option<ViewId>,
    /// The pane drawn over the wall (#85), or `null`.
    pub maximised: Option<ViewId>,
}

/// One workspace: its name, the panes in it, and which of them the wall remembers.
///
/// A workspace is **never removed by closing its last pane** (#141): `empty` says it has no tiling, and `panes`
/// is then empty, but the name and the chip stay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceView {
    pub name: String,
    /// The pane this workspace would focus when it comes back on screen, or `null` when it holds none.
    pub focused: Option<ViewId>,
    /// Whether it holds no tiling at all — #141's legal, empty workspace.
    pub empty: bool,
    /// **The arrangement** (#218): how this workspace's panes tile, with **pane ids** for leaves. `null` for a
    /// workspace the wall cannot tile — an empty one, which a client draws as a row, exactly as the wall does.
    pub tiling: Option<Tiling>,
    pub panes: Vec<PaneView>,
}

/// **What a pane is** (#116, #136), in the words the wire uses: `map`, `tree`, `newsroom`.
///
/// An enum rather than a string, so a client that receives one it does not know is a client that can say so
/// (`INV-07`'s rule at the far end), and so this file cannot spell a fourth kind by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Map,
    Tree,
    Newsroom,
}

/// One pane: what it is, what it is called, and where it is looking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneView {
    pub id: ViewId,
    pub kind: Kind,
    /// What the name plate draws: the operator's name, or the pane's number.
    pub title: String,
    /// Where a map pane is looking. **`null` for a pane that projects nothing** — a tree or a newsroom has no
    /// camera, and an invented one would be a wall claiming to look somewhere it cannot.
    pub camera: Option<CameraView>,
    pub focused: bool,
    /// The hull whose card this pane is showing, or `null` (#99).
    pub selected: Option<i64>,
}

/// A camera, in the shape a `map.camera` intent takes — so a client can read one and send it back.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraView {
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
}

/// **The arrangement**, a workspace's tiling (#218).
///
/// The **same spelling** `layout.toml` and the `Scene` use — `split`, `ratio`, `a`, `b`, `pane` — so a reader that
/// parses one parses the other, with one deliberate difference: a leaf is a **pane id**, the integer every intent
/// names, and not an index into the workspace's pane list.
///
/// The difference is the whole type. A file is written and read whole, so an index is a shorthand; a client holds a
/// projection across changes, and after a `pane.close` every index past the gap names a different pane — a phone
/// would draw the right arrangement of the wrong panes. Indices never travel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Tiling {
    /// One pane, by the id every intent about a pane names.
    Leaf { pane: ViewId },
    /// Two nodes beside each other (`side`: `a` left, `b` right) or one above the other (`stack`: `a` on top).
    ///
    /// `ratio` is **`a`'s share of the split** and it is always present: a file may omit it and mean one half, a
    /// projection always says what it means (`INV-01`'s rule, where an absence would be a guess).
    Split {
        split: Axis,
        ratio: f32,
        a: Box<Tiling>,
        b: Box<Tiling>,
    },
}
