//! **The shared vocabulary** between the wall and the wire.
//!
//! A leaf: it may use `std` and `map::geometry::Camera`, and nothing else. `ui` and `control` both name
//! these types, so neither has to name the other. The wall turns an [`Edge`] into its own drop; a wire
//! parser turns a word into an [`Intent`]. Neither conversion belongs here.

use crate::map::geometry::Camera;

/// What a pane is called. Small numbers, unique for the life of the layout.
pub type ViewId = u32;

/// **What a pane is** (#116): a map, or a chain of command.
///
/// A pane used to *be* a map — a camera, a map thread, a frame — and this ticket's tree has none of
/// those. The kind is a value on the pane rather than a second pane type, because layout, grip, name,
/// chrome and filter are the pane's: a tree pane is a pane, and every one of those keeps working.
///
/// The distinction is load-bearing for `INV-04`: a tree pane has **no frame**, so there is nothing for
/// a marker to be projected against, and it draws nothing that projects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PaneContent {
    /// A camera, a map thread, a frame, and everything that projects.
    #[default]
    Map,
    /// The chain of command. `organisation` names the structure the pane is about; `None` is **every
    /// structure the wall knows**, which is the chain of command as a whole — the supreme command above
    /// each branch root, because `/tni-commander` is one post shared by every branch.
    Tree { organisation: Option<i64> },
    /// **The newsroom** (#136): the strategic-environment feed, one page of stories at a time. No
    /// camera, no thread, no frame — the pane holds a [`newsroom::View`] and draws rows.
    Newsroom,
}

impl PaneContent {
    /// Whether this pane draws a map. The one question every caller that used to take a pane for
    /// granted asks.
    pub fn is_map(&self) -> bool {
        matches!(self, PaneContent::Map)
    }
}

/// **What a rename sheet is renaming** (#139): a pane, or a workspace.
///
/// The sheet is one sheet — one field, one limit, one Enter/Escape contract — and this is the only thing
/// that differs between naming a pane and naming the workspace on screen. Keeping it as a value rather
/// than a second sheet is what stops the two renaming paths from drifting apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameTarget {
    Pane(ViewId),
    Workspace(usize),
}

/// **Whose pointer a cursor is** (#205).
///
/// A pointer is a hand, and a fleet has more than one: the wall's own, a CLI's, a phone's. Each cursor is keyed by
/// this, so one client moving its pointer cannot move another's — and `ADR-0006` decision 5 is the reason the wall
/// draws every one of them: a press must be visible where it lands.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin(String);

impl Origin {
    /// The wall's own pointer — the OS's, for the hand on the glass.
    pub fn wall() -> Origin {
        Origin("wall".to_string())
    }

    /// **The client with no identity of its own**: a `ctl` over the local socket. One client at a time and its
    /// credential is the socket's file mode (`INV-08`), so "ctl" is the honest name for it.
    pub fn ctl() -> Origin {
        Origin("ctl".to_string())
    }

