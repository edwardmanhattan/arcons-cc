//! The view: the header, the map, and the panel where the picture lands.
//!
//! The rules it already obeys:
//!
//! - **The UI thread never blocks.** `view` builds elements and returns. The map thread hands its
//!   frames over through [`rune::Pixels::set`], which is callable from any thread; the reads thread
//!   answers over a channel. Nothing here waits for either.
//! - **The map is behind the markers, not in them.** MapLibre draws water, coastlines and labels;
//!   units are overlay elements (#6) so they can be selected, carded and moved (#7).
//! - **A gesture moves the camera, not the picture.** The wheel zooms about the pointer and a drag
//!   moves the ground under it; the frame catches up on the next render, which is what makes a drag
//!   feel like dragging water rather than a photograph.
//! - **Every colour and size comes from [`tokens`].** Nothing in this file names one.
//! - **A pane is a rectangle from one function** ([`view::Layout::rects`]) and a drag on one is a
//!   preview of an operation ([`view::Layout::preview`]) the release applies: the gap a lifted pane
//!   shows is computed by the same code that moves it, so it cannot open where the pane will not land.
//!
//! Without `--features map` the map element is absent and the panel says so: the engine is optional
//! so the gate stays free of C++ and of a network. `--probe-map` is the headless witness.

pub mod filter;
pub mod icons;
pub mod installations;
pub mod markers;
pub mod newsroom;
pub mod objects;
pub mod saved;
pub mod snapshot;
pub mod symbol;
pub mod tokens;
pub mod tree;
pub mod unitcard;
pub mod view;
mod viewpoint;

pub use filter::Filter;
pub use view::{Dir, Layout, Rect, ViewId};

use std::process::ExitCode;

use rune::{AppSpec, Cx, El, col, el, row, spacer, text};

use crate::config;
use crate::control::{self, Intent};
use crate::live::LiveThread;
use crate::map::geometry::Camera;
use crate::units::Picture;

/// The **window title** — what a window manager and a task bar read.
///
/// It is no longer the wordmark's text: since #138 the header draws the **active workspace's name** where
/// the app's name used to be, because that is the operator's question ("where am I?") and the app's name
/// is what the window already says.
pub const TITLE: &str = "Arcons Command Center";

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

/// One pane: a camera, the map thread that renders it, and the frame it last drew.
///
/// The `Option`s are the two build modes: without `--features map` there is no thread (and no frame
/// but a scene's synthetic one), and a thread that is starting has not drawn yet.
pub struct Pane {
    pub id: ViewId,
    /// **What this pane is** (#116). A map pane holds a camera, a thread and a frame; a tree pane holds
    /// none of them and draws a structure instead.
    pub content: PaneContent,
    /// The camera this pane is holding. Its map thread renders that; markers project against the
    /// camera of the frame on screen instead (`INV-04`), which is not the same number mid-gesture.
    pub camera: Camera,
    /// The map thread and the pixels it paints. `None` without `--features map`.
    #[cfg(feature = "map")]
    pub map: Option<crate::map::thread::MapThread>,
    /// The frame the map last handed over: its camera and size are what this pane's markers project
    /// against.
    pub frame: Option<crate::map::geometry::Rendered>,
    /// The frame the view was last drawn for, so a new one redraws this pane's markers exactly once.
    /// Bookkeeping for the map handover, so a build without the engine has no use for it.
    #[cfg(feature = "map")]
    frame_seen: Option<crate::map::geometry::Rendered>,
    /// The camera at the start of a drag on this pane, so a pan is absolute rather than a sum of deltas.
    #[cfg(feature = "map")]
    drag_origin: Option<Camera>,
    /// **What this pane is looking at** (#99): the hull it is asking about, that hull's card, and the
    /// filter it will draw with. The wall's data is shared — one picture, one socket, one fleet — and the
    /// *question* is not: two panes may ask about two hulls, and a press in one must not answer in the other.
    pub view: PaneView,
    /// **The tree pane's camera** (#127): where the tree's own points sit in the pane. `None` on a map
    /// pane, like the map thread above — a pane holds the camera its content has, and two tree panes
    /// hold two. It is a value on the pane rather than in the drawing, so a gesture, a fit and a capture
    /// all read the same camera.
    pub tree: Option<crate::ui::tree::TreeCamera>,
    /// **The newsroom pane's view** (#136): the tab, the page, the rows and the query whose reply may
    /// still land. `None` on every other kind of pane, like the tree camera above: a pane holds the state
    /// its content has, and two newsroom panes hold two.
    pub news: Option<newsroom::View>,
    /// **The name the operator typed** (#83, #113). `None` means "nobody named this pane": the plate then
    /// draws the pane's own number from [`Pane::title`], because a title is not a name invented — a name is
    /// something somebody typed.
    ///
    /// It lives on the pane and not in `settings.toml`, because a pane does not survive a restart yet — a
    /// saved layout (which panes, at which cameras, in what tiling) is its own ticket, and a name that
    /// outlived the pane it named would be a label on a wall that no longer exists.
    pub name: Option<String>,
    /// **The hull this pane's camera is locked onto** (#169), or `None`.
    ///
    /// A follow is a **live mode**, not a place: `layout.toml` remembers where a pane is looking (#153), and a hull
    /// that has stopped reporting is not a place to come back to — so this is deliberately not saved, and the pane
    /// comes back where it was.
    pub follow: Option<i64>,
}

impl Pane {
    /// **What the plate says** (#113): the name the operator typed, or the pane's own number.
    ///
    /// The number comes from the pane's **id**, not its position: a re-tile keeps an id and a close does
    /// not renumber the others, so a wall can honestly show `pane-01` and `pane-03` after the second pane
    /// was closed. Renumbering by position would rename panes whenever they were dragged.
    pub fn title(&self) -> String {
        self.name.clone().unwrap_or_else(|| default_title(self.id))
    }

    pub fn new(id: ViewId, camera: Camera) -> Self {
        Self {
            id,
            content: PaneContent::Map,
            camera,
            #[cfg(feature = "map")]
            map: None,
            frame: None,
            #[cfg(feature = "map")]
            frame_seen: None,
            view: PaneView::default(),
            tree: None,
            news: None,
            name: None,
            follow: None,
            #[cfg(feature = "map")]
            drag_origin: None,
        }
    }

    /// A newsroom pane (#136): a list of stories instead of a picture. No camera use beyond the layout,
    /// no map thread, no frame — nothing in it projects.
    pub fn newsroom(id: ViewId, camera: Camera) -> Self {
        Self {
            content: PaneContent::Newsroom,
            news: Some(newsroom::View::new()),
            ..Self::new(id, camera)
        }
    }

    /// A pane that is not a map (#116): no thread, no frame, and a structure to draw.
    pub fn tree(id: ViewId, camera: Camera, organisation: Option<i64>) -> Self {
        Self {
            content: PaneContent::Tree { organisation },
            // A tree pane is a canvas (#127): it starts unfitted, and the first layout fits the whole
            // forest into it. A map pane has no tree camera at all, which is why this is `Some` here only.
            tree: Some(crate::ui::tree::TreeCamera::default()),
            ..Self::new(id, camera)
        }
    }

    /// **Whether this pane's picture is nobody's job yet** (#160): a map that no thread is rendering.
    ///
    /// **The kind is the whole question.** A restored pane, a pane moved between workspaces and a pane
    /// opened from a menu used to be three separate spawn sites, and #153's restore site forgot its half —
    /// which is a wall of map panes with no picture. Asking the pane means there is one answer, and the
    /// gate can hold every path that builds a pane to it.
    ///
    /// A build with no map engine has no thread to give a pane, so every map pane answers `true` there:
    /// [`State::spawn_missing_maps`] is a no-op, and this is the answer the gate's restore test reads.
    pub fn needs_map_thread(&self) -> bool {
        // Two answers rather than one guarded by an `if`: clippy's `needless_bool` reads the guard plus
        // the cfged `true` as `self.content.is_map()` and asks for it, and it is right.
        #[cfg(feature = "map")]
        {
            self.content.is_map() && self.map.is_none()
        }
        #[cfg(not(feature = "map"))]
        {
            self.content.is_map()
        }
    }
}

/// One pane's own view state (#99): the part of "what is on screen" that belongs to a window rather
/// than to the wall.
///
/// The camera was already here. The selection was not, and it showed: `State::selected` was wall-wide, so
/// `projections()` — the one function that draws "markers + card against this frame" — drew the same card
/// in every pane, and a press in one pane opened a card in all of them.
///
/// **The filter is here** (#112), beside the selection and the range, because the pane's state is one
/// thing: a wall-wide filter would be #99's shared selection in a new costume — press one pane's toggle
/// and every pane loses its buoys.
#[derive(Clone, Debug, Default)]
pub struct PaneView {
    /// The hull this pane is asking about. One per pane (`INV-10` per pane, not per wall).
    pub selected: Option<i64>,
    /// The card, built when this pane's selection or its fix changes — never per frame.
    pub card: Option<crate::units::card::Card>,
    /// The age text this pane's card drew last frame, so a second passing redraws the card and nothing
    /// else. Per pane for the same reason the card is: two cards age at two moments.
    last_age: String,
    /// Whether **this pane** draws symbols or pictures, latched with hysteresis on its own frame's zoom
    /// (`units::mark`). One latch per pane (#115): each pane has its own zoom, so each pane has its own
    /// representation — zooming one pane into a hull's picture must not switch its neighbour.
    pub range: crate::units::mark::Range,
    /// **What this pane hides** (#112): the three layers and the backend's two vocabularies. The data
    /// stays shared; the exclusions are the pane's, so two panes on one water can show the bases and
    /// the buoys.
    pub filter: Filter,
    /// **The facility this pane is asking about** (#109). A facility is not a `Unit`, so it is not
    /// `selected`: the hull's card and the facility's card are two answers to two questions, and a
    /// pane has one card. Selecting either clears the other.
    pub installation: Option<i64>,
}

/// The socket queue's patience: how long the fixes it delivered waited for a frame (#1).
///
/// v0.1's definition of done promises that a fix moves its marker **within 1 s of arrival**, so the wall
/// measures what it promises: every fix's wait is recorded, a worst wait past the promise is a warning in
/// the log, and the `socket` line carries the number. The fix is applied either way — a late marker is
/// better than a vanished hull — but a wall that is quietly two seconds behind its own feed is a wall
/// nobody can trust the picture of.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixLatency {
    last_ms: u64,
    max_ms: u64,
    /// How long the view loop had been idle when the worst fix was applied — the number that says whether
    /// "late" meant "slow" or "nothing was being drawn".
    max_gap_ms: u64,
    late: u64,
    /// Late, but only because no frame ran for that long: the window was occluded or the screen blanked, so
    /// the wall drew nothing and the fix waited for the first frame there was. Not a stall — and worth
    /// counting apart, because a wall that is not on screen is allowed to be still (`#1`, measured over 30
    /// minutes: one fix waited 2954 ms, and the frames it waited for were the suspended ones).
    idle: u64,
}

/// The promise, in milliseconds: the DoD's "within 1 s of arrival", minus nothing.
pub const FIX_LATENCY_LIMIT_MS: u64 = 1_000;

impl FixLatency {
    /// One fix's wait, recorded and bounded, against how long the loop had been idle.
    ///
    /// A fix can only be applied by a frame, so a wait is bounded below by the gap before the frame that
    /// applied it. If the wait *is* that gap, the wall applied it at the first opportunity and the delay was
    /// the drawing, not the wall; if the wait is longer, something else held it and that is a bug (`INV-05`).
    pub fn note(&mut self, waited: std::time::Duration, poll_gap: std::time::Duration) {
        let ms = waited.as_millis() as u64;
        let gap = poll_gap.as_millis() as u64;
        self.last_ms = ms;
        // Ties go to the **smaller** gap: two waits of the same length, one for a two-second idle and one
        // across a 16 ms frame, are not the same event, and the second is the one to look at.
        if ms > self.max_ms || (ms == self.max_ms && gap < self.max_gap_ms) {
            self.max_ms = ms;
            self.max_gap_ms = gap;
        }
        // 50 ms of slack: the wait starts when the socket thread takes the publication, which is somewhere
        // inside the gap, and the apply itself is a frame's work.
        if ms > FIX_LATENCY_LIMIT_MS {
            if gap.saturating_add(50) >= ms {
                self.idle += 1;
            } else {
                self.late += 1;
            }
        }
    }

    /// The worst wait so far, or `-1` before the first fix — a wall that has applied nothing has no number
    /// to report and must not invent one.
    pub fn max_ms(&self) -> i64 {
        if self.last_ms == 0 && self.max_ms == 0 {
            return -1;
        }
        self.max_ms as i64
    }

    /// How many fixes waited more than a second for a frame that was not the reason: the loop was running
    /// and did not apply them.
    pub fn late(&self) -> u64 {
        self.late
    }

    /// How many waited longer than a second only because no frame was being drawn at all.
    pub fn idle(&self) -> u64 {
        self.idle
    }

    /// The idle gap before the worst wait, in ms: the other half of `fix_ms`, and what tells the two apart.
    pub fn max_gap_ms(&self) -> i64 {
        if self.max_ms == 0 {
            return -1;
        }
        self.max_gap_ms as i64
    }
}

/// A lifted pane (#70): which pane, and the drop its handle is currently over.
///
/// The drop is recomputed from the pointer on every move, and the **preview layout** is derived from
/// it — dragging is a preview of an operation, not an operation in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneDrag {
    pub pane: ViewId,
    pub drop: Option<view::Drop>,
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

/// A rename in progress (#83, #139): what is being renamed, and the field being typed into.
///
/// The field is Rune's `TextEdit` and not a `String`: it owns the caret, the selection, the input method and
/// the undo history, and a wall shows an operator's keyboard the same way any other app does. The commit is
/// one function — Enter — and abandoning is Escape.
pub struct Rename {
    pub target: RenameTarget,
    pub edit: rune::TextEdit,
}

impl Rename {
    /// Open a rename with the current name in the field and the caret at the end — so typing replaces
    /// nothing until the operator says so with a selection or a backspace.
    pub fn new(target: RenameTarget, current: Option<&str>) -> Rename {
        let text = current.unwrap_or_default().to_string();
        let caret = text.chars().count();
        let mut edit = rune::TextEdit::default();
        edit.text.clone_from(&text);
        edit.anchor = caret;
        edit.focus = caret;
        edit.focused = true;
        Rename { target, edit }
    }

    /// **What the sheet is called** (#139): a workspace and a pane are different things, and a sheet that
    /// said "pane" over a workspace would be the sort of wrong an operator only notices afterwards.
    pub fn title(&self) -> &'static str {
        match self.target {
            RenameTarget::Pane(_) => RENAME_TITLE,
            RenameTarget::Workspace(_) => RENAME_WORKSPACE_TITLE,
        }
    }
}

/// **The only place the default title's form is written** (#113): `pane-01`, `pane-02`, … from the pane's
/// id, zero-padded to two digits so a wall of nine panes does not draw a column of ragged edges.
///
/// Past 99 it simply grows (`pane-100`): a wall with a hundred panes is not a wall this app is for.
pub fn default_title(id: ViewId) -> String {
    format!("pane-{id:02}")
}

/// What a name shows: trimmed, cut to `tokens::NAME_MAX_CHARS` with a drawn ellipsis. `None` for a name
/// that is empty once trimmed — an empty plate is worse than no plate.
pub fn shown_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut shown: String = trimmed.chars().take(tokens::NAME_MAX_CHARS).collect();
    if trimmed.chars().count() > tokens::NAME_MAX_CHARS {
        shown.push(ELLIPSIS);
    }
    Some(shown)
}

/// The drawn context menu: the pane it was asked on, and the point in that pane.
///
/// The point is in **device pixels**, because that is what it is used for twice: the menu is drawn at
/// it (divided by the scale) and the new pane's camera is unprojected at it (against the pane's frame,
/// which is device pixels too). Storing points would put the menu in the right place and the camera in
/// the wrong one on a 2× panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Menu {
    pub pane: ViewId,
    pub at: (f32, f32),
    /// The open submenu's key, when one is open (#140).
    pub open: Option<&'static str>,
    /// The row the keyboard is on, in the surface that is open (#164).
    pub ring: Option<usize>,
}

/// What the view loop holds. #3 fills the picture, #4 moves it, #6 draws it, #7 selects from it.
pub struct State {
    pub picture: Picture,
    /// The panes, in the order they were created (#68). One pane is the wall as it always was.
    ///
    /// Each pane owns its camera, its map thread and the frame that thread last drew — which is what
    /// makes `INV-04` a per-pane claim: a marker projects against **its pane's** frame, never another's.
    /// **The workspaces, in the order the chips draw** (#138). The active one is the wall on screen; the
    /// others keep their panes, cameras and frames, and their map threads are told to stand down.
    pub workspaces: Vec<Workspace>,
    /// Which workspace is active, as an index into [`State::workspaces`].
    pub active: usize,
    /// **One id space for every pane on the wall** (#138). A layout does not mint ids any more: a pane
    /// that moves to another workspace keeps its number, and two tilings must not both hand out 2.
    next_pane: ViewId,
    /// The picture panel's size in points, from its own layout. `None` until the first layout pass, when
    /// the panes are drawn into a provisional viewport the size of the window.
    pub panel: Option<(f32, f32)>,
    /// **What the wall is doing** (#101): one value, so "a menu and a rename at once" is not a state the
    /// type can express. Everything that used to be a field beside this one — the menu, the drag, the
    /// border, the rename, the settings sheet — is a variant of it.
    pub interaction: Interaction,
    /// Set when a layout pass changed something the drawing depends on (the panel's size, a pane's
    /// frame), so `poll` redraws once instead of every frame.
    pub layout_dirty: bool,
    /// What the display remembers between runs: the settings file (#82). A scene gets the defaults, so a
    /// capture never depends on the machine it was rendered on.
    pub settings: crate::config::settings::Settings,
    /// **When the arrangement last changed, on the frame clock** (#153): the debounce before the wall
    /// writes `layout.toml`. `None` is "nothing to write", and a scene never sets it — a capture that wrote
    /// to its machine's config directory would make the next capture's goldens depend on this one.
    pub arranged_at: Option<f64>,
    /// Device pixels per point, as the last layout pass reported it. The one number a *toggle* needs and
    /// cannot get from a layout callback: restoring a maximised pane resizes its map thread, and resizing
    /// it to the wrong scale is the blurred pane #85 exists to avoid.
    pub scale: f32,
    /// The focused pane the threads were last told about, so a focus change re-cadences them exactly
    /// once (#71): the focused pane at 4 Hz, every other pane at 1 Hz. Only a wall has threads to tell.
    #[cfg(feature = "map")]
    cadenced: Option<ViewId>,
    /// Whether the first map frame's camera has been reported. One log line per process, so a wall showing
    /// the wrong sea leaves a trace rather than a screenshot to argue with (`#56`).
    pub camera_reported: bool,
    /// **The client's cursor** (#167): where a remote is pointing, when it last moved, and whether it is pressing.
    /// `None` until a client points — a wall nobody is driving has no cursor to draw.
    pub cursor: Option<Cursor>,
    /// **How many times the arrangement has changed** (#201): the `seq` a `Scene` carries, so a viewer can order two
    /// scenes and notice a gap without an event log. The authority owns it once there is one (#202); today it moves
    /// wherever `arranged()` is called or a scene is applied.
    pub scene_seq: u64,
    /// **An intent script waiting for its frame** (#172): a scene that drives the wall **only** through the
    /// control plane carries its steps here, and `poll` applies the ones that are due. Empty on a wall — nothing
    /// scripts a wall — which is why it is a field on the state and not an environment variable.
    pub pending_intents: Vec<(f64, crate::control::Intent)>,
    /// How many of [`State::pending_intents`] have been applied, in order: a script is a sequence, and applying
    /// step 3 before step 2 would be a different wall.
    pub intents_applied: usize,
    /// **A viewer's session to the authority** (#202): `None` when this process is the wall, which is the default and
    /// the only case a capture is in. `Some` means another process owns the arrangement and this one draws the scenes
    /// it is given.
    #[cfg(unix)]
    pub session: Option<crate::control::authority::Session>,
    /// **The control plane's listener** (#165): the socket a client drives the wall through, and the queue of
    /// requests this thread answers between frames. `None` in a capture (a scene opens nothing, `INV-11`) and
    /// when the plane is turned off — so a wall with no socket is a wall nothing can be driven through, and
    /// `--self-check` says which it is.
    ///
    /// Unix only: the transport is a unix socket, and a Windows wall has no plane until someone decides on a
    /// named pipe (`src/control.rs` says the same where the modules are gated).
    #[cfg(unix)]
    pub control: Option<crate::control::Server>,
    /// The socket actor, when a credential was configured. `None` means the wall is read-only.
    pub live: Option<LiveThread>,
    /// The reads thread: the card asks it for identity, commander and specification.
    pub backend: Option<crate::data::Backend>,
    /// **The pane a `/viewpoint` reply may move** (#108): its id, captured when the read was asked
    /// for. `None` means no read is in flight, or its answer has already been folded in — an answer
    /// moves one pane, once.
    viewpoint_pane: Option<ViewId>,
    /// What the socket last said about itself.
    pub link: Link,
    /// How many fixes the socket has delivered **and the picture applied**, for the header's
    /// "is anything happening" question.
    pub fixes: u64,
    /// How long fixes waited in the socket's queue before a frame applied them (#1).
    pub fix_latency: FixLatency,
    /// How long the view loop was idle before the frame now being built. A fix's wait is measured against
    /// it, because a frame is the only thing that can apply one (`State::note_fix_latency`).
    pub poll_gap: std::time::Duration,
    /// The frame clock at the last poll, for the gap above — **not** `Instant::now()`, which `tests/scaffold.rs`
    /// refuses in the view: the only clock a frame may read is the frame clock (`INV-11`).
    last_poll_logical: Option<f64>,
    /// A fix older than this draws as stale. From `config.toml`, so a deployment whose feed is slow
    /// says so in its own units instead of arguing with a constant (`#8`).
    pub stale_after: std::time::Duration,
    /// The instant the process (or the scene) started. Ages are measured from here plus
    /// [`State::logical`], never from `Instant::now()`: a capture has to be byte-identical twice
    /// (`INV-11`), and a wall clock in a view is the surest way to lose that.
    origin: std::time::Instant,
    /// Seconds the frame loop believes have passed. Rune feeds a virtual clock to `poll` in
    /// headless runs, so a scene at `--at 2` is exactly two seconds old every time.
    logical: f64,
    /// A capture, not a wall: no socket, no map surface, and nothing on screen to report about them.
    pub headless: bool,
    /// Set by a marker's press, so the map's press handler does not undo it in the same click.
    /// Reset at the top of every frame's poll.
    marker_press: bool,
    /// When the last "socket" heartbeat was logged, on the wall's own uptime clock (ms).
    last_report_ms: u64,
    /// Dimensions of hulls the wall has read, for scaling a picture. Absent means "not read yet", which
    /// draws the picture clamped rather than at a scale nobody measured.
    pub sizes: std::collections::HashMap<i64, crate::units::mark::Size>,
    /// Presigned picture URLs from the fleet register, by hull.
    pub image_urls: std::collections::HashMap<i64, String>,
    /// Decode-and-fetch cache for the pictures. Created in `setup`, because it needs the waker.
    pub images: Option<rune::ImageCache>,
    /// `true` once the register has been asked for, so a failed read is retried on the next zoom-in
    /// rather than on every frame.
    pub asked_register: bool,
    /// Hulls whose dimensions have been asked for, so the wall does not ask on every frame.
    pub asked_sizes: std::collections::HashSet<i64>,
    /// Each hull's picture, **decoded**, as a cache handle this state owns.
    ///
    /// The handle is the picture for as long as the state holds it: Rune's cache keeps a weak entry per
    /// content hash, so a handle dropped between frames lets the entry expire and the next `load` starts
    /// the decode again. Asking once, when the bytes arrive, is both cheaper and — more importantly — the
    /// only way `poll` can see a decode *finish* (`#51`).
    pub pictures: std::collections::HashMap<i64, rune::Image>,
    /// Hulls whose picture was handed to the cache and has not been drawn yet. The frame a decode lands
    /// on is the frame that must be redrawn, and nothing else in the loop can notice that (`#51`).
    pub pictures_pending: std::collections::HashSet<i64>,
    /// Hulls whose picture has been asked for, so a missing one is not re-asked every frame.
    pub asked_images: std::collections::HashSet<i64>,
    /// **The tactical-object cache (#110)**, read once at init because a placement embeds its symbol
    /// and a manifest is the offline library: a page of a cache manifest is an incomplete cache.
    /// `manifest` is keyed by symbol `code`; `placements` are the operational marks (`id_game IS NULL`).
    pub manifest:
        std::collections::HashMap<String, crate::data::types::TacticalObjectManifestEntry>,
    pub placements: Vec<crate::data::types::TacticalObjectPlacement>,
    /// **The facility vocabulary and the facilities** (#109's reads, cached at init for #112's panel and
    /// for the layer #109 draws). `GET /installations` is unpaginated, so this is the complete set and a
    /// type's count is the cache's, not a page's. Neither list is re-read per frame: a refresh is a
    /// deliberate read.
    pub installation_types: Vec<crate::data::types::InstallationType>,
    pub installations: Vec<crate::data::types::Installation>,
    /// **The tactical-object picker's vocabulary** (`GET /tactical-objects/facets`), read once at init:
    /// the panel's domain and category rows and the **server's** counts for them. `None` is "not read
    /// yet", which draws no vocabulary rows rather than an empty vocabulary.
    pub facets: Option<crate::data::types::TacticalObjectFacets>,
    /// Decoded artwork, by the manifest's `asset_key`. The same `ImageCache` route a hull's picture
    /// takes: bytes arrive as a reply, decode off the UI thread, one handle per key (`#110`).
    pub assets: std::collections::HashMap<String, rune::Image>,
    /// Assets handed to the cache that have not been drawn yet, so the frame a decode lands on redraws.
    pub assets_pending: std::collections::HashSet<String>,
    /// Assets already asked for, and assets whose URL expired or was refused: the latter draw the
    /// generic mark for the life of the process rather than asking again every frame.
    pub asked_assets: std::collections::HashSet<String>,
    pub asset_failures: std::collections::HashSet<String>,
    /// Symbols that cannot draw their artwork — an unsigned `asset_url`, or a code the manifest does
    /// not carry — logged once each so a wall with no pictures is a line in the log, not a mystery.
    pub generic_logged: std::collections::HashSet<String>,
    /// How many register reads have failed in a row. Never reset by the passage of time: a wall that cannot
    /// read `/units` should be able to say how long it has not been able to.
    pub register_attempts: u32,
    /// When a failed read may be asked for again, on the wall's own clock. Absent means "no waiting".
    ///
    /// A refusal used to be permanent: `asked_register` and `asked_images` were set before the request and
    /// never cleared, so one `403`, one expired presigned URL or one dropped response meant no pictures for
    /// the life of the process, with nothing said (`#53`).
    pub register_retry_at: Option<std::time::Instant>,
    /// Pictures that failed, with their attempt count and when to ask again.
    pub image_retries: std::collections::HashMap<i64, Retry>,
    /// How many hulls the fleet register returned, across every page it took. The heartbeat repeats it so a
    /// truncated register is visible on a wall (`#52`) — `registered=25` next to `units=127` is the alarm.
    pub registered: usize,
    /// **The standing orders of battle** (#116): the structures the wall knows, the forests it has read,
    /// and the supreme command above them. Read on demand — a wall with no tree pane asks for none of it.
    pub organisations: Vec<crate::data::types::UnitOrganisation>,
    pub organisation_trees:
        std::collections::HashMap<i64, crate::data::types::UnitOrganisationDetail>,
    /// `GET /tni-commander`: one post shared by every branch, and `commander` is `None` when it is
    /// vacant — a real state, not a missing read.
    pub tni_command: Option<crate::data::types::TniCommandPost>,
    /// The structure reads a tree pane has already asked for, so a failed read is one log line rather
    /// than a request every frame.
    asked_organisations: std::collections::HashSet<i64>,
    asked_organisations_list: bool,
    asked_tni: bool,
}

/// **A cursor the wall owns** (#167): where a client is pointing, in the picture panel's points.
///
/// The wall's own pointer is the OS's — it belongs to whichever hand is on the glass. A client that has no hand (a
/// remote, a CLI, a test) needs a pointer the *wall* keeps, so that "press here" means the same thing whatever
/// produced it. This is that value: a point, whether its button is down, and when it last moved.
///
/// **It fades.** A cursor nobody has moved for [`CURSOR_FADE`] is not drawn: a stray pointer on a wall at 3 a.m.
/// is a bug that looks like a mouse. The fade is on the frame clock, like every other age.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cursor {
    /// The point, in the **picture panel's** points — the space the panes tile in, and the space markers are
    /// drawn in. Not the window's: the header's chips are the OS pointer's, and a client's cursor lives on the
    /// picture (see `docs/control.md`).
    pub at: (f32, f32),
    /// Whether the button is down — a press that has not been released.
    pub down: bool,
    /// The frame clock when it last moved, so the fade is a function of the frame and not of a wall clock.
    pub moved_at: f64,
}

impl Cursor {
    /// How long a cursor that has not moved is still drawn for: **a beat and a half**.
    ///
    /// Long enough that a person reading a card still sees where their remote is pointing; short enough that a
    /// wall nobody is touching has no pointer on it. A cursor re-appears on the next `cursor.move`, and a press
    /// keeps it up whatever the clock says.
    pub const FADE: f64 = 1.5;

    /// Whether a client's cursor is drawn at `now`: it exists, and it has moved recently.
    ///
    /// A button that is **down** keeps it visible whatever the clock says: a remote holding a press is a remote
    /// still using the pointer, and a cursor that vanished mid-gesture is a bug that looks like a lost connection.
    pub fn visible_at(&self, now: f64) -> bool {
        self.down || (now - self.moved_at) < Self::FADE
    }

    /// How far from `point` this cursor is, in points — the radius a press hits within.
    ///
    /// A remote's press is a coordinate, not a pixel the person aimed at: a client on a phone guesses a point on a
    /// map it has only read, so a press within [`Cursor::HIT`] of a hull's marker is that hull's press.
    pub fn distance_to(&self, point: (f64, f64)) -> f64 {
        let dx = f64::from(self.at.0) - point.0;
        let dy = f64::from(self.at.1) - point.1;
        (dx * dx + dy * dy).sqrt()
    }

    /// How close a press has to be to a marker to be that marker's, in points.
    pub const HIT: f64 = 14.0;
}

impl Default for State {
    /// The configuration the process installed, or the defaults when there is none — which is what a
    /// test has, and what `cargo run` has before `config::set_current`.
    fn default() -> Self {
        let config = crate::config::current();
        let camera = match config {
            Some(config) => Camera::new(config.map_center, config.map_zoom),
            None => Camera::START,
        };
        Self {
            picture: Picture::default(),
            // **Three workspaces, the operator's default** (#138): the first holds the pane the wall
            // has always opened with, the other two are places panes can be put.
            workspaces: vec![
                Workspace::with_pane("Workspace 1".to_string(), Pane::new(1, camera)),
                Workspace::empty("Workspace 2".to_string()),
                Workspace::empty("Workspace 3".to_string()),
            ],
            active: 0,
            next_pane: 2,
            settings: crate::config::settings::Settings::default(),
            arranged_at: None,

            scale: 1.0,

            panel: None,
            interaction: Interaction::Idle,
            layout_dirty: false,

            #[cfg(feature = "map")]
            cadenced: None,
            cursor: None,
            scene_seq: 0,
            #[cfg(unix)]
            session: None,
            pending_intents: Vec::new(),
            intents_applied: 0,
            #[cfg(unix)]
            control: None,
            live: None,
            backend: None,
            viewpoint_pane: None,
            link: Link::default(),
            fixes: 0,
            fix_latency: FixLatency::default(),
            poll_gap: std::time::Duration::ZERO,
            last_poll_logical: None,
            stale_after: config
                .map(|config| config.stale_after)
                .unwrap_or(crate::units::STALE_AFTER),
            origin: std::time::Instant::now(),
            logical: 0.0,
            headless: false,
            marker_press: false,
            last_report_ms: 0,
            sizes: std::collections::HashMap::new(),
            image_urls: std::collections::HashMap::new(),
            images: None,
            asked_register: false,
            asked_sizes: std::collections::HashSet::new(),
            pictures: std::collections::HashMap::new(),
            pictures_pending: std::collections::HashSet::new(),
            asked_images: std::collections::HashSet::new(),
            manifest: std::collections::HashMap::new(),
            placements: Vec::new(),
            installation_types: Vec::new(),
            installations: Vec::new(),
            facets: None,
            assets: std::collections::HashMap::new(),
            assets_pending: std::collections::HashSet::new(),
            asked_assets: std::collections::HashSet::new(),
            asset_failures: std::collections::HashSet::new(),
            generic_logged: std::collections::HashSet::new(),
            camera_reported: false,
            register_attempts: 0,
            register_retry_at: None,
            image_retries: std::collections::HashMap::new(),
            registered: 0,
            organisations: Vec::new(),
            organisation_trees: std::collections::HashMap::new(),
            tni_command: None,
            asked_organisations: std::collections::HashSet::new(),
            asked_organisations_list: false,
            asked_tni: false,
        }
    }
}

impl State {
    /// The clock every age in the view is measured against. The *only* source of "now" in the view,
    /// which is what makes a capture reproducible: `--scene` drives `logical` from Rune's virtual
    /// clock, and a windowed run from the real one.
    /// The decode cache, created on first use with **the app's waker**, so a decode that finishes wakes
    /// the wall instead of waiting for something else to redraw it.
    ///
    /// This used to be a no-op waker on the argument that "the wall redraws while a picture is pending, so
    /// it never needs to be woken". That argument was wrong in one place and it cost the picture: the
    /// pending redraws stop on the very frame the decode completes, so nothing drew it (`#51`).
    pub fn image_cache_with(&mut self, waker: rune::Waker) -> &rune::ImageCache {
        self.images
            .get_or_insert_with(move || rune::ImageCache::new(waker))
    }

    /// The cache for a caller with no waker to give — a headless scene, whose bytes are decoded before the
    /// first frame and whose clock is virtual (`INV-11`). A windowed wall must use [`Self::image_cache_with`].
    pub fn image_cache(&mut self) -> &rune::ImageCache {
        self.images
            .get_or_insert_with(|| rune::ImageCache::new(std::sync::Arc::new(|| {}) as rune::Waker))
    }

    /// Hand a hull's bytes to the cache, once, and remember that the frame it lands on needs drawing.
    pub fn offer_picture(&mut self, id_unit: i64, bytes: std::sync::Arc<[u8]>) {
        let image = self.image_cache().load(bytes);
        self.pictures.insert(id_unit, image);
        self.pictures_pending.insert(id_unit);
    }

    /// The same route for a tactical object's artwork, keyed by the manifest's `asset_key` (#110).
    ///
    /// The key is what survives: a presigned `asset_url` expires, and a re-signed URL names the same
    /// bytes. The frame the decode lands on is remembered for the same reason a hull's picture is —
    /// a decode that finishes between two polls changes nothing the loop would otherwise see (#51).
    pub fn offer_asset(&mut self, asset_key: &str, bytes: std::sync::Arc<[u8]>) {
        let image = self.image_cache().load(bytes);
        self.assets.insert(asset_key.to_string(), image);
        self.assets_pending.insert(asset_key.to_string());
    }

    pub fn now(&self) -> std::time::Instant {
        self.origin + std::time::Duration::from_secs_f64(self.logical)
    }

    /// Take a wire-shaped picture as the opening state, stamped at the logical clock. A scene's ages
    /// then come from the fixture's own `age_seconds` and nothing else.
    pub fn load_fixture(&mut self, entries: &[crate::data::types::FeedPosition]) {
        let now = self.now();
        self.picture.replace_from_feed_at(entries, now);
        self.fixes = 0;
    }

    /// A state holding a picture, for tests and for a caller that has one already. The rest — the
    /// camera, the frame, the socket — starts at its default.
    pub fn new(picture: Picture) -> Self {
        Self {
            picture,
            ..Self::default()
        }
    }

    /// The pane the operator is working in. There is always one: closing the last pane is refused.
    pub fn focused_pane(&self) -> &Pane {
        let id = self.focused();
        self.pane(id).unwrap_or_else(|| {
            // A layout naming a pane that is gone is a bug in the tree, not in the drawing; fall back
            // rather than panic in a view function, which would take the wall down at 2 a.m.
            let first = self.pane_ids().first().copied().unwrap_or(1);
            self.pane(first).unwrap_or_else(|| {
                static FALLBACK: std::sync::OnceLock<Pane> = std::sync::OnceLock::new();
                FALLBACK.get_or_init(|| Pane::new(1, Camera::START))
            })
        })
    }

    /// The focused pane, for a caller that is writing to it. `None` on a workspace with no panes: there
    /// is no pane to write to, and a workspace is allowed to be empty (#138).
    pub fn focused_pane_mut(&mut self) -> Option<&mut Pane> {
        let id = self.focused();
        if !self.panes().iter().any(|pane| pane.id == id)
            && let Some(first) = self.pane_ids().first().copied()
        {
            self.focus_pane(first);
        }
        let id = self.focused();
        let index = self.panes().iter().position(|pane| pane.id == id)?;
        self.panes_mut().get_mut(index)
    }

    /// **Switch workspaces** (#138): the wall on screen becomes another one.
    ///
    /// The interaction is ended rather than carried across: a menu, a filter panel or a rename names a
    /// pane in the workspace it was opened on. Hidden workspaces' maps stand down on the next frame
    /// (`apply_cadence`), so three workspaces are not three walls rendering.
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index >= self.workspaces.len() || index == self.active {
            return false;
        }
        self.end();
        self.active = index;
        #[cfg(feature = "map")]
        {
            // Force the cadence pass on the next frame: the pane count may not have changed.
            self.cadenced = None;
        }
        self.arranged();
        self.layout_dirty = true;
        true
    }

    /// **Append a workspace and switch to it** (#138) — the header's `+`. It is **empty**: a new
    /// workspace is a place to put panes, not a copy of the one on screen.
    pub fn add_workspace(&mut self) -> usize {
        let index = self.workspaces.len();
        self.workspaces
            .push(Workspace::empty(default_workspace_name(index)));
        self.switch_to(index);
        index
    }

    /// The pane a menu is open on, and where — `None` when no menu is open.
    pub fn menu(&self) -> Option<Menu> {
        match self.interaction {
            Interaction::Menu {
                pane,
                at,
                open,
                ring,
            } => Some(Menu {
                pane,
                at,
                open,
                ring,
            }),
            _ => None,
        }
    }

    /// The pane whose filter panel is open — `None` when no panel is. The panel is anchored to that
    /// pane's toggle, so the pane is all the state needs.
    pub fn filtering(&self) -> Option<ViewId> {
        match self.interaction {
            Interaction::Filtering { pane } => Some(pane),
            _ => None,
        }
    }

    /// The lifted pane and its drop, if one is in the air.
    pub fn dragging(&self) -> Option<PaneDrag> {
        match self.interaction {
            Interaction::Lifted(drag) => Some(drag),
            _ => None,
        }
    }

    /// The border a drag is holding, if one is.
    pub fn resizing(&self) -> Option<view::Border> {
        match self.interaction {
            Interaction::Resizing(border) => Some(border),
            _ => None,
        }
    }

    /// The rename in progress, if one is.
    pub fn rename(&self) -> Option<&Rename> {
        match &self.interaction {
            Interaction::Renaming(rename) => Some(rename),
            _ => None,
        }
    }

    /// The rename's field, for the text input's `on_edit`. `None` when nothing is renaming.
    pub fn renaming_mut(&mut self) -> Option<&mut Rename> {
        match &mut self.interaction {
            Interaction::Renaming(rename) => Some(rename),
            _ => None,
        }
    }

    /// Whether the settings sheet is open.
    pub fn settings_open(&self) -> bool {
        matches!(self.interaction, Interaction::Settings)
    }

    /// The pane drawn over the tiling, from the layout that owns it.
    pub fn maximised(&self) -> Option<ViewId> {
        self.layout().and_then(|layout| layout.maximised())
    }

    /// **Enter a state, if the table allows it** (#101). Returns whether it happened, so a caller can do the
    /// follow-up work only when the wall actually moved.
    ///
    /// One place, because the alternative is what this replaced: six fields, each set from whichever handler
    /// felt like it, and nothing to say which combinations were meant to exist.
    pub fn begin(&mut self, begin: Begin) -> bool {
        if !begin.allowed_from(&self.interaction) {
            return false;
        }
        self.interaction = match begin {
            Begin::Lift(drag) => Interaction::Lifted(drag),
            Begin::Resize(border) => Interaction::Resizing(border),
            Begin::Menu { pane, at } => Interaction::Menu {
                pane,
                at,
                open: None,
                // A menu opened by a press has no ring: the pointer is where the operator's attention is, and a
                // ring nobody put there would be a second answer to "which row" (#164).
                ring: None,
            },
            Begin::Filter { pane } => Interaction::Filtering { pane },
            Begin::Rename(rename) => Interaction::Renaming(rename),
            Begin::Settings => Interaction::Settings,
        };
        self.layout_dirty = true;
        true
    }

    /// Release whatever is held: the one way back to `Idle` (Escape, a release, a dismissal).
    pub fn end(&mut self) -> bool {
        if matches!(self.interaction, Interaction::Idle) {
            return false;
        }
        self.interaction = Interaction::Idle;
        self.layout_dirty = true;
        true
    }

    /// The workspace on screen.
    pub fn active(&self) -> &Workspace {
        &self.workspaces[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active]
    }

    /// The active workspace's tiling, or `None` when it holds no panes.
    pub fn layout(&self) -> Option<&Layout> {
        self.active().layout.as_ref()
    }

    pub fn layout_mut(&mut self) -> Option<&mut Layout> {
        self.active_mut().layout.as_mut()
    }

    /// The active workspace's panes.
    pub fn panes(&self) -> &[Pane] {
        &self.active().panes
    }

    pub fn panes_mut(&mut self) -> &mut Vec<Pane> {
        &mut self.active_mut().panes
    }

    /// **A pane, whichever workspace holds it** (#138): ids are the wall's, so looking one up is not a
    /// question about the active workspace.
    pub fn pane(&self, id: ViewId) -> Option<&Pane> {
        self.workspaces
            .iter()
            .flat_map(|workspace| workspace.panes.iter())
            .find(|pane| pane.id == id)
    }

    pub fn pane_mut(&mut self, id: ViewId) -> Option<&mut Pane> {
        self.workspaces
            .iter_mut()
            .flat_map(|workspace| workspace.panes.iter_mut())
            .find(|pane| pane.id == id)
    }

    /// **Which workspace holds this pane** (#140): the id is the wall's, so a pane is findable from any
    /// workspace — and the one question the move and the submenu both ask.
    pub fn workspace_of(&self, pane: ViewId) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|workspace| workspace.panes.iter().any(|held| held.id == pane))
    }

    /// One pane of one workspace, mutably — for a caller that has the index and the id and must not
    /// search the active workspace for it (#140).
    pub fn workspace_pane_mut(&mut self, index: usize, pane: ViewId) -> Option<&mut Pane> {
        self.workspaces
            .get_mut(index)?
            .panes
            .iter_mut()
            .find(|held| held.id == pane)
    }

    /// The next pane id on the wall. The only place one is minted (#138).
    pub fn allocate_pane(&mut self) -> ViewId {
        let id = self.next_pane;
        self.next_pane += 1;
        id
    }

    // **Thin questions the view asks of the tiling.** They exist so two hundred call sites do not each
    // unwrap an `Option<Layout>`; the ones that must handle an empty workspace ask `layout()` instead
    // (`maximised`, the picture panel). On an empty workspace these answer "no panes" — `focused()`
    // names no pane, so every lookup of it returns `None` rather than a wrong pane.
    pub fn focused(&self) -> ViewId {
        self.layout().map(|layout| layout.focused()).unwrap_or(0)
    }

    pub fn focus_pane(&mut self, id: ViewId) -> bool {
        let focused = self.layout_mut().is_some_and(|layout| layout.focus(id));
        if focused {
            self.arranged();
        }
        focused
    }

    pub fn pane_ids(&self) -> Vec<ViewId> {
        self.layout().map(|layout| layout.ids()).unwrap_or_default()
    }

    pub fn pane_count(&self) -> usize {
        self.layout().map(|layout| layout.len()).unwrap_or(0)
    }

    pub fn is_single_pane(&self) -> bool {
        self.layout().is_some_and(|layout| layout.is_single())
    }

    pub fn pane_rect(&self, id: ViewId) -> Option<Rect> {
        self.layout()?.rect_of(self.pane_viewport(), id)
    }

    pub fn set_ratio(&mut self, border: &view::Border, ratio: f32) -> bool {
        let set = self
            .layout_mut()
            .is_some_and(|layout| layout.set_ratio(border, ratio));
        if set {
            self.arranged();
        }
        set
    }

    pub fn border_at(&self, at: (f32, f32), grip: f32) -> Option<view::Border> {
        self.layout()?.border_at(self.pane_viewport(), at, grip)
    }

    pub fn borders(&self) -> Vec<view::Border> {
        self.layout()
            .map(|layout| layout.borders(self.pane_viewport()))
            .unwrap_or_default()
    }

    pub fn drop_for(&self, dragged: ViewId, at: (f32, f32)) -> Option<view::Drop> {
        self.layout()?.drop_for(self.pane_viewport(), dragged, at)
    }

    pub fn preview(&self, drop: view::Drop) -> Option<Layout> {
        self.layout()?.preview(drop)
    }

    pub fn apply_drop(&mut self, drop: view::Drop) -> bool {
        self.layout_mut().is_some_and(|layout| layout.apply(drop))
    }

    pub fn maximise(&mut self, id: ViewId) -> bool {
        self.layout_mut().is_some_and(|layout| layout.maximise(id))
    }

    pub fn restore_tiling(&mut self) {
        if let Some(layout) = self.layout_mut() {
            layout.restore();
        }
    }

    /// Remove a pane from the active workspace: its slot collapses and its map thread is dropped.
    ///
    /// **The last pane of a workspace is still refused** — closing a workspace's last pane and landing
    /// the focus on another workspace is #141's rule, and it is a different ticket. What #138 makes
    /// expressible is a workspace that was *never* given a pane, or whose panes were moved away by the
    /// move ticket: `Workspace::empty` is a valid state, and `layout()` answers `None` for it.
    /// **May this pane be closed?** (#141) — a pane with a sibling in its workspace, or a pane somewhere
    /// else on the wall. The menu row and the action read this one answer, so the control and the action
    /// cannot disagree about the last pane.
    pub fn can_close(&self, id: ViewId) -> bool {
        let Some(index) = self.workspace_of(id) else {
            return false;
        };
        if self.workspaces[index].panes.len() > 1 {
            return true;
        }
        // **The wall always keeps one pane** (#141). Three empty workspaces is a wall that can only be
        // fixed with the `+` and `Focus on new pane`, which is not a state to hand an operator at 2 a.m.
        self.workspaces
            .iter()
            .enumerate()
            .any(|(other, workspace)| other != index && !workspace.panes.is_empty())
    }

    /// **The nearest previous workspace that still has a pane** (#141): `active-1, active-2, … 0`, then
    /// `active+1, … n-1`. The descending scan is what makes a tie resolve to the lower index.
    pub fn nearest_previous_non_empty(&self, from: usize) -> Option<usize> {
        let has_panes = |index: usize| {
            self.workspaces
                .get(index)
                .is_some_and(|workspace| !workspace.panes.is_empty())
        };
        (0..from)
            .rev()
            .find(|&index| has_panes(index))
            .or_else(|| ((from + 1)..self.workspaces.len()).find(|&index| has_panes(index)))
    }

    /// **Close a pane** (#81, #141): its slot collapses into its sibling, or — when it was its workspace's
    /// only pane — the workspace becomes **empty** and the wall lands on the nearest previous workspace
    /// that still has one. Dropping the pane sends its map thread's stop and joins it.
    ///
    /// `false` for a pane no workspace holds, and for **the last pane on the wall** (#141): the rule is
    /// [`State::can_close`], the same answer the menu row is built from.
    pub fn close_pane(&mut self, id: ViewId) -> bool {
        if !self.can_close(id) {
            return false;
        }
        let Some(index) = self.workspace_of(id) else {
            return false;
        };
        if !self.workspaces[index].close(id) {
            return false;
        }
        // **Nothing hidden may still hold it**: ids are the wall's (#138), and a pane that survived in a
        // workspace nobody draws would be a map thread that never stops.
        for workspace in &mut self.workspaces {
            workspace.panes.retain(|pane| pane.id != id);
        }
        // The pane a surface was opened on is gone, so the surface goes with it.
        let on_it = match self.interaction {
            Interaction::Menu { pane, .. } | Interaction::Filtering { pane } => pane == id,
            Interaction::Renaming(Rename {
                target: RenameTarget::Pane(pane),
                ..
            }) => pane == id,
            _ => false,
        };
        if on_it {
            self.interaction = Interaction::Idle;
        }
        // **Land on the nearest previous workspace that still has a pane** (#141), and focus **the pane
        // the operator was last in there** — `Layout::focused()` remembers it, and it is not necessarily
        // the first leaf. A workspace that keeps a pane keeps the one its tiling focused; the collapse did
        // that.
        if index == self.active
            && self.workspaces[index].panes.is_empty()
            && let Some(target) = self.nearest_previous_non_empty(index)
        {
            self.switch_to(target);
        }
        // **A workspace that just became a one-pane wall** (#166): the survivor's frame was its *slot's*, and markers
        // project against a frame rather than a pane (`INV-04`) — so they would sit in the corner of a pane that is
        // now the whole panel. Clearing it lets the next frame hand it one at its new size: a wall's map thread, a
        // capture's synthetic frame.
        let survived = self.workspaces[index]
            .layout
            .as_ref()
            .filter(|layout| layout.len() == 1)
            .map(|layout| layout.focused());
        if let Some(id) = survived
            && let Some(pane) = self.workspace_pane_mut(index, id)
        {
            pane.frame = None;
        }
        self.arranged();
        self.layout_dirty = true;
        true
    }

    /// The focused pane's camera: what a gesture on the wall moves.
    pub fn camera(&self) -> Camera {
        self.focused_pane().camera
    }

    pub fn camera_of(&self, id: ViewId) -> Camera {
        self.pane(id)
            .map(|pane| pane.camera)
            .unwrap_or_else(|| self.camera())
    }

    pub fn set_camera(&mut self, camera: Camera) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.camera = camera;
            self.arranged();
        }
    }

    pub fn set_camera_of(&mut self, id: ViewId, camera: Camera) {
        if let Some(pane) = self.pane_mut(id) {
            pane.camera = camera;
            // **Any camera intent ends a follow** (#169): otherwise the operator pans and the next frame pulls the
            // camera back to the hull, which is a wall fighting the hand that is driving it.
            pane.follow = None;
            // **The frame's camera moves with the pane's** (#187): markers project against the frame (`INV-04`), and a
            // camera that changed with no gesture makes no new frame — the picture would keep drawing the old place.
            // A wall's map thread replaces this with the frame it renders, at this camera.
            if let Some(frame) = pane.frame {
                pane.frame = Some(crate::map::geometry::Rendered { camera, ..frame });
            }
            self.arranged();
        }
    }

    /// The focused pane's frame: the one markers project against when there is one pane.
    pub fn frame(&self) -> Option<crate::map::geometry::Rendered> {
        self.focused_pane().frame
    }

    /// A frame for a scene or a test, on the focused pane. A wall's frames come from the map thread.
    pub fn set_frame(&mut self, frame: crate::map::geometry::Rendered) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.frame = Some(frame);
        }
        self.layout_dirty = true;
    }

    /// The viewport the panes tile: the picture panel's own points, or the window's until it is known.
    pub fn pane_viewport(&self) -> crate::map::geometry::Viewport {
        let (w, h) = self
            .panel
            .unwrap_or((tokens::WINDOW_SIZE.0, tokens::WINDOW_SIZE.1));
        crate::map::geometry::Viewport::new(f64::from(w), f64::from(h))
    }

    /// Where each pane is drawn, from the same function `verify` checks a capture with — the layout's
    /// cells, pulled in by the gap the operator chose (#82).
    pub fn pane_rects(&self) -> Vec<(ViewId, Rect)> {
        self.layout()
            .map(|layout| view::panes_on(layout, self.pane_viewport(), self.settings.pane_gap))
            .unwrap_or_default()
    }

    /// One fix's wait for a frame: recorded, and said out loud when it breaks v0.1's promise (#1).
    pub fn note_fix_latency(&mut self, waited: std::time::Duration) {
        self.fix_latency.note(waited, self.poll_gap);
        if waited.as_millis() as u64 > FIX_LATENCY_LIMIT_MS {
            if self.poll_gap.as_millis() as u64 + 50 >= waited.as_millis() as u64 {
                tracing::debug!(
                    waited_ms = waited.as_millis() as u64,
                    idle_ms = self.poll_gap.as_millis() as u64,
                    "a fix waited for the first frame there was: the wall was not being drawn"
                );
            } else {
                tracing::warn!(
                    waited_ms = waited.as_millis() as u64,
                    idle_ms = self.poll_gap.as_millis() as u64,
                    fixes = self.fixes,
                    "a fix waited longer than a second for a frame, and the loop was running \
                     (INV-05: the UI thread must not block)"
                );
            }
        }
    }

    /// The preferences the display remembers (#82), read once when the wall opens.
    ///
    /// **Never for a scene**: a capture renders on the machine it runs on, and a settings file there would
    /// make the picture depend on who is logged in — the end of `INV-11`. A scene that wants gaps sets them
    /// in its own code (`panes-gaps`), which is also how the gate covers both looks.
    pub fn with_saved_settings(mut self) -> State {
        if rune::is_headless() {
            return self;
        }
        let path = crate::config::settings_file();
        let (settings, note) = crate::config::settings::Settings::read(&path);
        self.settings = settings;
        tracing::info!("{note} (pane gap {} pt)", settings.pane_gap);
        self
    }

    /// How wide a border's grab is, in points: at least the gutter's own grip, and at least the gap — a
    /// pointer in the middle of an 8 pt gap is 4 pt from the line, and a 6 pt grip would not reach it.
    pub fn pane_grip(&self) -> f32 {
        tokens::PANE_GRIP.max(self.settings.pane_gap)
    }

    /// Split a pane and give the new one a camera. The wall gets a map thread for it; a scene gets a
    /// frame from its own layout (`view::pane_laid_out`), because a capture has no engine.
    pub fn split_pane(&mut self, target: ViewId, camera: Camera) -> Option<ViewId> {
        self.split_pane_as(target, camera, PaneContent::Map)
    }

    /// Split a pane and say **what the new one is** (#116). No thread is started here, whatever the
    /// kind: `poll` starts one for every map pane that has none (#160), so this function does not have
    /// to know which kinds need one.
    pub fn split_pane_as(
        &mut self,
        target: ViewId,
        camera: Camera,
        content: PaneContent,
    ) -> Option<ViewId> {
        let rect = self.pane_rect(target).unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            w: self.pane_viewport().w as f32,
            h: self.pane_viewport().h as f32,
        });
        // **The wall mints the id** (#138), not the layout: a pane that moves between workspaces keeps
        // its number, and two tilings must not both hand out 2.
        let id = self.allocate_pane();
        let dir = rect.longer_edge();
        self.layout_mut()?.split_with(target, dir, id)?;
        self.panes_mut().push(match content {
            PaneContent::Map => Pane::new(id, camera),
            PaneContent::Tree { organisation } => Pane::tree(id, camera, organisation),
            PaneContent::Newsroom => Pane::newsroom(id, camera),
        });
        // **The new pane needs no thread started here** (#160): `poll` gives a thread to every map
        // pane that has none, on the next frame, at the slot this split just gave it. A second door
        // would be a second answer — and a kind that forgot to take it is #153's bug.
        self.arranged();
        self.layout_dirty = true;
        Some(id)
    }

    /// How many frames each pane has drawn, in pane order (#71).
    ///
    /// The map thread's own counter, idle ticks included: a pane whose number has stopped is a pane that
    /// is not drawing, which is not the same as a pane whose camera has not moved. Without the engine
    /// there are no threads and no frames to count.
    pub fn pane_frames(&self) -> Vec<u64> {
        #[cfg(feature = "map")]
        {
            self.panes()
                .iter()
                .map(|pane| pane.map.as_ref().map(|map| map.frames()).unwrap_or(0))
                .collect()
        }
        #[cfg(not(feature = "map"))]
        {
            Vec::new()
        }
    }

    /// **Which map panes have no picture** (#160): every map pane on the wall — all workspaces, not
    /// only the one on screen — with the camera it holds and the points it should render at.
    ///
    /// Pure and engine-free on purpose, so the gate can ask it on a machine with no GPU: it answers from
    /// the model, and a wall that has lost a pane's thread fails this on the rung above the one that needs
    /// a driver. The rects come from [`view::panes_on`], the same answer the picture draws them from.
    pub fn panes_without_maps(&self) -> Vec<(ViewId, Camera, (f32, f32))> {
        let viewport = self.pane_viewport();
        let gap = self.settings.pane_gap;
        let mut missing = Vec::new();
        for workspace in &self.workspaces {
            let Some(layout) = workspace.layout.as_ref() else {
                continue;
            };
            for (id, rect) in view::panes_on(layout, viewport, gap) {
                let Some(pane) = workspace.panes.iter().find(|pane| pane.id == id) else {
                    continue;
                };
                if pane.needs_map_thread() {
                    missing.push((id, pane.camera, (rect.w.max(1.0), rect.h.max(1.0))));
                }
            }
        }
        missing
    }

    /// **Every map pane is rendering, always** (#160): start the thread of any map pane that has none.
    ///
    /// Called from `poll` on every frame rather than from the three places that create a pane, because
    /// that is how #153 broke: `apply_saved` built a wall out of the file and the only spawn left was
    /// `setup`'s, which takes the focused pane whatever it is. One question, asked of the wall once a
    /// frame, is the version that cannot be forgotten by the next path that opens a pane.
    ///
    /// A healthy wall pays one walk of its panes per frame and no allocation. The first frame after a
    /// restore draws on this frame's layout pass, which is the same frame `poll` precedes, so nothing
    /// flashes; and the size handed over is corrected by `pane_laid_out` on that pass.
    pub fn spawn_missing_maps(&mut self) -> bool {
        if self.headless {
            return false;
        }
        let missing = self.panes_without_maps();
        if missing.is_empty() {
            return false;
        }
        #[cfg(feature = "map")]
        {
            let mut spawned = false;
            for (id, camera, (w, h)) in missing {
                let size = (w.round().max(1.0) as u32, h.round().max(1.0) as u32);
                if let Some(pane) = self.pane_mut(id) {
                    pane.map = Some(crate::map::thread::MapThread::spawn(camera, size));
                    spawned = true;
                }
            }
            if spawned {
                // A thread that has just started has never been told a cadence (#71). Forgetting the
                // last one makes the next pass of `poll` apply it, so a restored wall's maps render at
                // the wall's rate rather than at whatever the pane that started it had.
                self.cadenced = None;
            }
            spawned
        }
        #[cfg(not(feature = "map"))]
        {
            // Nothing to give a pane in a build with no engine: the list is what a wall with one would
            // spawn, and `picture_content` says so in words instead.
            let _ = missing;
            false
        }
    }

    /// Tell every pane's map thread how often to redraw: the focused pane at the wall's cadence, the
    /// others at a quarter of it (#71).
    #[cfg(feature = "map")]
    pub fn apply_cadence(&self) {
        // **Three workspaces are not three walls rendering** (#138): only the active workspace's panes
        // get a cadence; every hidden one is paused, so a switch is the only thing that wakes it.
        for (index, workspace) in self.workspaces.iter().enumerate() {
            let active = index == self.active;
            let focused = workspace.layout.as_ref().map(|layout| layout.focused());
            for pane in &workspace.panes {
                if let Some(map) = &pane.map {
                    map.set_cadence(if !active {
                        crate::map::thread::HIDDEN_INTERVAL
                    } else if Some(pane.id) == focused {
                        crate::map::thread::IDLE_INTERVAL
                    } else {
                        crate::map::thread::UNFOCUSED_INTERVAL
                    });
                }
            }
        }
    }

    /// Close every pane's map thread, in order, before the process leaves (#71).
    ///
    /// **N panes are N contexts.** The engine asserts in `~RunLoop` when a process with a live map exits,
    /// and each `MapThread`'s drop sends its stop and waits — so this is the place a wall with panes
    /// shuts down, and the reason it is a named function rather than a hope about drop order.
    pub fn close_maps(&mut self) {
        #[cfg(feature = "map")]
        for workspace in &mut self.workspaces {
            for pane in &mut workspace.panes {
                // Dropping the handle stops the thread and joins it.
                pane.map = None;
            }
        }
    }

    /// The pane a marker's press belongs to, for the card: the one whose frame projects the hull.
    pub fn pane_showing(&self, id_unit: i64) -> Option<ViewId> {
        self.pane_rects().into_iter().find_map(|(id, _)| {
            let pane = self.pane(id)?;
            let frame = pane.frame?;
            let marker = crate::ui::markers::place(
                &self.picture,
                &frame,
                &pane.view.filter,
                self.now(),
                self.stale_after,
                pane.view.range,
                &|unit| self.sizes.get(&unit).copied(),
            )
            .into_iter()
            .find(|marker| marker.id_unit == id_unit)?;
            let _ = marker;
            Some(id)
        })
    }

    /// The hull the **focused pane** is asking about — what Escape closes and the card scenes show.
    ///
    /// There is no wall-wide selection to read any more (#99): the focus ring says which pane the keys
    /// are in, and this follows it.
    pub fn selection(&self) -> Option<i64> {
        self.selected_in(self.focused())
    }

    /// The hull **one pane** is asking about.
    pub fn selected_in(&self, pane: ViewId) -> Option<i64> {
        self.pane(pane).and_then(|pane| pane.view.selected)
    }

    /// One pane's card, if it has one.
    pub fn card_in(&self, pane: ViewId) -> Option<&crate::units::card::Card> {
        self.pane(pane).and_then(|pane| pane.view.card.as_ref())
    }

    /// **The facility this pane is asking about**, if any (#109).
    pub fn installation_in(&self, pane: ViewId) -> Option<i64> {
        self.pane(pane).and_then(|pane| pane.view.installation)
    }

    /// Select a hull **in one pane**: build that pane's card from the picture and ask the reads thread for
    /// the rest.
    ///
    /// Selecting the same hull twice in the same pane does not re-ask: a card that re-fetches on every press
    /// is a card that flickers between answers. Two panes selecting the same hull ask twice, which is correct
    /// and cheap — they are two questions, and the reads thread is one thread with a queue.
    /// **Follow a hull** (#169): the pane's camera centres that hull's fix every frame, at `zoom` if one is given.
    ///
    /// The fix comes from the picture the wall has, so a hull that has not reported **cannot be followed** — absence
    /// is not zero (`INV-01`) — and the caller says so by name rather than following a guess.
    pub fn follow(&mut self, pane: ViewId, id_unit: i64, zoom: Option<f64>) -> bool {
        let Some(pos) = self
            .picture
            .units()
            .iter()
            .find(|unit| unit.unit.id_unit == id_unit)
            .and_then(|unit| unit.drawn(self.now()))
        else {
            return false;
        };
        let Some(held) = self.pane_mut(pane) else {
            return false;
        };
        if !held.content.is_map() {
            return false;
        }
        held.follow = Some(id_unit);
        // The camera is set now rather than on the next frame, so a capture and a client see the move at once — and
        // the frame with it, because markers project against a frame (`INV-04`) and a follow moves a camera with no
        // gesture to make a new one.
        held.camera = Camera::new(
            (pos.lat, pos.lon),
            zoom.unwrap_or(held.camera.zoom).clamp(
                crate::map::geometry::MIN_ZOOM,
                crate::map::geometry::MAX_ZOOM,
            ),
        );
        if let Some(frame) = held.frame {
            held.frame = Some(crate::map::geometry::Rendered {
                camera: held.camera,
                ..frame
            });
        }
        self.layout_dirty = true;
        true
    }

    /// **One frame of every follow** (#169): the camera of each pane that is locked onto a hull, moved to that
    /// hull's current fix.
    ///
    /// It does **not** go through `set_camera_of` — that would cancel the follow it is serving — and it does not
    /// call `arranged()`, because a follow is not an arrangement: the file keeps the camera the operator left
    /// (#153). Returns whether anything moved, so the loop redraws once.
    pub fn tick_follows(&mut self) -> bool {
        let now = self.now();
        let mut moved = false;
        let follows: Vec<(ViewId, i64, Camera)> = self
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.panes.iter())
            .filter_map(|pane| {
                let unit = pane.follow?;
                Some((pane.id, unit, pane.camera))
            })
            .collect();
        for (pane, unit, camera) in follows {
            let Some(pos) = self
                .picture
                .units()
                .iter()
                .find(|held| held.unit.id_unit == unit)
                .and_then(|held| held.drawn(now))
            else {
                // **A hull that stopped reporting ends the follow** (#169): a camera with nowhere to be is a pane
                // that would otherwise sit still and claim to be tracking.
                if let Some(pane) = self.pane_mut(pane) {
                    pane.follow = None;
                }
                moved = true;
                continue;
            };
            let next = Camera::new((pos.lat, pos.lon), camera.zoom);
            if let Some(held) = self.pane_mut(pane) {
                // **The frame is what the picture is drawn from** (`INV-04`), so it is the thing that must be
                // current: a wall's map thread hands over a frame at this camera, and a capture has no thread, so the
                // frame is re-made here. Comparing the frame as well as the camera is what makes the first `follow`
                // move the hulls and not only the number.
                let stale =
                    held.camera != next || held.frame.is_some_and(|frame| frame.camera != next);
                if stale {
                    held.camera = next;
                    if let Some(frame) = held.frame {
                        held.frame = Some(crate::map::geometry::Rendered {
                            camera: next,
                            ..frame
                        });
                    }
                    moved = true;
                }
            }
        }
        if moved {
            self.layout_dirty = true;
        }
        moved
    }

    pub fn select_in(&mut self, pane: ViewId, id_unit: i64) {
        if self.selected_in(pane) == Some(id_unit) {
            return;
        }
        if let Some(pane) = self.pane_mut(pane) {
            pane.view.selected = Some(id_unit);
            // One pane, one card: a hull's card replaces the facility's (#109).
            pane.view.installation = None;
        } else {
            return;
        }
        self.rebuild_card(pane);
        if let Some(backend) = &self.backend {
            backend.request(crate::data::Command::Unit(id_unit));
            backend.request(crate::data::Command::Commander(id_unit));
            backend.request(crate::data::Command::Specifications(id_unit));
        }
    }

    /// A press on a hull's marker, **in one pane**: select it there, or close that pane's card if it is
    /// already open.
    ///
    /// One click, one meaning — which is why the marker's press and the map's "press the background to
    /// clear" handler do not both decide: pressing a marker records that this press was a marker's, and
    /// the map's handler stands down for it.
    /// A press on a hull: select it, or clear the pane if it is already the one selected (#99).
    ///
    /// **The toggle is the gesture's rule, the selection is the intent's value** (#170): the press decides
    /// *whether* this is a selection or a clearing and says the result to the control plane, which is what a
    /// client without a pointer sends directly.
    pub fn press_marker(&mut self, pane: ViewId, id_unit: i64) {
        self.marker_press = true;
        let unit = (self.selected_in(pane) != Some(id_unit)).then_some(id_unit);
        control::apply(self, Intent::Select { pane, unit }).ok();
    }

    /// A press on the map itself: clear that pane, unless the press was a marker's.
    pub fn press_background(&mut self, pane: ViewId) {
        if !self.marker_press {
            control::apply(self, Intent::Select { pane, unit: None }).ok();
        }
    }

    /// **The frame clock**: the wall's own time, which is virtual in a capture and real on a wall.
    ///
    /// Public because a cursor's fade, a scene's assertion and a test all ask "when is it for the wall?" — and the
    /// answer must be the frame's clock, never `Instant::now()` (`INV-11`).
    pub fn frame_clock(&self) -> f64 {
        self.logical
    }

    /// **Set the frame clock for one scripted step** (#172): a scene's intent file says *when* each step happens,
    /// and a step that changes a cursor's fade must land at its own time rather than at the end of the script.
    ///
    /// `poll` owns this clock; the only other writer is the script runner, and it restores it after each step.
    pub fn set_frame_clock(&mut self, at: f64) {
        self.logical = at;
    }

    /// **Move a client's cursor** (#167), in the picture panel's points.
    ///
    /// A cursor appears with its first move; a wall no client has pointed at has none, and `--self-check` does not
    /// claim one. The frame clock is passed in so this stays a function of the frame like every other age.
    pub fn cursor_move(&mut self, at: (f32, f32)) {
        // The frame clock, which is the only clock a frame may read: a cursor's fade is an age like any other.
        let now = self.logical;
        let at = (
            at.0.clamp(0.0, self.pane_viewport().w as f32),
            at.1.clamp(0.0, self.pane_viewport().h as f32),
        );
        self.cursor = Some(match self.cursor {
            Some(cursor) => Cursor {
                at,
                down: cursor.down,
                moved_at: now,
            },
            None => Cursor {
                at,
                down: false,
                moved_at: now,
            },
        });
        self.layout_dirty = true;
    }

    /// **A client's press** (#167): the hull under its cursor, or the pane it is over, or nothing.
    ///
    /// The button goes down here and the *effect* goes through the control plane like every other input, so a
    /// remote's press and a hand's press are one code path. What a press can reach is the picture panel — a hull
    /// (its card) and the water (a clear, and the pane's focus) — because that is the space this cursor lives in;
    /// the header's chips and a menu's rows are Rune's own layout, and reaching them needs an event injection Rune
    /// does not expose (recorded in `docs/control.md`).
    pub fn cursor_press(&mut self) {
        let now = self.logical;
        let Some(cursor) = self.cursor else {
            return;
        };
        self.cursor = Some(Cursor {
            down: true,
            moved_at: now,
            ..cursor
        });
        let at = (f64::from(cursor.at.0), f64::from(cursor.at.1));
        // The pane under the point, in the panel's own space.
        let Some((id, rect)) = self.pane_rects().into_iter().find(|(_, rect)| {
            at.0 >= f64::from(rect.x)
                && at.0 < f64::from(rect.x + rect.w)
                && at.1 >= f64::from(rect.y)
                && at.1 < f64::from(rect.y + rect.h)
        }) else {
            // A press outside every pane is a press on the panel's gutter, and a gutter is a resize: not a thing
            // a remote can do yet, and not a thing to pretend to do.
            self.layout_dirty = true;
            return;
        };
        let local = (at.0 - f64::from(rect.x), at.1 - f64::from(rect.y));
        let hull = self.hull_at(id, local);
        control::apply(self, Intent::FocusPane(id)).ok();
        match hull {
            Some(unit) => {
                control::apply(
                    self,
                    Intent::Select {
                        pane: id,
                        unit: Some(unit),
                    },
                )
                .ok();
            }
            // The water: clear this pane's card, as a press on the map does (#99).
            None => {
                control::apply(
                    self,
                    Intent::Select {
                        pane: id,
                        unit: None,
                    },
                )
                .ok();
            }
        }
        self.layout_dirty = true;
    }

    /// **A client's release** (#167): the button comes up, and the cursor starts its fade from here.
    pub fn cursor_release(&mut self) {
        let now = self.logical;
        if let Some(cursor) = self.cursor {
            self.cursor = Some(Cursor {
                down: false,
                moved_at: now,
                ..cursor
            });
            self.layout_dirty = true;
        }
    }

    /// **Close every pane in the active workspace but the focused one** (#166).
    ///
    /// One at a time, through [`State::close_pane`]: the refusal, the map thread's join, the surface that was on the
    /// pane and the cadence are that function's, and a bulk shortcut would be a second copy of all four. The
    /// focused pane's workspace only — a wall-wide reset would empty workspaces a remote cannot see.
    ///
    /// Returns how many panes went, so the caller can say whether anything happened.
    pub fn close_panes_but_focused(&mut self) -> Option<usize> {
        // A workspace with no tiling has no focused pane, and no pane to keep: that is a refusal, not a reset.
        self.layout()?;
        let keep = self.focused();
        let doomed: Vec<ViewId> = self
            .active()
            .panes
            .iter()
            .map(|pane| pane.id)
            .filter(|id| *id != keep)
            .collect();
        let mut closed = 0;
        for id in doomed {
            if self.close_pane(id) {
                closed += 1;
            }
        }
        Some(closed)
    }

    /// **Close every workspace but the one on screen** (#166).
    ///
    /// The active workspace is never closed and never emptied: a client that has read the wall knows which one it is
    /// looking at, and one that has not may not take the wall's focus away. A closed workspace's panes **drop**, and
    /// a `Pane`'s map thread is joined by its drop (the rule `close_maps` relies on), so nothing is leaked and
    /// nothing is closed twice.
    ///
    /// Returns how many workspaces went.
    pub fn close_workspaces_but_active(&mut self) -> usize {
        let before = self.workspaces.len();
        // **The index follows the workspace, not the number** (#166): removing one before the kept workspace moves it
        // down, and a `keep` that stayed put would stop pointing at it — and then remove the wall the operator is
        // looking at on the next step.
        let mut keep = self.active;
        let mut index = 0;
        while index < self.workspaces.len() {
            if index == keep {
                index += 1;
                continue;
            }
            // Its panes drop with it, and a `Pane`'s map thread is joined by its drop (the rule `close_maps` relies
            // on) — nothing is leaked, and nothing is closed twice.
            self.workspaces.remove(index);
            if index < keep {
                keep -= 1;
            }
        }
        self.active = keep;
        let closed = before - self.workspaces.len();
        if closed > 0 {
            // The panes that went took their threads; the ones left keep their cadence.
            #[cfg(feature = "map")]
            self.apply_cadence();
            self.arranged();
            self.layout_dirty = true;
        }
        closed
    }

    /// The hull whose marker is within [`Cursor::HIT`] of `local` (a point in `pane`'s own points), if one is.
    ///
    /// It projects the pane's markers exactly as the drawing does — the same `markers::place`, the same filter, the
    /// same clock — so a press lands on the hull that is drawn there rather than on a second opinion about it.
    /// **Absence is not a miss**: a hull with no fix is not in the list, so it cannot be pressed.
    fn hull_at(&self, pane: ViewId, local: (f64, f64)) -> Option<i64> {
        let held = self.pane(pane)?;
        let frame = held.frame?;
        let radius = 1.0
            / if self.scale > 0.0 {
                f64::from(self.scale)
            } else {
                1.0
            };
        crate::ui::markers::place(
            &self.picture,
            &frame,
            &held.view.filter,
            self.now(),
            self.stale_after,
            held.view.range,
            &|unit| self.sizes.get(&unit).copied(),
        )
        .into_iter()
        .map(|marker| (marker.id_unit, marker.x * radius, marker.y * radius))
        .map(|(unit, x, y)| {
            let dx = x - local.0;
            let dy = y - local.1;
            (unit, (dx * dx + dy * dy).sqrt())
        })
        .filter(|(_, distance)| *distance <= Cursor::HIT)
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(unit, _)| unit)
    }

    /// Clear one pane's selection. A pane has one card, so clearing it is clearing the only one there is.
    pub fn clear_selection(&mut self, pane: ViewId) {
        if let Some(pane) = self.pane_mut(pane) {
            pane.view.selected = None;
            pane.view.card = None;
            pane.view.last_age.clear();
            // A facility's card is this pane's card too (#109): closing the card closes both.
            pane.view.installation = None;
        }
    }

    /// Rebuild **one pane's** card from the current picture — on selection, and when its fix changes.
    fn rebuild_card(&mut self, pane: ViewId) {
        let Some(id_unit) = self.selected_in(pane) else {
            if let Some(pane) = self.pane_mut(pane) {
                pane.view.card = None;
            }
            return;
        };
        let Some(view) = self
            .picture
            .units()
            .iter()
            .find(|unit| unit.unit.id_unit == id_unit)
            .cloned()
        else {
            // A selection for a hull the picture no longer names: the card goes with it.
            if let Some(pane) = self.pane_mut(pane) {
                pane.view.card = None;
            }
            return;
        };
        let now = self.now();
        let stale_after = self.stale_after;
        let Some(target) = self.pane_mut(pane) else {
            return;
        };
        let previous = target.view.card.take();
        let mut card = crate::units::card::Card::new(&view, now, stale_after);
        // Keep what the reads already said for THIS hull: re-asking on every fix would make the
        // commander row flicker between answers every few seconds.
        if let Some(previous) = previous
            && previous.unit.id_unit == id_unit
        {
            card.identity = previous.identity;
            card.commander = previous.commander;
            card.specification = previous.specification;
        }
        target.view.card = Some(card);
    }
}

/// **What the wall is doing**, one value (#101).
///
/// Before this, six independent fields — `settings_open`, `menu`, `resizing`, `dragging`, `rename`,
/// `maximised` — allowed fifteen meaningful combinations of which four are reachable. Two of the operator's
/// reports were exactly that: a sheet with no legal way out (#98) and a lift whose drop disagreed with the
/// geometry it would produce (#100).
///
/// Every state here is a thing that holds the **pointer** or the **keyboard**, so the machine's rule is the
/// one an operator already assumes: one gesture at a time, and whatever you start next releases what you
/// were holding. `Maximised` is not here — it is a layout fact (`Layout::maximised`), because it survives
/// every one of these coming and going.
#[derive(Default)]
// One value exists at a time, so the size of the largest variant is one stack slot in `State` — not the
// per-element cost it is when an enum is held in a collection. Boxing `Rename` would make every reader
// dereference it for a saving nothing can spend.
#[allow(clippy::large_enum_variant)]
pub enum Interaction {
    /// Nothing is being held: the wall is the picture, and a press goes wherever it lands.
    #[default]
    Idle,
    /// A pane is in the air with the drop it would make (#70, #84). The pointer is the lift's.
    Lifted(PaneDrag),
    /// A border between two panes is being held (#69). The pointer is the border's.
    Resizing(view::Border),
    /// A pane's menu is open (#77, #81, #83). Passive: it holds nothing, so a press elsewhere closes it.
    Menu {
        pane: ViewId,
        at: (f32, f32),
        /// **Which row's second surface is open** (#140), by its key: the waterfall. `None` is the first
        /// surface only.
        open: Option<&'static str>,
        /// **The row the keyboard is on** (#164): an index into the rows of the surface that is open — the menu,
        /// or the waterfall when one is. `None` is no ring, which is where a pointer-opened menu starts.
        ring: Option<usize>,
    },
    /// A pane's filter panel is open (#111). **The pointer is the panel's**, but it is not modal: the
    /// picture stays visible behind it, and a press outside dismisses it.
    Filtering { pane: ViewId },
    /// A pane is being named (#83). **The keyboard is the sheet's**, and the sheet is modal.
    Renaming(Rename),
    /// The settings sheet is open (#82, #98). Modal, and it holds the pointer.
    Settings,
}

/// A state the wall may be asked to enter. The `Interaction` variants are the states; this is the vocabulary
/// of *requests*, so `begin` can answer yes or no in one place rather than each handler deciding a rule.
// The same reasoning as `Interaction`: a request lives for the length of one call.
#[allow(clippy::large_enum_variant)]
pub enum Begin {
    Lift(PaneDrag),
    Resize(view::Border),
    Menu { pane: ViewId, at: (f32, f32) },
    Filter { pane: ViewId },
    Rename(Rename),
    Settings,
}

impl Begin {
    /// The transition table, in code — `docs/adr/0004-interaction-state.md` is its prose and a test keeps
    /// the two in step.
    ///
    /// The rule is one hold at a time: a gesture that holds the pointer cannot be started while another
    /// holds it, and the modal sheets cannot be entered from behind each other. Re-entering the state you
    /// are in is allowed for the ones that can move (a menu re-asked, a lift re-aimed) and refused for the
    /// ones that cannot (the settings sheet is not two settings sheets).
    pub fn allowed_from(&self, now: &Interaction) -> bool {
        match (self, now) {
            // A gesture starts from nothing, or from a surface that holds nothing and can be dismissed by
            // the press that starts it.
            (Begin::Lift(_) | Begin::Resize(_), Interaction::Idle | Interaction::Menu { .. }) => {
                true
            }
            // A menu opens where nothing has the pointer; from another menu it moves.
            (Begin::Menu { .. }, Interaction::Idle | Interaction::Menu { .. }) => true,
            // A filter panel is a hold, but not a modal one: it opens from nothing or from a menu (which
            // holds nothing), and a second ask moves it. Once it is open the pointer is its.
            (
                Begin::Filter { .. },
                Interaction::Idle | Interaction::Menu { .. } | Interaction::Filtering { .. },
            ) => true,
            // Naming comes from the menu ("Name this pane…") and from the wall, never from behind a sheet.
            (Begin::Rename(_), Interaction::Idle | Interaction::Menu { .. }) => true,
            // The settings sheet replaces a menu — the header's own control is the way in — and replaces
            // itself, so pressing it twice is not two sheets.
            (
                Begin::Settings,
                Interaction::Idle | Interaction::Menu { .. } | Interaction::Settings,
            ) => true,
            _ => false,
        }
    }
}

impl std::fmt::Debug for Interaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The states by name: a `Rename`'s field has no `Debug` of its own, and its caret is not the point.
        let name = match self {
            Interaction::Idle => "Idle",
            Interaction::Lifted(_) => "Lifted",
            Interaction::Resizing(_) => "Resizing",
            Interaction::Menu { .. } => "Menu",
            Interaction::Filtering { .. } => "Filtering",
            Interaction::Renaming(_) => "Renaming",
            Interaction::Settings => "Settings",
        };
        f.write_str(name)
    }
}

/// The socket's state, as the header shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Link {
    /// No credential: the wall reads and does not listen.
    #[default]
    Absent,
    /// A socket is running and the picture is proven current.
    Live,
    /// Connected or reconnecting, but the picture is not proven current (`INV-09`).
    Unproven,
    /// A refusal a human has to fix, or a failure to start.
    Failed(String),
}

/// **A named, independent tiling of panes** (#138). One is active at a time.
///
/// A workspace owns its panes and the layout that tiles them, so two workspaces have two cameras, two
/// focus rings and two tilings. **Empty is valid**: `layout` is `None` and `panes` is empty, and the
/// picture panel says "No panes yet." — a workspace is a place panes *can* be, not a place panes are.
///
/// Pane ids are **not** the layout's: `State::next_pane` mints them for the whole wall, so a pane can be
/// moved between workspaces and keep its id, its map thread and its frame.
pub struct Workspace {
    pub name: String,
    /// The tiling, or `None` when this workspace holds no panes. There is no "empty layout": a layout
    /// with no panes would be a tiling of nothing, which is what `None` says.
    pub layout: Option<Layout>,
    pub panes: Vec<Pane>,
}

impl Workspace {
    /// A workspace with one pane and its tiling: the wall as it opens.
    pub fn with_pane(name: String, pane: Pane) -> Self {
        let layout = Layout::single_with(pane.id);
        Self {
            name,
            layout: Some(layout),
            panes: vec![pane],
        }
    }

    /// **Close a pane here** (#141): a multi-pane tiling collapses into the pane's sibling; the only pane
    /// leaves the workspace **empty**, which is a state (#138) and not an error.
    ///
    /// `Layout` still refuses to hold nothing — a tiling of nothing is `None`, and `None` is the
    /// workspace's — so removing the last pane is this function's job, and disowning the tiling is its
    /// way of removing it.
    pub fn close(&mut self, id: ViewId) -> bool {
        let Some(layout) = self.layout.as_mut() else {
            return false;
        };
        if layout.len() > 1 {
            if !layout.close(id) {
                return false;
            }
        } else if layout.contains(id) {
            self.layout = None;
        } else {
            return false;
        }
        self.panes.retain(|pane| pane.id != id);
        true
    }

    /// A workspace with nowhere to put a pane yet: the state the operator asked for.
    pub fn empty(name: String) -> Self {
        Self {
            name,
            layout: None,
            panes: Vec::new(),
        }
    }
}

/// The whole window.
pub fn view(state: &State, cx: &mut Cx<State>) -> El {
    // The first frame is where the scale is known. `view` gets `&State`, so the camera and the map
    // are reached through interior mutability only where they must be — here, the scale is a plain
    // read for the layout maths.
    let scale = cx.env().scale;
    let units = if state.picture.is_empty() {
        "no picture yet".to_string()
    } else {
        match state.picture.len() {
            1 => "1 unit".to_string(),
            n => format!("{n} units"),
        }
    };
    // The pane count is in the header as soon as there is more than one (#71): a tiled wall should say
    // how many pictures it is showing, and "panes=3" next to "127 units" is the line a reader needs when
    // one of them is not drawing.
    let units = if state.pane_count() > 1 {
        format!("{units} · panes={}", state.pane_count())
    } else {
        units
    };
    // A wall left full size is a line in a file, not a mystery in a photograph (#85).
    let units = match state.maximised() {
        Some(id) => format!("{units} · max={id}"),
        None => units,
    };
    let (link, link_colour) = match &state.link {
        Link::Live => (
            format!("live · {units} · {} fixes", state.fixes),
            tokens::accent(),
        ),
        Link::Unproven => (format!("reconnecting · {units}"), tokens::ink_dim()),
        Link::Absent => (units, tokens::ink_dim()),
        Link::Failed(error) => (format!("socket down: {error}"), tokens::ink_dim()),
    };

    // A banner that does not cover the map: it is a row above the map, so the picture is never
    // obscured while the socket is reconnecting — the wall's whole job is the picture.
    let banner = match (&state.link, state.headless) {
        // A scene has no socket to be wrong about.
        (_, true) => None,
        (Link::Live, _) => None,
        (Link::Absent, _) => {
            Some("no socket: this wall is read-only until a credential is configured")
        }
        (Link::Unproven, _) => {
            Some("connecting: no new fixes yet, the picture is what was last read")
        }
        (Link::Failed(error), _) => Some(error.as_str()),
    };

    let header = row()
        .w_full()
        .gap(tokens::GAP)
        // **The active workspace's name, where the wordmark was** (#138), and **the thing you click to
        // rename it** (#139): a title is a rename's door on this wall, the same rule the pane's plate
        // follows (#83, #113). The app's own name stays the *window* title (`TITLE`), which is where a
        // window manager looks for it.
        .child(
            text(workspace_title(state.active(), state.active))
                .key("workspace-title")
                .font(tokens::WORDMARK)
                .color(tokens::accent())
                .help("Name this workspace")
                .on_press(|state: &mut State| {
                    let index = state.active;
                    control::apply(state, Intent::BeginRename(RenameTarget::Workspace(index))).ok();
                }),
        )
        .child(workspace_chips(state))
        .child(spacer())
        .child(
            text(link)
                .key("picture-status")
                .font(tokens::LABEL)
                .color(link_colour),
        )
        // **The header's controls, in words** — everything else on the wall is the picture. A word,
        // because there is room for one here and a word needs no catalogue to read (#82); the pane
        // chrome's icons say their meaning in help text for the same reason (#111).
        //
        // The tree control sits beside `settings` and **splits the layout** the way `Focus on new pane`
        // does (#116): one press opens the chain of command in a pane of its own.
        .child(
            text(TREE_LABEL)
                .key("tree-open")
                .font(tokens::LABEL)
                .color(tokens::ink_dim())
                .on_press(|state: &mut State| {
                    control::apply(
                        state,
                        Intent::OpenPane(PaneContent::Tree { organisation: None }),
                    )
                    .ok();
                }),
        )
        // The newsroom control (#136), in the same row and the same shape as the tree's: a word, and a
        // pane of its own. The operator's ask was a "newsroom" button on the topbar.
        .child(
            text(NEWSROOM_LABEL)
                .key("newsroom-open")
                .font(tokens::LABEL)
                .color(tokens::ink_dim())
                .on_press(|state: &mut State| {
                    control::apply(state, Intent::OpenPane(PaneContent::Newsroom)).ok();
                }),
        )
        .child(
            text(SETTINGS_LABEL)
                .key("settings-open")
                .font(tokens::LABEL)
                .color(tokens::ink_dim())
                .on_press(|state: &mut State| {
                    control::apply(state, Intent::OpenSettings).ok();
                }),
        );

    // The banner is a row of its own, and only when there is something to say: a flexible spacer
    // here would take half the window away from the map, which is exactly what a wall must not lose.
    let mut root = col()
        .w_full()
        .h_full()
        .padding(tokens::PAD)
        .gap(tokens::GAP)
        .fill(tokens::bg())
        .child(header);
    if let Some(message) = banner {
        root = root.child(
            text(message)
                .key("reconnect-banner")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }
    // **Only when open**: an empty child is still a child, and the root's own gap would take 8 pt off the
    // picture to lay it out — which is what a capture noticed (#82) before a golden was regenerated.
    let mut root = root.child(picture(state, scale));
    // The filter panel is a popover, not a child of the pane: a pane clips its picture, so an overlay
    // inside it would be cut off at the pane's edge and a panel next to the right-hand pane would be a
    // sliver. It is attached here, after its anchor, so Rune can place it beside the toggle.
    root = filter_panel(state, root);
    if state.settings_open() {
        root = root.child(settings_sheet(state, scale));
    }
    if state.rename().is_some() {
        root = root.child(rename_sheet(state));
    }
    root
}

/// What the header's one control says. A word, because there is room for one and a word needs no catalogue
/// to read (#82); the pane chrome's icons carry their meaning in help text (#111).
pub const SETTINGS_LABEL: &str = "settings";

/// What the header's tree control says (#116): a word, for the same reason the control beside it is one.
pub const TREE_LABEL: &str = "chain of command";

/// What the header's newsroom control says (#136). A word, like the two beside it.
pub const NEWSROOM_LABEL: &str = "newsroom";

/// What an empty workspace says, where the picture would be (#138): a state, not an error and not a
/// placeholder for a read that failed.
pub const NO_PANES: &str = "No panes yet.";

/// What a workspace is called until somebody names it (#138, #139): its number, from its place on the
/// wall. The one place that form is written — the chips, an empty rename and `+` all come through here.
pub fn default_workspace_name(index: usize) -> String {
    format!("Workspace {}", index + 1)
}

/// What the workspace's name shows in the header: the typed name, cut to the pane name's own limit with
/// the same drawn ellipsis, and the default when nobody has named it.
pub fn workspace_title(workspace: &Workspace, index: usize) -> String {
    shown_name(&workspace.name).unwrap_or_else(|| default_workspace_name(index))
}

/// The workspace chips and the `+` (#138): a selector, in the header, after the workspace's name.
///
/// A chip is Rune's workspace glyph and its number; the active one is raised (the name plate's fill and
/// the focus edge, the ink of the words on it), the others are dim. The a11y shape is a tab list, because
/// that is what the operator is choosing between — a `rune-kit::TabStrip` is for document tabs with drag
/// and tear-out, none of which a workspace selector wants, so this borrows the visuals and not the widget.
fn workspace_chips(state: &State) -> El {
    let mut chips = row()
        .key("workspace-chips")
        .h(tokens::CHIP_H)
        .items_center()
        .gap(tokens::CHIP_GAP)
        .role("tablist")
        .label("Workspaces");
    for index in 0..state.workspaces.len() {
        let active = index == state.active;
        let ink = if active {
            tokens::ink()
        } else {
            tokens::ink_dim()
        };
        chips = chips.child(
            row()
                .key(format!("workspace-chip-{index}"))
                .h(tokens::CHIP_H)
                .px(tokens::CHIP_PAD)
                .radius(tokens::CHIP_H / 2.0)
                .items_center()
                .gap(tokens::CHIP_PAD / 2.0)
                .fill(if active {
                    tokens::name_plate()
                } else {
                    tokens::clear()
                })
                .border(
                    tokens::HAIRLINE,
                    if active {
                        tokens::focus_edge()
                    } else {
                        tokens::hairline()
                    },
                )
                .role("tab")
                .checked(active)
                .label(state.workspaces[index].name.clone())
                .on_press(move |state: &mut State| {
                    control::apply(state, Intent::FocusWorkspace(index)).ok();
                })
                // **Name a workspace that is not on screen** (#154): a right-click on a chip opens the
                // rename sheet for **that** chip, without switching to it. The gesture is the pane menu's
                // (a secondary press names a thing), the sheet is #139's, and the target is the one thing
                // `RenameTarget::Workspace` already carried — the index, not the active workspace.
                //
                // No menu in between: a one-row surface over a chip would be a second thing to learn for
                // a thing the title already teaches, and `Interaction::Menu` is anchored to a pane.
                .on_secondary_press(move |state: &mut State, _at, _tx: &mut rune::Tx| {
                    control::apply(state, Intent::BeginRename(RenameTarget::Workspace(index))).ok();
                })
                .help("Rename this workspace")
                .child(
                    rune::icon(&icons::WORKSPACE, tokens::CHIP_ICON)
                        .key(format!("workspace-chip-icon-{index}"))
                        .color(ink),
                )
                .child(
                    text(format!("{}", index + 1))
                        .key(format!("workspace-chip-label-{index}"))
                        .font(tokens::LABEL)
                        .color(ink),
                ),
        );
    }
    chips.child(
        rune::icon(&icons::WORKSPACE_NEW, tokens::CHIP_ICON)
            .key("workspace-new")
            .color(tokens::ink_dim())
            .help("New workspace")
            .on_press(|state: &mut State| {
                control::apply(state, Intent::NewWorkspace).ok();
            }),
    )
}

/// The header's newsroom control (#136): **the feed in a pane of its own**, focused, asking page 1.
///
/// The same path the tree control takes — split the focused pane, give the new one its content, ask its
/// first read — so a one-pane wall gets its first newsroom the way a split wall gets another, and a
/// second newsroom pane sits on its own tab and its own page.
pub(crate) fn open_newsroom_pane(state: &mut State) {
    open_pane_focused(state, PaneContent::Newsroom);
}

/// Open a pane of this kind beside the focused one (#170): the header's two controls and a CLI's
/// `pane.open` are one function, so a pane opened from a socket asks its reads the way a pressed button does.
pub(crate) fn open_pane_focused(state: &mut State, content: PaneContent) -> Option<ViewId> {
    let target = state.focused();
    open_pane_beside(state, target, content)
}

/// Open a pane of this kind beside `target`, asking whatever that kind needs on the frame it is made.
///
/// **An empty workspace has nothing to split** (#141's state), so the pane it opens is that workspace's first
/// rather than a silent no-op. Before #165 the header's tree and newsroom controls did nothing there — which
/// looks exactly like a broken button — and a CLI found it in the first minute.
pub(crate) fn open_pane_beside(
    state: &mut State,
    target: ViewId,
    content: PaneContent,
) -> Option<ViewId> {
    let camera = state.camera_of(target);
    let id = if state.layout().is_none() {
        let id = state.allocate_pane();
        let pane = match content.clone() {
            PaneContent::Map => Pane::new(id, camera),
            PaneContent::Tree { organisation } => Pane::tree(id, camera, organisation),
            PaneContent::Newsroom => Pane::newsroom(id, camera),
        };
        let workspace = state.active_mut();
        workspace.layout = Some(Layout::single_with(id));
        workspace.panes.push(pane);
        state.focus_pane(id);
        id
    } else {
        state.split_pane_as(target, camera, content.clone())?
    };
    match content {
        PaneContent::Map => {}
        PaneContent::Tree { .. } => ask_tree_reads(state),
        PaneContent::Newsroom => ask_newsroom_page_one(state, id),
    }
    state.arranged();
    state.layout_dirty = true;
    Some(id)
}

/// The newsroom's first page, asked on the frame the pane opens: the reply lands between frames, so the pane
/// draws "reading the feed…" until it does — never a blank and never a wait.
fn ask_newsroom_page_one(state: &mut State, id: ViewId) {
    let query = state
        .pane_mut(id)
        .and_then(|pane| pane.news.as_mut())
        .map(|view| view.ask());
    match (query, &state.backend) {
        (Some(query), Some(backend)) => {
            backend.request(crate::data::Command::Stories(query));
        }
        // **A wall with no reads thread says so, in the pane.** "reading the feed…" forever is a
        // lie about a read nobody is performing, and the fix (a credential) is not on the wall.
        (_, None) => {
            if let Some(view) = state.pane_mut(id).and_then(|pane| pane.news.as_mut()) {
                view.state = newsroom::ReadState::Failed(
                    "this wall has no reads thread: no credential is configured".to_string(),
                );
            }
        }
        (None, _) => {}
    }
}

/// Ask for the reads a tree pane needs — the list of structures, each structure's forest, and the
/// supreme command — **once each**, off the UI thread.
///
/// Nothing here waits: the commands go on the reads thread's channel and the answers arrive as replies
/// between frames. A read that fails is one log line and no second ask, the rule the tactical-object
/// manifest keeps: a refresh is deliberate, not per frame.
fn ask_tree_reads(state: &mut State) {
    let mut wanted = Vec::new();
    let mut any_tree = false;
    for pane in state.panes() {
        if let PaneContent::Tree { organisation } = &pane.content {
            any_tree = true;
            match organisation {
                Some(id) => wanted.push(*id),
                None => wanted.extend(state.organisations.iter().map(|row| row.id)),
            }
        }
    }
    if !any_tree {
        return;
    }
    let mut commands = Vec::new();
    // The list first: `None` means "every structure the wall knows", and it knows none until this read.
    if state.organisations.is_empty() && !state.asked_organisations_list {
        state.asked_organisations_list = true;
        commands.push(crate::data::Command::UnitOrganisations);
    }
    if state.tni_command.is_none() && !state.asked_tni {
        state.asked_tni = true;
        commands.push(crate::data::Command::TniCommander);
    }
    for id in wanted {
        if !state.organisation_trees.contains_key(&id) && state.asked_organisations.insert(id) {
            commands.push(crate::data::Command::UnitOrganisation(id));
        }
    }
    if let Some(backend) = &state.backend {
        for command in commands {
            backend.request(command);
        }
    }
}

/// What the settings sheet calls its one setting, and what the row says when it is on and off.
pub const SETTING_GAPS: &str = "Gaps between panes";
pub const SETTING_ON: &str = "on";
pub const SETTING_OFF: &str = "off";
/// The way out of the sheet, in words. A key nobody was told about is not a way out (#98).
pub const SETTING_CLOSE: &str = "Close";

/// Close the settings sheet, from any of its three exits: the row, the scrim, Escape.
fn close_settings(state: &mut State) {
    // Only the sheet closes: a rename that was somehow open behind it must not be ended by this.
    if state.settings_open() {
        state.end();
    }
}

/// Open the settings sheet (#82) as the control plane's `surface.settings` (#170): the header's word and a
/// client's intent must open the same sheet, through the same machine (#101) — a sheet opened by assignment is a
/// sheet the machine does not know is there.
pub(crate) fn open_settings(state: &mut State) -> bool {
    let opened = state.begin(Begin::Settings);
    if opened {
        state.layout_dirty = true;
    }
    opened
}

/// **Dismiss one surface**, the ladder `Escape` walks (#140, #170).
///
/// One surface at a time, and in order: the waterfall before the menu, the menu before the interaction, the
/// maximised pane before the newsroom's detail, and the selection last. The order is the point — a key that
/// closed the most »outer« thing available would need a rule per surface, and this ladder is that rule.
///
/// Returns whether anything was dismissed, so a key handler knows whether it consumed the press.
pub(crate) fn close_surfaces(state: &mut State) -> bool {
    if !matches!(state.interaction, Interaction::Idle) {
        // **A waterfall closes one surface at a time** (#140): Escape takes the submenu first and the menu
        // on the next press, which is the same "one thing at a time" rule the rest of the ladder keeps.
        if let Interaction::Menu {
            pane,
            at,
            open: Some(_),
            ..
        } = state.interaction
        {
            state.interaction = Interaction::Menu {
                pane,
                at,
                open: None,
                ring: None,
            };
            state.layout_dirty = true;
            return true;
        }
        state.end();
        return true;
    }
    if state.maximised().is_some() {
        restore_full_size(state);
        return true;
    }
    let focused = state.focused();
    // The newsroom's detail first: it is a mode of the pane, and `back` is the same function the word
    // calls, so Escape and the button cannot disagree about what closing a story does.
    if state
        .pane_mut(focused)
        .and_then(|pane| pane.news.as_mut())
        .is_some_and(|view| view.back())
    {
        state.layout_dirty = true;
        return true;
    }
    if state.selected_in(focused).is_none() && state.installation_in(focused).is_none() {
        return false;
    }
    state.clear_selection(focused);
    true
}

/// The settings sheet (#82): the wall's own preferences, as a modal over the picture.
///
/// **A modal, not a panel in the flow**: the tiling is the thing being chosen for, so it has to stay
/// visible behind the sheet — and Rune's own modal stack is what makes the wall behind it inert, keeps the
/// focus in the sheet and closes it on a press outside, rather than a hand-rolled equivalent that would
/// get one of the three wrong.
fn settings_sheet(state: &State, scale: f32) -> El {
    let viewport = state.pane_viewport();
    let _ = scale;
    let (w, h) = tokens::SHEET_SIZE;
    let at = view::sheet_rect(viewport, tokens::SHEET_SIZE);
    let gaps_on = state.settings.pane_gap > 0.0;
    // The row is the control: pressing it flips the setting, which is what a switch on a wall with no
    // pointer-driven chrome has to be. It says its state in words, so a capture can assert the words.
    let gaps_row = row()
        .key("setting-gaps")
        .w_full()
        .gap(tokens::GAP)
        .items_center()
        .child(
            text(SETTING_GAPS)
                .font(tokens::LABEL)
                .color(tokens::ink())
                .grow(1.0),
        )
        .child(
            text(if gaps_on { SETTING_ON } else { SETTING_OFF })
                .key("setting-gaps-value")
                .font(tokens::LABEL)
                .color(if gaps_on {
                    tokens::accent()
                } else {
                    tokens::ink_dim()
                }),
        )
        .on_press(|state: &mut State| toggle_pane_gap(state));
    // The way out, as a row like the setting above it: same height, same words-not-icons rule. Pressing it
    // closes the sheet — the one exit the operator asked for (#98).
    let close = row()
        .key("settings-close")
        .w_full()
        .gap(tokens::GAP)
        .items_center()
        .child(
            text(SETTING_CLOSE)
                .font(tokens::LABEL)
                .color(tokens::accent())
                .grow(1.0),
        )
        .on_press(|state: &mut State| close_settings(state));
    let sheet = col()
        .key("settings-sheet")
        // Swallows a press that lands on the sheet rather than on one of its rows: without it the press
        // would reach the scrim underneath and close the sheet the operator was pressing in (#98).
        .on_press(|_: &mut State| {})
        .absolute(at.x, at.y)
        .size(w, h)
        .padding(tokens::PAD)
        .gap(tokens::GAP)
        .radius(tokens::PANEL_RADIUS)
        .fill(tokens::panel())
        .border(tokens::HAIRLINE, tokens::card_edge())
        .child(text("Settings").font(tokens::TITLE).color(tokens::ink()))
        .child(gaps_row)
        .child(
            text("saved to settings.toml, beside config.toml")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        )
        // A spacer, because the way out belongs at the bottom of the sheet and the tiling behind it must
        // stay visible: the sheet keeps its box and the row sits at the foot of it.
        .child(el().grow(1.0))
        .child(close);
    // The scrim: full-window, behind the sheet, and the press that dismisses.
    //
    // **`modal` alone does not dismiss this sheet.** Rune dismisses a modal when a press reaches something
    // *outside* the modal's node (`runtime/input/pointer.rs`), and here the modal node is the full-window
    // scrim with the sheet inside it: every press is on the modal's own surface, so nothing is ever
    // "outside" and the callback cannot fire. That is why the sheet had no way out but Escape (#98). The
    // scrim's own press does the work, and `modal` still earns its keep by making the wall behind inert.
    el().key("settings-scrim")
        // **Pinned, not sized**: a child in the flow takes height away from the picture, so opening the
        // sheet would reflow the wall it is a preference about. Pinned to all four edges, it covers the
        // root's box and the picture underneath is untouched.
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(tokens::scrim())
        .modal(true, |state: &mut State| close_settings(state))
        // Rune hands a press to the **nearest element under the pointer with a primary handler**, and only
        // there (#80) — so the scrim's press is a press on the scrim, and the sheet's own press below is how
        // a press *inside* the sheet is told apart from one outside it without a second copy of the box.
        .on_press(|state: &mut State| close_settings(state))
        .child(sheet)
}

/// The rename sheet (#83): a field over the wall, with the pane it names said in words.
///
/// A modal for the same three reasons the settings sheet is one. The field is Rune's own `TextEdit`, reached
/// through `on_edit`, so the caret, the selection, the input method and the keyboard all behave the way an
/// operator's keyboard expects — a hand-rolled `String` would be a caret drawn by us and an input method
/// that does not work.
fn rename_sheet(state: &State) -> El {
    let Some(rename) = state.rename() else {
        return el();
    };
    let width = state.panel.map(|(w, _)| w).unwrap_or(tokens::WINDOW_SIZE.0);
    let height = state.panel.map(|(_, h)| h).unwrap_or(tokens::WINDOW_SIZE.1);
    let (w, h) = tokens::SHEET_SIZE;
    let field = rune::text_input(&rename.edit, tokens::LABEL)
        .key("rename-field")
        .single_line()
        .w_full()
        .h(tokens::NAME_H + tokens::GAP)
        .px(tokens::GAP)
        .fill(tokens::bg())
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::HAIRLINE, tokens::card_edge())
        .on_edit(|state: &mut State| {
            // The sheet is the only thing that can be renaming, so the field is unambiguous.
            let rename = state.renaming_mut().expect("a rename sheet has a field");
            &mut rename.edit
        })
        .on_submit(|state: &mut State, _tx: &mut rune::Tx| commit_rename(state));
    let sheet = col()
        .key("rename-sheet")
        .absolute((width - w) / 2.0, (height - h) / 2.0)
        .size(w, h)
        .padding(tokens::PAD)
        .gap(tokens::GAP)
        .radius(tokens::PANEL_RADIUS)
        .fill(tokens::panel())
        .border(tokens::HAIRLINE, tokens::card_edge())
        .child(
            text(rename.title())
                .font(tokens::TITLE)
                .color(tokens::ink()),
        )
        .child(field)
        .child(
            text("Enter names it, Escape leaves it as it was")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    el().key("rename-scrim")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(tokens::scrim())
        .modal(true, |state: &mut State| {
            state.end();
            state.layout_dirty = true;
        })
        .child(sheet)
}

/// What the filter panel is called. Its rows are the backend's (#112), not a list written here.
pub const FILTER_TITLE: &str = "Filter";

/// The filter panel (#111, #112): a popover anchored to a pane's filter toggle, **not a modal**.
///
/// A filter is a choice about the picture, so the picture has to stay visible while it is made — the
/// scrim a sheet draws would hide the water the operator is choosing for. The panel is nevertheless a
/// hold: a transparent catcher behind it makes the press outside dismiss it and do nothing else, which
/// is what `Interaction::Filtering` records and `tests/transitions.rs` checks.
///
/// It is attached at the root rather than inside the pane because a pane clips its picture; a panel
/// hanging outside its pane's right edge would be cut off, and the right-hand pane is exactly where the
/// toggle sits nearest the window's edge.
///
/// **The rows are a projection of the backend's vocabulary** (`filter::rows`): three layer switches, the
/// facility types from `/installation-types`, and the domains and categories from
/// `/tactical-objects/facets` with the server's counts. The panel **scrolls inside itself** rather than
/// growing past the pane, because a vocabulary's length is not ours to decide and a panel that grew over
/// the water would hide the thing it filters.
fn filter_panel(state: &State, body: El) -> El {
    let Some(pane) = state.filtering() else {
        return body;
    };
    let rows = state
        .pane(pane)
        .map(|pane| {
            filter::rows(
                &pane.view.filter,
                state.facets.as_ref(),
                &state.installation_types,
                &state.installations,
            )
        })
        .unwrap_or_default();
    let (w, h) = tokens::filter_size(rows.len());
    let panel = col()
        .key("filter-panel")
        // Anchored to the toggle's own key, below it and right-aligned: the panel grows towards the
        // water, not over the hull the operator is about to filter.
        .anchored(
            filter_anchor(state, pane),
            rune::Side::Bottom,
            rune::Align::End,
            tokens::GAP,
        )
        .size(w, h)
        .padding(tokens::PAD)
        .gap(tokens::GAP)
        .radius(tokens::PANEL_RADIUS)
        .fill(tokens::bg())
        .border(tokens::HAIRLINE, tokens::card_edge())
        // A press that lands on the panel is not a press outside it: without this it would reach the
        // catcher below and close the panel the operator was reading.
        .on_press(|_: &mut State| {})
        .child(text(FILTER_TITLE).font(tokens::TITLE).color(tokens::ink()))
        .child(
            col()
                .key("filter-rows")
                .w_full()
                .grow(1.0)
                // A scroll viewport needs a bounded height in every flex ancestor, or it grows to its
                // content and never overflows (`El::scroll`'s own rule).
                .min_h(tokens::FILTER_SCROLL_MIN_H)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .children(rows.iter().map(|entry| filter_row(pane, entry))),
        );
    body.child(
        // The catcher is the panel's parent and the press that dismisses it. It is transparent and not
        // `modal`: the wall is dimmed by nothing and inert only for the pointer, which is the hold the
        // filter claims.
        el().key("filter-catcher")
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            // **Dismissing one surface is an intent** (#173): the panel's catcher walks the same ladder Escape
            // does, so a client's `surface.close` and a press on the wall's glass cannot disagree.
            .on_press(|state: &mut State| {
                control::apply(state, Intent::CloseSurfaces).ok();
            })
            .child(panel),
    )
}

/// One row of the filter panel: a label from the vocabulary, its count and state, and the press that
/// flips **this pane's** filter.
///
/// The press calls [`filter::toggle_in`], the one function behind every row, so a row cannot be added
/// that changes the wall instead of the pane. The state is also in words (`on` / `off`), not only in the
/// label's colour: a wall is read from across a room.
fn filter_row(pane: ViewId, entry: &filter::Row) -> El {
    let target = entry.target;
    let label_ink = if entry.hidden {
        tokens::ink_dim()
    } else {
        tokens::ink()
    };
    let value_ink = if entry.hidden {
        tokens::ink_dim()
    } else {
        tokens::accent()
    };
    let indent = if entry.depth > 0 {
        tokens::FILTER_INDENT
    } else {
        0.0
    };
    row()
        .key(format!("filter-row-{}", entry.key))
        .h(tokens::FILTER_ROW)
        .w_full()
        .items_center()
        .gap(tokens::GAP)
        .px(indent)
        .on_press(move |state: &mut State| {
            filter::toggle_in(state, pane, target);
        })
        .child(
            text(entry.label.clone())
                .font(tokens::LABEL)
                .color(label_ink)
                .grow(1.0),
        )
        .child(text(entry.value()).font(tokens::LABEL).color(value_ink))
}

/// The key of the filter toggle the panel anchors to. A maximised pane draws a second toggle over the
/// tiled one, and Rune anchors to the first node with the key: without the `-max-` key the panel would
/// hang off the hidden toggle in the slot behind the scrim instead of the one the operator pressed.
fn filter_anchor(state: &State, pane: ViewId) -> String {
    if state.maximised() == Some(pane) {
        format!("filter-max-{pane}")
    } else {
        format!("filter-{pane}")
    }
}

/// A press on the filter toggle: open the panel, or close it if this pane's is already open.
fn toggle_filter(state: &mut State, pane: ViewId) {
    state.focus_pane(pane);
    if state.filtering() == Some(pane) {
        state.end();
    } else {
        state.begin(Begin::Filter { pane });
    }
    state.layout_dirty = true;
}

/// The gap switch: `on` is a real gap, `off` is panes that share a border — the look the wall had before
/// settings existed, so the default draws what a wall drew yesterday.
///
/// The file is written **after** the picture changes, and a failed write is a log line: a read-only
/// configuration directory must not leave the operator unable to change the gap for this run.
fn toggle_pane_gap(state: &mut State) {
    state.settings.pane_gap = if state.settings.pane_gap > 0.0 {
        0.0
    } else {
        tokens::GAP
    };
    // The panes' drawn rectangles changed, so the maps have to render at the new size: the layout pass
    // calls `pane_laid_out`, which resizes each thread (#71, and #82's note about a blurred pane).
    state.layout_dirty = true;
    save_settings(state);
}

/// Write the settings file, and say so in the log — one line, never a fatal error.
fn save_settings(state: &State) {
    if state.headless {
        // A scene has no settings file: a capture must not write to the machine it is rendered on, and
        // `INV-11` would be a promise about the filesystem.
        return;
    }
    let path = crate::config::settings_file();
    match state.settings.write(&path) {
        Ok(()) => tracing::info!("settings: wrote {}", path.display()),
        Err(error) => tracing::warn!("settings: could not write {error}"),
    }
}

/// The map area: one panel, and the panes tiled inside it (#68).
///
/// The rectangles come from [`Layout::rects`] — the same function `verify` calls on a capture — and the
/// panel's own size from its `on_layout`, so the panes cover the panel exactly and a pane cannot be
/// drawn in one place and asserted in another. Until the first layout pass the viewport is the window's,
/// which is the size a capture's frames have always been.
///
/// **One pane is the wall as it was, byte for byte.** No wrapper, no clip, no ring: a wall with one
/// pane has no neighbour to be clipped away from and no second pane to be distinguished from, so the
/// element tree — and therefore the goldens and `INV-12`'s frame cost — stay exactly what they were.
/// Multi View adds the wrapper, and `#71`'s golden pass decides whether one pane should clip too.
fn maximised(state: &State, scale: f32, body: El) -> El {
    let Some(id) = state.maximised() else {
        return body;
    };
    let Some(pane) = state.pane(id) else {
        return body;
    };
    let viewport = state.pane_viewport();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: viewport.w as f32,
        h: viewport.h as f32,
    };
    // The frame the pane's markers project against **here**: the window's rectangle, and nothing else. A
    // projected hull is a function of the camera *and the frame*, so a maximised pane whose markers kept
    // the slot's frame would draw them at the slot's scale on a picture three times the size.
    let frame = view::synthetic_frame(pane.camera, rect, scale);
    let mut sheet = el()
        .key("max-pane")
        .absolute(0.0, 0.0)
        .size(rect.w, rect.h)
        .radius(tokens::PANEL_RADIUS)
        .fill(tokens::panel())
        .clip_rounded(0.0)
        .border(tokens::HAIRLINE, tokens::focus_edge())
        // The pane's map renders at **this** size while it is full size, and at its slot's again after
        // (#85): a pane that keeps rendering at its slot and is drawn at the window's is a blurred picture.
        .on_layout(move |state: &mut State, [_, _, width, height]| {
            state.scale = if scale > 0.0 { scale } else { 1.0 };
            resize_map(state, id, (width, height), scale);
        });
    // **A full-size pane is the pane, not a photograph of it**: the wheel, a right-click, a press and a drag
    // all reach it here. They are the wall's own handlers and not copies, because the frame each of them asks
    // the map for is the frame the map is rendering — which, while the pane is full size, is the window's.
    #[cfg(feature = "map")]
    {
        sheet = sheet
            .on_press(move |state: &mut State| press_in(state, id))
            .on_drag(move |state: &mut State, drag, _tx| drag_map(state, id, drag))
            .on_wheel(move |state: &mut State, event: rune::WheelEvent, _tx| {
                wheel(state, id, &event)
            })
            .on_secondary_press_local(move |state: &mut State, event: rune::PointerEvent, _tx| {
                open_menu(state, id, &event, scale)
            });
    }
    // **A full-size tree pane is the tree, at the window's size** (#116): no map, no markers, and no
    // synthetic frame — the same content the pane draws in its slot, over a bigger rectangle.
    if pane.content.is_map() {
        // On a wall the sheet shows the pane's own pixels, live: not a snapshot, not a second thread. A
        // scene has no map and draws the markers on the panel, which is the same picture minus the sea.
        #[cfg(feature = "map")]
        if let Some(map) = pane.map.as_ref()
            && map.error().is_none()
        {
            sheet = sheet.child(
                rune::el()
                    .key("max-map")
                    .w_full()
                    .h_full()
                    .pixels(map.pixels()),
            );
        }
        sheet = sheet.child(projections(state, id, &frame, scale));
    } else if let Some(tree) = tree::tree_for(state, id) {
        sheet = sheet.child(tree::content(state, id, rect, &tree));
    } else if pane.content == PaneContent::Newsroom {
        // A full-size newsroom pane is the same list over the window's rectangle: the rows the pane
        // draws, not a snapshot of them.
        sheet = sheet.child(newsroom::content(state, id));
    }
    sheet = sheet.child(name_plate(pane));
    // The same chrome row, and the maximise toggle says "back to the wall" because that is what the
    // press will do (#111). It is the pane's own toggle, not a second control invented for this shape.
    sheet = sheet.child(pane_chrome(state, id, rect));
    sheet = sheet.child(close_plate());
    // The wall behind, dimmed but still there: the other panes keep their cameras and their cadence, and
    // a reader can see what the full-size pane is one of.
    body.child(
        el().key("max-scrim")
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .fill(tokens::scrim())
            .child(sheet),
    )
}

/// The way back from a full-size pane, in words (#85, #111): a wall has no title bar to close, and a
/// word beside an arrow is the meaning the arrow cannot say on its own.
///
/// **Anchored to the top-right**, so the words fit whatever they say: an absolute x derived from the
/// label's own width is a plate that clips itself the first time the words change. Below the chrome
/// row, because the icons own the corner.
fn close_plate() -> El {
    text(MAX_CLOSE)
        .key("max-close")
        .font(tokens::LABEL)
        .color(tokens::ink())
        .pin(
            None,
            Some(tokens::GAP / 2.0 + tokens::PANE_BUTTON + tokens::GAP / 2.0),
            Some(tokens::GAP),
            None,
        )
        .h(tokens::NAME_H)
        .px(tokens::GAP)
        .radius(tokens::NAME_H / 2.0)
        .fill(tokens::name_plate())
        .border(tokens::HAIRLINE, tokens::focus_edge())
        .on_press(|state: &mut State| {
            if let Some(pane) = state.maximised() {
                control::apply(state, Intent::ToggleFullSize(pane)).ok();
            }
        })
}

/// **The client's cursor, drawn** (#167): a ring at the point, filled while the button is down.
///
/// It lives in the picture panel's space, so its point is the point a press means — and it draws **over** the
/// panes, the gutters and a held surface, because a pointer is above the wall it is pointing at.
///
/// A cursor that has not moved for [`Cursor::FADE`] is not drawn: `visible_at` is the whole rule, on the frame
/// clock.
fn cursor_glyph(state: &State) -> Option<El> {
    let cursor = state.cursor?;
    if !cursor.visible_at(state.logical) {
        return None;
    }
    let half = tokens::CURSOR / 2.0;
    // **A colour of its own** (#167): a pointer is not content, and white is nothing else on this wall — so a
    // capture can ask "is the cursor in the picture?" without guessing where it should be.
    let ink = tokens::cursor_ink();
    Some(
        el()
            // **Keyed by its point** (#167): a keyed element keeps its layout across frames, so a cursor that
            // moved under one key would be drawn where it used to be. One key per point means the ring follows
            // the point, and a still cursor still reuses one element.
            .key(format!(
                "client-cursor-{}-{}",
                cursor.at.0.round() as i32,
                cursor.at.1.round() as i32
            ))
            .absolute(cursor.at.0 - half, cursor.at.1 - half)
            .size(tokens::CURSOR, tokens::CURSOR)
            .radius(tokens::CURSOR / 2.0)
            .fill(if cursor.down { ink } else { tokens::clear() })
            .border(tokens::HAIRLINE * 2.0, ink),
    )
}

fn picture(state: &State, scale: f32) -> El {
    // **With a gap the picture area becomes the background**, and each pane a panel on it (#82): a gutter
    // of two panels meeting is a line, not a gutter. At gap 0 nothing changes — the same single panel, the
    // same goldens, byte for byte — which is why the look is a branch here and not a restyle everywhere.
    let gapped = state.settings.pane_gap > 0.0;
    let mut body = col()
        .key("picture")
        .w_full()
        .h_full()
        .grow(1.0)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::HAIRLINE, tokens::hairline())
        .fill(if gapped {
            tokens::bg()
        } else {
            tokens::panel()
        })
        .on_layout(|state: &mut State, [_, _, width, height]| panel_laid_out(state, width, height));

    // **An empty workspace is a state, not a failure** (#138): there is no tiling to draw, so the panel
    // says where panes would be. The header, the chips and the settings sheet are outside this panel and
    // draw as they always do.
    if state.layout().is_none() {
        return body.center().gap(tokens::EMPTY_GAP).child(
            text(NO_PANES)
                .key("no-panes")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }
    let mut body = if state.is_single_pane() {
        match state.pane_rects().into_iter().next() {
            Some((id, rect)) => single_pane(state, id, rect, scale, body),
            None => return body,
        }
    } else {
        for (id, rect) in state.pane_rects() {
            body = body.child(pane(state, id, rect, scale));
        }
        body
    };
    // **The gutters go over the panes and under everything held** (#114): a press in the space between
    // two panes is the resize it looks like, while a pane in the air, a full-size pane and a menu keep
    // the top of the picture — a drag in the air is still a lift, and a menu still dismisses on the
    // press that starts the next thing.
    body = gutters(state, body);
    // The pane in the air and the context menu are drawn last, over every pane — **and over the one-pane wall too**:
    // "Focus on new pane" lives in that menu, and a wall with one pane is exactly the wall that needs it
    // to get a second one. Drawing the menu only once there were panes made the first split impossible
    // to ask for.
    body = ghost(state, scale, body);
    body = maximised(state, scale, body);
    body = menu_view(state, scale, body);
    // **The client's cursor last, over everything** (#167): a pointer is above the wall it points at, and a remote
    // that is pressing a menu row or a hull should see where its point is.
    if let Some(cursor) = cursor_glyph(state) {
        body = body.child(cursor);
    }
    body
}

/// One element per border, over the space the panes leave between them (#114).
///
/// The gutter is not a pane's, so no pane's `on_drag` can reach it: with a gap, `panes_on` insets every
/// pane and the middle of the gap belongs to the picture element, which has no handler. These elements are
/// that handler, and their geometry is [`view::Border::gutter`] — the same `at`, `span` and split extent
/// the press and the ratio use, so what the eye sees between two panes is what the hand grabs.
///
/// **Nothing is drawn**: the gutter's colour is the picture's own background, and a fill here would be a
/// second copy of the look. The element exists to be hit, and its cursor says it is grabbable.
fn gutters(state: &State, body: El) -> El {
    let gap = state.settings.pane_gap;
    // At gap 0 the panes meet: the gutter has no width and the in-pane grip is the target, which is the
    // wall every golden before #82 renders. Adding zero-area elements there would only shadow panes.
    if gap <= 0.0 {
        return body;
    }
    let mut body = body;
    for border in state.borders() {
        let rect = border.gutter(gap);
        if rect.w <= 0.0 || rect.h <= 0.0 {
            continue;
        }
        let cursor = match border.dir {
            Dir::Horizontal => rune::Cursor::ResizeColumn,
            Dir::Vertical => rune::Cursor::ResizeRow,
        };
        body = body.child(
            el()
                // The pair names the split exactly, and a split's gutter is one element.
                .key(format!("gutter-{}-{}", border.a, border.b))
                .absolute(rect.x, rect.y)
                .size(rect.w, rect.h)
                .fill(tokens::clear())
                .cursor(cursor)
                .on_drag(move |state: &mut State, drag, _tx| drag_gutter(state, rect, drag)),
        );
    }
    body
}

/// One pane filling the panel: the whole wall, exactly as it was before Multi View.
fn single_pane(state: &State, id: ViewId, rect: Rect, scale: f32, body: El) -> El {
    let body = pane_content(state, id, rect, scale, true, body);
    let Some(pane) = state.pane(id) else {
        return body;
    };
    let mut layer = body;
    // A pane whose map has not drawn yet has no markers, but it **still has a title**: the plate is chrome
    // over the picture, not part of it. A tiled pane draws its plate before its first frame for the same
    // reason, and one pane is not a special case of "every pane has a title" (#113).
    if let Some(frame) = pane.frame {
        layer = layer.child(projections(state, id, &frame, scale));
    }
    // **Every pane has a title, including the only one** (#113): a lone pane draws `pane-01` in the corner.
    // The control must not appear from nowhere the first time the wall is split, and it is drawn after the
    // markers so a press on it is a press on the label rather than on a hull underneath.
    layer = layer.child(name_plate(pane));
    // The controls are chrome too, and on a one-pane wall they are its whole chrome: the wall the
    // operator's screenshot marked is not necessarily a tiled one (#111).
    layer.child(pane_chrome(state, id, rect))
}

/// One pane: its picture, its markers, its own frame, the handle that moves it, and the ring that says
/// the keys are here.
fn pane(state: &State, id: ViewId, rect: Rect, scale: f32) -> El {
    let focused = state.focused() == id;
    // A pane with space around it is a surface of its own, so it is rounded; a pane that shares a border
    // with its neighbour is a piece of one surface, and rounds nothing (#82).
    let gapped = state.settings.pane_gap > 0.0;
    let dragged = state.dragging().is_some_and(|drag| drag.pane == id);
    let mut body = el()
        .key(format!("pane-{id}"))
        .absolute(rect.x, rect.y)
        .size(rect.w.max(1.0), rect.h.max(1.0))
        // A pane that lands somewhere else glides there rather than blinking (`El::matched_crop`): the
        // swap reads as movement, which is what tells the operator what happened. At rest it draws
        // exactly the rectangle the layout says, so a settled capture is unchanged by it.
        .matched(format!("pane-{}", id))
        .matched_crop()
        .opacity(if dragged { tokens::PANE_LIFTED } else { 1.0 })
        .on_layout(move |state: &mut State, [_, _, width, height]| {
            pane_laid_out(state, id, width, height, scale)
        })
        // **Where the press landed decides what the drag is**: within a grip of a border it resizes,
        // anywhere else it pans that pane's map. The handler lives on the element a press actually
        // reaches — the map's element when there is one (`picture_content`), this wrapper when there is
        // not, because Rune hands a drag to the nearest element under the pointer that has a primary
        // handler and the map has one (`on_press`), so a handler up here would never be reached (#80).
        // **A pane is a window on its own water**: a marker or a picture that overflows must not
        // appear in the pane beside it.
        .clip_rounded(0.0)
        .radius(if gapped { tokens::PANEL_RADIUS } else { 0.0 })
        // A pane with a gutter around it is a surface of its own standing on the background; a pane that
        // shares a border with its neighbour has no fill of its own and the panel behind it is the picture
        // — which keeps the one-pane wall and the gapless tiling byte for byte what they were.
        .fill(if gapped {
            tokens::panel()
        } else {
            tokens::clear()
        })
        .border(
            tokens::HAIRLINE,
            if focused {
                tokens::focus_edge()
            } else {
                tokens::hairline()
            },
        );

    body = pane_content(state, id, rect, scale, false, body);
    // Without the map engine the pane itself is the surface under the pointer, so the pane carries the
    // gesture. With it, the map does — and a second handler here would be unreachable. A tree pane has
    // no camera to pan, so it carries no drag at all (#116).
    #[cfg(not(feature = "map"))]
    if state.pane(id).is_some_and(|pane| pane.content.is_map()) {
        body = body
            .on_drag(move |state: &mut State, drag, _tx| drag_pane(state, id, rect, drag, scale));
    }

    // Markers project against THIS pane's frame (`INV-04`): its camera, its size, and nothing else. A
    // pane whose map has not drawn yet draws no markers — and still has its handle.
    let mut layer = body;
    if let Some(frame) = state.pane(id).and_then(|pane| pane.frame) {
        layer = layer.child(projections(state, id, &frame, scale));
    }
    // **Chrome is drawn after the markers, in this order** (#113). The plate is a press target now — a
    // press on the label opens the rename sheet — and the handle is the grab target at the top centre.
    // Drawing the plate before the handle, and both after the markers, is what makes a press on the label
    // a rename rather than a hull selection or a lift, and a grab on the handle a lift rather than a
    // rename. The ordering is load-bearing, not tidy.
    if let Some(pane) = state.pane(id) {
        layer = layer.child(name_plate(pane));
    }
    // **Chrome, after the markers and after the plate** (#111): the filter and maximise controls are
    // pressed by a hand, so a press must land on them rather than on a hull or a name underneath.
    layer = layer.child(pane_chrome(state, id, rect));
    // The handle is drawn **last**, over everything: it is the element a grab lands on, and a grab must
    // never be a press on the hull — or on a button — underneath it.
    layer.child(handle(id, rect))
}

/// A pane's chrome row (#111): the filter toggle, then the maximise toggle, in its top-right corner.
///
/// **Drawn after the markers and after the name plate**, and before the handle. That order is the whole
/// contract: a press on a control must never reach the hull underneath it, and a grab on the handle must
/// never be a press on a button. The row is one element so the two controls cannot drift apart; their
/// rectangles come from [`view::chrome_buttons`], which `verify` samples.
///
/// The icon says what the press will do — outpacing arrows while the pane is in its slot, inwards when it
/// is full size — and its meaning is also in the help text, which is where a word fits.
fn pane_chrome(state: &State, pane: ViewId, rect: Rect) -> El {
    let (filter, _maximise) = view::chrome_buttons(rect);
    let full = state.maximised() == Some(pane);
    let filter_button = rune::icon(&icons::FILTER, tokens::PANE_BUTTON)
        .key(filter_anchor(state, pane))
        .color(tokens::ink())
        .help("Filter this pane")
        .on_press(move |state: &mut State| toggle_filter(state, pane));
    let (icon, help) = if full {
        (&icons::RESTORE, "Back to the wall")
    } else {
        (&icons::MAXIMISE, "Full size")
    };
    let maximise_button = rune::icon(icon, tokens::PANE_BUTTON)
        .key(format!("maximise-{pane}"))
        .color(tokens::ink())
        .help(help)
        .on_press(move |state: &mut State| {
            control::apply(state, Intent::ToggleFullSize(pane)).ok();
        });
    row()
        .key(format!("pane-chrome-{pane}"))
        // Pane-local: `chrome_buttons` answers in the pane's own rectangle, and the row is drawn inside
        // the pane, which is already at `rect.x`/`rect.y` (#84's offset lesson).
        .absolute(filter.x - rect.x, filter.y - rect.y)
        .gap(tokens::GAP / 2.0)
        .child(filter_button)
        .child(maximise_button)
}

/// The maximise toggle's press: one press, the same two functions the menu row used to call. Whichever
/// way the pane is, the button does the thing its icon shows.
pub(crate) fn toggle_full_size(state: &mut State, pane: ViewId) -> bool {
    if state.maximised() == Some(pane) {
        restore_full_size(state);
        // Restoring is always a change: the pane was over the wall and now it is in its slot.
        true
    } else {
        full_size_from_menu(state, pane);
        state.maximised() == Some(pane)
    }
}

/// A pane's handle: a small bar at the top of the pane, and the gesture that lifts the pane.
///
/// It is **its own element on top of the map**, which is how a grab avoids selecting the hull under the
/// pointer — `TabDragState`'s `suppress_click` in a different mechanism. The press that starts a lift
/// never reaches a marker, so nothing has to be undone afterwards.
fn handle(id: ViewId, rect: Rect) -> El {
    let (w, h) = tokens::PANE_HANDLE;
    // **Pane-local**, because the handle is drawn inside its pane: the pane is already at `rect.x`,
    // and adding that again puts the handle off the right of a pane that is not the first one.
    el().key(format!("handle-{id}"))
        .absolute(rect.w / 2.0 - w / 2.0, tokens::GAP / 2.0)
        .size(w, h)
        .radius(h / 2.0)
        .fill(tokens::handle_ink())
        .on_drag(move |state: &mut State, drag, _tx| lift(state, id, rect, drag))
}

/// The handle's gesture: lift, follow the pointer, drop.
///
/// The drop is recomputed from the pointer each move and the **preview** layout is what draws the gap,
/// so the gap is the place the pane lands — `Layout::preview` and `Layout::apply` are one operation.
fn lift(state: &mut State, id: ViewId, rect: Rect, drag: &mut rune::Drag) {
    // **The handle is its own element.** `Drag::x`/`y` are local to *it* (`runtime/input/gesture.rs`),
    // and the handle is a 28 × 4 box at the top centre of a pane that is itself at `rect` — so a drag
    // that skipped these offsets asked `drop_for` about a point near the window's top-left corner, which
    // is why a pane could only ever be dropped to the right of where the station began (#84).
    let at = view::handle_point(rect, (drag.x, drag.y));
    match drag.phase {
        rune::Phase::Down => {
            state.begin(Begin::Lift(PaneDrag {
                pane: id,
                drop: None,
            }));
            state.focus_pane(id);
        }
        rune::Phase::Move => {
            let drop = state.drop_for(id, at);
            if let Interaction::Lifted(drag) = &mut state.interaction
                && drag.drop != drop
            {
                drag.drop = drop;
            }
            state.layout_dirty = true;
        }
        rune::Phase::Up => {
            let drop = state.dragging().and_then(|drag| drag.drop);
            state.end();
            if let Some(drop) = drop
                && state.apply_drop(drop)
            {
                state.end();
            }
            state.layout_dirty = true;
        }
    }
}

/// The pane in the air: **the pane, drawn at the slot it will land in** (#84).
///
/// Before this a lifted pane was only its dimmed slot and an empty gap, so the thing being carried was
/// invisible: a hole in one place, a hand in another, and nothing between them. The ghost is the pane
/// itself — its own camera and its own hulls (`INV-04` holds in the air too), projected at the size it will
/// be drawn at once it lands — on the rectangle the release will use, so the picture during a drag is also
/// the answer to "where does it go".
///
/// **At the slot, not under the pointer**, which is a deliberate departure from #84's first sketch: a copy
/// hanging from the hand covers the slot it is about to land in — the one thing the operator is looking at —
/// and it is a second coordinate space to get wrong, in a ticket whose whole subject is a coordinate space
/// got wrong. The rectangle is `Layout::preview`'s answer: one function, used by the picture here and by
/// `verify` when it projects the pane's hulls into the same rectangle.
///
/// Its map pixels are not duplicated: one pane owns one `Pixels`. The hulls are what say *which* pane is
/// being carried, and they are the part that can be drawn twice.
fn ghost(state: &State, scale: f32, body: El) -> El {
    let Some(drag) = state.dragging() else {
        return body;
    };
    let Some(drop) = drag.drop else { return body };
    let Some(landing) = view::landing_rect(
        state.layout().expect("the scene has a tiling"),
        drop,
        state.pane_viewport(),
        drag.pane,
    ) else {
        return body;
    };
    let Some(pane) = state.pane(drag.pane) else {
        return body;
    };
    let rect = view::inset_rect(landing, tokens::GAP);
    let element = el()
        .key("pane-ghost")
        .absolute(rect.x, rect.y)
        .size(rect.w, rect.h)
        .radius(tokens::PANEL_RADIUS)
        // The plate is the slot's material — the ghost *is* the slot the pane will land in, with the pane
        // standing on it, and the hulls are what say which pane that is. Deliberately not a faded copy: a
        // ghost at a third of the panel's brightness is a ghost nobody sees on a dark wall.
        .fill(tokens::gap_fill())
        // Drawn over the panes, so it must not take the press that is carrying it: `inert` draws it and
        // passes every gesture through to what is below.
        .inert()
        .clip_rounded(0.0)
        .border(tokens::HAIRLINE, tokens::focus_edge());
    // **The slot's frame, not the pane's old one**: a dropped pane is drawn at the size it lands at, so
    // the ghost is its camera projected against the landing rectangle. Using the frame it had would cram
    // the hulls into the corner of a slot with a different shape.
    let frame = view::synthetic_frame(pane.camera, landing, scale);
    let sizes = |id_unit: i64| state.sizes.get(&id_unit).copied();
    let placed = markers::place(
        &state.picture,
        &frame,
        &pane.view.filter,
        state.now(),
        state.stale_after,
        pane.view.range,
        &sizes,
    );
    let pictures = markers::Pictures {
        images: &state.pictures,
    };
    body.child(element.child(markers::layer(
        &placed,
        &frame,
        scale,
        &pictures,
        state.dragging().map(|drag| drag.pane).unwrap_or_default(),
    )))
}

/// **One pane's picture**: its markers, projected against `frame`, and the card if the selected hull is in
/// that frame. Used by the single-pane wall, by a tiled pane and by the maximised pane (#85) — three callers
/// that must agree, because a second copy of this is how a marker is drawn against one camera and asserted
/// against another.
fn projections(
    state: &State,
    id: ViewId,
    frame: &crate::map::geometry::Rendered,
    scale: f32,
) -> El {
    let sizes = |id_unit: i64| state.sizes.get(&id_unit).copied();
    // **This pane's latch, not the wall's** (#115): the same frame it draws against is the frame that
    // decided whether its hulls are symbols or pictures.
    let range = state
        .pane(id)
        .map(|pane| pane.view.range)
        .unwrap_or_default();
    let default_filter = Filter::default();
    let filter = state
        .pane(id)
        .map(|pane| &pane.view.filter)
        .unwrap_or(&default_filter);
    let placed = markers::place(
        &state.picture,
        frame,
        filter,
        state.now(),
        state.stale_after,
        range,
        &sizes,
    );
    let pictures = markers::Pictures {
        images: &state.pictures,
    };
    let mut layer = markers::layer(&placed, frame, scale, &pictures, id);
    // **The installations are a layer of their own** (#109): the places, not the hulls. Projected
    // against **this** pane's frame (`INV-04`), asked of this pane's filter once, and drawn under the
    // objects so a buoy's label reads over a base rather than under it.
    let placed_facilities = installations::place(
        &state.installations,
        &state.installation_types,
        &state.manifest,
        frame,
        filter,
    );
    let selected_facility = state.installation_in(id);
    if let Some(facilities) = installations::layer(
        &placed_facilities,
        frame,
        scale,
        &state.assets,
        selected_facility,
        id,
    ) {
        layer = layer.child(facilities);
    }
    // **The tactical objects are a layer of their own** (#110), projected against **this** pane's
    // frame (`INV-04`): a tiled wall draws the same water's buoys in every pane showing it. Drawn
    // after the hulls so a buoy's label reads over the picture rather than under a hull passing it.
    // **The layer's own filter** (#112): the placement function asks the pane's filter, so a hidden
    // domain or category is absent from this pane's layer and every other layer is untouched.
    let placed_objects = objects::place(&state.placements, &state.manifest, frame, filter);
    if let Some(objects) = objects::layer(&placed_objects, frame, scale, &state.assets, id) {
        layer = layer.child(objects);
    }
    let points = if scale > 0.0 { f64::from(scale) } else { 1.0 };
    let viewport = (
        f64::from(frame.width) / points,
        f64::from(frame.height) / points,
    );
    if let Some(card) = state.card_in(id)
        && let Some(marker) = placed
            .iter()
            .find(|marker| marker.id_unit == card.unit.id_unit)
    {
        let anchor = (marker.x / points, marker.y / points);
        let placement = crate::units::card::place(anchor, viewport);
        layer = layer.child(unitcard::card(card, placement, state.now()));
    }
    // **The facility card** (#109): what the row carries, anchored to its mark in this pane. Drawn
    // only when the facility is still in the layer — a hidden layer draws no floating card.
    if let Some(selected) = selected_facility
        && let Some(mark) = placed_facilities.iter().find(|mark| mark.id == selected)
    {
        let anchor = (mark.x / points, mark.y / points);
        let placement = installations::card_placement(anchor, viewport);
        layer = layer.child(installations::card(mark, placement));
    }
    layer
}

/// A pane's **title**, on its own border (#83, #113): a plate in the top-left corner, left of the handle.
///
/// **Every pane draws one.** The plate is what the operator clicks to type a new name, so a fresh pane has
/// a control rather than a gap: it says the pane's own number (`pane-01`) until somebody names it. A name
/// the operator typed is still the only thing stored on the pane; the number is [`Pane::title`]'s answer,
/// not a name invented in `Pane.name`.
///
/// **Inside the pane, not straddling its edge**: a pane clips its picture (`clip_rounded`), so a plate
/// half outside would be cut in half — and a title is chrome, which belongs *on* the picture rather than
/// hanging off it.
///
/// Its press calls the same [`begin_rename`] the menu row calls, so there is one way into the sheet and no
/// second path to keep in step. It is drawn after the markers and before the handle in [`pane`]: the
/// ordering is load-bearing now that the plate is a press target — a press on the label must be a rename,
/// never a grab, and a grab on the handle must never be a press on the label.
fn name_plate(pane: &Pane) -> El {
    let id = pane.id;
    let title = pane.title();
    // A title is short by construction, so the ellipsis remains a property of typed names only. The
    // fallback covers a `name` that is empty once trimmed: it is not a name, so the pane keeps its number.
    let shown = shown_name(&title).unwrap_or_else(|| default_title(id));
    row()
        .key(format!("pane-name-{id}"))
        // The same offsets `view::name_probe` measures against: the plate sits half a gap in from the
        // pane's corner, so the probe lands on it rather than on the pane beside it.
        .absolute(tokens::GAP / 2.0, tokens::GAP / 2.0)
        .h(tokens::NAME_H)
        .max_w(tokens::NAME_MAX_W)
        .items_center()
        .px(tokens::GAP)
        .radius(tokens::NAME_H / 2.0)
        .fill(tokens::name_plate())
        .border(tokens::HAIRLINE, tokens::hairline())
        .on_press(move |state: &mut State| {
            control::apply(state, Intent::BeginRename(RenameTarget::Pane(id))).ok();
        })
        .child(
            text(shown)
                .key(format!("pane-title-{id}"))
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        )
}

/// **One pane's content, whatever the pane is** (#116): the map's picture, or the chain of command.
///
/// One function, because the single-pane wall, a tiled pane and a maximised pane all draw the same
/// pane — a second copy of "what a pane draws" is how a tree pane would appear in one of the three and
/// not the others.
fn pane_content(state: &State, id: ViewId, rect: Rect, scale: f32, single: bool, body: El) -> El {
    // **An explicit match over the kinds** (#136): a third kind must be a branch somebody wrote, not a
    // fallback that happens to draw nothing. A pane with no content is a map's pane before its frame.
    let Some(pane) = state.pane(id) else {
        return picture_content(state, id, rect, scale, single, body);
    };
    match &pane.content {
        PaneContent::Map => picture_content(state, id, rect, scale, single, body),
        PaneContent::Tree { .. } => match tree::tree_for(state, id) {
            Some(tree) => body.child(tree::content(state, id, rect, &tree)),
            // A tree pane whose structure has not been read yet draws an empty pane: no placeholder
            // words, and above all no map. The reads are asked for on the frame this pane was opened.
            None => body,
        },
        PaneContent::Newsroom => body.child(newsroom::content(state, id)),
    }
}

/// One pane's picture: the map's pixels, its failure, or the placeholder.
#[cfg(feature = "map")]
fn picture_content(
    state: &State,
    id: ViewId,
    rect: Rect,
    scale: f32,
    single: bool,
    body: El,
) -> El {
    if let Some(map) = state.pane(id).and_then(|pane| pane.map.as_ref()) {
        if map.error().is_none() {
            // Wheel, a right-click, a press and **the drag** belong to the map in both shapes: Rune hands
            // a gesture to the nearest element under the pointer that has a primary handler, the map has
            // one (`on_press`), so a drag on the pane's wrapper is never reached (#80).
            // `on_layout` is the one that differs: with one pane the map element owns it (the wall it has
            // always been), and with panes the wrapper does, because one `on_layout` per frame is a
            // measurable cost with 200 markers inside it (`INV-12`).
            let mut element = rune::el()
                .key("map")
                .w_full()
                .h_full()
                .pixels(map.pixels())
                .on_wheel(move |state: &mut State, event: rune::WheelEvent, _tx| {
                    wheel(state, id, &event)
                })
                .on_secondary_press_local(
                    move |state: &mut State, event: rune::PointerEvent, _tx| {
                        open_menu(state, id, &event, scale)
                    },
                )
                .on_press(move |state: &mut State| press_in(state, id))
                // The map fills its pane, so its local point is the pane's local point, and the pane's
                // origin is the conversion `drag_pane` makes (#80).
                .on_drag(move |state: &mut State, drag, _tx| {
                    drag_pane(state, id, rect, drag, scale)
                });
            if single {
                element = element.on_layout(move |state: &mut State, [_, _, width, height]| {
                    pane_laid_out(state, id, width, height, scale)
                });
            }
            return body.child(element);
        }
        if let Some(error) = map.error() {
            return body
                .center()
                .gap(tokens::GAP)
                .child(
                    text("THE MAP DID NOT START")
                        .key("map-error")
                        .font(tokens::TITLE)
                        .color(tokens::ink()),
                )
                .child(
                    text(error)
                        .key("map-error-detail")
                        .font(tokens::LABEL)
                        .color(tokens::ink_dim()),
                );
        }
    }
    placeholder(body, "the feed and the markers land here (#3, #6)")
}

#[cfg(not(feature = "map"))]
fn picture_content(
    state: &State,
    _id: ViewId,
    _rect: Rect,
    _scale: f32,
    _single: bool,
    body: El,
) -> El {
    // A scene has a picture and no map engine: the ground is the panel, and saying "built without
    // the map" would be a note about our build inside the thing under test.
    if state.headless {
        return body;
    }
    placeholder(body, "built without the map: cargo run --features map")
}

/// The panel that says the wall is waiting, used until there is a picture to draw.
fn placeholder(body: El, hint: &'static str) -> El {
    body.center()
        .gap(tokens::GAP)
        .child(
            text("AWAITING THE PICTURE")
                .key("picture-title")
                .font(tokens::TITLE)
                .color(tokens::ink()),
        )
        .child(
            text(hint)
                .key("picture-hint")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        )
}

/// The picture panel was laid out: remember its points, so the panes tile into it on the next frame.
///
/// `poll` redraws once when this changes — a capture's golden is taken after many frames, so the first
/// frame's provisional viewport can never be what a golden holds.
fn panel_laid_out(state: &mut State, width: f32, height: f32) {
    let size = (width.max(1.0), height.max(1.0));
    if state.panel != Some(size) {
        state.panel = Some(size);
        state.layout_dirty = true;
    }
}

/// A pane was laid out at `width` × `height` points.
///
/// A wall tells its map thread to render that many **device** pixels (the element is points, `Pixels`
/// is pixels). A headless scene has no thread, so the frame is synthesised at the pane's own size —
/// which is what makes a `panes` capture a picture of three pans at three cameras rather than one
/// frame stretched three ways. The one-pane case keeps the window-sized frame it has always had: those
/// goldens are the wall as it was, and `#71`'s golden pass unifies the two.
pub(crate) fn pane_laid_out(state: &mut State, id: ViewId, width: f32, height: f32, scale: f32) {
    // **A tree pane has no frame** (#116): there is no map thread to resize and no frame to project
    // against, so the layout pass has nothing to say to it. `INV-04` holds literally — no frame, and
    // therefore no marker anywhere in it.
    if state.pane(id).is_some_and(|pane| !pane.content.is_map()) {
        return;
    }
    let scale = if scale > 0.0 { scale } else { 1.0 };
    state.scale = scale;
    let points = Rect {
        x: 0.0,
        y: 0.0,
        w: width.max(1.0),
        h: height.max(1.0),
    };
    #[cfg(feature = "map")]
    {
        let (w, h) = (
            (width * scale).round().max(1.0) as u32,
            (height * scale).round().max(1.0) as u32,
        );
        if let Some(map) = state.pane(id).and_then(|pane| pane.map.as_ref()) {
            if map
                .rendered()
                .is_some_and(|frame| (frame.width, frame.height) == (w, h))
            {
                return;
            }
            map.resize(w, h);
            return;
        }
    }
    if state.is_single_pane() {
        return;
    }
    let Some(pane) = state.pane_mut(id) else {
        return;
    };
    let frame = view::synthetic_frame(pane.camera, points, scale);
    if pane.frame != Some(frame) {
        pane.frame = Some(frame);
        state.layout_dirty = true;
    }
}

/// The pane menu, at the point it was asked for: what a pane can do, and nothing else.
///
/// **`Close pane` appears only when there is more than one pane** (#81) — a wall whose last pane could be
/// closed is a wall that answers "no panes here", and `Layout::close` refuses it anyway, so offering it
/// would be a control that does nothing. The surface is as tall as it has items.
fn menu_view(state: &State, scale: f32, body: El) -> El {
    let Some(menu) = state.menu() else {
        return body;
    };
    let viewport = state.pane_viewport();
    let Some(rect) = state.pane_rect(menu.pane).map(|rect| {
        if state.maximised() == Some(menu.pane) {
            // A right-click on the full-size picture is a right-click on the picture the operator is
            // looking at, so the menu opens on that rectangle and not on the slot behind the scrim.
            Rect {
                x: 0.0,
                y: 0.0,
                w: viewport.w as f32,
                h: viewport.h as f32,
            }
        } else {
            rect
        }
    }) else {
        return body;
    };
    let points = if scale > 0.0 { scale } else { 1.0 };
    let items = menu_items(state, menu.pane);
    let (w, h) = tokens::menu_size(items.len());
    let (x, y) = menu_origin(rect, (menu.at.0 / points, menu.at.1 / points), (w, h));
    let mut surface = col()
        .key("pane-menu")
        .absolute(x, y)
        .size(w, h)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::HAIRLINE, tokens::card_edge())
        .fill(tokens::bg())
        // **The menu is a menu to an assistive client** (#164, #111's rule that chrome says what it is): rows are
        // menuitems, and the one the keyboard is on is the active descendant — which is the same row the ring is
        // drawn on, because they are the same value.
        .role("menu")
        .label("Pane menu");
    let ringed = menu.ring;
    if let Some(row) = ringed.and_then(|row| items.get(row)) {
        surface = surface.active_descendant(row.key.clone());
    }
    for (index, item) in items.iter().enumerate() {
        // Each row is its own target, so a press says **which** item it hit. The key is the row's, not the
        // surface's, which is what lets a capture assert that a second row is drawn at all.
        let key = item.key.clone();
        let mut row = row()
            .key(key)
            .h(tokens::MENU_ROW)
            .w_full()
            .items_center()
            // A label touching the menu's own border reads as clipped, whatever the box says.
            .px(tokens::GAP)
            .role("menuitem")
            .label(item.label.clone())
            .child(
                text(item.label.clone())
                    .font(tokens::LABEL)
                    .color(tokens::ink())
                    .grow(1.0),
            );
        // **The ring** (#164): the same `focus_edge` the wall uses for "this one has the keys", drawn as a border so
        // it survives a row's fill. It is not a hover: a menu's hover opens a waterfall, and a key's ring acts.
        if menu.open.is_none() && ringed == Some(index) {
            row = row
                .border(tokens::HAIRLINE * 2.0, tokens::focus_edge())
                .radius(tokens::PANEL_RADIUS / 2.0);
        }
        if item.opens() {
            // **A waterfall opens on hover** (#140), as `rune-kit::Menu` does: a press that had to be
            // found would make the second surface the third click of a two-click action. A press on the
            // row opens it too, because a wall also has a hand and a trackpad with no hover.
            if let MenuAction::Submenu(sub_key) = item.action {
                row = row.on_hover(move |state: &mut State| open_submenu(state, sub_key));
            }
            row = row.child(
                rune::icon(&icons::CHEVRON_RIGHT, tokens::MENU_CHEVRON)
                    .key(format!("{}-chevron", item.key))
                    .color(tokens::ink_dim()),
            );
        }
        let run = item.clone();
        let pane = menu.pane;
        surface = surface.child(row.on_press(move |state: &mut State| run.run(state, pane)));
    }
    let mut body = body.child(surface);

    // **The second surface** (#140): to the right of its row, clamped inside the pane it was asked on —
    // and flipped to the left when the pane is too narrow to hold it on the right, so a waterfall in a
    // narrow pane opens inwards rather than off the wall.
    if let Some(open) = menu.open
        && let Some(index) = items.iter().position(|item| item.key == open)
        && !items[index].children.is_empty()
    {
        let children = items[index].children.clone();
        let (sw, sh) = tokens::menu_size(children.len());
        let row_top = y + index as f32 * tokens::MENU_ROW;
        let right = x + w + tokens::GAP / 2.0;
        let sx = if right + sw <= rect.x + rect.w {
            right
        } else {
            (x - sw - tokens::GAP / 2.0).max(rect.x)
        };
        let sy = row_top.min(rect.y + rect.h - sh).max(rect.y);
        let mut sub = col()
            .key("pane-submenu")
            .absolute(sx, sy)
            .size(sw, sh)
            .radius(tokens::PANEL_RADIUS)
            .border(tokens::HAIRLINE, tokens::card_edge())
            .fill(tokens::bg())
            .role("menu")
            .label(
                children
                    .first()
                    .map(|_| "Move to workspace")
                    .unwrap_or("Menu"),
            );
        let sub_ring = if menu.open.is_some() { menu.ring } else { None };
        if let Some(row) = sub_ring.and_then(|row| children.get(row)) {
            sub = sub.active_descendant(row.key.clone());
        }
        for (index, child) in children.iter().enumerate() {
            let run = child.clone();
            let pane = menu.pane;
            let mut row = row()
                .key(child.key.clone())
                .h(tokens::MENU_ROW)
                .w_full()
                .items_center()
                .px(tokens::GAP)
                .role("menuitem")
                .label(child.label.clone())
                .child(
                    text(child.label.clone())
                        .font(tokens::LABEL)
                        .color(tokens::ink()),
                )
                .on_press(move |state: &mut State| run.run(state, pane));
            if sub_ring == Some(index) {
                row = row
                    .border(tokens::HAIRLINE * 2.0, tokens::focus_edge())
                    .radius(tokens::PANEL_RADIUS / 2.0);
            }
            sub = sub.child(row);
        }
        body = body.child(sub);
    }
    body
}

/// `Name this pane…`: open the rename sheet on this pane, with its current name in the field (#83).
fn begin_rename(state: &mut State, pane: ViewId) {
    let current = state.pane(pane).and_then(|pane| pane.name.clone());
    state.begin(Begin::Rename(Rename::new(
        RenameTarget::Pane(pane),
        current.as_deref(),
    )));
}

/// The same sheet, on whichever thing a target names (#170). One function, because `Intent::BeginRename` is the
/// control plane's spelling of the two gestures above and they must not diverge. A workspace that is not on
/// screen opens its sheet without switching to it (#154).
pub(crate) fn begin_rename_target(state: &mut State, target: RenameTarget) {
    match target {
        RenameTarget::Pane(pane) => begin_rename(state, pane),
        RenameTarget::Workspace(index) => begin_workspace_rename(state, index),
    }
}

/// **Name this workspace** (#139): the header's title is the door, as the pane's plate is the pane's.
///
/// The field starts with the name the header shows — the typed name, or the default — so the operator
/// edits what they can see rather than an empty box over an unnamed workspace.
pub(crate) fn begin_workspace_rename(state: &mut State, index: usize) {
    let Some(workspace) = state.workspaces.get(index) else {
        return;
    };
    let current = workspace_title(workspace, index);
    state.begin(Begin::Rename(Rename::new(
        RenameTarget::Workspace(index),
        Some(&current),
    )));
}

/// The maximise action: the pane is drawn over the wall and its map renders at the window's size (#85).
///
/// **It is still in its slot.** The tiling is untouched, the other panes keep their cameras and their
/// cadence, and restoring puts the picture back exactly as it was — this is "one pane for a moment", not
/// "make this the only pane", which is what a tiled wall's operator actually wants when the traffic gets
/// dense. The name says "from menu" because that is where #85 put it; since #111 the chrome's maximise
/// toggle calls the same function, in one press.
pub(crate) fn full_size_from_menu(state: &mut State, pane: ViewId) {
    state.end();
    if !state.maximise(pane) {
        return;
    }
    // The keys and the highest cadence belong to the pane being looked at (`#71`).
    state.focus_pane(pane);
    if let Some(panel) = state.panel {
        resize_map(state, pane, panel, state.scale);
    }
    state.layout_dirty = true;
}

/// The scene's entry to the same move: a scene has no menu row to press, so it calls what the row calls.
/// Public only for `scene`, so a capture is of the wall's own path and not of a second one written for it.
pub fn full_size_for_scene(state: &mut State, pane: ViewId) {
    full_size_from_menu(state, pane);
}

pub(crate) fn restore_full_size(state: &mut State) {
    let Some(pane) = state.maximised() else {
        return;
    };
    state.restore_tiling();
    // The map goes back to the pane's own rectangle, not to whatever it was before: the tiling may have
    // been dragged while the pane was full size.
    if let Some(rect) = state
        .pane_rect(pane)
        .map(|rect| view::inset_rect(rect, state.settings.pane_gap / 2.0))
    {
        let size = (rect.w.max(1.0), rect.h.max(1.0));
        resize_map(state, pane, size, state.scale);
    }
    state.layout_dirty = true;
}

/// Tell one pane's map thread to render at `points`, in the device pixels it actually draws (#85).
///
/// A maximised pane that keeps rendering at its slot's size is an upscaled picture — blurry, and the one
/// thing this ticket names as its reason to exist. So the thread is resized twice: once large, once back.
fn resize_map(state: &mut State, pane: ViewId, points: (f32, f32), scale: f32) {
    #[cfg(feature = "map")]
    {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let (w, h) = (
            (points.0 * scale).round().max(1.0) as u32,
            (points.1 * scale).round().max(1.0) as u32,
        );
        if let Some(map) = state.pane(pane).and_then(|pane| pane.map.as_ref()) {
            map.resize(w, h);
        }
    }
    #[cfg(not(feature = "map"))]
    let _ = (state, pane, points, scale);
}

/// **The row a `Down` or `Up` moves the ring to**, wrapping at both ends (#164).
///
/// A menu with no ring yet starts at the first row for `Down` and the last for `Up` — the two directions mean where
/// to begin as well as where to go, which is what a native menu does.
fn next_row(state: &State, step: isize) -> usize {
    let rows = menu_rows(state).len().max(1);
    let current = state.menu().and_then(|menu| menu.ring);
    match current {
        None if step >= 0 => 0,
        None => rows - 1,
        Some(row) => ((row as isize + step).rem_euclid(rows as isize)) as usize,
    }
}

/// Enter in the rename sheet: the name is the field's text, trimmed, and **what it renames is the
/// target** (#139).
///
/// A pane's empty field removes its name, and the plate falls back to the pane's number. A workspace is
/// never nameless: an empty field falls back to its default `Workspace N`, because the header has to say
/// something and a blank title is a wall that looks broken.
///
/// **A workspace name lives in memory only**, like a pane's: panes do not survive a restart, and a saved
/// layout/workspace file is its own ticket. Do not add a `settings.toml` field for this.
pub(crate) fn commit_rename(state: &mut State) {
    // The sheet is the only thing that renames, so taking its text and ending the state in one step is
    // unambiguous — and `end()` is what clears it, rather than a second way to leave the state.
    let Some((target, text)) = state
        .rename()
        .map(|rename| (rename.target, rename.edit.text.clone()))
    else {
        return;
    };
    state.end();
    rename_to(state, target, Some(&text));
}

/// **Apply a name to the thing it names** (#170): the sheet's Enter and the control plane's `rename.set` are one
/// function, so a name typed and a name sent cannot land differently — including what an empty one means.
pub(crate) fn rename_to(state: &mut State, target: RenameTarget, name: Option<&str>) -> bool {
    let trimmed = name.unwrap_or_default().trim();
    match target {
        RenameTarget::Pane(pane) => {
            let Some(pane) = state.pane_mut(pane) else {
                return false;
            };
            pane.name = shown_name(trimmed);
        }
        RenameTarget::Workspace(index) => {
            let Some(workspace) = state.workspaces.get_mut(index) else {
                return false;
            };
            workspace.name = if trimmed.is_empty() {
                default_workspace_name(index)
            } else {
                trimmed.to_string()
            };
        }
    }
    state.arranged();
    state.layout_dirty = true;
    true
}

/// One row of the pane menu: what it says, what a capture calls it, what a press does — and **the second
/// surface it opens**, if it opens one (#140).
///
/// The label is a `String` because a waterfall's rows are **workspace names** (`Straits watch`), not
/// constants; the key is one too, because the row for a workspace is identified by its index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    pub key: String,
    pub action: MenuAction,
    /// The submenu's rows. Empty for a leaf.
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    fn leaf(label: &str, key: &str, action: MenuAction) -> MenuItem {
        MenuItem {
            label: label.to_string(),
            key: key.to_string(),
            action,
            children: Vec::new(),
        }
    }

    fn submenu(label: &str, key: &'static str, children: Vec<MenuItem>) -> MenuItem {
        MenuItem {
            label: label.to_string(),
            key: key.to_string(),
            action: MenuAction::Submenu(key),
            children,
        }
    }

    /// Whether this row opens a second surface.
    pub fn opens(&self) -> bool {
        !self.children.is_empty()
    }

    /// **What this row means as an intent** (#170) — the row's own description of itself, so a test can say
    /// that every row a menu offers is expressible in the control plane's vocabulary.
    ///
    /// `None` is the one row that is *not* a change: a submenu's trigger opens a second surface of the menu it
    /// is already in, which is a place rather than a value.
    fn intent(&self, state: &State, pane: ViewId) -> Option<Intent> {
        Some(match self.action {
            // The point the menu was opened at, in the pane's own frame pixels — the row shows the water that
            // was clicked rather than the water the source pane was looking at (#77).
            MenuAction::Split => Intent::SplitAt {
                pane,
                at: state
                    .menu()
                    .map(|menu| (f64::from(menu.at.0), f64::from(menu.at.1)))?,
            },
            MenuAction::NewWorkspace => Intent::SplitToWorkspace(pane),
            MenuAction::Rename => Intent::BeginRename(RenameTarget::Pane(pane)),
            MenuAction::Close => Intent::ClosePane(pane),
            MenuAction::MoveTo(target) => Intent::MovePane {
                pane,
                workspace: target,
            },
            MenuAction::Submenu(_) => return None,
        })
    }

    /// Run the row's action on the pane the menu was asked on, **through the control plane** (#170). The menu
    /// closes in the action, not here: what a row does is the row's, and one of them has to close the menu
    /// *before* it changes the layout.
    fn run(&self, state: &mut State, pane: ViewId) {
        match self.intent(state, pane) {
            Some(intent) => {
                // A refusal is the row's own rule answering: `Close` on the wall's last pane is not offered
                // (that row is not built), so a refusal here is a race the redraw settles.
                control::apply(state, intent).ok();
            }
            // A trigger's press opens the surface its hover would: the wall has a mouse and a hand.
            None => {
                if let MenuAction::Submenu(key) = self.action {
                    open_submenu(state, key);
                }
            }
        }
    }
}

/// The menu a pane offers. Ordered: the thing that grows the wall first, the thing that shrinks it last —
/// where the destructive action is not the one under the pointer by accident.
///
/// **Full size left the menu in #111.** It is the pane chrome's one-press toggle now; a row here was a
/// right-click, a row read, and a press — three actions for a thing an operator does constantly.
pub fn menu_items(state: &State, pane: ViewId) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::leaf(MENU_SPLIT, "menu-split", MenuAction::Split),
        // **A new pane on a new workspace** (#140) sits beside the split it is a variation of: growth at
        // the top, and the destructive row still last.
        MenuItem::leaf(
            MENU_NEW_WORKSPACE,
            "menu-new-workspace",
            MenuAction::NewWorkspace,
        ),
        MenuItem::leaf(MENU_RENAME, "menu-rename", MenuAction::Rename),
    ];
    // The waterfall lists every workspace **except the one this pane is already in**, by name — "Move to
    // workspace > Straits watch" has to read, and a list of numbers would not.
    let here = state.workspace_of(pane);
    let elsewhere: Vec<MenuItem> = state
        .workspaces
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != here)
        .map(|(index, workspace)| {
            MenuItem::leaf(
                &workspace_title(workspace, index),
                &format!("menu-move-{index}"),
                MenuAction::MoveTo(index),
            )
        })
        .collect();
    if !elsewhere.is_empty() {
        items.push(MenuItem::submenu(MENU_MOVE, "menu-move", elsewhere));
    }
    // **The same predicate the action uses** (#141): a pane with a sibling, or one on another
    // workspace. A lone pane on a wall of empty workspaces is refused here and refused there.
    if state.can_close(pane) {
        items.push(MenuItem::leaf(MENU_CLOSE, "menu-close", MenuAction::Close));
    }
    items
}

/// Where the menu's top-left corner goes for a click at `at` in its pane, in the pane's own points.
///
/// The menu opens **at the click**, and is clamped inside its pane: a right-click 20 pt from the wall's
/// edge would otherwise draw half a menu over the window bezel, and the item would be off-screen on the
/// one wall that needs it. Pure, so the clamp is tested without a pointer, and it takes its own size
/// because the menu is as tall as its items (#81).
pub fn menu_origin(rect: Rect, at: (f32, f32), size: (f32, f32)) -> (f32, f32) {
    let (w, h) = size;
    let margin = tokens::GAP / 2.0;
    let x = (rect.x + at.0).clamp(
        rect.x + margin,
        (rect.x + rect.w - w - margin).max(rect.x + margin),
    );
    let y = (rect.y + at.1).clamp(
        rect.y + margin,
        (rect.y + rect.h - h - margin).max(rect.y + margin),
    );
    (x, y)
}

/// What the pane menu offers.
pub const MENU_SPLIT: &str = "Focus on new pane";
/// **Open a new pane on a new workspace** (#140). Growth at the top, beside the split it is a variation of.
pub const MENU_NEW_WORKSPACE: &str = "Focus on new workspace";
/// The waterfall's trigger: `Move to workspace > …` (#140).
pub const MENU_MOVE: &str = "Move to workspace";
pub const MENU_RENAME: &str = "Name this pane…";
pub const MENU_CLOSE: &str = "Close pane";

/// **Every row key a menu can open, as a literal** (#140, #201).
///
/// `Interaction::Menu` holds `open: Option<&'static str>` — a row's key is a compile-time name — and a `Scene`
/// carries it as text, so it needs the literal back. A key with an index in it (`menu-move-2`, one per workspace)
/// is deliberately absent: reopening a waterfall from a scene would be a scene inventing which workspace it means,
/// so a scene that names one opens the first surface instead, and a test keeps this list in step with the rows.
pub const MENU_KEYS: [&str; 5] = [
    "menu-split",
    "menu-new-workspace",
    "menu-rename",
    "menu-move",
    "menu-close",
];

/// The literal a scene's row key names, or `None` for a name the wall does not have.
pub fn menu_key(name: &str) -> Option<&'static str> {
    MENU_KEYS.iter().copied().find(|key| *key == name)
}

/// **What a menu row does** (#140): a value, so a row can name a workspace and a submenu can be a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Split,
    NewWorkspace,
    Rename,
    Close,
    /// Move this pane to the workspace at that index.
    MoveTo(usize),
    /// A row with a second surface: its children draw beside it, and this is the key that opens them.
    Submenu(&'static str),
}
/// What the workspace rename sheet is called (#139). The pane's keeps its own words.
pub const RENAME_WORKSPACE_TITLE: &str = "Name this workspace";
/// What the maximised pane's own close plate says, and the header's note about it (#85).
pub const MAX_CLOSE: &str = "back to the wall";
/// What the rename sheet is called, and what a name longer than `NAME_MAX_CHARS` ends with.
pub const RENAME_TITLE: &str = "Name this pane";
pub const ELLIPSIS: char = '…';

/// A right-click in a pane: focus it, and open the menu where the pointer is.
#[cfg(feature = "map")]
fn open_menu(state: &mut State, id: ViewId, event: &rune::PointerEvent, scale: f32) {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    control::apply(state, Intent::FocusPane(id)).ok();
    // Through the machine, so a right-click during a lift is refused rather than opening a surface the
    // operator's held pointer cannot reach (#101).
    state.begin(Begin::Menu {
        pane: id,
        at: (event.x * scale, event.y * scale),
    });
}

/// A primary press in a pane: close the menu (the click was not for it) and focus the pane.
#[cfg(feature = "map")]
fn press_in(state: &mut State, id: ViewId) {
    // Three intents, because a press on the water means three settled things (#170): the pane has the keys,
    // **that pane's** selection is cleared (#99 — the map underneath is that pane's, and the press must not
    // walk over to the pane that happens to have the keys), and the press was not for the menu, so the menu
    // goes.
    control::apply(state, Intent::FocusPane(id)).ok();
    control::apply(
        state,
        Intent::Select {
            pane: id,
            unit: None,
        },
    )
    .ok();
    control::apply(state, Intent::CloseSurfaces).ok();
}

/// #81 `Close pane`: the pane goes, its slot collapses into its sibling, and its map thread is joined
/// before it is forgotten — a wall that leaks a render thread per closed pane is a wall that dies at the
/// third split.
/// **A new pane on a new workspace** (#140): the workspace's `+` and the menu's row differ in one thing —
/// this one also puts a pane in it, of the same kind and camera as the pane the menu was asked on.
///
/// A **new** pane, not a copy: a map gets its own camera, a tree the same structure with a fresh camera,
/// a newsroom a fresh view at its default tab and page, and the map's thread is started by `poll` like
/// every other map pane's (#160). The source workspace keeps its pane and its layout, exactly as `Focus
/// on new pane` keeps the source pane.
pub fn new_workspace_from_pane(state: &mut State, pane: ViewId) -> Option<ViewId> {
    let source = state.pane(pane)?;
    let content = source.content.clone();
    let camera = source.camera;
    // `add_workspace` appends, switches and focuses — the workspace is empty but for the pane below.
    let index = state.add_workspace();
    let id = state.allocate_pane();
    let created = match content {
        PaneContent::Map => Pane::new(id, camera),
        PaneContent::Tree { organisation } => Pane::tree(id, camera, organisation),
        PaneContent::Newsroom => Pane::newsroom(id, camera),
    };
    let workspace = &mut state.workspaces[index];
    workspace.layout = Some(Layout::single_with(id));
    workspace.panes.push(created);
    // No thread here (#160): `poll` starts one for every map pane that has none, and this pane's slot
    // is the whole panel from the moment the layout above exists.
    state.focus_pane(id);
    state.end();
    state.arranged();
    state.layout_dirty = true;
    Some(id)
}

/// **Move this pane to another workspace** (#140).
///
/// The `Pane` itself travels — its map handle, its tree camera, its newsroom view, its name, its filter and
/// its frame — so a map that moves does not blink, reload or lose its camera. **Nothing is cloned and no
/// thread is closed**: the source's `Vec<Pane>` gives it up and the target's takes it, and `close_maps` is
/// never called on the way.
///
/// The source keeps whatever else it had; if the moved pane was its last, the source is **empty**
/// (`layout = None`), which the model allows. The target becomes active either way, because that is where
/// the operator just put something.
pub fn move_pane_to(state: &mut State, pane: ViewId, target: usize) -> bool {
    let Some(source) = state.workspace_of(pane) else {
        return false;
    };
    if target == source || target >= state.workspaces.len() {
        return false;
    }
    let Some(position) = state.workspaces[source]
        .panes
        .iter()
        .position(|held| held.id == pane)
    else {
        return false;
    };
    let moved = state.workspaces[source].panes.remove(position);
    // The source's tiling: collapse the leaf, or — when it was the only one — keep no tiling at all.
    let source_was_single = state.workspaces[source]
        .layout
        .as_ref()
        .is_none_or(|layout| layout.len() <= 1);
    if source_was_single {
        state.workspaces[source].layout = None;
    } else if let Some(layout) = state.workspaces[source].layout.as_mut() {
        layout.close(pane);
    }

    // Into the target: its only pane, or beside the pane it is focused on.
    let target_empty = state.workspaces[target].layout.is_none();
    if target_empty {
        state.workspaces[target].layout = Some(Layout::single_with(pane));
    } else {
        let focused = state.workspaces[target]
            .layout
            .as_ref()
            .map(|layout| layout.focused());
        let Some(focused) = focused else {
            return false;
        };
        let inserted = state.workspaces[target]
            .layout
            .as_mut()
            .and_then(|layout| layout.split_side_with(focused, view::Side::Right, pane));
        if inserted.is_none() {
            // The tiling refused the id (it was already there, or the leaf vanished): put the pane back
            // rather than lose it, and say the move did not happen.
            state.workspaces[source].panes.insert(position, moved);
            if source_was_single {
                state.workspaces[source].layout = Some(Layout::single_with(pane));
            }
            return false;
        }
    }
    state.workspaces[target].panes.push(moved);
    state.switch_to(target);
    state.focus_pane(pane);
    state.end();
    state.arranged();
    state.layout_dirty = true;
    true
}

/// Open a row's second surface (#140). The interaction carries which one, so a capture can say the
/// waterfall is open and Escape can close it before the menu.
pub fn open_submenu(state: &mut State, key: &'static str) {
    if let Interaction::Menu { pane, at, .. } = state.interaction {
        state.interaction = Interaction::Menu {
            pane,
            at,
            open: Some(key),
            // **A second surface takes the ring with it** (#164): the rows on screen changed, so an index into the
            // old ones is a ring on the wrong row. `menu_ring` puts it on the first row of the new surface.
            ring: None,
        };
        state.layout_dirty = true;
    }
}

/// Close the waterfall and keep the menu (#164): `Left`, and the ring goes back to the row that opened it.
pub fn close_submenu(state: &mut State) {
    if let Interaction::Menu { pane, at, ring, .. } = state.interaction {
        state.interaction = Interaction::Menu {
            pane,
            at,
            open: None,
            ring,
        };
        state.layout_dirty = true;
    }
}

/// **The rows the keyboard's ring can be on** (#164): the menu's, or the open waterfall's.
pub fn menu_rows(state: &State) -> Vec<MenuItem> {
    let Some(menu) = state.menu() else {
        return Vec::new();
    };
    let items = menu_items(state, menu.pane);
    match menu.open {
        Some(key) => items
            .into_iter()
            .find(|item| item.key == key)
            .map(|item| item.children)
            .unwrap_or_default(),
        None => items,
    }
}

/// **Put the ring on a row of the surface that is open** (#164). `None` takes it off.
///
/// The row is clamped rather than refused: a ring that moved past the end of a list that just got shorter is a
/// ring nobody meant, and the menu is the wall's, so it answers with the nearest row that exists.
pub fn menu_ring(state: &mut State, row: Option<usize>) -> bool {
    let rows = menu_rows(state).len();
    let Interaction::Menu {
        pane,
        at,
        open,
        ring,
        ..
    } = state.interaction
    else {
        return false;
    };
    let next = match row {
        None => None,
        Some(_) if rows == 0 => None,
        Some(row) => Some(row.min(rows - 1)),
    };
    if next == ring {
        return false;
    }
    state.interaction = Interaction::Menu {
        pane,
        at,
        open,
        ring: next,
    };
    state.layout_dirty = true;
    true
}

/// **Move the ring by `step` rows, wrapping** (#164): `Up` and `Down` are one function, so the two ends behave the
/// same — a ring that stopped at the last row would be a menu the keyboard cannot get out of.
pub fn menu_ring_step(state: &mut State, step: isize) -> bool {
    let rows = menu_rows(state).len();
    if rows == 0 {
        return false;
    }
    let current = state.menu().and_then(|menu| menu.ring);
    let next = match current {
        None if step >= 0 => 0,
        None => rows - 1,
        Some(row) => {
            let moved = row as isize + step;
            moved.rem_euclid(rows as isize) as usize
        }
    };
    menu_ring(state, Some(next))
}

/// **Act on the ringed row** (#164): the row the keyboard is on, and never the row the pointer is over.
///
/// A row that opens a second surface opens it and puts the ring on its first row; a leaf runs its action, which is
/// `MenuAction::run` — the same function a press calls, so a key and a press cannot mean two things.
pub fn menu_choose(state: &mut State) -> bool {
    let Some(menu) = state.menu() else {
        return false;
    };
    let Some(ring) = menu.ring else {
        return false;
    };
    let rows = menu_rows(state);
    let Some(row) = rows.get(ring) else {
        return false;
    };
    if row.opens() {
        let key = match row.action {
            MenuAction::Submenu(key) => key,
            _ => return false,
        };
        open_submenu(state, key);
        let opened = menu_rows(state).len();
        if opened > 0 {
            menu_ring(state, Some(0));
        }
        return true;
    }
    row.run(state, menu.pane);
    true
}

pub(crate) fn close_from_menu(state: &mut State, id: ViewId) -> bool {
    state.end();
    // `close_pane` drops the pane — which stops its map thread and joins it (the rule `close_maps` relies
    // on) — collapses its slot or empties its workspace, and lands the wall on the nearest previous
    // workspace when it was that workspace's last pane (#141).
    let closed = state.close_pane(id);
    if !closed {
        // The last pane on the wall: nothing happened, and the menu said so by not offering it. A redraw
        // keeps the menu from being left open on a pane that is still there.
        state.layout_dirty = true;
    }
    closed
}

/// `Focus on new pane`: the clicked ground becomes a pane of its own, same zoom.
///
/// The geographic point is the pane's **own** camera and frame, unprojected at the click — the inverse
/// of the projection every marker is drawn with, which is why it lives in `map::geometry` beside it.
/// **Split a pane at a point in its own frame** (#77): the new pane shows the water at that point, at the old
/// pane's zoom.
///
/// The point is a value — the menu's own anchor today, a cursor's press from #167 tomorrow — which is why the
/// geometry is here and the request is `Intent::SplitAt`.
pub(crate) fn split_at(state: &mut State, id: ViewId, at: (f64, f64)) -> Option<ViewId> {
    let pane = state.pane(id)?;
    let Some(frame) = pane.frame else {
        // No frame yet means no camera to unproject against: a pane opened at a guessed point would
        // be a pane showing the wrong water, which is the failure `INV-04` exists to prevent.
        state.layout_dirty = true;
        return None;
    };
    let camera = Camera::new(
        crate::map::geometry::unproject(frame.camera, frame.viewport(), at.0, at.1),
        frame.camera.zoom,
    );
    state.split_pane(id, camera)
}

/// A wheel step: zoom about the pointer, so the ground under the cursor stays under it.
#[cfg(feature = "map")]
fn wheel(state: &mut State, id: ViewId, event: &rune::WheelEvent) -> bool {
    let Some(frame) = state
        .pane(id)
        .and_then(|pane| pane.map.as_ref())
        .and_then(|map| map.rendered())
    else {
        // Before the first frame the viewport is unknown; drop the gesture rather than apply it
        // through a guess, which would jump the camera on the very first wheel event.
        return false;
    };
    let viewpoint = frame.viewport();
    if !viewpoint.is_usable() {
        // Before the first frame the viewport is unknown; the gesture is dropped rather than applied
        // through a guess, which would jump the camera on the first wheel event.
        return false;
    }
    let delta = f64::from(event.dy) * crate::map::geometry::WHEEL_ZOOM_PER_PX;
    let moved = crate::map::geometry::zoom_about(
        state.camera_of(id),
        viewpoint,
        (f64::from(event.x), f64::from(event.y)),
        delta,
    );
    // **The gesture computes, the intent applies** (#170): the zoom maths is the viewer's, and the camera it
    // settles on is a value the control plane can also be sent.
    control::apply(
        state,
        Intent::Camera {
            pane: id,
            camera: moved,
        },
    )
    .ok();
    if let Some(map) = state.pane(id).and_then(|pane| pane.map.as_ref()) {
        map.set_camera(moved);
    }
    true
}

/// A drag: move the ground with the hand, from the camera the drag started on.
///
/// Absolute rather than incremental on purpose: [`rune::Drag`] reports travel since the press, so
/// summing per-event deltas would compound and the map would run away from the pointer.
#[cfg(feature = "map")]
fn drag_map(state: &mut State, id: ViewId, drag: &mut rune::Drag) {
    let Some(frame) = state
        .pane(id)
        .and_then(|pane| pane.map.as_ref())
        .and_then(|map| map.rendered())
    else {
        return;
    };
    let camera = state.camera_of(id);
    match drag.phase {
        rune::Phase::Down => {
            if let Some(pane) = state.pane_mut(id) {
                pane.drag_origin = Some(camera);
            }
        }
        rune::Phase::Move => {
            // **The picture follows the hand; the state waits for it to stop** (#173). A pan is a continuous
            // gesture, and the pane's camera is a *settled* value: writing it per event would be sixty state
            // changes a second, sixty `arranged()` calls (the save debounce never fires, #153) and a camera
            // that a client would have to reproduce event by event. The map thread is told every move because
            // the picture has to follow; `State` is told once, on release.
            if let Some(map) = state.pane(id).and_then(|pane| pane.map.as_ref()) {
                map.set_camera(dragged(state, id, frame, drag));
            }
        }
        rune::Phase::Up => {
            // The settled camera, through the control plane: **the value a client would send is the value the
            // viewer used**, and it is applied once — which is also when the arrangement first becomes worth
            // saving.
            let settled = dragged(state, id, frame, drag);
            let current = state.camera_of(id);
            if let Some(pane) = state.pane_mut(id) {
                pane.drag_origin = None;
            }
            // A press that did not move is not a change: applying the camera it started on would schedule a
            // write of an arrangement that is already on disk (`arranged()`), and say "changed" for a click.
            if settled != current {
                control::apply(
                    state,
                    Intent::Camera {
                        pane: id,
                        camera: settled,
                    },
                )
                .ok();
            }
            // One render now rather than at the next idle tick: the gesture just ended, and the
            // operator is looking at where it ended.
            if let Some(map) = state.pane(id).and_then(|pane| pane.map.as_ref()) {
                map.render_now();
            }
        }
    }
}

/// Where a pan has got to: the camera the hand has dragged to, from the one it started on.
///
/// Absolute rather than incremental: [`rune::Drag`] reports travel since the press, so summing per-event deltas
/// would compound and the map would run away from the pointer. Called on every move (for the picture) and once on
/// release (for the state, #173).
#[cfg(feature = "map")]
fn dragged(
    state: &State,
    id: ViewId,
    frame: crate::map::geometry::Rendered,
    drag: &rune::Drag,
) -> Camera {
    let origin = state
        .pane(id)
        .and_then(|pane| pane.drag_origin)
        .unwrap_or_else(|| state.camera_of(id));
    crate::map::geometry::pan(
        origin,
        frame.viewport(),
        f64::from(drag.dx),
        f64::from(drag.dy),
    )
}

/// A drag on a tiled pane: a border grip resizes the split, anywhere else pans that pane's map (#69).
#[cfg(feature = "map")]
fn drag_pane(state: &mut State, id: ViewId, rect: Rect, drag: &mut rune::Drag, scale: f32) {
    match drag.phase {
        rune::Phase::Down => {
            let at = view::viewport_point(rect, (drag.start_x, drag.start_y));
            begin_resize(state, at);
            if state.resizing().is_none() {
                drag_map(state, id, drag);
            }
        }
        rune::Phase::Move => {
            if let Some(border) = state.resizing() {
                // The pointer's coordinate along the border's own axis, in the viewport's points. A
                // point, not a delta: the ratio follows the line the operator is holding, and a
                // gesture that leaves the pane and comes back lands where the pointer is.
                let pointer = view::viewport_point(rect, (drag.x, drag.y));
                let pointer = match border.dir {
                    Dir::Horizontal => pointer.0,
                    Dir::Vertical => pointer.1,
                };
                if state.set_ratio(&border, border.ratio_for(pointer)) {
                    state.layout_dirty = true;
                }
                let _ = scale;
            } else {
                drag_map(state, id, drag);
            }
        }
        rune::Phase::Up => {
            if state.resizing().is_some() {
                // Nothing to render: the panes are elements, not pixels. The redraw is the layout
                // change, and `poll` will see `layout_dirty`.
                state.end();
            } else {
                drag_map(state, id, drag);
            }
        }
    }
}

/// Begin holding the border under the pointer, if there is one (#69, and a state since #101): a press
/// within a grip of a border is a border's, anywhere else is the map's.
fn begin_resize(state: &mut State, at: (f32, f32)) {
    let Some(border) = state.border_at(at, state.pane_grip()) else {
        return;
    };
    state.begin(Begin::Resize(border));
}

/// A drag that began **in the gutter** (#114): the same `begin_resize` / `set_ratio` / `end` the pane's
/// own border grip uses (#69), reached from an element that exists only because the gap does.
///
/// `rect` is the gutter element's rectangle, and its origin is the offset from the pointer's own points
/// to the viewport's — the same conversion `drag_pane` makes for a pane (#80). Every question the layout
/// answers (`border_at`, `ratio_for`) is asked in the viewport's points.
fn drag_gutter(state: &mut State, rect: Rect, drag: &mut rune::Drag) {
    match drag.phase {
        rune::Phase::Down => {
            let at = view::viewport_point(rect, (drag.start_x, drag.start_y));
            begin_resize(state, at);
        }
        rune::Phase::Move => {
            let Some(border) = state.resizing() else {
                return;
            };
            // A point, not a delta: the ratio follows the line the operator is holding, and a gesture
            // that leaves the gutter and comes back lands where the pointer is.
            let pointer = view::viewport_point(rect, (drag.x, drag.y));
            if state.set_ratio(&border, border.ratio_for(border.axis_of(pointer))) {
                state.layout_dirty = true;
            }
        }
        rune::Phase::Up => {
            if state.resizing().is_some() {
                state.end();
            }
        }
    }
}

/// Without the engine there is still something to drag: the border between two panes (#69).
#[cfg(not(feature = "map"))]
fn drag_pane(state: &mut State, _id: ViewId, rect: Rect, drag: &mut rune::Drag, _scale: f32) {
    match drag.phase {
        rune::Phase::Down => {
            let at = view::viewport_point(rect, (drag.start_x, drag.start_y));
            begin_resize(state, at);
        }
        rune::Phase::Move => {
            if let Some(border) = state.resizing() {
                let pointer = view::viewport_point(rect, (drag.x, drag.y));
                let pointer = match border.dir {
                    Dir::Horizontal => pointer.0,
                    Dir::Vertical => pointer.1,
                };
                if state.set_ratio(&border, border.ratio_for(pointer)) {
                    state.layout_dirty = true;
                }
            }
        }
        rune::Phase::Up => {
            if state.resizing().is_some() {
                state.end();
            }
        }
    }
}

/// How the window opens: sized from the command line, or covering the primary output.
///
/// Rune exposes fullscreen as a runtime toggle (`Tx::toggle_full_screen`), not as a startup option,
/// so `--fullscreen` is still the closest honest thing — borderless, at the primary output's own
/// size and position. #8 decides when the wall toggles the real one, and records what it decided.
fn window_options(window: config::Window, displays: &[rune::Display]) -> rune::WindowOptions {
    let options = rune::WindowOptions {
        min_size: Some(tokens::MIN_WINDOW),
        // A wall has nothing to click: no title bar to drag, no grip to resize. Fullscreen is
        // borderless as well, and takes the primary output's own geometry.
        borderless: !window.decorations,
        ..Default::default()
    };
    if !window.fullscreen {
        return options;
    }
    let primary = displays
        .iter()
        .find(|display| display.primary)
        .or_else(|| displays.first());
    rune::WindowOptions {
        borderless: true,
        position: primary.map(|display| display.position),
        physical_size: primary.map(|display| display.size),
        ..options
    }
}

/// Fold what the reads thread said into the open card.
///
/// The three reads fill in independently: the card shows a skeleton row for a pending one and keeps
/// its content if one fails, so a slow commander lookup never blanks the name.
fn drain_card_reads(state: &mut State) -> bool {
    let Some(backend) = &state.backend else {
        return false;
    };
    let mut replies = Vec::new();
    while let Some(reply) = backend.try_recv() {
        replies.push(reply);
    }
    if replies.is_empty() {
        return false;
    }
    // The register and the dimensions are about the wall, not about the card: they are taken whether or
    // not anything is selected, and they are what a picture is scaled from.
    let mut changed = false;
    for reply in &replies {
        changed |= apply_reply(state, reply);
    }
    let replies: Vec<crate::data::Reply> = replies
        .into_iter()
        .filter(|reply| {
            matches!(
                reply,
                crate::data::Reply::Unit(..)
                    | crate::data::Reply::Commander(..)
                    | crate::data::Reply::Specifications(..)
            )
        })
        .collect();
    // **Every pane asking about that hull is answered** (#99): two panes may show the same hull's card,
    // and one of them silently missing its commander row would be the shared-selection bug in a new
    // costume. Panes are few, so this is a walk, not an index.
    for reply in &replies {
        for pane in state.panes_mut().iter_mut() {
            if pane.view.selected.is_none() || pane.view.card.is_none() {
                continue;
            }
            let wants = pane.view.selected;
            let Some(card) = pane.view.card.as_mut() else {
                continue;
            };
            match reply {
                crate::data::Reply::Unit(id, result) if Some(*id) == wants => {
                    card.identity = match result {
                        Ok(unit) => crate::units::card::Fetch::Ready((**unit).clone()),
                        Err(error) => crate::units::card::Fetch::Failed(error.to_string()),
                    };
                    changed = true;
                }
                crate::data::Reply::Commander(id, result) if Some(*id) == wants => {
                    card.commander = match result {
                        Ok(entries) => crate::units::card::Fetch::Ready(entries.clone()),
                        Err(error) => crate::units::card::Fetch::Failed(error.to_string()),
                    };
                    changed = true;
                }
                crate::data::Reply::Specifications(id, result) if Some(*id) == wants => {
                    card.specification = match result {
                        Ok(versions) => crate::units::card::Fetch::Ready(versions.clone()),
                        Err(error) => crate::units::card::Fetch::Failed(error.to_string()),
                    };
                    changed = true;
                }
                _ => {}
            }
        }
    }
    changed
}

/// Apply one reply from the reads thread. Split out of the drain loop so a test can hand the state a reply
/// without a backend: what a failure does to the *next* request is the behaviour `#53` is about, and it is
/// not observable through a socket. The `photo` scene uses the same door, so a capture covers the route the
/// wall actually takes for a picture (`#54`) instead of a shortcut past it.
pub(crate) fn apply_reply(state: &mut State, reply: &crate::data::Reply) -> bool {
    let mut changed = false;
    match reply {
        crate::data::Reply::Units(Ok(register)) => {
            state.image_urls.clear();
            state.registered = register.units.len();
            tracing::info!(
                rows = register.units.len(),
                pages = register.pages,
                total = register.total_records,
                truncated = register.truncated,
                "the fleet register was read"
            );
            for row in &register.units {
                if let Some(url) = &row.image_url {
                    state.image_urls.insert(row.id, url.clone());
                }
            }
            // The register is fresh, so every picture that gave up may try again: its URL may have
            // been the expired thing, and only a re-read can hand it a new one.
            state.image_retries.clear();
            state.register_retry_at = None;
            state.register_attempts = 0;
            changed = true;
        }
        crate::data::Reply::Image(id_unit, Ok(bytes)) => {
            state.offer_picture(*id_unit, std::sync::Arc::from(bytes.as_slice()));
            changed = true;
        }
        crate::data::Reply::TacticalObjectManifest(Ok(manifest)) => {
            // **The offline library, cached by code** (#110). An unsigned entry is kept: a signing
            // failure omits the URL, not the entry, because the map still needs to know the symbol
            // exists and what it is called.
            state.manifest.clear();
            let mut unsigned = 0;
            for entry in &manifest.symbols {
                if entry.asset_url.is_none() {
                    unsigned += 1;
                }
                state.manifest.insert(entry.code.clone(), entry.clone());
            }
            tracing::info!(
                version = manifest.version,
                symbols = manifest.symbols.len(),
                unsigned,
                "the tactical-object manifest was cached"
            );
            changed = true;
        }
        crate::data::Reply::TacticalObjectManifest(Err(error)) => {
            // A refresh is deliberate, not per frame: say it once and draw no objects rather than
            // asking again every frame for a read the account may not be allowed.
            tracing::warn!(
                endpoint = "GET /tactical-objects/manifest",
                error = %error,
                "the tactical-object manifest did not read: no object draws its artwork until a refresh"
            );
        }
        crate::data::Reply::TacticalObjectPlacements(Ok(pages)) => {
            state.placements.clone_from(&pages.rows);
            tracing::info!(
                marks = pages.rows.len(),
                pages = pages.pages,
                total = pages.total_records,
                truncated = pages.truncated,
                "the operational placements were read"
            );
            if pages.truncated {
                tracing::warn!(
                    marks = pages.rows.len(),
                    "the placement read hit its page cap: the map draws part of the operational layer"
                );
            }
            changed = true;
        }
        crate::data::Reply::TacticalObjectPlacements(Err(error)) => {
            tracing::warn!(
                endpoint = "GET /tactical-object-placements",
                error = %error,
                "the operational placements did not read: the map draws no tactical objects"
            );
        }
        crate::data::Reply::TacticalAsset(asset_key, Ok(bytes)) => {
            state.offer_asset(asset_key, std::sync::Arc::from(bytes.as_slice()));
            changed = true;
        }
        crate::data::Reply::TacticalAsset(asset_key, Err(error)) => {
            // **An expired URL is a re-sign, not an error state** (#110): log it once, keep the generic
            // mark, and never turn a picture into a failed read. Asking again on every frame would
            // hammer object storage with a URL that has already expired.
            if state.asset_failures.insert(asset_key.clone()) {
                tracing::warn!(
                    asset = %asset_key,
                    error = %error,
                    "the tactical object's artwork did not arrive: the generic mark is drawn until a refresh"
                );
            }
        }
        crate::data::Reply::Image(id_unit, Err(error)) => {
            // A picture that cannot be fetched draws the symbol, says why **in the log**, and is asked
            // for again after a growing wait. The old code printed to stderr — which a wall with
            // `--log-file` and no console never keeps — and never retried (#53).
            let attempts = state
                .image_retries
                .get(id_unit)
                .map(|retry| retry.attempts)
                .unwrap_or(0)
                .saturating_add(1);
            let wait = retry_after(attempts);
            state.image_retries.insert(
                *id_unit,
                Retry {
                    attempts,
                    at: state.now() + wait,
                },
            );
            state.asked_images.remove(id_unit);
            tracing::warn!(
                hull = id_unit,
                attempt = attempts,
                retry_in_s = wait.as_secs(),
                error = %error,
                "the picture did not arrive: the symbol is drawn until it does"
            );
            // A presigned URL expires; the only way to see a fresh one is to re-read the register.
            state.asked_register = false;
        }
        crate::data::Reply::Units(Err(error)) => {
            state.register_attempts = state.register_attempts.saturating_add(1);
            let attempts = state.register_attempts;
            let wait = REGISTER_RETRY;
            state.asked_register = false;
            state.register_retry_at = Some(state.now() + wait);
            tracing::warn!(
                attempts,
                retry_in_s = wait.as_secs(),
                endpoint = "GET /units",
                error = %error,
                "the fleet register did not read: no hull can draw its picture until it does"
            );
        }
        crate::data::Reply::Specifications(id, Ok(versions)) => {
            // The newest version is the one in force: the list is newest first.
            if let Some(size) = versions.first().and_then(size_of) {
                state.sizes.insert(*id, size);
                changed = true;
            }
        }
        crate::data::Reply::Viewpoint(result) => {
            changed |= viewpoint::apply(state, result);
        }
        // **The standing orders of battle** (#116). The list names the structures; each structure's
        // forest is its own read, asked for by the tree pane that draws it. Nothing is defaulted: a read
        // that fails is one log line and a branch missing from the chain of command, never an empty
        // structure drawn as though the server had said so.
        crate::data::Reply::UnitOrganisations(Ok(list)) => {
            tracing::info!(
                structures = list.len(),
                "the standing orders of battle were read"
            );
            state.organisations.clone_from(list);
            changed = true;
        }
        crate::data::Reply::UnitOrganisations(Err(error)) => {
            tracing::warn!(
                endpoint = "GET /unit-organisations",
                error = %error,
                "the orders of battle did not read: a chain-of-command pane has no structures to draw"
            );
        }
        crate::data::Reply::UnitOrganisation(id, Ok(detail)) => {
            tracing::info!(
                organisation = id,
                roots = detail.nodes.len(),
                "a standing structure was read"
            );
            state.organisation_trees.insert(*id, (**detail).clone());
            changed = true;
        }
        crate::data::Reply::UnitOrganisation(id, Err(error)) => {
            tracing::warn!(
                organisation = id,
                endpoint = "GET /unit-organisations/{id}",
                error = %error,
                "the structure did not read: its branch is missing from the chain of command"
            );
        }
        crate::data::Reply::TniCommander(Ok(post)) => {
            // **A vacant post is not a missing read** (#116): `commander` is absent when nobody is
            // appointed, and the post is drawn without a name rather than with one invented here.
            tracing::info!(
                post = %post.position_title,
                vacant = post.commander.is_none(),
                "the supreme command was read"
            );
            state.tni_command = Some((**post).clone());
            changed = true;
        }
        // **A story's provenance** (#137): applied only to the panes still on that story. A slow story
        // that lands after `back`, or after another row was pressed, answers a question nobody is
        // asking any more — the detail is the pane's state, so it is compared, not merely delivered.
        crate::data::Reply::Story(id, result) => {
            let mut applied = 0;
            for pane in state.panes_mut() {
                if let Some(view) = pane.news.as_mut()
                    && view.apply_detail(*id, result)
                {
                    applied += 1;
                }
            }
            match result {
                Ok(detail) if applied > 0 => tracing::info!(
                    story = id,
                    articles = detail.articles.len(),
                    source_count = detail.story.source_count,
                    panes = applied,
                    "a story's provenance was read"
                ),
                Err(error) if applied > 0 => tracing::warn!(
                    story = id,
                    error = %error,
                    panes = applied,
                    "the story did not read: the pane shows what it can and offers retry"
                ),
                _ => tracing::debug!(
                    story = id,
                    "a story arrived after the operator left it: discarded"
                ),
            }
            if applied > 0 {
                changed = true;
            }
        }
        // **The newsroom** (#136). A page is applied only to the panes waiting for exactly that query:
        // a reply is the answer to a question, and a question the operator has already changed is not
        // one to answer. `panes` in the log line is how many were still waiting — zero is a late page,
        // dropped on purpose.
        crate::data::Reply::Stories(query, result) => {
            let mut applied = 0;
            let mut relocated = Vec::new();
            for pane in state.panes_mut() {
                if let Some(view) = pane.news.as_mut()
                    && view.apply(*query, result)
                {
                    applied += 1;
                    // **A page the deployment no longer holds** (#153): a wall restored on page 4 of a feed
                    // that now has two opens on the last page that exists and asks for it, rather than
                    // sitting on a page nobody wrote.
                    if let Some(pagination) = view.pagination
                        && view.page > pagination.total_pages
                        && pagination.total_pages >= 1
                    {
                        relocated.push(view.ask_page(pagination.total_pages));
                    }
                }
            }
            for query in relocated {
                if let Some(backend) = &state.backend {
                    backend.request(crate::data::Command::Stories(query));
                }
            }
            match result {
                Ok(page) if applied > 0 => tracing::info!(
                    scope = query.scope.as_str(),
                    page = page.pagination.current_page,
                    stories = page.rows.len(),
                    total = page.pagination.total_records,
                    panes = applied,
                    "a newsroom page was read"
                ),
                Err(error) if applied > 0 => tracing::warn!(
                    scope = query.scope.as_str(),
                    page = query.page,
                    error = %error,
                    panes = applied,
                    "the newsroom page did not read: the pane keeps the rows it has"
                ),
                _ => tracing::debug!(
                    scope = query.scope.as_str(),
                    page = query.page,
                    "a newsroom page arrived after the question changed: discarded"
                ),
            }
            if applied > 0 {
                changed = true;
            }
        }
        crate::data::Reply::TniCommander(Err(error)) => {
            tracing::warn!(
                endpoint = "GET /tni-commander",
                error = %error,
                "the supreme command did not read: the chain of command is drawn without its root"
            );
        }
        // **The filter's vocabularies** (#112). None of these is re-read per frame: they are the
        // backend's vocabulary, and the panel is a projection of it. A failed read is one log line and
        // an empty (or shorter) vocabulary, never a panel that invents rows.
        crate::data::Reply::InstallationTypes(Ok(types)) => {
            tracing::info!(
                types = types.len(),
                endpoint = "GET /installation-types",
                "the facility vocabulary was cached"
            );
            state.installation_types.clone_from(types);
            changed = true;
        }
        crate::data::Reply::InstallationTypes(Err(error)) => {
            tracing::warn!(
                endpoint = "GET /installation-types",
                error = %error,
                "the facility vocabulary did not read: the filter draws no facility rows"
            );
        }
        crate::data::Reply::Installations(Ok(places)) => {
            tracing::info!(
                facilities = places.len(),
                endpoint = "GET /installations",
                "the facilities were cached: the complete set, so a type's count is the cache's"
            );
            state.installations.clone_from(places);
            changed = true;
        }
        crate::data::Reply::Installations(Err(error)) => {
            tracing::warn!(
                endpoint = "GET /installations",
                error = %error,
                "the facilities did not read: the filter's type counts are short until a refresh"
            );
        }
        crate::data::Reply::TacticalObjectFacets(Ok(facets)) => {
            tracing::info!(
                total = facets.total,
                domains = facets.domains.len(),
                categories = facets.categories.len(),
                endpoint = "GET /tactical-objects/facets",
                "the filter vocabulary was cached: the counts are the server's"
            );
            state.facets = Some(facets.clone());
            changed = true;
        }
        crate::data::Reply::TacticalObjectFacets(Err(error)) => {
            tracing::warn!(
                endpoint = "GET /tactical-objects/facets",
                error = %error,
                "the filter vocabulary did not read: the panel draws the layers only"
            );
        }
        _ => {}
    }
    changed
}

/// A hull's length and beam from one specification version, when both were measured.
///
/// Both are needed: a picture scaled from a length and drawn at a guessed beam is a picture that lies
/// about its shape. Absent either one, the hull is drawn clamped (`units::mark`) and says so.
fn size_of(version: &crate::data::types::UnitSpecification) -> Option<crate::units::mark::Size> {
    Some(crate::units::mark::Size {
        length_m: version.loa_m?,
        beam_m: version.beam_m?,
    })
}

/// Once a second, the age text changes: redraw the card, and nothing else.
///
/// This is the ticket's "the age advances without a re-render of the map": the model is untouched, so
/// the map element, the marker layer and every other element diff to themselves.
fn tick_card_age(state: &mut State) -> bool {
    // The age advances on the **frame** clock, not the wall clock: `poll`'s answer decides when Rune
    // rebuilds the element tree, so a real-clock comparison here makes *when* the card is redrawn
    // depend on how fast the machine is — which is exactly how a capture stops being reproducible
    // (#35). Same rule as every other `now` in the view: `State::now()` and nothing else.
    //
    // Per pane, because two cards age at two moments (#99): one wall-wide string made a second card's
    // rebuild depend on the first one's text.
    let now = state.now();
    let mut changed = false;
    for pane in state.panes_mut().iter_mut() {
        let Some(card) = &pane.view.card else {
            continue;
        };
        let text = card.age_text(now);
        if text != pane.view.last_age {
            pane.view.last_age = text;
            changed = true;
        }
    }
    changed
}

/// **Escape, in one place** (#101): the thing being held is released, or the focused pane's card is closed.
///
/// This used to be five `if`s in a row over five fields, each with its own `is_some()`; the order *was* the
/// policy, and it could only be read by reading it. Now it is a match on the state the wall is in, which is
/// the same information the operator sees, and the order is the enum's:
///
/// 1. whatever `interaction` holds — a rename, a lift, the settings sheet, a menu, a filter panel;
/// 2. a maximised pane, which is a **layout** fact rather than a state of being held (a pane drawn over the
///    tiling is not a gesture): Escape is the way back the close plate also offers;
/// 3. the focused pane's own selection — **the newsroom's open story** (#137) and the map's card (#99).
///    Both are the pane's state, so they go last and only in the pane that has one; a pane is a feed or a
///    map, so the two are never both open.
fn on_key(state: &mut State, event: &rune::KeyEvent, _tx: &mut rune::Tx) -> bool {
    // **The menu's keyboard model** (#164): arrows walk the ring, Enter acts on it, Right and Left open and close
    // the waterfall. Every one of them is an intent, so a remote with no keyboard sends the same words.
    if state.menu().is_some() {
        let intent = match event.key {
            rune::Key::Down => Some(Intent::MenuFocus {
                row: Some(next_row(state, 1)),
            }),
            rune::Key::Up => Some(Intent::MenuFocus {
                row: Some(next_row(state, -1)),
            }),
            rune::Key::Right => Some(Intent::MenuSubmenu { open: true }),
            rune::Key::Left => Some(Intent::MenuSubmenu { open: false }),
            rune::Key::Enter => Some(Intent::MenuChoose),
            _ => None,
        };
        if let Some(intent) = intent {
            // A refusal is the model's own answer (a row that opens nothing, no ring to act on): the key is
            // consumed either way, because a menu that let Enter through to the wall would be two gestures.
            control::apply(state, intent).ok();
            return true;
        }
    }
    if event.key != rune::Key::Escape {
        return false;
    }
    // **Escape is an intent like any other** (#170): the ladder that decides which surface goes is
    // `ui::close_surfaces`, and a socket's `surface.close` walks the same one — there is no second answer to
    // "what does dismissing mean" for a client that has no key.
    control::apply(state, Intent::CloseSurfaces).is_ok_and(|changed| changed.0)
}
/// Start the socket actor, once, before the first frame.
///
/// Nothing here blocks: the actor owns its thread and its runtime, and `wake` is how it says "draw
/// this now" without the view loop ever awaiting anything.
fn start_socket(state: &mut State, wake: rune::Waker) {
    let Some(config) = config::current() else {
        // No configuration at all: the wall draws and listens to nothing. That is a deployment fact,
        // not an error, and `--self-check` is where it is reported.
        return;
    };
    state.live = Some(LiveThread::spawn_with(
        config.minos.clone(),
        config.socket_url.clone(),
        config.reconnect_after,
        move || wake(),
    ));
    // A spawned actor is not a live wall: the header says "connecting" until the socket has proved
    // itself and the picture has been re-read (`INV-09`). Leaving the default here would claim there
    // is no credential while one is being used.
    state.link = Link::Unproven;
}

/// Drain what the socket said since the last frame.
fn drain_socket(state: &mut State) -> bool {
    // Drain first, apply second: the updates own their data, and applying while holding the socket
    // borrow would keep the whole state immutable for the length of the drain.
    let updates: Vec<crate::live::Update> = match &state.live {
        Some(live) => std::iter::from_fn(|| live.try_recv()).collect(),
        None => return false,
    };
    let changed = !updates.is_empty();
    for update in updates {
        apply_update(state, update);
    }
    changed
}

/// One socket update, as the view loop takes it.
///
/// Separated from the drain so it can be tested without a socket: what the header says, and what the
/// picture believes, are the two things a wall gets wrong when this is wired by hand.
fn apply_update(state: &mut State, update: crate::live::Update) {
    use crate::live::Update;
    match update {
        Update::Picture(feed) => {
            state.picture.replace_from_feed(&feed);
            state.link = Link::Live;
        }
        Update::Fix { fix, received } => {
            // **This is the wall.** A publication is the only thing that makes the picture newer than
            // the last REST read (#28): counting it and keeping the old position is a photograph with
            // a counter on it.
            state.picture.apply_fix(&fix);
            state.fixes += 1;
            // How long the fix waited for a frame, as a number the log carries (#1): v0.1's definition of
            // done promises a marker moves "within 1 s of arrival", and a promise with no measurement is a
            // sentence. Late is applied — dropping a fix would be worse — and said out loud.
            state.note_fix_latency(received.elapsed());
            // A card is a view of one unit, so it follows that unit's fix. Rebuilt here rather than
            // every frame: a fix for another hull does not change this card's numbers.
            let asking: Vec<ViewId> = state
                .panes()
                .iter()
                .filter(|pane| pane.view.selected == Some(fix.id_unit))
                .map(|pane| pane.id)
                .collect();
            for pane in asking {
                state.rebuild_card(pane);
            }
        }
        // Everything else means "not current": a connect is not a picture (`INV-09`), a refresh is
        // not a picture, and a retry is not a picture.
        Update::Connected | Update::Untrusted | Update::RefreshDue | Update::Retrying { .. } => {
            state.link = Link::Unproven;
        }
        Update::Refused { code, reason } => {
            state.link = Link::Failed(format!("refused {code}: {reason}"))
        }
        Update::Failed(error) => state.link = Link::Failed(error),
    }
}

/// Start the map thread, once, before the first frame. A `fn` because `AppSpec::setup` is one.
/// **One pane of a capture keeps the frame it has always had** (#71, #140, #172).
///
/// A headless wall has no map thread to hand a frame over, and markers still have to project against something
/// real: a capture's single pane gets a **synthetic** frame the size of the window, at the pane's own camera. A
/// scene with more panes gets each frame from that pane's own layout (`pane_laid_out`), which is where a wall gets
/// its frames too.
///
/// It runs in `setup` **and** in `poll`, because a pane can arrive after the scene was built: a new workspace
/// (#140), a move, a close that leaves one pane. Without the second call a wall whose only pane came late drew no
/// markers at all — found by an intent-driven capture whose twin seeds its frame at start-up (#172).
///
/// **A feed or a tree in a one-pane scene gets no frame**: a frame is what a pane's markers project against, and a
/// pane that draws none must not have one to project into.
pub(crate) fn seed_single_pane_frame(state: &mut State) {
    if !state.is_single_pane() || !state.focused_pane().content.is_map() {
        return;
    }
    if state.focused_pane().frame.is_some() {
        return;
    }
    let camera = state.camera();
    state.set_frame(crate::map::geometry::Rendered {
        camera,
        width: tokens::WINDOW_SIZE.0 as u32,
        height: tokens::WINDOW_SIZE.1 as u32,
    });
}

/// **Whether this capture asks the wall to rebuild itself from its own scene** (#201), once.
///
/// `ARCONS_REPLAY=1` is read once and answered once — **the first frame after every scripted step has landed**.
///
/// Earlier than that proves the wall the script started from; every frame rebuilds against a layout that a wall only
/// makes once. One round trip, at the moment a client would have taken one, is what a viewer does with a snapshot.
fn replay_due(state: &State) -> bool {
    replay_enabled()
        && state.frame_clock() > 0.0
        && state.intents_applied >= state.pending_intents.len()
        && !replayed_once()
}

/// Whether the replay has happened, as a flag rather than a field: it is a capture's business, not a wall's.
fn replayed_once() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    DONE.swap(true, Ordering::Relaxed)
}

fn replay_enabled() -> bool {
    use std::sync::OnceLock;
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| std::env::var_os("ARCONS_REPLAY").is_some_and(|value| value != "0"))
}

/// **Dial the authority a viewer was pointed at** (#202) over the one carrier that exists for a process on this
/// machine: the control socket. A relay's carrier arrives with #196, and the session does not change when it does.
#[cfg(unix)]
fn connect_authority(
    path: &std::path::Path,
) -> std::io::Result<crate::control::authority::Session> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    // A scene is small and a viewer waits for one at most once a frame: a read that blocks the session's own thread
    // (never the UI thread, `INV-05`) is the right trade, and a timeout keeps a wedged authority from holding it for
    // ever.
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let carrier = crate::control::carrier::Unix::new(stream)?;
    crate::control::authority::Session::new(Box::new(carrier))
}

fn setup(state: &mut State, waker: rune::Waker) {
    // A scene is offline by construction: `--snap` renders a fixture, and a capture that opened a
    // socket would be a capture that depends on a server being awake.
    // The image cache decodes; it does not fetch, so a headless scene can use the same path a wall uses.
    // It gets the app's waker, because a decode finishing is exactly the event that must draw a frame.
    state.image_cache_with(waker.clone());
    if rune::is_headless() {
        state.headless = true;
        // A scene declares its own viewport: there is no map thread to hand one over, and markers
        // still have to project against something real. The size is the one `capture.sh` pins.
        seed_single_pane_frame(state);
        return;
    }
    // **A viewer connects to the authority it was told about** (#202): the socket beside a wall on this machine, or the
    // path `ARCONS_AUTHORITY` names. With nothing configured this is `Local` and no line is written — the wall owns
    // everything, which is what a capture, a test and a single display are. A viewer that cannot reach its authority
    // says so once and carries on as a wall: a screen that refuses to draw anything because a socket is absent is
    // worse than one drawing its own default wall and saying why.
    #[cfg(unix)]
    {
        let role = crate::control::authority::Role::from_env();
        if let crate::control::authority::Role::Remote(path) = &role {
            match connect_authority(path) {
                Ok(session) => {
                    tracing::info!(
                        "authority: a viewer of {} ({})",
                        path.display(),
                        role.label()
                    );
                    state.session = Some(session);
                }
                Err(error) => tracing::warn!(
                    "authority: {} is not reachable ({error}); this wall owns its own state until it is",
                    path.display()
                ),
            }
        }
    }
    // **Windows has no carrier, and says so** (#195, #202): the control plane is a unix socket and a named pipe is a
    // decision nobody has taken. A `#[cfg(not(unix))]` *branch* would leave the code after it type-checked against
    // fields that do not exist there, so this is a whole block.
    #[cfg(not(unix))]
    if let crate::control::authority::Role::Remote(_) = crate::control::authority::Role::from_env()
    {
        tracing::warn!(
            "authority: no carrier on this platform — the control plane is a unix socket, so this wall owns its state"
        );
    }

    // **The control plane** (#165). A capture returned above: `INV-11` forbids a scene reading or writing a
    // file, and a socket is both. A wall can turn the plane off (`ARCONS_CONTROL=0`) and says so in one line,
    // because "a client cannot connect" is otherwise a mystery with a log to read.
    #[cfg(not(unix))]
    {
        // **A unix socket is the transport** (#165), so a Windows wall has no plane until someone decides on a
        // named pipe. Saying it once here is better than a client discovering it as a timeout.
        tracing::info!("control plane off: this platform has no unix socket");
    }
    #[cfg(unix)]
    {
        if std::env::var("ARCONS_CONTROL").ok().as_deref() == Some("0") {
            tracing::info!("control plane off: ARCONS_CONTROL=0");
        } else {
            let path = crate::control::socket_path();
            match crate::control::Server::spawn(path.clone()) {
                Ok(server) => {
                    tracing::info!(
                        "control plane listening on {} (protocol {}, mode 0600)",
                        server.path().display(),
                        crate::control::PROTOCOL
                    );
                    state.control = Some(server);
                }
                // A wall with no socket is still a wall: the picture is the job, and a client that cannot
                // connect is told why here rather than discovering it as a timeout.
                Err(problem) => {
                    tracing::warn!("control plane not started at {}: {problem}", path.display())
                }
            }
        }
    }
    // A stop has to reach the loop even when nothing is drawing (`#43`): the flag alone is unread while
    // the event loop waits for an event no compositor is going to send.
    let stopping = waker.clone();
    crate::platform::wake_on_termination(move || stopping());
    // And keep the model moving while the compositor is not drawing it: a quiet feed plus a covered
    // window is otherwise a wall that has stopped thinking (`#43`).
    let ticking = waker.clone();
    crate::platform::keep_alive(move || ticking());
    start_socket(state, waker.clone());
    if let Some(config) = config::current() {
        // The card's three reads run here, on the reads thread, never on the UI thread.
        state.backend = crate::data::Backend::spawn(config.minos.clone()).ok();
    }
    if let Some(backend) = &state.backend {
        // **The tactical-object cache, read once at init** (#110): the manifest is the offline library
        // (unpaginated on purpose), and the placements are the operational marks. A refresh is
        // deliberate, not per frame, so neither is asked again by `poll`.
        backend.request(crate::data::Command::TacticalObjectManifest);
        backend.request(crate::data::Command::TacticalObjectPlacements);
        // **The filter panel's vocabularies, and the facilities' cache** (#112): the facility types and
        // the complete installation list (they are unpaginated, so the counts are the cache's), and the
        // tactical-object facets (their counts are the server's). Read once at init, like the manifest:
        // a refresh is deliberate, and a pane does not re-read the vocabulary when it opens.
        backend.request(crate::data::Command::InstallationTypes);
        backend.request(crate::data::Command::Installations);
        backend.request(crate::data::Command::TacticalObjectFacets);
    }
    // **Where the wall opens** (#108): one read, off this thread, for the pane that exists now. The
    // first frame draws the seed camera whether or not it has answered — a late read is not a blank
    // wall — and the answer moves that pane and no other.
    viewpoint::ask(state);
}

/// Every frame: hand the view loop whatever the socket said, and say whether it changed anything.
fn poll(state: &mut State, elapsed: f64, tx: &mut rune::Tx) -> bool {
    // Rune's `elapsed` is the frame clock: virtual in a headless scene, real in a window. Every age
    // in the view is derived from it, so nothing in a capture depends on how fast the machine was.
    state.logical = elapsed;
    // How long the loop was idle before this frame: the lower bound on any fix's wait, and what tells a stall
    // apart from a wall that simply was not being drawn (#1). Measured on **the frame clock**, because that is
    // the clock a frame is allowed to read — and the one a fix's wait is bounded by.
    state.poll_gap = state
        .last_poll_logical
        .map(|last| (elapsed - last).max(0.0))
        .map(std::time::Duration::from_secs_f64)
        .unwrap_or_default();
    state.last_poll_logical = Some(elapsed);
    // **A cursor that has just reached its fade must be drawn once more** (#167): nothing else on the wall
    // changed, so nothing would ask for a frame — and the ring would stay on screen at the last place it was
    // drawn. The frame where visibility flips is the one the fade needs.
    if let Some(cursor) = state.cursor {
        let now = cursor.visible_at(elapsed);
        let before = cursor.visible_at(elapsed - state.poll_gap.as_secs_f64());
        if now != before {
            state.layout_dirty = true;
        }
    }
    // **A scene can drive the wall through the control plane** (#172): the steps it carries are applied here, in
    // order, when their frame comes. This is the same `apply` the pointer and the socket use — a capture is not a
    // second kind of client, it is a client whose clock is virtual.
    let scripted = crate::scene::apply_due_intents(state, elapsed);
    // **A capture can rebuild its own wall** (#201): with `ARCONS_REPLAY=1` the scene's state is turned into a
    // `Scene` and applied straight back, on the first frame. The golden rendered that way is the proof that the
    // `Scene` is the whole arrangement — if a pane's camera, name, filter, selection or surface is missing from it,
    // the picture differs and `tests/scene_replay.rs` says so. A wall never does this: `apply_scene` is what a
    // viewer does with a scene the authority sent (#202).
    // Once, and **after the script**: a scene that an intent script drives changes after its first frame, and a round
    // trip taken before the last step would prove the wall the script started from and nothing it reached.
    if state.headless && replay_due(state) {
        state.rebuild_from_scene();
    }

    // **Every follow, one frame of it** (#169): a pane locked onto a hull follows that hull's fix, and the wall
    // redraws only when the camera actually moved.
    let followed = state.tick_follows();
    // A pane that arrived after the scene was built has no frame from `setup`, and a one-pane wall refuses to
    // hand one over in `pane_laid_out` (#172). Same rule, one frame later — and a capture's, so a wall is left
    // alone.
    if state.headless {
        seed_single_pane_frame(state);
    }
    // systemd stops a wall with `SIGTERM`. Quitting through the runtime's own path drops the GL
    // context in order, instead of a hard exit that leaves the driver complaining.
    if crate::platform::terminated() {
        tracing::info!(
            "terminating on SIGTERM/SIGINT after {} ms",
            crate::platform::since_start()
        );
        // **The stop, in the one order that keeps a clean stop clean** (#153, #174): the arrangement is written
        // while the state is whole, the deadline is armed once nothing of the wall's is left to lose, and the map
        // threads are joined last — that join is the step a wedged thread holds past systemd's timeout.
        stop(
            state,
            &crate::ui::saved::saved_layout_path(),
            &crate::platform::arm_exit_deadline,
        );
        tx.quit();
        return false;
    }

    // **The arrangement is written once it holds still** (#153): `arranged()` records the frame a change
    // happened on, and the write itself goes to a thread — a camera gesture is a hundred changes a second
    // and a `fsync` on the drawing thread would be a stutter on every one of them.
    if state.save_due() {
        state.save_layout();
    }
    // **A viewer draws what the authority says** (#202): one scene request at a time, and whatever has come back is
    // applied here — on the only thread that owns `State`, exactly like a request from the plane. The `seq` makes a
    // scene that has not moved cost one comparison rather than a rebuild.
    #[cfg(unix)]
    let mut viewed = false;
    #[cfg(unix)]
    {
        // Taken out while it is drained, so the session's borrow ends before `apply_scene` needs the state mutably.
        let answers: Vec<_> = match &state.session {
            Some(session) => {
                if !session.busy() {
                    session.send(crate::control::wire::Request::Scene);
                }
                std::iter::from_fn(|| session.next()).collect()
            }
            None => Vec::new(),
        };
        for answer in answers {
            if let crate::control::wire::Answer::Scene(scene) = answer {
                viewed |= state.apply_scene(&scene);
            }
        }
    }
    #[cfg(not(unix))]
    let viewed = false;

    // **The control plane's requests, applied here and nowhere else** (#165): a socket read on this thread
    // would block the frame, and an `apply` on the listener's thread would be two threads changing one wall.
    // The queue is drained, not waited on: a wall that waited for a client would freeze for one that has not
    // finished typing.
    #[cfg(unix)]
    let mut controlled = false;
    #[cfg(unix)]
    while let Some(job) = state
        .control
        .as_ref()
        .and_then(crate::control::Server::next_job)
    {
        // **A change, a look, or the arrangement** (#181, #201): all three are answered here, between frames, because
        // `State` is this thread's — and by `control::authority::answer`, so a wall's loop and a viewer's session
        // cannot grow two opinions about what a request means.
        let (answer, did) = crate::control::authority::answer(state, &job.request);
        controlled |= did;
        job.answer(answer);
    }
    #[cfg(not(unix))]
    let controlled = false;
    let mut changed = drain_socket(state);
    changed |= scripted;
    changed |= followed;
    changed |= controlled;
    changed |= viewed;
    changed |= drain_card_reads(state);
    changed |= tick_card_age(state);
    // **A newsroom pane restored from the file has asked for nothing yet** (#153): one read, on this
    // frame, so the pane that comes back on page 2 draws its rows rather than "reading the feed…" forever.
    changed |= crate::ui::saved::ask_restored_feeds(state);

    // "Start to first map frame" is a number in the log rather than a claim in a ticket. Without the
    // engine there is no map frame, so the first view pass is what gets timed.
    if let Some(frame) = state.frame() {
        // **The camera the map actually rendered, once.** `frame.camera` is what the map thread drew and
        // what markers project against (`INV-04`); the pane's own camera is what was asked for. They
        // disagreeing is a wall showing the wrong sea — which is what #56 was filed for and could not be
        // reproduced, so the next report gets a line to look at instead of a screenshot to argue with.
        let asked = state.camera();
        if !state.headless && !state.camera_reported {
            state.camera_reported = true;
            tracing::info!(
                lat = frame.camera.center.0,
                lon = frame.camera.center.1,
                zoom = frame.camera.zoom,
                width = frame.width,
                height = frame.height,
                asked_lat = asked.center.0,
                asked_lon = asked.center.1,
                asked_zoom = asked.zoom,
                panes = state.pane_count(),
                "the map's first frame"
            );
            if frame.camera != asked {
                tracing::warn!(
                    "the map drew a different camera than the one asked for: markers project against the \
                     drawn frame, so this is the picture the wall shows (#56)"
                );
            }
        }
        crate::platform::first_frame("first map frame");
    } else if cfg!(not(feature = "map")) {
        crate::platform::first_frame("first frame (no map engine)");
    }
    changed |= drain_card_reads(state);
    changed |= tick_card_age(state);

    // **A map pane always has a thread** (#160): every pane that draws a picture, in every workspace,
    // gets one on the frame it is missing one — a wall restored from `layout.toml`, a pane moved
    // between workspaces, and a pane opened by `split_pane` all take this door. It runs before the
    // cadence pass so a thread that started this frame is told its rate on this frame.
    changed |= state.spawn_missing_maps();

    // **The focused pane is the one that renders at the wall's cadence; the others are there, not
    // live** (#71). Told to the threads when the focus changes, so a wall at rest does not send a
    // message a frame.
    #[cfg(feature = "map")]
    if !state.headless && state.cadenced != Some(state.focused()) {
        state.cadenced = Some(state.focused());
        state.apply_cadence();
    }
    // The frame each pane's map last handed over is what that pane's markers project against
    // (`INV-04`). Reading it here, rather than in the view, keeps the view a function of the state.
    // The panes are walked **mutably and by value**, not indexed out of `panes()`: the handover writes
    // each pane's own two fields, and a slice cannot be written through. `panes_mut` is the active
    // workspace's, which is the only one whose maps are at the wall cadence (#138).
    #[cfg(feature = "map")]
    for pane in state.panes_mut() {
        let rendered = pane.map.as_ref().and_then(|map| map.rendered());
        if let Some(frame) = rendered
            && pane.frame != Some(frame)
        {
            pane.frame = Some(frame);
        }
        if pane.frame != pane.frame_seen {
            pane.frame_seen = pane.frame;
            changed = true;
        }
    }
    if state.layout_dirty {
        state.layout_dirty = false;
        changed = true;
    }
    // **The range follows the pane's own frame's zoom, not the camera's** (#115): mid-gesture they
    // disagree, and a hull must not switch representation while it is still projected against the
    // previous frame. One latch per pane, because each pane has its own zoom — a pane zoomed into a
    // hull's picture must not switch its neighbour's representation.
    for pane in state.panes_mut() {
        if let Some(frame) = pane.frame {
            let latched = pane.view.range.update(frame.camera.zoom);
            if latched != pane.view.range {
                pane.view.range = latched;
                changed = true;
            }
        }
    }
    // In picture range **any pane** needs two things the feed does not carry: the register (picture URLs,
    // one read for the fleet) and each hull's dimensions (what the picture is scaled by). The register, the
    // decoded pictures and the per-frame budget are the wall's shared resources — one cache, several
    // views (#115) — so the ask is gated by any pane being in picture range, not by the focused one.
    if state
        .panes()
        .iter()
        .any(|pane| pane.view.range == crate::units::mark::Range::Picture)
        && let Some(backend) = &state.backend
    {
        if !state.asked_register && due(state.now(), state.register_retry_at) {
            state.asked_register = true;
            backend.request(crate::data::Command::Units);
        }
        let mut asked_images = 0;
        for unit in state.picture.units() {
            if asked_images >= 2 {
                break;
            }
            let id = unit.unit.id_unit;
            if unit.position.is_some()
                && !state.pictures.contains_key(&id)
                && !state.asked_images.contains(&id)
                && due(
                    state.now(),
                    state.image_retries.get(&id).map(|retry| retry.at),
                )
                && let Some(url) = state.image_urls.get(&id).cloned()
            {
                state.asked_images.insert(id);
                backend.request(crate::data::Command::Image(id, url));
                asked_images += 1;
            }
        }
        let mut asked = 0;
        for unit in state.picture.units() {
            if asked >= 4 {
                break;
            }
            if unit.position.is_some()
                && !state.sizes.contains_key(&unit.unit.id_unit)
                && !state.asked_sizes.contains(&unit.unit.id_unit)
            {
                state.asked_sizes.insert(unit.unit.id_unit);
                backend.request(crate::data::Command::Specifications(unit.unit.id_unit));
                asked += 1;
            }
        }
    }

    // **The tactical-object artwork is fetched once per symbol** (#110), off the UI thread: the
    // manifest names an `asset_key` and a presigned URL, and the decoded bytes are cached by the key
    // the way a hull's picture is. Only the symbols the map actually places are fetched, a few per
    // frame; an unsigned entry is logged once and draws the generic mark rather than failing a read.
    if let Some(backend) = &state.backend {
        // **Both layers name symbols**: a placement embeds its own code, and a facility's code is its
        // *kind's* (#109). One list, deduplicated, so a code both layers use is fetched once.
        let mut codes: Vec<String> = state
            .placements
            .iter()
            .map(|placement| placement.symbol.code.clone())
            .collect();
        for installation in &state.installations {
            if let Some(code) = state
                .installation_types
                .iter()
                .find(|kind| kind.id == installation.id_installation_type)
                .and_then(|kind| kind.symbol_code.clone())
            {
                codes.push(code);
            }
        }
        codes.sort();
        codes.dedup();
        let mut asked = 0;
        for code in &codes {
            if asked >= ASSETS_PER_FRAME {
                break;
            }
            let code = code.as_str();
            let Some(entry) = state.manifest.get(code) else {
                // A placed symbol is always in the manifest the library serves. A code that is not is
                // a library the wall's cache has not caught up with: still draw something, and say so
                // once. The empty-manifest case is "not read yet", not a missing symbol.
                if !state.manifest.is_empty() && state.generic_logged.insert(code.to_string()) {
                    tracing::warn!(
                        symbol = %code,
                        "the symbol is not in the cached manifest: the map draws the generic mark"
                    );
                }
                continue;
            };
            let key = entry.asset_key.clone();
            if state.assets.contains_key(&key)
                || state.asked_assets.contains(&key)
                || state.asset_failures.contains(&key)
            {
                continue;
            }
            let Some(url) = entry.asset_url.clone() else {
                if state.generic_logged.insert(key) {
                    tracing::warn!(
                        symbol = %code,
                        "the symbol's asset_url is unsigned: the map draws the generic mark"
                    );
                }
                continue;
            };
            state.asked_assets.insert(key.clone());
            backend.request(crate::data::Command::TacticalAsset(key, url));
            asked += 1;
        }
    }

    // **A tree pane's reads** (#116): the structures, their forests and the supreme command, asked once
    // each and only when a tree pane exists. Nothing here waits — the answers arrive between frames.
    ask_tree_reads(state);

    // A picture still decoding is a reason to draw the next frame, and the frame a decode *lands* on is a
    // reason to draw one more. Both matter, and the second is the one that was missing (`#51`): a decode
    // that completes between two polls changes nothing the loop would otherwise notice, so a wall that
    // only redrew "while pending" kept the last tree — the symbol — forever. The waker now wakes the app
    // too, but a headless render is not waiting on one, and this costs two comparisons per frame.
    changed |= pictures_need_a_frame(&mut state.pictures_pending, &state.pictures);
    changed |= assets_need_a_frame(&mut state.assets_pending, &state.assets);
    // A marker still travelling between two fixes needs the next frame drawn, and stops asking on its
    // own when the travel window ends: a wall at rest rebuilds nothing.
    if state.picture.travelling(state.now()) {
        changed = true;
    }
    // Every 30 s the log says what the wall has received, so "it is not moving" is a line in a file
    // rather than a report that ends in a relaunch (#28). Nothing here is drawn: the header already
    // carries the count, and a wall has no console.
    // On the wall's own clock, not the frame clock: a log line is about *time passing*, and a window the
    // compositor is not drawing must still say once a minute that it is alive. Headless scenes skip it —
    // there is no elapsed time there, only a virtual clock (`INV-11`).
    let uptime_ms = crate::platform::since_start() as u64;
    if !state.headless
        && uptime_ms.saturating_sub(state.last_report_ms) >= REPORT_EVERY.as_millis() as u64
    {
        state.last_report_ms = uptime_ms;
        tracing::info!(
            fixes = state.fixes,
            units = state.picture.units().len(),
            // Two numbers, because their disagreement is the bug (#52): a wall that knows 25 hulls while
            // the feed carries 127 is reading one page of the register.
            registered = state.registered,
            pictures = state.image_urls.len(),
            link = ?state.link,
            // One number per pane (#71): a tiled wall quietly drawing three of four panes is a line in a
            // file rather than a report that ends in a relaunch (#28, #51).
            pane_frames = ?state.pane_frames(),
            // The socket queue's worst wait, in ms, and how idle the loop was when it was applied: `-1`
            // before the first fix. Two numbers, because "late" and "not drawing" are different failures.
            fix_ms = state.fix_latency.max_ms(),
            fix_gap_ms = state.fix_latency.max_gap_ms(),
            fix_idle = state.fix_latency.idle(),
            // A number or -1: a wall whose platform cannot say must say so, not invent a figure.
            rss_mb = crate::platform::rss_mb().map(|mb| mb.round() as i64).unwrap_or(-1),
            "socket"
        );
    }

    // The next frame starts a new gesture: a marker's press has done its job either way.
    state.marker_press = false;
    changed
}

/// A picture that failed, and when it may be asked for again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retry {
    /// How many times **this** picture has failed. The first failure is 1, not 0, so the log reads the way
    /// an operator counts.
    pub attempts: u32,
    /// When to ask again, on the wall's own clock (`State::now`), never the wall clock (`INV-11`).
    pub at: std::time::Instant,
}

/// How long to wait before asking for a failed picture again: 5 s, doubling, capped at five minutes.
///
/// A doubling wait is the difference between a backend that is down for a minute and a wall that hammers it
/// sixty times a second; the cap is the difference between "retrying" and "given up" — this wall does not
/// give up, because a picture that failed once is usually a URL that expired, and the register re-read a
/// moment later hands out a fresh one (`#53`).
pub fn retry_after(attempts: u32) -> std::time::Duration {
    const FIRST: u64 = 5;
    const CAP: u64 = 300;
    let doublings = attempts.saturating_sub(1).min(16);
    std::time::Duration::from_secs((FIRST << doublings).min(CAP))
}

/// The register is one read for the whole fleet, so its retry is a flat minute rather than a curve: it is
/// never asked per hull, and a wall whose account cannot read `/units` should say so once a minute rather
/// than every frame.
pub const REGISTER_RETRY: std::time::Duration = std::time::Duration::from_secs(60);

/// Whether a wait has expired. `None` is "never waited", which is always due.
fn due(now: std::time::Instant, at: Option<std::time::Instant>) -> bool {
    at.is_none_or(|at| now >= at)
}

/// Whether this frame must be drawn because of a picture: while a decode is pending, **and** on the frame
/// it lands.
///
/// The second half is the whole of `#51`. A decode that completes between two polls changes nothing the
/// loop can otherwise see — the marker layer is not rebuilt, so the last tree (the symbol) stays on screen
/// until some unrelated event draws again. So a hull whose picture has just become drawable is taken off
/// the pending set *and* the frame that took it off is drawn; everything after it is quiet.
fn pictures_need_a_frame(
    pending: &mut std::collections::HashSet<i64>,
    pictures: &std::collections::HashMap<i64, rune::Image>,
) -> bool {
    if pending.is_empty() {
        return false;
    }
    let landed: Vec<i64> = pending
        .iter()
        .filter(|id| pictures.get(id).is_some_and(|picture| picture.is_ready()))
        .copied()
        .collect();
    for id in landed {
        pending.remove(&id);
    }
    true
}

/// The same rule for a tactical object's artwork, keyed by the manifest's `asset_key` (#110).
fn assets_need_a_frame(
    pending: &mut std::collections::HashSet<String>,
    assets: &std::collections::HashMap<String, rune::Image>,
) -> bool {
    if pending.is_empty() {
        return false;
    }
    let landed: Vec<String> = pending
        .iter()
        .filter(|key| assets.get(*key).is_some_and(|asset| asset.is_ready()))
        .cloned()
        .collect();
    for key in landed {
        pending.remove(&key);
    }
    true
}

/// How many tactical-object assets are asked for in one frame. Off the UI thread either way, but a
/// wall that fired all 56 at once would open 56 requests at boot for the sake of the three it draws.
pub const ASSETS_PER_FRAME: usize = 4;

/// How often the wall says what it has received. Long enough to be a heartbeat, short enough that a
/// frozen picture is noticed inside a coffee break.
const REPORT_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// Open the display and run until the compositor closes it.
pub fn run(window: config::Window, passthrough: Vec<String>) -> ExitCode {
    let spec = AppSpec::<State> {
        min_size: Some(tokens::MIN_WINDOW),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: crate::scene::SCENES,
        ..AppSpec::new(
            TITLE,
            window.size,
            || State::default().with_saved_settings().with_saved_layout(),
            view,
        )
    };

    // Rune's own flags (`--snap`, `--scene`, `--tree`, `--script`) arrive here untouched: #9's
    // captures are `cargo run -- --snap out/<scene>.png`, and swallowing the flag would make the
    // capture succeed at producing nothing.
    let result = rune::try_run_windows_with_args(
        rune::RunOptions::default(),
        passthrough,
        move |displays| {
            let options = window_options(window, displays);
            [rune::WindowSpec::from_app(
                "main",
                rune::App::new(spec).window_options(options),
            )]
        },
    );

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // `tracing`, not `eprintln!`: with `--log-file` the log is the only artefact a wall leaves
            // behind, and the two writers agree on where a line belongs (#53).
            tracing::error!("cannot open a window: {message}");
            tracing::error!(
                "hint: the display needs a compositor; `cargo run -- --version` does not"
            );
            ExitCode::FAILURE
        }
    }
}

/// **What a wall does when it is told to stop** (#153, #174), as one function, so the order can be read and
/// tested rather than trusted.
///
/// 1. **The arrangement is written** while the state is whole: a `SIGTERM` a second after a split must not lose
///    the split, and a slow disk must not turn a clean stop into a truncated file — so this completes first.
/// 2. **The deadline is armed**, and only now: Rune ends the process if the quit is still running after
///    [`crate::platform::QUIT_DEADLINE`]. Armed before the write, a slow disk would be a lost arrangement.
/// 3. **The map threads are joined**, which is the step that can hang: the engine asserts in `~RunLoop` when a
///    process with a live map walks out, and with N panes there are N contexts to close (#71). A thread that will
///    not let go now costs a logged stop instead of a `SIGKILL`.
///
/// `arm` is a parameter rather than a call inside so a test can watch the order against a file it owns: the closure
/// runs at the moment the wall would arm the deadline, not afterwards.
pub fn stop(state: &mut State, layout: &std::path::Path, arm: &dyn Fn()) {
    state.flush_layout_at(layout);
    arm();
    state.close_maps();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn frame_once(state: State) -> Runtime<State> {
        // Headless: layout and paint with no display and no GPU. The virtual clock is 0.0, so a
        // first frame is deterministic (INV-11 in waiting).
        let mut rt = Runtime::new(
            state,
            view,
            tokens::WINDOW_SIZE,
            Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        rt.frame(0.0);
        rt
    }

    #[test]
    fn the_placeholder_panel_is_drawn_inside_the_window() {
        let rt = frame_once(State::default());
        let (rect, opacity) = rt
            .presented("picture")
            .expect("the picture panel must be drawn");
        assert!(opacity > 0.9, "the panel must be opaque, got {opacity}");
        let (x, y, w, h) = (rect[0], rect[1], rect[2], rect[3]);
        assert!(w > 0.0 && h > 0.0, "the panel must have area, got {w}x{h}");
        assert!(
            x >= 0.0
                && y >= 0.0
                && x + w <= tokens::WINDOW_SIZE.0
                && y + h <= tokens::WINDOW_SIZE.1,
            "the panel must stay inside the window, got {rect:?}"
        );
    }

    #[test]
    fn a_reconnecting_wall_says_so_above_the_map_and_never_on_top_of_it() {
        // The banner is a row, not an overlay: a wall whose socket is down must not hide the picture
        // it is still showing, and must not look like it has stopped reporting.
        let mut state = state_with_two_hulls();
        state.link = Link::Unproven;
        let rt = frame_once(state);
        let (banner, opacity) = rt
            .presented("reconnect-banner")
            .expect("the banner is drawn");
        assert!(opacity > 0.9);
        // The map with the engine, the picture panel without it: either way, the thing below.
        let (map, _) = rt
            .presented("map")
            .or_else(|| rt.presented("picture"))
            .expect("the picture is drawn");
        assert!(
            banner[1] + banner[3] <= map[1] + 0.5,
            "the banner covers the map: banner {banner:?} over map {map:?}"
        );

        // It is absent when the wall is live: a banner that is always there is furniture.
        let mut state = state_with_two_hulls();
        state.link = Link::Live;
        assert!(frame_once(state).find("reconnect-banner").is_none());
    }

    /// **The header's newsroom control opens a feed pane that asks page 1** (#136) — the same path the
    /// tree control takes, so a one-pane wall gets its first newsroom the way a split wall gets another.
    /// The ask is recorded on the pane, which is what makes the reply checkable and a late page
    /// discardable; a scene has no reads thread, so the backend being absent is the point.
    #[test]
    fn the_header_control_opens_a_newsroom_pane_that_asks_page_one() {
        let mut state = State::default();
        let first = state.focused();
        open_newsroom_pane(&mut state);
        assert_eq!(state.pane_count(), 2, "the control splits the wall");
        let opened = state.focused();
        assert_ne!(
            opened, first,
            "the pane the operator asked for has the keys"
        );
        let pane = state.pane(opened).expect("the new pane");
        assert_eq!(pane.content, PaneContent::Newsroom);
        assert!(
            pane.frame.is_none(),
            "a feed has no frame: nothing is projected into it"
        );
        let view = pane.news.as_ref().expect("a newsroom pane holds its view");
        assert_eq!(view.scope, crate::data::types::NewsScope::International);
        assert_eq!(view.page, 1);
        assert_eq!(
            view.asked,
            Some(crate::data::types::NewsQuery {
                scope: crate::data::types::NewsScope::International,
                window: crate::data::types::NewsWindow::Latest,
                page: 1,
            }),
            "the first page is asked on the frame the pane is opened"
        );
        // And a second press is a second pane, on its own tab and page.
        open_newsroom_pane(&mut state);
        assert_eq!(state.pane_count(), 3);
        assert_eq!(
            state
                .panes()
                .iter()
                .filter(|pane| pane.news.is_some())
                .count(),
            2,
            "two feeds, two views"
        );
    }

    /// **A pane that cannot read says so**, instead of "reading the feed…" for the life of the process: a
    /// wall with no credential has no reads thread, and the pane is where the operator would look.
    #[test]
    fn a_newsroom_pane_with_no_reads_thread_says_so() {
        let mut state = State::default();
        assert!(
            state.backend.is_none(),
            "a state with no wall behind it has no reads thread"
        );
        open_newsroom_pane(&mut state);
        let view = state
            .panes()
            .iter()
            .find_map(|pane| pane.news.as_ref())
            .expect("the feed pane");
        assert!(
            matches!(&view.state, newsroom::ReadState::Failed(why) if why.contains("credential")),
            "the pane must say why it cannot read, got {:?}",
            view.state
        );
    }

    /// **A reply for a question no pane is waiting for changes nothing** (#136): the routing half of the
    /// late-page rule. The data layer carries the query back for exactly this, so a regional page cannot
    /// draw under the national heading.
    #[test]
    fn a_reply_for_a_query_no_pane_asked_is_discarded() {
        let mut state = State::default();
        open_newsroom_pane(&mut state);
        let stranger = crate::data::types::NewsQuery {
            scope: crate::data::types::NewsScope::National,
            window: crate::data::types::NewsWindow::Latest,
            page: 3,
        };
        let before = state
            .panes()
            .iter()
            .find_map(|pane| pane.news.as_ref())
            .map(|view| (view.scope, view.state.clone(), view.asked))
            .expect("the feed pane");
        let changed = apply_reply(
            &mut state,
            &crate::data::Reply::Stories(stranger, Err(crate::data::Error::Unauthorized)),
        );
        assert!(!changed, "a late page is not a redraw");
        let view = state
            .panes()
            .iter()
            .find_map(|pane| pane.news.as_ref())
            .expect("the feed pane");
        assert!(view.stories.is_empty(), "and it draws no rows");
        assert_eq!(
            (view.scope, view.state.clone(), view.asked),
            before,
            "a stale reply leaves the pane exactly as it was: the tab, the state and the ask are the \
             operator's"
        );
    }

    /// **Escape closes the newsroom's story before anything wall-wide** (#137): the detail is the
    /// pane's own state, on the same rung as the map's card, and a second Escape with nothing open is
    /// not consumed.
    #[test]
    fn escape_closes_the_newsroom_story_and_nothing_else() {
        let mut state = State::default();
        open_newsroom_pane(&mut state);
        let pane = state.focused();
        let escape = rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        };
        let mut tx = rune::Tx::default();
        assert!(
            !on_key(&mut state, &escape, &mut tx),
            "Escape with no story open is not consumed"
        );

        // A story open, through the view's own two steps: press the row, apply the read.
        let id = 1103;
        assert!(
            state
                .pane_mut(pane)
                .and_then(|pane| pane.news.as_mut())
                .is_some_and(|view| view.select(id))
        );
        let mut tx = rune::Tx::default();
        assert!(
            on_key(&mut state, &escape, &mut tx),
            "Escape closes the open story"
        );
        let view = state
            .pane(pane)
            .and_then(|pane| pane.news.as_ref())
            .expect("the feed pane");
        assert_eq!(view.selected, None, "the list is back");
        assert_eq!(view.detail, None);

        // And it did not touch the list's own question: the tab is still the operator's.
        assert_eq!(
            view.scope,
            crate::data::types::NewsScope::International,
            "closing a story is not a tab press"
        );
        let mut tx = rune::Tx::default();
        assert!(
            !on_key(&mut state, &escape, &mut tx),
            "with the list back, Escape is nothing again"
        );
    }

    /// **Three workspaces, and only the first has a pane** (#138): the wall opens as it always did and
    /// two places to put panes sit beside it, empty.
    #[test]
    fn the_default_wall_has_three_workspaces_and_only_the_first_has_a_pane() {
        let state = State::default();
        assert_eq!(state.workspaces.len(), 3);
        assert_eq!(state.active, 0);
        let names: Vec<&str> = state
            .workspaces
            .iter()
            .map(|workspace| workspace.name.as_str())
            .collect();
        assert_eq!(names, vec!["Workspace 1", "Workspace 2", "Workspace 3"]);
        assert_eq!(state.panes().len(), 1, "the wall as it opens: one pane");
        assert!(state.layout().is_some());
        for workspace in state.workspaces.iter().skip(1) {
            assert!(
                workspace.layout.is_none() && workspace.panes.is_empty(),
                "{:?} is a place panes can be, not a copy of the wall",
                workspace.name
            );
        }
    }

    /// **Ids are the wall's** (#138): a pane keeps its number whichever workspace holds it, and a closed
    /// pane's number is never handed out again — that is what makes a move between workspaces possible.
    #[test]
    fn a_pane_id_is_the_walls_and_is_never_reissued() {
        let mut state = State::default();
        let first = state.focused();
        let second = state.split_pane(first, Camera::START).expect("a split");
        assert!(state.pane(second).is_some());
        assert!(
            state.close_pane(second),
            "a pane of a two-pane workspace closes"
        );
        assert!(state.pane(second).is_none());
        let third = state
            .split_pane(first, Camera::START)
            .expect("another split");
        assert_ne!(
            third, second,
            "a closed pane's id is not reused: a stale reference would find a stranger"
        );
        // And a pane in a hidden workspace is still *a pane of the wall*.
        let hidden = state.allocate_pane();
        state.workspaces[2].layout = Some(Layout::single_with(hidden));
        state.workspaces[2]
            .panes
            .push(Pane::new(hidden, Camera::START));
        assert!(state.pane(hidden).is_some());
        assert!(
            !state.pane_ids().contains(&hidden),
            "the hidden workspace's pane is not in the active tiling"
        );
        assert_eq!(
            state.pane_count(),
            2,
            "and not counted in the active one either"
        );
    }

    /// **Switching keeps what each workspace was looking at** (#138): its panes, its cameras and its
    /// focus ring, because they live on the workspace and not on the wall.
    #[test]
    fn switching_keeps_each_workspaces_panes_and_focus() {
        let mut state = State::default();
        let first = state.focused();
        let second = state.split_pane(first, Camera::START).expect("a split");
        state.focus_pane(first);
        state.set_camera_of(second, Camera::new((1.5, 2.5), 9.0));

        // A workspace of its own, with a pane of its own.
        let index = state.add_workspace();
        assert_eq!(index, 3);
        assert_eq!(state.active, 3);
        assert_eq!(state.active().name, "Workspace 4");
        assert!(state.layout().is_none(), "the new workspace is empty");
        let own = state.allocate_pane();
        state.workspaces[index].layout = Some(Layout::single_with(own));
        state.workspaces[index]
            .panes
            .push(Pane::new(own, Camera::START));

        assert!(state.switch_to(0));
        assert_eq!(
            state.focused(),
            first,
            "workspace 1's focus ring is its own"
        );
        assert_eq!(state.camera_of(second).center, (1.5, 2.5));
        assert_eq!(state.pane_count(), 2);

        assert!(state.switch_to(index));
        assert_eq!(state.focused(), own);
        assert_eq!(state.pane_count(), 1);
        assert!(
            !state.switch_to(index),
            "the workspace already on screen is not a switch"
        );
        assert!(!state.switch_to(99), "there is no such workspace");
    }

    /// **A workspace rename commits to that workspace, and a pane rename to its pane** (#139): the sheet
    /// is one sheet with a target, so this is the test that keeps the two from swapping.
    /// **Empty is not a name** (#139): a workspace falls back to its default `Workspace N`, because the
    /// header has to say something; a pane's empty field removes its name, as it always has.
    /// **A long workspace name is cut with the same ellipsis a pane's is** (#139), and **Escape leaves the
    /// old one** — abandoning a rename is not a commit of nothing.
    /// **`Focus on new workspace` keeps the source and makes a new pane of the same kind** (#140): a new
    /// workspace, active, with one pane at the source's camera — the source workspace untouched.
    #[test]
    fn a_new_workspace_takes_a_pane_of_the_sources_kind() {
        let mut state = State::default();
        let source = state.focused();
        let camera = Camera::new((1.25, 2.5), 9.5);
        state.set_camera_of(source, camera);

        let created =
            new_workspace_from_pane(&mut state, source).expect("a new workspace and pane");
        assert_eq!(state.workspaces.len(), 4, "the wall opens with three");
        assert_eq!(state.active, 3, "and the new one is on screen");
        assert_ne!(created, source, "a new pane, not a move");
        let pane = state.pane(created).expect("the new pane");
        assert!(pane.content.is_map(), "a map source makes a map pane");
        assert_eq!(pane.camera.center, camera.center, "at the source's camera");
        assert_eq!(state.pane_count(), 1, "one pane in the new workspace");
        assert_eq!(
            state.workspaces[0].panes.len(),
            1,
            "and the source workspace keeps its pane"
        );
        assert_eq!(state.focused(), created);
        assert!(
            state.menu().is_none(),
            "the action closes the menu it was pressed in"
        );

        // A newsroom source makes a newsroom pane, at its default tab and page.
        let mut state = State::default();
        let map = state.focused();
        let news = state
            .split_pane_as(map, Camera::START, PaneContent::Newsroom)
            .expect("a feed beside the map");
        let created = new_workspace_from_pane(&mut state, news).expect("a new workspace");
        let pane = state.pane(created).expect("the new pane");
        assert!(pane.news.is_some(), "a feed source makes a feed pane");
        let view = pane.news.as_ref().expect("its view");
        assert_eq!(view.scope, crate::data::types::NewsScope::International);
        assert_eq!(
            view.page, 1,
            "a fresh view, not a copy of the source's page"
        );
        assert!(
            view.stories.is_empty(),
            "and not a copy of the source's rows: a new pane, not a clone"
        );
    }

    /// **A move takes the pane, not a copy** (#140): the id, the content and the view travel, the source
    /// keeps whatever else it had, and the target becomes active.
    #[test]
    fn a_move_carries_the_pane_and_leaves_the_rest() {
        let mut state = State::default();
        let map = state.focused();
        let news = state
            .split_pane_as(map, Camera::START, PaneContent::Newsroom)
            .expect("a feed beside the map");
        // The feed has a page of its own to lose if the pane were cloned.
        if let Some(view) = state.pane_mut(news).and_then(|pane| pane.news.as_mut()) {
            view.scope = crate::data::types::NewsScope::Regional;
            view.page = 2;
        }
        assert!(move_pane_to(&mut state, news, 1));
        assert_eq!(state.active, 1, "the target is the workspace on screen");
        let moved = state.pane(news).expect("the pane is still the wall's");
        assert_eq!(moved.id, news, "and it kept its id");
        let view = moved.news.as_ref().expect("its own view");
        assert_eq!(view.scope, crate::data::types::NewsScope::Regional);
        assert_eq!(view.page, 2, "the pane's own state travelled with it");
        assert_eq!(state.pane_count(), 1, "alone in an empty target");
        assert_eq!(
            state.workspaces[0].panes.len(),
            1,
            "the source keeps its map"
        );
        assert!(state.workspaces[0].panes[0].content.is_map());
        assert!(state.menu().is_none());
        assert!(
            !move_pane_to(&mut state, news, 1),
            "a pane cannot move to where it is"
        );
    }

    /// **Moving a workspace's last pane leaves it empty** (#140): an empty workspace is the model's, and
    /// the source's tiling goes with its last pane.
    #[test]
    fn moving_the_last_pane_empties_its_workspace() {
        let mut state = State::default();
        // Workspace 2 gets a pane of its own, so moving it away empties it.
        let index = state.add_workspace();
        let id = state.allocate_pane();
        state.workspaces[index].layout = Some(Layout::single_with(id));
        state.workspaces[index]
            .panes
            .push(Pane::new(id, Camera::START));
        assert!(move_pane_to(&mut state, id, 0));
        assert!(
            state.workspaces[index].layout.is_none() && state.workspaces[index].panes.is_empty(),
            "the source workspace is empty, which is a state and not an error"
        );
        assert_eq!(state.active, 0, "and the target is active");
        assert_eq!(
            state.pane_count(),
            2,
            "the target's pane, and the one that moved in"
        );
    }

    #[test]
    fn the_window_can_be_told_to_have_no_decorations() {
        // A wall has no window manager affordances to click, and `borderless` is how Rune says so.
        let plain = window_options(config::Window::default(), &[]);
        assert!(!plain.borderless, "the default window is decorated");

        let frozen = config::Window {
            decorations: false,
            ..config::Window::default()
        };
        let options = window_options(frozen, &[]);
        assert!(options.borderless, "--no-decorations is borderless");
        assert_eq!(options.min_size, Some(tokens::MIN_WINDOW));
    }

    #[test]
    fn a_picture_read_makes_the_wall_live_and_that_is_the_only_thing_that_does() {
        // The wiring a wall gets wrong by hand: "connected" is not "live". Only a completed picture
        // read may make the header claim the picture is current (INV-09).
        let mut state = State::default();
        assert_eq!(state.link, Link::Absent);
        apply_update(&mut state, crate::live::Update::Connected);
        assert_eq!(
            state.link,
            Link::Unproven,
            "a connect proves the socket, not the picture"
        );
        apply_update(&mut state, crate::live::Update::Untrusted);
        assert_eq!(state.link, Link::Unproven);

        let feed = vec![
            crate::data::types::FeedPosition {
                id_unit: 8,
                name: "KRI Abdul Halim Perdanakusuma".into(),
                hull_number: Some("KRI-355".into()),
                position: None,
            },
            crate::data::types::FeedPosition {
                id_unit: 1,
                name: "KRI Cakra".into(),
                hull_number: None,
                position: None,
            },
        ];
        apply_update(&mut state, crate::live::Update::Picture(feed));
        assert_eq!(state.link, Link::Live);
        assert_eq!(
            state.picture.len(),
            2,
            "the read is authoritative for membership"
        );

        apply_update(
            &mut state,
            crate::live::Update::Refused {
                code: 101,
                reason: "bad token".into(),
            },
        );
        assert!(
            matches!(&state.link, Link::Failed(message) if message.contains("101")),
            "a refusal must be visible, got {:?}",
            state.link
        );
    }

    fn picture_with_two_hulls() -> Picture {
        use crate::data::types::{FeedFix, FeedPosition};
        use chrono::{TimeZone, Utc};
        let fix = |lat: f64, speed: Option<f64>| FeedFix {
            latitude: lat,
            longitude: 106.9,
            speed_kn: speed,
            speed_ms: None,
            course_deg: None,
            accuracy_m: None,
            recorded_at: Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap(),
            received_at: Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap(),
            age_seconds: 4.0,
            backfilled: false,
        };
        let mut picture = Picture::default();
        picture.replace_from_feed(&[
            FeedPosition {
                id_unit: 8,
                name: "KRI Abdul Halim Perdanakusuma".into(),
                hull_number: Some("KRI-355".into()),
                position: Some(fix(-6.10, Some(0.0))),
            },
            FeedPosition {
                id_unit: 9,
                name: "KRI Cakra".into(),
                hull_number: None,
                position: Some(fix(-6.11, None)),
            },
        ]);
        picture
    }

    /// The focused pane's card, for tests that are about one pane's card (#99).
    fn card_of(state: &State) -> Option<&crate::units::card::Card> {
        state.card_in(state.focused())
    }

    /// The age text the focused pane's card drew last, which is what `tick_card_age` compares.
    fn card_age(state: &State) -> String {
        state
            .pane(state.focused())
            .map(|pane| pane.view.last_age.clone())
            .unwrap_or_default()
    }

    fn state_with_two_hulls() -> State {
        let camera = Camera::new((-6.108, 106.910), 11.0);
        let mut state = State::new(picture_with_two_hulls());
        state.set_camera(camera);
        state.set_frame(crate::map::geometry::Rendered {
            camera,
            width: 1280,
            height: 720,
        });
        state
    }

    /// The card's age advances on the **frame** clock. #35: it advanced on the wall clock, so `poll`
    /// answered "changed" at a moment that depended on how fast the machine was — and since `poll`'s
    /// answer is what makes Rune rebuild the element tree, *which* age was drawn into a capture was a
    /// race. On a fast machine a two-second scene drew `4 s old`; on a slow one, `5 s old`.
    /// **The frame a picture's decode lands on is a frame that must be drawn.**
    ///
    /// This is `#51` as a test. The wall used to redraw only *while* a picture was pending
    /// (`if !is_ready() { changed = true }`), so the moment the decode finished the loop went quiet — and
    /// the symbol the last tree held stayed on screen forever, with the picture fetched, decoded and
    /// never drawn. Every call here for a pending or just-landed hull must say "draw"; the first call
    /// after it has landed must say "nothing to do".
    #[test]
    fn the_frame_a_picture_lands_on_is_drawn() {
        let bytes: std::sync::Arc<[u8]> = std::sync::Arc::from(crate::scene::PHOTO_FIXTURE);
        let mut state = State::default();
        state.offer_picture(8, bytes);
        let mut pending = state.pictures_pending.clone();
        assert_eq!(pending.len(), 1, "offering a picture schedules its frame");

        let mut landed_at = None;
        for frame in 0..600 {
            let changed = pictures_need_a_frame(&mut pending, &state.pictures);
            assert!(
                changed,
                "frame {frame}: a picture that has not landed yet, or has just landed, must be drawn"
            );
            if pending.is_empty() {
                landed_at = Some(frame);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let landed_at = landed_at.expect(
            "the fixture picture never decoded: the wall would draw a symbol forever (#51)",
        );
        // The number the ticket asks for: the frame at which the picture became drawable. A frame count,
        // not a duration, so it says the same thing on a slow machine and a fast one.
        println!("the picture became drawable on frame {landed_at}");
        assert!(
            state.pictures[&8].is_ready(),
            "the state owns the handle, so the decode's result survives the frame that asked for it"
        );
        assert!(
            landed_at > 0,
            "the decode cannot land on the frame it was requested on: this test would prove nothing"
        );
        assert!(
            !pictures_need_a_frame(&mut pending, &state.pictures),
            "once the picture has landed, the wall goes quiet again — a wall that redraws forever is #51's              other half"
        );
    }

    /// The other half of `#51`: the state, not the view, owns the decode handle.
    ///
    /// Asking the cache from the view (`cache.load(bytes)` per marker per frame) re-ran the decode — the
    /// cache keeps a *weak* entry per content hash, so a dropped handle expires it — and left nothing for
    /// `poll` to watch. A handle the state keeps is a handle that stays ready.
    #[test]
    fn a_picture_asked_for_once_stays_decoded() {
        let bytes: std::sync::Arc<[u8]> = std::sync::Arc::from(crate::scene::PHOTO_FIXTURE);
        let mut state = State::default();
        state.offer_picture(8, bytes);
        for _ in 0..600 {
            if state.pictures[&8].is_ready() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(state.pictures[&8].is_ready(), "the fixture never decoded");
        // Several frames later, with nothing asking the cache again, it is still ready and still the same
        // decoded size: the entry did not expire underneath the wall.
        for _ in 0..3 {
            let _ = pictures_need_a_frame(&mut state.pictures_pending, &state.pictures);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(state.pictures[&8].is_ready());
        assert_eq!(
            state.pictures[&8].size(),
            Some([400, 120]),
            "the fixture is the hull silhouette, 400x120 — the decoded size is the proof the handle is \
             still the decoded picture and not a fresh request"
        );
    }

    #[test]
    fn the_card_redraws_on_the_frame_clock_and_not_the_wall_clock() {
        let mut state = state_with_two_hulls();
        state.select_in(state.focused(), 8);
        // Settle: the first tick after a selection records the text it drew.
        tick_card_age(&mut state);
        assert!(
            !tick_card_age(&mut state),
            "nothing has moved, nothing to redraw"
        );

        state.logical = 1.0;
        assert!(
            tick_card_age(&mut state),
            "one second of the frame clock is a redraw; no wall-clock time passed in this test at all,              which is the whole point"
        );
        assert_eq!(
            card_age(&state),
            "5 s old",
            "the fixture's fix is 4 s old at logical 0"
        );

        state.logical = 2.0;
        assert!(tick_card_age(&mut state));
        assert_eq!(card_age(&state), "6 s old");
    }

    /// #1: the socket queue's patience is measured, and a wait past the promise is counted — the number
    /// v0.1's definition of done is decided by, since "within 1 s of arrival" is otherwise a sentence.
    #[test]
    fn a_fix_s_latency_is_measured_and_a_late_one_is_counted() {
        use std::time::Duration;

        let mut state = state_with_two_hulls();
        assert_eq!(
            state.fix_latency.max_ms(),
            -1,
            "a wall that has applied nothing has no number, and invents none"
        );

        state.note_fix_latency(Duration::from_millis(12));
        assert_eq!(state.fix_latency.max_ms(), 12);
        assert_eq!(state.fix_latency.late(), 0);

        // Worse, then better: the max is the worst, not the last.
        state.note_fix_latency(Duration::from_millis(400));
        state.note_fix_latency(Duration::from_millis(3));
        assert_eq!(state.fix_latency.max_ms(), 400);

        // Exactly at the promise is inside it; a millisecond past is not.
        state.note_fix_latency(Duration::from_millis(FIX_LATENCY_LIMIT_MS));
        assert_eq!(state.fix_latency.late(), 0, "1 s is within 1 s");
        state.note_fix_latency(Duration::from_millis(FIX_LATENCY_LIMIT_MS + 1));
        assert_eq!(state.fix_latency.late(), 1);
        assert_eq!(
            state.fix_latency.max_ms(),
            (FIX_LATENCY_LIMIT_MS + 1) as i64
        );
    }

    /// #1, measured over thirty minutes: one fix waited 2954 ms **because the window was not being drawn**,
    /// and that is not a stall. The wait and the loop's idle gap are recorded together so the two cannot be
    /// confused — a wall that is not on screen is allowed to be still; a wall that is on screen and two
    /// seconds behind its feed is not.
    #[test]
    fn a_late_fix_is_told_apart_from_a_wall_nobody_was_drawing() {
        use std::time::Duration;
        let mut latency = FixLatency::default();

        // A two-second wait for a two-second gap: applied at the first opportunity there was.
        latency.note(Duration::from_millis(2_010), Duration::from_millis(2_000));
        assert_eq!(latency.late(), 0, "the wall was idle, not slow");
        assert_eq!(latency.idle(), 1);

        // A two-second wait across a 16 ms gap: the loop was running and did not apply it.
        latency.note(Duration::from_millis(2_010), Duration::from_millis(16));
        assert_eq!(latency.late(), 1, "this one is a stall (INV-05)");
        assert_eq!(latency.idle(), 1);

        // The worst wait keeps the gap it happened in, whichever came first.
        assert_eq!(latency.max_ms(), 2_010);
        assert_eq!(latency.max_gap_ms(), 16);
    }

    /// A fix applied through the wall's own path records its wait: the statistic is wired to the update, not
    /// just to the struct.
    #[test]
    fn an_applied_fix_records_its_wait() {
        let mut state = state_with_two_hulls();
        let received = std::time::Instant::now() - std::time::Duration::from_millis(5);
        apply_update(
            &mut state,
            crate::live::Update::Fix {
                received,
                fix: crate::live::wire::LiveFix {
                    id_unit: 8,
                    name: "KRI Abdul Halim Perdanakusuma".into(),
                    hull_number: Some("KRI-355".into()),
                    latitude: -6.20,
                    longitude: 106.99,
                    speed_kn: Some(12.0),
                    speed_ms: Some(6.17),
                    course_deg: Some(90.0),
                    accuracy_m: Some(5.0),
                    recorded_at: chrono::Utc::now(),
                    received_at: chrono::Utc::now(),
                    backfilled: false,
                },
            },
        );
        assert!(
            state.fix_latency.max_ms() >= 5,
            "the wait is measured from the socket's own instant, got {}",
            state.fix_latency.max_ms()
        );
        assert_eq!(state.fix_latency.late(), 0, "five milliseconds is not late");
        assert_eq!(state.fix_latency.idle(), 0);
    }

    /// A fix from the socket has to **move a hull**, not tick a counter.
    ///
    /// #28: `Update::Fix` counted the publication and kept the position from the last REST read, so
    /// the wall only ever showed what a relaunch had read. This is that bug as a test: the position
    /// after the fix is the fix's, the header's count moves with it, and the frame is marked as
    /// travelling so the next frame draws the move.
    #[test]
    fn a_fix_from_the_socket_moves_its_marker() {
        let mut state = state_with_two_hulls();
        let before = position_of(&state.picture, 8);

        apply_update(
            &mut state,
            crate::live::Update::Fix {
                received: std::time::Instant::now(),
                fix: crate::live::wire::LiveFix {
                    id_unit: 8,
                    name: "KRI Abdul Halim Perdanakusuma".into(),
                    hull_number: Some("KRI-355".into()),
                    latitude: -6.20,
                    longitude: 106.99,
                    speed_kn: Some(12.0),
                    speed_ms: Some(6.17),
                    course_deg: Some(90.0),
                    accuracy_m: Some(5.0),
                    recorded_at: chrono::Utc::now(),
                    received_at: chrono::Utc::now(),
                    backfilled: false,
                },
            },
        );

        assert_eq!(state.fixes, 1, "the header's count is the fix's");
        let after = position_of(&state.picture, 8);
        assert!(
            (after.lat - -6.20).abs() < 2e-6 && (after.lon - 106.99).abs() < 2e-6,
            "the picture took the fix's position, got {after:?} (was {before:?})"
        );
        assert!(
            state.picture.travelling(state.now()),
            "the marker is drawn travelling, so `poll` keeps the frames coming until it arrives"
        );
    }

    /// **A card belongs to the pane that asked for it** (#99). The wall's data is shared; the question is
    /// not. The neighbour must not gain a card, and a press on the neighbour's water must not take this
    /// pane's card away — which is what "the selection is the wall's" looked like on the wall.
    #[test]
    fn a_card_belongs_to_the_pane_that_asked_for_it() {
        let mut state = state_with_two_hulls();
        let first = state.focused();
        let camera = state.camera_of(first);
        let second = state.split_pane(first, camera).expect("a second pane");
        state.select_in(first, 8);

        assert_eq!(state.selected_in(first), Some(8));
        assert!(
            state.card_in(first).is_some(),
            "the pane that asked has its card"
        );
        assert_eq!(
            state.selected_in(second),
            None,
            "the neighbour has no selection of its own"
        );
        assert!(
            state.card_in(second).is_none(),
            "INV-10: a card belongs to one pane, so the neighbour draws none"
        );

        // A press on the neighbour's water clears *that* pane, not this one.
        state.press_background(second);
        assert!(
            state.card_in(first).is_some(),
            "a press in another pane must not close this pane's card"
        );

        // And selecting in the neighbour leaves this pane's card alone.
        state.select_in(second, 9);
        assert_eq!(state.card_in(first).map(|card| card.unit.id_unit), Some(8));
        assert_eq!(state.card_in(second).map(|card| card.unit.id_unit), Some(9));
    }

    /// A closed pane takes its card with it: the selection is inside the pane, so it cannot outlive it.
    #[test]
    fn closing_a_pane_drops_its_card() {
        let mut state = state_with_two_hulls();
        let first = state.focused();
        let camera = state.camera_of(first);
        let second = state.split_pane(first, camera).expect("a second pane");
        state.select_in(second, 8);
        assert!(state.card_in(second).is_some());

        assert!(state.close_pane(second));
        state.panes_mut().retain(|pane| pane.id != second);
        assert!(
            state
                .panes()
                .iter()
                .all(|pane| pane.view.selected.is_none()),
            "no pane is left holding the closed pane's selection"
        );
    }

    /// The card is a view of one hull, so that hull's fix has to reach it.
    #[test]
    fn a_fix_updates_the_open_card() {
        let mut state = state_with_two_hulls();
        state.select_in(state.focused(), 8);
        assert_eq!(card_of(&state).map(|card| card.speed_kn), Some(Some(0.0)));

        apply_update(
            &mut state,
            crate::live::Update::Fix {
                received: std::time::Instant::now(),
                fix: crate::live::wire::LiveFix {
                    id_unit: 8,
                    name: "KRI Abdul Halim Perdanakusuma".into(),
                    hull_number: Some("KRI-355".into()),
                    latitude: -6.10,
                    longitude: 106.9,
                    speed_kn: Some(21.5),
                    speed_ms: None,
                    course_deg: None,
                    accuracy_m: None,
                    recorded_at: chrono::Utc::now(),
                    received_at: chrono::Utc::now(),
                    backfilled: false,
                },
            },
        );

        assert_eq!(
            card_of(&state).map(|card| card.speed_kn),
            Some(Some(21.5)),
            "the open card follows its hull's fix"
        );
    }

    fn position_of(picture: &crate::units::Picture, id_unit: i64) -> crate::units::Pos {
        picture
            .units()
            .iter()
            .find(|unit| unit.unit.id_unit == id_unit)
            .expect("the hull is in the picture")
            .position
            .expect("the hull has a fix")
    }

    #[test]
    fn one_card_at_a_time_and_clearing_leaves_none() {
        // INV-10: selecting a hull replaces the selection. Two cards is the failure this exists for.
        let mut state = state_with_two_hulls();
        state.select_in(state.focused(), 8);
        assert_eq!(card_of(&state).map(|card| card.unit.id_unit), Some(8));
        state.select_in(state.focused(), 9);
        assert_eq!(
            card_of(&state).map(|card| card.unit.id_unit),
            Some(9),
            "the second selection replaces the first"
        );
        assert_eq!(state.selection(), Some(9));
        let focused = state.focused();
        state.clear_selection(focused);
        assert!(
            card_of(&state).is_none(),
            "INV-10: a cleared selection has no card"
        );
        assert_eq!(state.selection(), None);
    }

    #[test]
    fn a_press_on_a_hull_toggles_it_and_a_press_on_the_map_does_not_steal_it() {
        // The two handlers fire for one click. Whichever runs first, the marker's press wins and the
        // map's does not undo it — and pressing the same hull again closes the card.
        let mut state = state_with_two_hulls();
        state.press_marker(state.focused(), 8);
        state.press_background(state.focused());
        assert_eq!(
            state.selection(),
            Some(8),
            "the map's handler stood down for the marker's press"
        );
        assert!(card_of(&state).is_some());

        let mut state = state_with_two_hulls();
        state.press_marker(state.focused(), 8);
        state.marker_press = false; // as the end of that frame does
        state.press_marker(state.focused(), 8);
        assert!(
            card_of(&state).is_none(),
            "the second press on the same hull closes its card"
        );

        state.marker_press = false;
        state.press_background(state.focused());
        assert_eq!(state.selection(), None, "a press on the map clears");
    }

    #[test]
    fn the_card_is_a_panel_with_a_real_height() {
        // The reported bug: `.size(WIDTH, 0.0)` painted the fill into a zero-height box, so the card
        // was loose text over the map. A drawn height is the proof, and the panel's own colour at its
        // centre is the second half of it.
        let mut state = state_with_two_hulls();
        state.select_in(state.focused(), 8);
        let rt = frame_once(state);
        let (rect, _) = rt.presented("card").expect("the card is drawn");
        assert!(
            rect[3] > 100.0,
            "the card panel has a real height, got {} (zero means the fill is invisible)",
            rect[3]
        );
        assert!(rect[2] > 200.0, "and a width, got {}", rect[2]);
    }

    /// #84: a lift is cancelled by Escape, and cancelling it closes nothing else — the gesture is the
    /// live thing, the menu and the card are behind it.
    #[test]
    fn escape_cancels_a_lift_and_nothing_behind_it() {
        // The decision is the machine's now (#101): Escape ends whatever is held, and while a pane is in
        // the air that is the lift — so the card behind it survives, which is the invariant's point.
        let mut state = state_with_two_hulls();
        let focused = state.focused();
        state.select_in(focused, 8);
        assert!(
            state.begin(Begin::Lift(PaneDrag {
                pane: focused,
                drop: None
            })),
            "a lift starts on an idle wall"
        );
        let escape = rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        };
        let mut tx = rune::Tx::default();
        assert!(on_key(&mut state, &escape, &mut tx), "Escape is consumed");
        assert!(
            !matches!(state.interaction, Interaction::Lifted(_)),
            "Escape in the air puts the pane down"
        );
        assert!(
            state.card_in(focused).is_some(),
            "and does not close the card behind it"
        );
        let tab = rune::KeyEvent {
            key: rune::Key::Tab,
            mods: rune::Mods::default(),
        };
        let mut tx = rune::Tx::default();
        assert!(!on_key(&mut state, &tab, &mut tx), "only Escape");
    }

    #[test]
    fn escape_closes_the_card_only_when_there_is_one() {
        let mut state = state_with_two_hulls();
        let escape = rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        };
        let mut tx = rune::Tx::default();
        assert!(
            !on_key(&mut state, &escape, &mut tx),
            "Escape with nothing open is not consumed"
        );
        let focused = state.focused();
        state.select_in(focused, 8);
        let mut tx = rune::Tx::default();
        assert!(
            on_key(&mut state, &escape, &mut tx),
            "Escape closes the open card"
        );
        assert!(card_of(&state).is_none());
        // And a key that is not Escape never closes it, however the state looks.
        state.select_in(focused, 8);
        let tab = rune::KeyEvent {
            key: rune::Key::Tab,
            mods: rune::Mods::default(),
        };
        let mut tx = rune::Tx::default();
        assert!(!on_key(&mut state, &tab, &mut tx));
        assert!(
            card_of(&state).is_some(),
            "Tab is not Escape, so the card stays"
        );
    }

    #[test]
    fn the_open_card_is_drawn_inside_the_window_and_keeps_its_marker() {
        // INV-10 at the render level: the card exists, is on screen, and stays inside the window for
        // a marker at the very edge (where a careless card would clip).
        let mut state = state_with_two_hulls();
        state.select_in(state.focused(), 8);
        let rt = frame_once(state);
        let (rect, opacity) = rt.presented("card").expect("the card is drawn");
        assert!(opacity > 0.9);
        let (x, y, w, h) = (rect[0], rect[1], rect[2], rect[3]);
        assert!(w > 0.0 && h > 0.0, "the card has area: {w}x{h}");
        assert!(
            x >= 0.0
                && y >= 0.0
                && x + w <= tokens::WINDOW_SIZE.0
                && y + h <= tokens::WINDOW_SIZE.1,
            "INV-10: the card leaves the window: {rect:?}"
        );
        assert!(
            rt.find("card-name").is_some(),
            "the card shows the hull's name"
        );
        assert!(
            rt.find("card-hull").is_some(),
            "and its hull number, when it has one"
        );
    }

    #[test]
    fn clicking_a_markers_pixel_opens_its_card_and_a_pan_keeps_it_attached() {
        // The ticket's acceptance criterion, driven the way an operator drives it: press the marker's
        // own pixel, then move the camera and check the card followed the marker.
        let mut rt = frame_once(state_with_two_hulls());
        // **The key names its pane** (#99): two panes may draw the same hull, so a hit target keyed by
        // the hull alone would be the neighbour's. This test asked for `hit-8` until the pane went into
        // the key, which is how it learned the symbol marker's key had not been updated with the picture's.
        let pane_key = rt.state.focused();
        let (x, y) = rt
            .find(&format!("hit-{pane_key}-8"))
            .expect("the marker has a hit target");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.invalidate();
        rt.frame(1.0 / 60.0);
        assert!(
            rt.state.card_in(rt.state.focused()).is_some(),
            "a press on the marker opens its card"
        );
        let before = rt.presented("card").expect("card drawn").0;
        let marker_before = rt
            .find(&format!("marker-{pane_key}-8"))
            .expect("marker placed");

        // Pan: the frame the map hands over moves 400 px left, and the markers move with it.
        let camera = Camera::new((-6.108, 106.910 - 0.05), 11.0);
        rt.state.set_camera(camera);
        rt.state.set_frame(crate::map::geometry::Rendered {
            camera,
            width: 1280,
            height: 720,
        });
        // In the app `poll` notices the new frame and asks for the redraw; here the test says so
        // directly, which is the same thing without an app harness.
        rt.invalidate();
        // Markers are laid out by a spring, so one frame is a fraction of the way: let it settle,
        // which is what the operator sees half a second after the pan ends.
        for _ in 0..40 {
            rt.frame(1.0 / 60.0);
        }

        let marker_after = rt
            .find(&format!("marker-{pane_key}-8"))
            .expect("marker still placed");
        let after = rt.presented("card").expect("card still drawn").0;
        assert!(
            (marker_after.0 - marker_before.0).abs() > 100.0,
            "the pan moved the marker: {marker_before:?} → {marker_after:?}"
        );
        assert!(
            (after[0] - before[0]).abs() > 50.0 || after[1] != before[1],
            "INV-10: the card follows its marker, it does not stay put"
        );
        assert!(
            after[0] >= 0.0
                && after[1] >= 0.0
                && after[0] + after[2] <= tokens::WINDOW_SIZE.0
                && after[1] + after[3] <= tokens::WINDOW_SIZE.1,
            "INV-10: the card stayed inside the window after the pan: {after:?}"
        );
        rt.state.clear_selection(rt.state.focused());
        assert!(rt.state.card_in(rt.state.focused()).is_none());
    }

    #[test]
    fn a_card_for_a_hull_without_a_hull_number_has_no_hull_row() {
        // An empty row is worse than a missing one: it claims the field exists and is blank.
        let mut state = state_with_two_hulls();
        state.select_in(state.focused(), 9);
        let rt = frame_once(state);
        assert!(rt.find("card-name").is_some());
        assert!(
            rt.find("card-hull").is_none(),
            "KRI Cakra has no hull number, so no row"
        );
    }

    #[test]
    fn the_status_line_says_no_picture_yet_rather_than_zero_units() {
        // The scaffold has asked nobody, so the view may not print "0 units" — that would be a
        // claim about the fleet. Same distinction as INV-09, one layer up.
        let state = State::default();
        assert!(state.picture.is_empty());
        let rt = frame_once(state);
        assert!(
            rt.find("picture-status").is_some(),
            "the status line must be laid out"
        );
        assert!(
            rt.find("picture-title").is_some(),
            "the panel must have its heading"
        );
    }

    /// **A pane starting does not stall a pane that is already up** (#68).
    ///
    /// This is the acceptance item that needs the engine, so it is `#[ignore]`d and run by hand — the
    /// gate builds no MapLibre and needs no GPU:
    ///
    /// ```sh
    /// cargo test --features map --lib -- --ignored --nocapture a_second_pane
    /// ```
    ///
    /// Two threads, two `LiveMap`s. The second loads a style from the shared cache (about a second on
    /// this laptop, `docs/research/multi-view.md`) while the first is mid-pan; the first must keep
    /// publishing frames at its own cadence throughout. A shared render loop — the design spike B
    /// measured and rejected — is exactly what would make this fail.
    #[cfg(feature = "map")]
    #[test]
    #[ignore = "map engine: needs a GPU and, when the cache is cold, a style load"]
    fn a_second_pane_starts_without_stalling_the_first() {
        use crate::map::geometry::{Camera, Rendered};
        use crate::map::thread::MapThread;
        use std::time::{Duration, Instant};

        let first = MapThread::spawn(Camera::START, (960, 540));
        let started = Instant::now();
        while first.rendered().is_none() {
            assert!(
                first.error().is_none(),
                "the first pane failed to start: {:?}",
                first.error()
            );
            assert!(
                started.elapsed() < Duration::from_secs(40),
                "the first pane never published a frame"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        // Start the second pane while the first is panning, and time how long the first takes to
        // publish a new frame — i.e. how long it is blocked by somebody else's style load.
        let pan = Camera::new((-6.2, 106.8), 11.5);
        first.set_camera(pan);
        let second = MapThread::spawn(Camera::new((-7.9, 112.5), 9.0), (960, 540));

        let pan_started = Instant::now();
        let mut published = None;
        while pan_started.elapsed() < Duration::from_secs(20) {
            if let Some(frame) = first.rendered()
                && (frame.camera.center.0 - pan.center.0).abs() < 1e-9
            {
                published = Some(pan_started.elapsed());
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let after_pan = published.expect("the first pane never rendered the pan");

        // And the second pane does come up, or this test would be about a pane that never appeared.
        let second_started = Instant::now();
        while second.rendered().is_none() {
            assert!(
                second.error().is_none(),
                "the second pane failed to start: {:?}",
                second.error()
            );
            assert!(
                second_started.elapsed() < Duration::from_secs(40),
                "the second pane never published a frame"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        println!(
            "panes: the first pane published the pan {after_pan:?} after it was asked, while the \
             second pane was loading"
        );

        // A blocked pane would take the time a style load takes; a free one is a frame's work. Five
        // seconds is far above a frame and far below a stalled wall, which is what this is about.
        assert!(
            after_pan < Duration::from_secs(5),
            "the first pane took {after_pan:?} to publish a pan while a second pane started: panes \
             are sharing a render loop, which is the design #64 rejected"
        );
        let _: Option<Rendered> = first.rendered();
    }

    /// #70, #84: a tiled wall draws a handle per pane, and the pane in the air only while one is lifted.
    ///
    /// The handle is its own element **above** the markers, which is how a grab avoids selecting the hull
    /// under the pointer: `TabDragState`'s `suppress_click` in a different mechanism, because the press
    /// that starts a lift never reaches a marker in the first place.
    #[test]
    fn a_tiled_wall_has_a_handle_per_pane_and_a_ghost_only_while_lifted() {
        let panes = crate::scene::state_for("panes");
        let rt = frame_once(panes);
        for id in 1..=3 {
            assert!(
                rt.find(&format!("handle-{id}")).is_some(),
                "pane {id} has no handle: a pane that cannot be picked up cannot be moved"
            );
        }
        assert!(
            rt.find("pane-ghost").is_none(),
            "a ghost is drawn only while a pane is lifted"
        );

        let dragging = crate::scene::state_for("panes-drag");
        assert!(dragging.dragging().is_some(), "the scene lifts a pane");
        let rt = frame_once(dragging);
        assert!(
            rt.find("pane-ghost").is_some(),
            "a lifted pane is drawn on the slot it lands in — that is what says which pane is in the air"
        );
    }

    /// One pane is the wall as it was: no handle, no ring, no gap (#68, #70).
    #[test]
    fn one_pane_has_no_handle_and_no_ring() {
        let state = crate::scene::state_for("picture");
        assert!(state.is_single_pane());
        let rt = frame_once(state);
        assert!(
            rt.find("handle-1").is_none(),
            "a wall with one pane has nothing to pick up"
        );
        assert!(rt.find("pane-gap").is_none());
    }

    /// **N panes are N map contexts, and the wall closes all of them** (#71).
    ///
    /// Ignored because it opens the engine; run by hand:
    ///
    /// ```sh
    /// cargo test --features map --lib -- --ignored --nocapture four_panes_close
    /// ```
    ///
    /// The engine asserts in `~RunLoop` (`Failed to close loop`) when a process with a live map leaves,
    /// so a wall with panes has to join every thread before it exits. The test's own success — the
    /// process still running afterwards — is the assertion that no context was left behind.
    #[cfg(feature = "map")]
    #[test]
    #[ignore = "map engine: needs a GPU and, when the cache is cold, a style load"]
    fn four_panes_close_their_maps_without_an_abort() {
        let mut state = State {
            headless: false,
            panel: Some((1280.0, 720.0)),
            ..State::default()
        };
        let first = state.pane_ids()[0];
        let mut ids = vec![first];
        for index in 1..4 {
            let from = ids[index - 1];
            let id = state
                .split_pane(from, Camera::START)
                .expect("a split spawns a pane");
            ids.push(id);
        }
        // **One door for every pane** (#160): `poll` asks the wall which map panes have no thread and
        // starts them, so this is the wall's own path rather than a fourth spawn site in a test.
        assert!(state.spawn_missing_maps(), "four panes, four threads");
        assert!(
            !state.spawn_missing_maps(),
            "a wall whose panes are all drawing has nothing left to start"
        );
        assert_eq!(
            state
                .panes()
                .iter()
                .filter(|pane| pane.map.is_some())
                .count(),
            4,
            "four panes, four map threads"
        );
        // Wait for each pane to publish a frame, so this test is about closing live contexts.
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(60)
            && state.panes().iter().any(|pane| {
                pane.map
                    .as_ref()
                    .is_some_and(|map| map.rendered().is_none())
            })
        {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let drawn = state
            .panes()
            .iter()
            .filter(|pane| {
                pane.map
                    .as_ref()
                    .is_some_and(|map| map.rendered().is_some())
            })
            .count();
        println!("panes: {drawn}/4 panes published a frame before shutdown");
        state.close_maps();
        assert!(
            state.panes().iter().all(|pane| pane.map.is_none()),
            "close_maps leaves no map thread running"
        );
        println!("panes: all four map threads joined, and this process is still up");
    }

    /// **The cadence does what the header claims** (#71): the focused pane redraws at 4 Hz, the others at
    /// 1 Hz, and the focused pane's pan is not slowed by the three that are idling.
    ///
    /// Ignored because it opens the engine and takes three seconds of wall time:
    ///
    /// ```sh
    /// cargo test --features map --lib -- --ignored --nocapture cadence
    /// ```
    ///
    /// Frame counts come from `Pane::frames`, which `poll` increments when a pane's frame changes — the
    /// same number the log line prints, so this test and the wall cannot disagree about what "4 Hz" means.
    #[cfg(feature = "map")]
    #[test]
    #[ignore = "map engine: needs a GPU; takes three seconds of real time"]
    fn the_focused_pane_keeps_its_cadence_while_three_idle() {
        use crate::map::thread::{IDLE_INTERVAL, UNFOCUSED_INTERVAL};
        use std::time::Duration;

        let mut state = State {
            headless: false,
            panel: Some((1280.0, 720.0)),
            ..State::default()
        };
        let first = state.pane_ids()[0];
        let mut ids = vec![first];
        for index in 1..4 {
            let from = ids[index - 1];
            ids.push(state.split_pane(from, Camera::START).expect("a split"));
        }
        // The wall's one door (#160): `poll` starts a thread for every map pane that has none.
        assert!(state.spawn_missing_maps(), "four panes, four threads");
        state.focus_pane(ids[0]);
        state.apply_cadence();
        for pane in state.panes() {
            let Some(map) = &pane.map else { continue };
            assert_eq!(map.error(), None, "every pane's map started");
        }

        // Count the frames each pane publishes over a stretch of wall time, by reading the frames the
        // threads hand over (the same counter `poll` keeps).
        // Wait for every pane to have drawn once: four concurrent style loads on a cold cache take a
        // couple of seconds on this laptop, and measuring the cadence during them would measure the
        // load (the same reason `map_frame_cost` pumps before it times).
        let waiting = std::time::Instant::now();
        while waiting.elapsed() < Duration::from_secs(60)
            && state.panes().iter().any(|pane| {
                pane.map
                    .as_ref()
                    .is_some_and(|map| map.frames() == 0 && map.error().is_none())
            })
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        println!(
            "panes: {} panes, {} with a map thread, first frames after {:?}",
            state.pane_count(),
            state.panes().iter().filter(|p| p.map.is_some()).count(),
            waiting.elapsed()
        );
        let window = Duration::from_millis(3000);
        let before = state.pane_frames();
        std::thread::sleep(window);
        let after = state.pane_frames();
        let counted: Vec<u64> = after
            .iter()
            .zip(before.iter())
            .map(|(after, before)| after.saturating_sub(*before))
            .collect();
        println!(
            "panes: frames in {:?} — focused {:?} = {}, others {:?} = {:?} (cadences {:?} / {:?})",
            window,
            ids[0],
            counted[0],
            &ids[1..],
            &counted[1..],
            IDLE_INTERVAL,
            UNFOCUSED_INTERVAL
        );

        // Roughly 4 Hz and 1 Hz, with room for a style-load hitch at the start: a 3 s window at 4 Hz is
        // 12 frames and at 1 Hz is 3. The bounds are wide because this is a scheduler, not a clock —
        // what it catches is a wall that forgets to slow the panes it is not showing.
        let expected_focused = window.as_secs_f64() / IDLE_INTERVAL.as_secs_f64();
        let expected_idle = window.as_secs_f64() / UNFOCUSED_INTERVAL.as_secs_f64();
        assert!(
            f64::from(counted[0] as u32) >= expected_focused * 0.5,
            "the focused pane drew {} frames in {window:?}; {expected_focused:.0} is its cadence — a \
             focused pane that idles like the others is a wall whose operator sees a still picture",
            counted[0]
        );
        for (index, frames) in counted.iter().enumerate().skip(1) {
            assert!(
                f64::from(*frames as u32) <= expected_idle * 2.0,
                "pane {} drew {frames} frames in {window:?} while unfocused; {expected_idle:.0} is its \
                 cadence — an unfocused pane at the focused pane's rate is the budget #71 exists for",
                ids[index]
            );
        }
        state.close_maps();
    }

    /// **A wall with one pane can ask for a second one** (#68 follow-up): the right-click menu is the
    /// only door to "Focus on new pane", so it has to be drawn before there are any panes to tile.
    ///
    /// It was not: `picture` returned the one-pane wall before it drew the menu, so a right-click set the
    /// state and nothing appeared — the first split was impossible to ask for, and no capture showed it
    /// because a capture has no pointer to right-click with.
    #[test]
    fn the_pane_menu_is_drawn_on_a_wall_with_one_pane() {
        let mut state = crate::scene::state_for("picture");
        assert!(state.is_single_pane(), "the wall this test is about");
        let pane = state.focused();
        state.begin(Begin::Menu {
            pane,
            at: (200.0, 160.0),
        });
        let rt = frame_once(state);
        assert!(
            rt.find("pane-menu").is_some(),
            "the menu is not in the tree: right-click would set state and show nothing"
        );
        assert!(
            rt.find("menu-split").is_some(),
            "the menu has no item, so there is nothing to press"
        );
    }

    /// The menu is only drawn while it is open: a wall at rest draws no menu, and one Escape takes it
    /// away without closing the card behind it.
    #[test]
    fn escape_closes_the_menu_before_the_card() {
        let mut state = crate::scene::state_for("card-open");
        let pane = state.focused();
        state.begin(Begin::Menu {
            pane,
            at: (10.0, 10.0),
        });
        assert!(card_of(&state).is_some(), "the scene has a card open");
        let rt = frame_once(state);
        assert!(rt.find("pane-menu").is_some());

        // What `on_key` does on Escape: the menu first, the selection untouched.
        let mut state = crate::scene::state_for("card-open");
        let pane = state.focused();
        state.begin(Begin::Menu {
            pane,
            at: (10.0, 10.0),
        });
        let escape = rune::KeyEvent {
            key: rune::Key::Escape,
            mods: Default::default(),
        };
        let mut tx = rune::Tx::default();
        assert!(on_key(&mut state, &escape, &mut tx), "Escape is consumed");
        assert!(state.menu().is_none(), "the menu closed");
        assert!(card_of(&state).is_some(), "and the card behind it did not");
    }

    /// **Escape releases in the enum's order** (#101): what is held, then the layout's full-size pane, then
    /// the pane's card. Three presses, three effects, and none of them does two things — which is the policy
    /// that used to live in the order of five `if`s and could only be read by reading it.
    #[test]
    fn escape_releases_one_thing_at_a_time() {
        let mut state = state_with_two_hulls();
        let focused = state.focused();
        state.select_in(focused, 8);
        assert!(state.maximise(focused), "the pane goes full size");
        assert!(state.begin(Begin::Settings), "and the sheet is over it");

        let escape = rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        };
        let mut tx = rune::Tx::default();
        assert!(on_key(&mut state, &escape, &mut tx), "the sheet takes it");
        assert!(!state.settings_open(), "the sheet is gone");
        assert_eq!(
            state.maximised(),
            Some(focused),
            "and the full-size pane is still up: one press, one thing"
        );
        assert!(state.card_in(focused).is_some(), "and the card survived it");

        let mut tx = rune::Tx::default();
        assert!(
            on_key(&mut state, &escape, &mut tx),
            "the second press reaches the layout"
        );
        assert_eq!(state.maximised(), None, "the pane is back in its slot");
        assert!(
            state.card_in(focused).is_some(),
            "and the card survived that too"
        );

        let mut tx = rune::Tx::default();
        assert!(
            on_key(&mut state, &escape, &mut tx),
            "the third reaches the card"
        );
        assert!(state.card_in(focused).is_none(), "the card closes last");
    }

    /// The menu opens **at the click and stays inside its pane** (#77): a right-click 20 pt from the
    /// wall's edge would otherwise draw half a menu over the bezel, on the one wall that needs it.
    /// **The ring wraps at both ends** (#164): `Down` past the last row is the first, and `Up` from nothing is the
    /// last — a menu the keyboard cannot leave by walking is a menu with a trap in it.
    #[test]
    fn the_menu_ring_wraps_at_both_ends() {
        let mut state = crate::scene::state_for("pane-menu");
        let pane = state.focused();
        state.begin(Begin::Menu {
            pane,
            at: (20.0, 30.0),
        });
        let rows = menu_rows(&state).len();
        assert!(rows >= 4, "the menu has rows to walk");
        let mut seen = Vec::new();
        for _ in 0..rows {
            assert!(menu_ring_step(&mut state, 1));
            seen.push(state.menu().and_then(|menu| menu.ring));
        }
        assert_eq!(
            seen.first().copied().flatten(),
            Some(0),
            "the first Down from no ring lands on the first row"
        );
        assert_eq!(
            seen.last().copied().flatten(),
            Some(rows - 1),
            "and walking the rows visits the last one without leaving the menu"
        );
        // One more Down is the first row again.
        assert!(menu_ring_step(&mut state, 1));
        assert_eq!(
            state.menu().and_then(|menu| menu.ring),
            Some(0),
            "the ring wraps"
        );

        // Up from the first row is the last, and Up from no ring is the last too.
        menu_ring(&mut state, Some(0));
        assert!(menu_ring_step(&mut state, -1));
        assert_eq!(state.menu().and_then(|menu| menu.ring), Some(rows - 1));
        menu_ring(&mut state, None);
        assert!(menu_ring_step(&mut state, -1));
        assert_eq!(
            state.menu().and_then(|menu| menu.ring),
            Some(rows - 1),
            "Up from nothing begins at the end"
        );
    }

    /// **Enter acts on the ringed row, and never on the row the pointer is over** (#164): the keyboard's answer to
    /// "which row" is the ring, and the ring is a value in the machine rather than the drawing's hover.
    #[test]
    fn enter_acts_on_the_ringed_row() {
        // `Name this pane…` is the third row of a pane menu; the ring decides, not a hover.
        let mut state = crate::scene::state_for("pane-menu");
        let pane = state.focused();
        state.begin(Begin::Menu {
            pane,
            at: (20.0, 30.0),
        });
        let rows = menu_rows(&state);
        let rename = rows
            .iter()
            .position(|row| row.label == MENU_RENAME)
            .expect("the menu offers a rename");
        assert!(menu_ring(&mut state, Some(rename)));
        assert!(menu_choose(&mut state));
        assert!(
            matches!(
                state.rename().map(|rename| rename.target),
                Some(RenameTarget::Pane(_))
            ),
            "Enter on `Name this pane` opens that sheet"
        );
        assert!(
            matches!(state.interaction, Interaction::Renaming(_)),
            "and the menu it was in is gone: a row's action closes it"
        );
    }

    /// **Right opens the waterfall and puts the ring on its first row; Left closes it and the ring stays on the row
    /// that opened it** (#164). One `open` value, one `ring` value, and the two surfaces never both claim it.
    #[test]
    fn right_opens_the_waterfall_and_left_closes_it() {
        let mut state = crate::scene::state_for("panes-menu");
        let pane = state.focused();
        state.begin(Begin::Menu {
            pane,
            at: (20.0, 30.0),
        });
        let items = menu_rows(&state);
        let move_row = items
            .iter()
            .position(|row| row.label == MENU_MOVE)
            .expect("the waterfall's trigger");
        assert!(menu_ring(&mut state, Some(move_row)));
        assert!(menu_choose(&mut state), "Enter on the trigger opens it");
        let menu = state.menu().expect("still open");
        assert!(menu.open.is_some(), "the second surface is the waterfall's");
        assert_eq!(menu.ring, Some(0), "and the ring is on its first row");
        assert_eq!(
            menu_rows(&state).len(),
            items[move_row].children.len(),
            "the rows the ring walks are the waterfall's"
        );
        assert!(close_submenu_through(&mut state));
        let menu = state.menu().expect("the menu stays");
        assert!(menu.open.is_none(), "Left closes one surface, not both");
    }

    /// The same thing the key handler does for `Left`, so the test drives the model and not a key event.
    fn close_submenu_through(state: &mut State) -> bool {
        crate::control::apply(state, crate::control::Intent::MenuSubmenu { open: false }).is_ok()
    }

    /// **The ring is the active descendant** (#164): the row the keyboard is on is the row an assistive client is
    /// told about, and it is the row the ring is drawn on — one value, two readers.
    #[test]
    fn the_ringed_row_is_the_menus_active_descendant() {
        let mut state = crate::scene::state_for("pane-menu");
        let pane = state.focused();
        state.begin(Begin::Menu {
            pane,
            at: (20.0, 30.0),
        });
        assert!(menu_ring(&mut state, Some(2)));
        let menu = state.menu().expect("open");
        let rows = menu_rows(&state);
        let ringed = rows.get(2).expect("a third row").key.clone();
        let rt = frame_once(state);
        // The tree carries the menu, its rows, and the ringed row by the key the active descendant names.
        assert!(
            rt.find("pane-menu").is_some(),
            "the menu is in the element tree"
        );
        assert!(
            rt.find(&ringed).is_some(),
            "the ringed row ({ringed}) is in the tree by its key, which is what the active descendant names"
        );
        assert_eq!(menu.ring, Some(2), "and the state agrees about which row");
    }

    /// **A fresh cursor is in the tree** (#167): the state has one, and the view puts an element at its point.
    /// The pixel half is the capture; this is the half a rasteriser cannot answer for.
    #[test]
    fn a_fresh_client_cursor_is_drawn_at_its_point() {
        let state = crate::scene::state_for("cursor-hull");
        let cursor = state.cursor.expect("the scene placed a cursor");
        assert!(cursor.visible_at(2.0), "the capture is at 2 s");
        assert!(
            cursor_glyph(&state).is_some(),
            "a visible cursor must be an element: a state value nothing draws is a pointer nobody sees"
        );
        // The capture's clock: `scripts/capture.sh` renders at `--at 2`, and the fade is read off that clock.
        let mut idle = crate::scene::state_for("cursor-idle");
        idle.set_frame_clock(2.0);
        assert!(
            cursor_glyph(&idle).is_none(),
            "and a faded one must not be: the scene moved it {:.2} s before the capture, and the fade is {:.1} s",
            2.0 - idle.cursor.expect("the scene placed a cursor").moved_at,
            Cursor::FADE
        );
    }

    #[test]
    fn the_menu_opens_at_the_click_and_never_leaves_its_pane() {
        let pane = Rect {
            x: 16.0,
            y: 40.0,
            w: 600.0,
            h: 400.0,
        };
        let (w, h) = tokens::menu_size(1);
        // A click in the middle: the menu's corner is the click.
        assert_eq!(menu_origin(pane, (200.0, 150.0), (w, h)), (216.0, 190.0));
        // A click at the far corner: pushed back inside, by the margin.
        let (x, y) = menu_origin(pane, (595.0, 395.0), (w, h));
        assert!(
            x + w <= pane.right() && y + h <= pane.bottom(),
            "the menu left its pane: ({x}, {y}) {w}×{h} in {pane:?}"
        );
        // A click outside the pane (a stale menu after a re-tile) is still inside it.
        let (x, y) = menu_origin(pane, (-500.0, -500.0), (w, h));
        assert!(x >= pane.x && y >= pane.y, "({x}, {y}) is inside {pane:?}");
        // A pane smaller than a menu keeps the menu's corner at the pane's corner rather than inverting
        // the clamp.
        let tiny = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 20.0,
        };
        let (x, y) = menu_origin(tiny, (50.0, 10.0), (w, h));
        assert!(x >= 0.0 && y >= 0.0, "({x}, {y})");
    }

    /// #85: a pane that goes full size asks its map thread for the window's frame, and asks for its own
    /// back on the way out — a maximised pane that keeps rendering at its slot is an upscaled picture.
    ///
    /// The engine's side of that is `MapThread::resize` reaching the renderer, which the gate cannot see
    /// (it builds no MapLibre), so this is `#[ignore]`d and run by hand:
    ///
    /// ```sh
    /// cargo test --features map --lib -- --ignored --nocapture a_pane_resizes
    /// ```
    #[cfg(feature = "map")]
    #[test]
    #[ignore = "map engine: needs a GPU"]
    fn a_pane_resizes_to_full_size_and_back() {
        use crate::map::thread::MapThread;
        use std::time::{Duration, Instant};

        let pane = MapThread::spawn(crate::map::geometry::Camera::START, (960, 540));
        let wait = |pane: &MapThread, want: (u32, u32)| {
            let started = Instant::now();
            while pane
                .rendered()
                .is_none_or(|frame| (frame.width, frame.height) != want)
            {
                assert!(
                    pane.error().is_none(),
                    "the pane failed: {:?}",
                    pane.error()
                );
                assert!(
                    started.elapsed() < Duration::from_secs(40),
                    "the pane never published a {want:?} frame (last: {:?})",
                    pane.rendered()
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            started.elapsed()
        };

        let first = wait(&pane, (960, 540));
        println!("panes: the first {first:?} at 960x540");
        pane.resize(1920, 1080);
        let full = wait(&pane, (1920, 1080));
        println!("panes: full size, 1920x1080, in {full:?} — the frame the maximised pane draws");
        // A second resize **on the same thread**: this is the restore, and it is the one that leaves a
        // blurry picture if it never arrives.
        pane.resize(960, 540);
        let back = wait(&pane, (960, 540));
        println!("panes: back to its slot, 960x540, in {back:?}");
        assert!(
            full < Duration::from_secs(20) && back < Duration::from_secs(20),
            "a resize is a frame, not a reload: {full:?} and {back:?}"
        );
    }

    /// #85: going full size and back leaves the tiling, the cameras and the wall's own state exactly as it
    /// was — the claim the capture cannot make on its own, because a capture is one moment.
    #[test]
    fn full_size_and_back_changes_nothing_but_the_layer() {
        let mut state = crate::scene::state_for("panes");
        state.headless = true;
        let tiles = state.pane_rects();
        let cameras: Vec<_> = state.panes().iter().map(|pane| pane.camera).collect();
        let id = tiles[1].0;

        full_size_for_scene(&mut state, id);
        assert_eq!(state.maximised(), Some(id));
        assert_eq!(
            state.focused(),
            id,
            "the keys go to the pane being looked at"
        );
        assert_eq!(
            state.pane_rects(),
            tiles,
            "the tiling is untouched: a full-size pane is a bigger rectangle, not a new layout"
        );

        restore_full_size(&mut state);
        assert_eq!(state.maximised(), None);
        assert_eq!(state.pane_rects(), tiles);
        assert_eq!(
            state
                .panes()
                .iter()
                .map(|pane| pane.camera)
                .collect::<Vec<_>>(),
            cameras,
            "and every camera is where it was"
        );
    }

    /// #83: a name is trimmed and cut, and an empty one is no name at all — the three things the plate
    /// draws for and the sheet commits.
    #[test]
    fn a_name_is_trimmed_cut_and_never_empty() {
        assert_eq!(
            shown_name("  Surabaya approaches ").as_deref(),
            Some("Surabaya approaches")
        );
        assert_eq!(
            shown_name("   "),
            None,
            "an empty plate is worse than no plate"
        );
        assert_eq!(shown_name(""), None);

        let long = "x".repeat(tokens::NAME_MAX_CHARS + 10);
        let shown = shown_name(&long).expect("a long name is still a name");
        assert_eq!(shown.chars().count(), tokens::NAME_MAX_CHARS + 1);
        assert!(
            shown.ends_with(ELLIPSIS),
            "a cut name says it was cut: {shown:?}"
        );
        // Exactly at the limit is not cut: the ellipsis is for a name that lost something.
        let exact = "y".repeat(tokens::NAME_MAX_CHARS);
        assert_eq!(shown_name(&exact).as_deref(), Some(exact.as_str()));
    }

    /// #83: renaming opens on the pane's current name, commits on Enter, and abandons on Escape — and a
    /// committed empty field removes the name rather than drawing an empty plate.
    /// #113: the title is the typed name or the pane's own number, and the number is the pane's **id**,
    /// zero-padded to two digits. The form is written in one place, so this is its one literal check.
    #[test]
    fn a_panes_title_is_its_name_or_its_id() {
        for (id, expected) in [
            (1, "pane-01"),
            (2, "pane-02"),
            (9, "pane-09"),
            (10, "pane-10"),
            (23, "pane-23"),
            (99, "pane-99"),
            (100, "pane-100"),
        ] {
            assert_eq!(
                default_title(id),
                expected,
                "the default title is the pane's id, zero-padded to two digits"
            );
        }
        let mut pane = Pane::new(7, crate::map::geometry::Camera::START);
        assert_eq!(pane.title(), "pane-07");
        pane.name = shown_name("  Surabaya approaches ");
        assert_eq!(
            pane.title(),
            "Surabaya approaches",
            "a name somebody typed is the title"
        );
    }

    /// #113: **every pane draws its title, and the plate is the control**. The press goes through the
    /// same `begin_rename` the menu row calls, and a default title is not a name: the sheet opens empty.
    #[test]
    fn every_pane_has_a_title_and_pressing_it_types_a_new_one() {
        // A lone pane draws `pane-01` too: a control that appears only once the wall is split appears
        // from nowhere. The frame is deliberately absent here: a pane whose map has not drawn yet still
        // has its title, because the plate is chrome rather than part of the picture.
        let mut single = crate::scene::state_for("picture");
        // A scene's state carries the frame a capture draws (it is the wall the capture renders), so the
        // before-first-frame case is made here rather than borrowed from a scene (#172).
        let focused = single.focused();
        single
            .pane_mut(focused)
            .expect("the scene has a focused pane")
            .frame = None;
        assert!(
            single.frame().is_none(),
            "this is the before-first-frame case"
        );
        let single_rt = frame_once(single);
        assert!(
            single_rt.find("pane-name-1").is_some(),
            "a one-pane wall draws its title"
        );
        assert_eq!(
            single_rt
                .elements()
                .iter()
                .find(|element| element.key.as_deref() == Some("pane-title-1"))
                .and_then(|element| element.text.as_deref()),
            Some("pane-01"),
            "the lone pane's title is its own number"
        );

        let mut rt = frame_once(crate::scene::state_for("panes-named"));
        let ids = rt.state.pane_ids();
        assert_eq!(ids.len(), 3);
        // The scene's third pane is the one nobody named, and its id is 3: `pane-03`, not `pane-01`.
        assert_eq!(
            rt.state.pane(ids[2]).expect("the third pane").title(),
            "pane-03"
        );
        let expected = [
            "Surabaya approaches".to_string(),
            shown_name("the strait at first light, and the traffic separation scheme")
                .expect("a long name is still a name"),
            "pane-03".to_string(),
        ];
        for (id, expected) in ids.iter().zip(expected) {
            let key = format!("pane-name-{id}");
            let (x, y) = rt
                .find(&key)
                .unwrap_or_else(|| panic!("pane {id} draws no title plate"));
            let picked = rt
                .element_at(x, y)
                .unwrap_or_else(|| panic!("nothing is at pane {id}'s plate"));
            let title_key = format!("pane-title-{id}");
            assert!(
                matches!(
                    picked.key(),
                    Some(found) if found == key.as_str() || found == title_key.as_str()
                ),
                "the plate is chrome over the markers: a press on the label must reach the label, \
                 not a hull or the handle (got {:?})",
                picked.key()
            );
            let drawn = rt
                .elements()
                .iter()
                .find(|element| element.key.as_deref() == Some(title_key.as_str()))
                .and_then(|element| element.text.clone());
            assert_eq!(
                drawn.as_deref(),
                Some(expected.as_str()),
                "pane {id}'s title"
            );
        }

        // Press the unnamed pane's plate: the same sheet the menu row opens, with an empty field. A
        // default title is not a name, so Enter on an untouched field leaves it unnamed.
        let id = ids[2];
        let (x, y) = rt.find(&format!("pane-name-{id}")).expect("the plate");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
        let rename = rt
            .state
            .rename()
            .expect("a press on the plate opens the sheet");
        assert_eq!(rename.target, RenameTarget::Pane(id));
        assert_eq!(rename.edit.text, "", "a default title is not a name");
        assert!(
            rt.state.dragging().is_none(),
            "a press on the label is a rename, never a lift"
        );
    }

    /// #82: the gap switch moves every pane, names its own state, and widens the border's grip — a pointer
    /// in the middle of an 8 pt gutter is 4 pt from the line, which a 6 pt grip does not reach.
    #[test]
    fn the_gap_switch_opens_the_gutter_and_widens_the_grip() {
        let mut state = crate::scene::state_for("panes");
        // A scene writes nothing: the settings file belongs to a wall, and a test that wrote the machine's
        // would be a test that changes the person running it.
        state.headless = true;
        assert_eq!(state.settings.pane_gap, 0.0, "the wall's own default");
        let cells = state.pane_rects();
        let before = state.pane_rects();
        assert_eq!(before, cells, "at gap 0 the panes are the cells");

        // The middle of the border, where the gutter will be: 4 pt from the line once the gap is on.
        let mid = (cells[0].1.right(), cells[0].1.y + cells[0].1.h / 2.0);
        assert!(
            state.border_at(mid, state.pane_grip()).is_some(),
            "the border is grabbable at gap 0"
        );

        toggle_pane_gap(&mut state);
        assert_eq!(state.settings.pane_gap, tokens::GAP);
        assert!(
            state.layout_dirty,
            "the picture changed, so it must be redrawn"
        );
        let after = state.pane_rects();
        assert_eq!(after.len(), before.len());
        for ((_, cell), (_, drawn)) in cells.iter().zip(after.iter()) {
            assert!((drawn.x - (cell.x + tokens::GAP / 2.0)).abs() < 0.001);
            assert!((drawn.w - (cell.w - tokens::GAP)).abs() < 0.001);
        }
        // The grip now covers the gutter, and the same point is still on the border — measured against
        // the narrow grip, which would miss it.
        assert!(state.pane_grip() >= tokens::GAP);
        assert!(
            state.border_at(mid, state.pane_grip()).is_some(),
            "a pointer in the middle of the gutter still grabs the border"
        );

        // And back off again, which is what the second press of the row does.
        toggle_pane_gap(&mut state);
        assert_eq!(state.settings.pane_gap, 0.0);
        assert_eq!(state.pane_rects(), cells);
    }

    /// #81/#111: a wall with one pane offers the growth rows and no `Close pane`, a tiled wall adds it,
    /// **Every row a menu offers is a change the control plane can express** (#170).
    ///
    /// The table test the epic asked for: a row with no intent is a control a socket cannot reach, and an intent
    /// whose verb is missing is one the document cannot name. The waterfall's trigger is the single exception and
    /// it says why — it opens a second *surface* of the menu it is already in, which is a place, not a value.
    #[test]
    fn every_menu_row_is_an_intent_or_a_surface() {
        let state = crate::scene::state_for("panes-menu");
        let pane = state.focused();
        let mut seen: Vec<&'static str> = Vec::new();
        let mut submenus = 0;
        for item in crate::ui::menu_items(&state, pane) {
            match item.intent(&state, pane) {
                Some(intent) => seen.push(intent.verb()),
                None => {
                    assert!(
                        matches!(item.action, MenuAction::Submenu(_)),
                        "the row {:?} neither changes the wall nor opens a surface: a control a socket \
                         cannot reach (#170)",
                        item.label
                    );
                    assert!(
                        item.opens(),
                        "a submenu's trigger must have rows to show, or it is a press that does nothing"
                    );
                    submenus += 1;
                }
            }
            // A waterfall's leaves are rows too: each says something settled about *which* workspace.
            for child in &item.children {
                let intent = child.intent(&state, pane).unwrap_or_else(|| {
                    panic!(
                        "the waterfall's row {:?} is not an intent, and a leaf is never a surface (#170)",
                        child.label
                    )
                });
                assert_eq!(
                    intent.verb(),
                    "pane.move",
                    "a waterfall leaf is `pane.move` and nothing else"
                );
                assert!(
                    !seen.contains(&intent.verb()),
                    "the leaves and the rows must not share a verb: the verbs are the vocabulary, not the menu"
                );
            }
        }
        assert_eq!(submenus, 1, "exactly one row opens a second surface");

        // Every verb the menu produces is one the vocabulary's own table names, so a renamed intent cannot leave
        // a row pointing at a name nothing answers to.
        let known: Vec<&'static str> = crate::control::vocabulary()
            .iter()
            .map(crate::control::Intent::verb)
            .collect();
        for verb in seen {
            assert!(
                known.contains(&verb),
                "the menu produces {verb}, which no intent in the vocabulary declares (#170)"
            );
        }
    }

    /// and the menu is as tall as the items it has — the surface used to be `size()`d for one row, which
    /// clips the second. `Full size` left the menu for the pane chrome, so the row count is one lower than
    /// it was. #140 adds `Focus on new workspace` and the waterfall, in that order: growth at the top,
    /// the destructive row last.
    #[test]
    fn the_menu_offers_close_only_when_there_is_something_to_close() {
        let one = crate::scene::state_for("pane-menu");
        let pane = one.focused();
        assert_eq!(
            crate::ui::menu_items(&one, pane)
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec![MENU_SPLIT, MENU_NEW_WORKSPACE, MENU_RENAME, MENU_MOVE],
            "the last pane cannot be closed, so the menu does not offer it; the waterfall is there \
             because the wall has workspaces this pane is not in"
        );

        let tiled = crate::scene::state_for("panes-menu");
        let pane = tiled.focused();
        let items = crate::ui::menu_items(&tiled, pane);
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec![
                MENU_SPLIT,
                MENU_NEW_WORKSPACE,
                MENU_RENAME,
                MENU_MOVE,
                MENU_CLOSE
            ]
        );
        assert_eq!(
            tokens::menu_size(items.len()),
            (tokens::MENU_WIDTH, tokens::MENU_ROW * 5.0),
            "five rows, not one row's box clipped"
        );
        // **The waterfall lists the workspaces this pane is not in**, by name (#140): three by default,
        // so two rows — and the trigger is the only row that opens a second surface.
        let move_row = items
            .iter()
            .find(|item| item.label == MENU_MOVE)
            .expect("the waterfall's row");
        assert_eq!(
            move_row
                .children
                .iter()
                .map(|child| child.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Workspace 2", "Workspace 3"],
            "the pane is in workspace 1, so the submenu offers the other two"
        );
        assert!(move_row.opens());
        assert_eq!(move_row.action, MenuAction::Submenu("menu-move"));
    }

    /// #81: closing a pane takes its slot out of the layout, keeps the focus on a live pane, and refuses the
    /// last one — the three things the menu row relies on.
    #[test]
    fn closing_a_pane_collapses_its_slot_and_keeps_a_focused_pane() {
        let mut state = crate::scene::state_for("panes-menu");
        let closed = state.focused();
        let before = state.pane_rects().len();
        close_from_menu(&mut state, closed);
        let rects = state.pane_rects();
        assert_eq!(rects.len(), before - 1, "one pane fewer");
        assert!(
            !rects.iter().any(|(id, _)| *id == closed),
            "the closed pane is gone from the layout"
        );
        assert!(
            !state.panes().iter().any(|pane| pane.id == closed),
            "and gone from the wall, so its map thread is joined"
        );
        assert!(
            rects.iter().any(|(id, _)| *id == state.focused()),
            "the focus is on a pane that still exists"
        );
        // The layout still covers the panel exactly: a close collapses a slot, it does not leave a hole.
        let area: f32 = rects.iter().map(|(_, rect)| rect.w * rect.h).sum();
        let viewport = state.pane_viewport();
        assert!(
            (area - viewport.w as f32 * viewport.h as f32).abs() < 1.0,
            "the panes cover the panel: {area} vs {}",
            viewport.w as f32 * viewport.h as f32
        );

        // Down to the last pane, which is then refused and leaves the wall unchanged.
        while state.pane_count() > 1 {
            let next = state.focused();
            close_from_menu(&mut state, next);
        }
        assert_eq!(state.pane_count(), 1, "one pane left, and one pane object");
        let last = state.focused();
        close_from_menu(&mut state, last);
        assert_eq!(
            state.pane_rects().len(),
            1,
            "the last pane is not closeable: a wall with no panes draws nothing"
        );
        assert_eq!(state.pane_count(), 1);
    }
}

/// **Closing a workspace's last pane** (#141): a workspace is allowed to be empty, and the wall lands on
/// the nearest previous workspace that still has a pane — on **the pane the operator was last in there**,
/// which is the one thing a "switch to the workspace" that forgot `Layout::focused()` would get wrong.
/// **Naming a workspace that is not on screen** (#154): `RenameTarget::Workspace` carries an index, and
/// the chip's right-click is the door. What a capture cannot say — that the commit lands on the *other*
/// workspace and leaves the wall's title alone — is here.
#[cfg(test)]
mod hidden_rename_tests {
    use super::*;

    #[test]
    fn a_hidden_workspace_is_renamed_and_the_title_is_not() {
        let mut state = crate::scene::state_for("workspaces");
        state.headless = true;
        assert_eq!(state.active, 0);
        begin_workspace_rename(&mut state, 1);
        let Some(rename) = state.rename() else {
            panic!("the chip's right-click opens the sheet");
        };
        assert_eq!(
            rename.target,
            RenameTarget::Workspace(1),
            "the chip names **its** workspace, not the one on screen"
        );
        assert_eq!(
            rename.edit.text, "Workspace 2",
            "the field starts with the name the chip stands for"
        );
        if let Some(rename) = state.renaming_mut() {
            rename.edit.text = "Straits watch".to_string();
        }
        commit_rename(&mut state);

        assert_eq!(state.workspaces[1].name, "Straits watch");
        assert_eq!(
            state.workspaces[0].name, "Workspace 1",
            "and nothing else was renamed"
        );
        assert_eq!(state.active, 0, "and the wall did not switch to it");
        assert_eq!(
            workspace_title(&state.workspaces[0], 0),
            "Workspace 1",
            "the title is the active workspace's, so it did not change"
        );
        assert_eq!(workspace_title(&state.workspaces[1], 1), "Straits watch");
        assert!(state.menu().is_none());
    }

    /// **Escape leaves a hidden workspace as it was** — the sheet is the title's, and its way out is the
    /// same whether the workspace it names is on screen or not.
    #[test]
    fn escaping_a_hidden_rename_changes_nothing() {
        let mut state = crate::scene::state_for("workspaces");
        state.headless = true;
        begin_workspace_rename(&mut state, 1);
        if let Some(rename) = state.renaming_mut() {
            rename.edit.text = "Half a name".to_string();
        }
        state.end();
        assert_eq!(state.workspaces[1].name, "Workspace 2");
        assert!(state.rename().is_none());
    }
}

#[cfg(test)]
mod workspace_close_tests {
    use super::*;

    /// Workspace 1 with two panes, focused on its **second**, and a one-pane workspace 2 that is active.
    fn two_and_a_newsroom() -> (State, ViewId) {
        let mut state = State::default();
        let first = state.focused();
        let second = state
            .split_pane_as(first, Camera::START, PaneContent::Map)
            .expect("a second map pane");
        state.focus_pane(second);
        // Workspace 2 with one newsroom pane, through the move the menu row calls.
        let newsroom = state
            .split_pane_as(second, Camera::START, PaneContent::Newsroom)
            .expect("a feed");
        state.focus_pane(second);
        assert!(crate::ui::move_pane_to(&mut state, newsroom, 1));
        assert_eq!(state.active, 1, "workspace 2 is on screen");
        (state, newsroom)
    }

    #[test]
    fn closing_a_one_pane_workspace_empties_it_and_lands_on_the_previous_one() {
        let (mut state, newsroom) = two_and_a_newsroom();
        assert!(state.can_close(newsroom), "a pane elsewhere allows it");
        assert!(state.close_pane(newsroom));
        assert_eq!(
            state.active, 0,
            "the nearest previous workspace with a pane"
        );
        assert_eq!(
            state.workspaces.len(),
            3,
            "and the empty one stays in the chips"
        );
        let emptied = &state.workspaces[1];
        assert!(emptied.layout.is_none(), "an empty workspace has no tiling");
        assert!(emptied.panes.is_empty());
        assert_eq!(emptied.name, "Workspace 2", "and it keeps its name");
        assert_eq!(state.panes().len(), 2, "workspace 1 still has both panes");
        assert!(
            state.pane(newsroom).is_none(),
            "and nothing anywhere still holds the closed pane"
        );
        // **The remembered pane, not the first leaf**: the layout's focus is what the wall shows.
        let ids = state.pane_ids();
        assert_eq!(state.focused(), ids[1], "the pane it was last in");
        assert_ne!(state.focused(), ids[0]);
    }

    #[test]
    fn closing_a_pane_with_a_sibling_keeps_its_workspace() {
        let mut state = crate::scene::state_for("panes-menu");
        let closed = state.focused();
        let before = state.pane_count();
        assert!(before > 1, "the scene tiles more than one pane");
        assert!(state.can_close(closed));
        assert!(state.close_pane(closed));
        assert_eq!(
            state.active, 0,
            "no workspace switch: this one still has a pane"
        );
        assert_eq!(state.pane_count(), before - 1);
        assert!(state.workspaces[0].layout.is_some());
        assert_eq!(state.focused(), state.pane_ids()[0], "focus on the sibling");
    }

    /// **The wall always keeps a pane** (#141): three workspaces, two of them empty, and the lone pane
    /// refuses to close — and the menu row that would close it is absent for the same reason.
    #[test]
    fn the_last_pane_on_the_wall_is_refused() {
        let mut state = crate::scene::state_for("pane-menu");
        let only = state.focused();
        assert_eq!(state.pane_count(), 1);
        assert!(
            state.workspaces.iter().skip(1).all(|w| w.panes.is_empty()),
            "the scene's two other workspaces are empty"
        );
        assert!(!state.can_close(only));
        assert!(!state.close_pane(only), "and the action agrees with it");
        assert_eq!(state.pane_count(), 1);
        assert!(state.pane(only).is_some());
        assert!(
            crate::ui::menu_items(&state, only)
                .iter()
                .all(|item| item.label != MENU_CLOSE),
            "a wall of empty workspaces cannot be fixed with a menu row, so the row is not offered"
        );
        // A pane on **another** workspace is what makes the same pane closeable.
        let mut state = state;
        let index = state.add_workspace();
        let id = state.allocate_pane();
        state.workspaces[index].layout = Some(Layout::single_with(id));
        state.workspaces[index]
            .panes
            .push(Pane::new(id, Camera::START));
        assert!(
            state.can_close(only),
            "with a pane on workspace 4, workspace 1's lone pane can close"
        );
    }

    /// The rule's scan, as the ticket states it: `active-1, … 0`, then `active+1, … n-1`.
    #[test]
    fn the_scan_prefers_the_nearest_previous_workspace() {
        let mut state = State::default();
        while state.workspaces.len() < 4 {
            state.add_workspace();
        }
        // One pane in each of workspaces 1 and 2, none in 3, a pane in 4.
        for index in [0, 1] {
            let id = state.allocate_pane();
            state.workspaces[index].layout = Some(Layout::single_with(id));
            state.workspaces[index]
                .panes
                .push(Pane::new(id, Camera::START));
        }
        let id = state.allocate_pane();
        state.workspaces[3].layout = Some(Layout::single_with(id));
        state.workspaces[3].panes.push(Pane::new(id, Camera::START));

        // From 2 with 1 non-empty: the previous one.
        assert_eq!(state.nearest_previous_non_empty(2), Some(1));
        // From 2 and 1 empty, 0 non-empty: skips the hole.
        state.workspaces[1].layout = None;
        state.workspaces[1].panes.clear();
        assert_eq!(state.nearest_previous_non_empty(2), Some(0));
        // From 3 with 2 and 1 empty: 0, not the pane after it.
        assert_eq!(state.nearest_previous_non_empty(3), Some(0));
        // From 0 there is no previous one, so the scan turns around.
        assert_eq!(state.nearest_previous_non_empty(0), Some(2 + 1));
        // With no workspace holding a pane, nothing to land on.
        for workspace in &mut state.workspaces {
            workspace.layout = None;
            workspace.panes.clear();
        }
        assert_eq!(state.nearest_previous_non_empty(1), None);
    }

    /// The pane a surface was opened on is gone, so the surface goes with it — and so does the
    /// maximised pane, which would otherwise name a pane that no longer exists.
    #[test]
    fn closing_the_pane_under_a_surface_clears_it() {
        let mut state = State::default();
        let first = state.focused();
        let second = state
            .split_pane_as(first, Camera::START, PaneContent::Map)
            .expect("a second pane");
        state.focus_pane(second);
        state.begin(Begin::Menu {
            pane: second,
            at: (20.0, 20.0),
        });
        state.maximise(second);
        assert!(state.close_pane(second));
        assert!(state.menu().is_none(), "the menu closed with its pane");
        assert!(state.maximised().is_none(), "and nothing is maximised");
        assert_eq!(state.pane_count(), 1);
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[test]
    fn a_rename_commits_to_its_target_and_nowhere_else() {
        let mut state = State::default();
        let pane = state.focused();

        // The workspace, through the title's own door.
        begin_workspace_rename(&mut state, 1);
        assert_eq!(
            state.rename().map(|rename| rename.target),
            Some(RenameTarget::Workspace(1))
        );
        assert_eq!(
            state.rename().map(Rename::title),
            Some(RENAME_WORKSPACE_TITLE),
            "the sheet says what it is naming"
        );
        assert_eq!(
            state
                .rename()
                .map(|rename| rename.edit.text.clone())
                .as_deref(),
            Some("Workspace 2"),
            "the field starts with what the header shows"
        );
        state.renaming_mut().expect("a rename").edit.text = "  Straits watch  ".into();
        commit_rename(&mut state);
        assert_eq!(state.workspaces[1].name, "Straits watch", "trimmed");
        assert_eq!(
            state.workspaces[0].name, "Workspace 1",
            "and only the workspace that was being renamed"
        );
        assert!(
            state.pane(pane).is_some_and(|pane| pane.name.is_none()),
            "a workspace rename is not a pane rename"
        );

        // The pane, through the plate's door: the workspace's name is untouched.
        begin_rename(&mut state, pane);
        assert_eq!(
            state.rename().map(Rename::title),
            Some(RENAME_TITLE),
            "the pane sheet keeps its own words"
        );
        state.renaming_mut().expect("a rename").edit.text = "Watchkeeper".into();
        commit_rename(&mut state);
        assert_eq!(
            state
                .pane(pane)
                .and_then(|pane| pane.name.clone())
                .as_deref(),
            Some("Watchkeeper")
        );
        assert_eq!(state.workspaces[1].name, "Straits watch");
    }

    #[test]
    fn an_empty_rename_keeps_a_workspaces_default_and_clears_a_panes_name() {
        let mut state = State::default();
        let pane = state.focused();
        begin_workspace_rename(&mut state, 2);
        state.renaming_mut().expect("a rename").edit.text = "   ".into();
        commit_rename(&mut state);
        assert_eq!(state.workspaces[2].name, "Workspace 3");

        begin_rename(&mut state, pane);
        state.renaming_mut().expect("a rename").edit.text = "Named".into();
        commit_rename(&mut state);
        begin_rename(&mut state, pane);
        state.renaming_mut().expect("a rename").edit.text = "  ".into();
        commit_rename(&mut state);
        assert_eq!(
            state.pane(pane).and_then(|pane| pane.name.clone()),
            None,
            "an empty pane name removes it, and the plate falls back to the pane's number"
        );
        assert_eq!(
            state.pane(pane).map(Pane::title).as_deref(),
            Some(default_title(pane).as_str()),
            "and the plate falls back to the pane's number"
        );
    }

    #[test]
    fn a_long_workspace_name_is_cut_and_escape_leaves_the_old_one() {
        let mut state = State::default();
        begin_workspace_rename(&mut state, 0);
        let long = "x".repeat(tokens::NAME_MAX_CHARS + 12);
        state.renaming_mut().expect("a rename").edit.text = long.clone();
        commit_rename(&mut state);
        assert_eq!(state.workspaces[0].name, long, "the raw name is kept");
        let shown = workspace_title(&state.workspaces[0], 0);
        assert_eq!(
            shown.chars().count(),
            tokens::NAME_MAX_CHARS + 1,
            "the title cuts it to the pane name's limit, with a drawn ellipsis"
        );
        assert!(shown.ends_with(ELLIPSIS));

        // Escape: `end()` is what a cancel does, and the name is not touched.
        begin_workspace_rename(&mut state, 0);
        state.renaming_mut().expect("a rename").edit.text = "Something else".into();
        state.end();
        assert_eq!(state.workspaces[0].name, long, "Escape commits nothing");
        assert!(state.rename().is_none());
    }

    #[test]
    fn the_rename_sheet_names_a_pane_and_abandons_cleanly() {
        let mut state = crate::scene::state_for("panes");
        let id = state.focused();
        begin_rename(&mut state, id);
        let rename = state.rename().expect("the sheet opens");
        assert_eq!(rename.target, RenameTarget::Pane(id));
        assert_eq!(rename.edit.text, "", "the pane had no name");
        assert_eq!(
            rename.edit.focus, 0,
            "the caret is at the end of what there is"
        );

        state.renaming_mut().expect("open").edit.text = "  Underway  ".to_string();
        commit_rename(&mut state);
        assert!(state.rename().is_none(), "the sheet closes on commit");
        assert_eq!(
            state.pane(id).and_then(|pane| pane.name.as_deref()),
            Some("Underway"),
            "the name is stored trimmed"
        );

        // Reopening starts from the name it has, so a rename edits rather than retypes.
        begin_rename(&mut state, id);
        assert_eq!(state.rename().expect("reopened").edit.text, "Underway");
        // Escape: the field goes, the name stays.
        state.end();
        assert_eq!(
            state.pane(id).and_then(|pane| pane.name.as_deref()),
            Some("Underway")
        );

        // An empty field next time removes it.
        begin_rename(&mut state, id);
        state.renaming_mut().expect("open").edit.text = "   ".to_string();
        commit_rename(&mut state);
        assert_eq!(state.pane(id).and_then(|pane| pane.name.as_deref()), None);
        assert_eq!(
            state.pane(id).expect("the pane").title(),
            default_title(id),
            "an unnamed pane still has a title: its own number"
        );
    }
}