    /// A named client — a paired phone's device name, once #194 lands. Trimmed, and nothing else: a client's name is
    /// its own, not a key this wall gets to respell.
    pub fn named(name: &str) -> Origin {
        Origin(name.trim().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Origin {
    fn default() -> Self {
        Origin::ctl()
    }
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// **Which slot of a pane a drop names** (#219): an edge splits the target and puts the dropped pane on that
/// side, and `Centre` swaps the two.
///
/// It is the pointer's `view::Drop` with the geometry taken out — a client that has read the wall's `tiling`
/// knows which panes are where and which one is beside which, and a phone has no frame to measure a point
/// against. So the settled result of a drag is said in panes and edges, and the wall turns it back into the drop the pointer's release applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    /// The one edge that is not an edge: the two panes trade places.
    Centre,
}

/// Every edge, so a table over them is exhaustive rather than representative — the rule `RefusalKind::of_word`
/// keeps, for the same reason: a word this wall does not use is `None`, never a default (`INV-07`).
pub const EDGES: &[Edge] = &[
    Edge::Left,
    Edge::Right,
    Edge::Top,
    Edge::Bottom,
    Edge::Centre,
];

impl Edge {
    /// The word on the wire and in `docs/control.md`.
    pub fn word(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Top => "top",
            Edge::Bottom => "bottom",
            Edge::Centre => "centre",
        }
    }

    /// The edge a word names, or `None` for a word this wall does not use (`INV-07`: no default).
    pub fn of_word(word: &str) -> Option<Edge> {
        EDGES.iter().copied().find(|edge| edge.word() == word)
    }
}

/// **The one verb a device with no row may send** (#262): `pair.claim`, named once beside the vocabulary it
/// belongs to, so the relay's gate cannot drift from [`Intent::verb`]'s spelling.
pub const PAIR_CLAIM: &str = "pair.claim";

/// **One settled change**, asked for by a hand or a socket.
///
/// Every variant is a value a caller can produce without knowing anything about the wall's internals — which is
/// what makes the same vocabulary serve the pointer, the CLI and, later, a phone.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    // ---- panes
    /// Give this pane the keys (`State::focus_pane`).
    FocusPane(ViewId),
    /// Draw this pane over the wall, or put it back if it is already there (#85, #111).
    ToggleFullSize(ViewId),
    /// Remove this pane — refused on the wall's last one (#141).
    ClosePane(ViewId),
    /// Open the rename sheet on this pane or workspace (#83, #139, #154), without switching to it.
    BeginRename(RenameTarget),
    /// Set a name now, with no sheet: a socket's rename is a name, not nine keystrokes.
    /// `None` clears a pane's name (it falls back to its number); a workspace falls back to `Workspace N`.
    Rename {
        target: RenameTarget,
        name: Option<String>,
    },
    /// Split this pane, the new one showing the same water: the header's tree and newsroom controls (#116,
    /// #136) and a CLI's `pane split`.
    SplitPane { pane: ViewId, content: PaneContent },
    /// Split this pane **at a point in its own frame** — `Focus on new pane` (#77), which shows the water the
    /// operator clicked rather than the water the old pane was looking at.
    SplitAt { pane: ViewId, at: (f64, f64) },
    /// The same pane on a workspace of its own (#140).
    SplitToWorkspace(ViewId),
    /// Carry this pane into another workspace, threads and all (#140).
    MovePane { pane: ViewId, workspace: usize },
    /// **Land this pane in that pane's slot** (#219): the settled result of a drag, said in panes and edges
    /// instead of in a point a client cannot see. `edge` is a side — the target splits and this pane takes it
    /// (`view::Drop::Retile`) — or `centre`, which swaps the two (`view::Drop::Swap`).
    ///
    /// Exactly `Layout::apply` through `State::apply_drop`, the call the pointer's release makes, so the preview
    /// a client draws and the wall's landing are one function. Both panes are in the **active** workspace: a
    /// crossing is `pane.move` and then this, two settled changes.
    Drop {
        pane: ViewId,
        onto: ViewId,
        edge: Edge,
    },
    /// **Move the gutter between two neighbours** (#219): the settled result of a border drag. The pair names the
    /// border (`Layout::borders`), and `ratio` is the share its first side takes — the number `tiling` and
    /// `layout.toml` already carry — clamped to [`crate::config::layout::RATIO_RANGE`].
    Resize {
        pane: ViewId,
        other: ViewId,
        ratio: f32,
    },
    /// **Close every pane in the active workspace but the focused one** (#166). Never a workspace the client
    /// cannot see: a wall-wide reset is a weapon.
    CloseOtherPanes,
    /// **Close every pane in the active workspace, and leave it empty on the chips** (#241). The one refusal is
    /// #141's: a workspace that holds every pane the wall has cannot be emptied, because the wall always keeps one
    /// pane — the request is refused **whole** (`the last pane on the wall`), never half-applied.
    ///
    /// It is not `CloseOtherPanes` said twice: that verb keeps the pane with the keys and the workspace it is in, and
    /// this one gives up both. A client that could not tell them apart could not say which reset it asked for, and an
    /// **empty workspace** is a state the operator may ask for deliberately — it is drawn (`No panes yet.`) and it is
    /// on the projection (`empty: true`, #141), which is what lets a phone say "empty this one" in one request
    /// instead of N closes it computed from a projection another client may have changed under it
    /// (`docs/mobile-remote.md` decision 1).
    CloseAllPanes,
    /// **Close every workspace but the one on screen** (#166). The active workspace is never closed and never
    /// emptied, and every dropped pane joins its map thread.
    CloseOtherWorkspaces,
    /// **Close one empty workspace** (#242). The workspace is named by its index, the same address every other
    /// workspace intent uses. It refuses a workspace that still holds panes (`the workspace holds panes`) — an
    /// arrangement is not reconstructible from the wire, and emptying is #241's separate, visible change — and the
    /// wall's last workspace (`the last workspace on the wall`), the rule `pane.close` keeps one level down.
    ///
    /// **The wall lands where a pane close lands**: closing the active workspace lands on the nearest previous
    /// workspace that still holds a pane, or on the previous index when every remaining workspace is empty.
    CloseWorkspace(usize),
    /// Open a pane of this kind beside the focused one (the header's controls, a CLI's `pane open`).
    OpenPane(PaneContent),
    /// **Lock this pane's camera onto a hull** (#169), or stop with `unit: None`. `zoom` sets it once; without one,
    /// the pane keeps the zoom it had.
    Follow {
        pane: ViewId,
        unit: Option<i64>,
        zoom: Option<f64>,
    },
    /// Put this pane's selection on one hull, or clear it with `None` (#99).
    Select { pane: ViewId, unit: Option<i64> },

    // ---- workspaces
    /// Make this workspace the wall on screen (#138).
    FocusWorkspace(usize),
    /// Append an empty workspace and switch to it (#138).
    NewWorkspace,

    // ---- map
    /// Put this pane's camera here — a **value**, so a pan is one intent when the hand stops, not sixty.
    Camera { pane: ViewId, camera: Camera },

    // ---- a menu's keyboard model (#164)
    /// Put the ring on a row of the surface that is open — the menu, or the waterfall — or take it off with `None`.
    /// The row is clamped to the rows that exist.
    MenuFocus { row: Option<usize> },
    /// Open the ringed row's second surface, or close the one that is open.
    MenuSubmenu { open: bool },
    /// Act on the ringed row: a row that opens a surface opens it; a leaf runs its action.
    MenuChoose,

    // ---- the client's cursor (#167)
    /// Put a client's cursor at a point in the **picture panel's** points, and start its fade from now.
    CursorMove {
        /// **Whose pointer** (#205): absent is the local client's, which is what a `ctl` is.
        origin: Option<Origin>,
        at: (f32, f32),
    },
    /// Press where the cursor is: the hull under it (its card), or the pane it is over (the keys, and a cleared
    /// card).
    CursorPress { origin: Option<Origin> },
    /// Release. The cursor starts fading from here.
    CursorRelease { origin: Option<Origin> },

    // ---- pairing (#194)
    /// **Open pairing**: draw six digits on the wall for two minutes (`ui::State::pairing`). The code is a
    /// picture for the room and is **never** in the answer — a client that could read its own pairing code
    /// could pair itself.
    PairBegin,
    /// **A phone claims the code**: `devices.toml` gains the SHA-256 of a fresh 32-byte key, the name and when,
    /// and the key itself travels back through the carrier, once.
    PairClaim {
        /// The six digits drawn on the wall, as a phone types them.
        code: String,
        /// The name the phone gives itself: what `pair.forget` removes and what a cursor's `origin` says.
        name: String,
    },
    /// **Revoke a device**: remove its row from `devices.toml` by name. A name that is not there changes
    /// nothing — a revocation is a deletion, not a rule to refuse.
    PairForget { name: String },

    // ---- surfaces
    /// The settings sheet (#82).
    OpenSettings,
    /// One surface at a time, in the ladder `Escape` walks (#140): the waterfall, then the menu, then the
    /// interaction, then the maximised pane, then the newsroom's detail, then the selection.
    CloseSurfaces,
}

impl Intent {
    /// The verb this intent is on the wire, as `docs/control.md` names it (#165).
    ///
    /// One place, so a document, a CLI and a test cannot each spell it differently.
    pub fn verb(&self) -> &'static str {
        match self {
            Intent::FocusPane(_) => "pane.focus",
            Intent::ToggleFullSize(_) => "pane.full_size",
            Intent::ClosePane(_) => "pane.close",
            Intent::BeginRename(_) => "rename.begin",
            Intent::Rename { .. } => "rename.set",
            Intent::SplitPane { .. } => "pane.split",
            Intent::SplitAt { .. } => "pane.split_at",
            Intent::SplitToWorkspace(_) => "workspace.from_pane",
            Intent::MovePane { .. } => "pane.move",
            Intent::Drop { .. } => "pane.drop",
            Intent::Resize { .. } => "pane.resize",
            Intent::CloseOtherPanes => "pane.close_others",
            Intent::CloseAllPanes => "pane.close_all",
            Intent::CloseOtherWorkspaces => "workspace.close_others",
            Intent::CloseWorkspace(_) => "workspace.close",
            Intent::OpenPane(_) => "pane.open",
            Intent::Follow { .. } => "object.follow",
            Intent::Select { .. } => "pane.select",
            Intent::FocusWorkspace(_) => "workspace.focus",
            Intent::NewWorkspace => "workspace.new",
            Intent::Camera { .. } => "map.camera",
            Intent::MenuFocus { .. } => "menu.focus",
            Intent::MenuSubmenu { .. } => "menu.submenu",
            Intent::MenuChoose => "menu.choose",
            Intent::CursorMove { .. } => "cursor.move",
            Intent::CursorPress { .. } => "cursor.press",
            Intent::CursorRelease { .. } => "cursor.release",
            Intent::PairBegin => "pair.begin",
            Intent::PairClaim { .. } => PAIR_CLAIM,
            Intent::PairForget { .. } => "pair.forget",
            Intent::OpenSettings => "surface.settings",
            Intent::CloseSurfaces => "surface.close",
        }
    }
}

/// **One sample of every variant**, so a table over the vocabulary is exhaustive rather than representative.
///
/// Public because the tables that must not drift are outside this module: `docs/control.md`'s verb list, the
/// CLI's words and the wire codec are all checked against it, and a test-gated copy would be invisible to an
/// integration test. Adding a variant without adding it here leaves the vocabulary with a hole — which is the
/// point: a table nobody enumerates is a table that stops describing the enum (#170, #165).
pub fn vocabulary() -> Vec<Intent> {
    vec![
        Intent::FocusPane(1),
        Intent::ToggleFullSize(1),
        Intent::ClosePane(1),
        Intent::BeginRename(RenameTarget::Pane(1)),
        Intent::Rename {
            target: RenameTarget::Workspace(0),
            name: Some("Straits watch".to_string()),
        },
        Intent::SplitPane {
            pane: 1,
            content: PaneContent::Map,
        },
        Intent::SplitAt {
            pane: 1,
            at: (0.0, 0.0),
        },
        Intent::SplitToWorkspace(1),
        Intent::MovePane {
            pane: 1,
            workspace: 2,
        },
        Intent::Drop {
            pane: 1,
            onto: 2,
            edge: Edge::Top,
        },
        Intent::Resize {
            pane: 1,
            other: 2,
            ratio: 0.58,
        },
        Intent::CloseOtherPanes,
        Intent::CloseAllPanes,
        Intent::CloseOtherWorkspaces,
        Intent::CloseWorkspace(1),
        Intent::OpenPane(PaneContent::Newsroom),
        Intent::Follow {
            pane: 1,
            unit: Some(12),
            zoom: None,
        },
        Intent::Select {
            pane: 1,
            unit: None,
        },
        Intent::FocusWorkspace(0),
        Intent::NewWorkspace,
        Intent::Camera {
            pane: 1,
            camera: Camera::new((-6.1, 106.8), 10.0),
        },
        Intent::MenuFocus { row: Some(1) },
        Intent::MenuSubmenu { open: true },
        Intent::MenuChoose,
        Intent::CursorMove {
            at: (100.0, 80.0),
            origin: None,
        },
        Intent::CursorPress { origin: None },
        Intent::CursorRelease { origin: None },
        Intent::PairBegin,
        Intent::PairClaim {
            code: "481902".to_string(),
            name: "phone".to_string(),
        },
        Intent::PairForget {
            name: "phone".to_string(),
        },
        Intent::OpenSettings,
        Intent::CloseSurfaces,
    ]
}
