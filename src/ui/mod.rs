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

pub mod icons;
pub mod markers;
pub mod symbol;
pub mod tokens;
pub mod unitcard;
pub mod view;

pub use view::{Dir, Layout, Rect, ViewId};

use std::process::ExitCode;

use rune::{AppSpec, Cx, El, col, el, row, spacer, text};

use crate::config;
use crate::live::LiveThread;
use crate::map::geometry::Camera;
use crate::units::Picture;

/// The window title, and the wordmark's text.
pub const TITLE: &str = "Arcons Command Center";

/// One pane: a camera, the map thread that renders it, and the frame it last drew.
///
/// The `Option`s are the two build modes: without `--features map` there is no thread (and no frame
/// but a scene's synthetic one), and a thread that is starting has not drawn yet.
pub struct Pane {
    pub id: ViewId,
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
    /// What the operator called this pane (#83). `None` draws nothing: a name nobody typed is not a name.
    ///
    /// It lives on the pane and not in `settings.toml`, because a pane does not survive a restart yet — a
    /// saved layout (which panes, at which cameras, in what tiling) is its own ticket, and a name that
    /// outlived the pane it named would be a label on a wall that no longer exists.
    pub name: Option<String>,
}

impl Pane {
    pub fn new(id: ViewId, camera: Camera) -> Self {
        Self {
            id,
            camera,
            #[cfg(feature = "map")]
            map: None,
            frame: None,
            #[cfg(feature = "map")]
            frame_seen: None,
            view: PaneView::default(),
            name: None,
            #[cfg(feature = "map")]
            drag_origin: None,
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
/// **`filter` is deliberately here before it is used.** The operator's next ask is per-pane filters
/// (ships in pane 1, planes in pane 2, altitude in pane 3). Another field added later would be the same
/// mistake a second time: the pane's state is one thing, so it has one home.
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
    /// Which hulls this pane draws (all of them, for now). The operator's next ticket: a pane per
    /// question — ships, planes, altitide — and the data stays shared.
    pub filter: Filter,
}

/// What a pane draws. `All` today; the vocabulary exists so a filter is a value and not a boolean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Filter {
    /// Every hull the wall knows about.
    #[default]
    All,
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

/// A rename in progress (#83): which pane, and the field being typed into.
///
/// The field is Rune's `TextEdit` and not a `String`: it owns the caret, the selection, the input method and
/// the undo history, and a wall shows an operator's keyboard the same way any other app does. The commit is
/// one function — Enter — and abandoning is Escape.
pub struct Rename {
    pub pane: ViewId,
    pub edit: rune::TextEdit,
}

impl Rename {
    /// Open a rename on a pane, with its current name in the field and the caret at the end — so typing
    /// replaces nothing until the operator says so with a selection or a backspace.
    pub fn new(pane: ViewId, current: Option<&str>) -> Rename {
        let text = current.unwrap_or_default().to_string();
        let caret = text.chars().count();
        let mut edit = rune::TextEdit::default();
        edit.text.clone_from(&text);
        edit.anchor = caret;
        edit.focus = caret;
        edit.focused = true;
        Rename { pane, edit }
    }
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
}

/// What the view loop holds. #3 fills the picture, #4 moves it, #6 draws it, #7 selects from it.
pub struct State {
    pub picture: Picture,
    /// The panes, in the order they were created (#68). One pane is the wall as it always was.
    ///
    /// Each pane owns its camera, its map thread and the frame that thread last drew — which is what
    /// makes `INV-04` a per-pane claim: a marker projects against **its pane's** frame, never another's.
    pub panes: Vec<Pane>,
    /// How the panes tile, and which one the operator is working in.
    pub layout: Layout,
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
    /// The socket actor, when a credential was configured. `None` means the wall is read-only.
    pub live: Option<LiveThread>,
    /// The reads thread: the card asks it for identity, commander and specification.
    pub backend: Option<crate::data::Backend>,
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
            panes: vec![Pane::new(1, camera)],
            layout: Layout::single(),
            settings: crate::config::settings::Settings::default(),

            scale: 1.0,

            panel: None,
            interaction: Interaction::Idle,
            layout_dirty: false,

            #[cfg(feature = "map")]
            cadenced: None,
            live: None,
            backend: None,
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
            camera_reported: false,
            register_attempts: 0,
            register_retry_at: None,
            image_retries: std::collections::HashMap::new(),
            registered: 0,
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
        let id = self.layout.focused();
        self.pane(id).unwrap_or_else(|| {
            // A layout naming a pane that is gone is a bug in the tree, not in the drawing; fall back
            // rather than panic in a view function, which would take the wall down at 2 a.m.
            let first = self.layout.ids().first().copied().unwrap_or(1);
            self.pane(first).unwrap_or_else(|| {
                static FALLBACK: std::sync::OnceLock<Pane> = std::sync::OnceLock::new();
                FALLBACK.get_or_init(|| Pane::new(1, Camera::START))
            })
        })
    }

    pub fn focused_pane_mut(&mut self) -> &mut Pane {
        let id = self.layout.focused();
        if !self.panes.iter().any(|pane| pane.id == id)
            && let Some(first) = self.layout.ids().first().copied()
        {
            self.layout.focus(first);
        }
        let id = self.layout.focused();
        if let Some(index) = self.panes.iter().position(|pane| pane.id == id) {
            &mut self.panes[index]
        } else {
            self.panes.push(Pane::new(id, Camera::START));
            let index = self.panes.len() - 1;
            &mut self.panes[index]
        }
    }

    /// The pane a menu is open on, and where — `None` when no menu is open.
    pub fn menu(&self) -> Option<Menu> {
        match self.interaction {
            Interaction::Menu { pane, at } => Some(Menu { pane, at }),
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
        self.layout.maximised()
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
            Begin::Menu { pane, at } => Interaction::Menu { pane, at },
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

    pub fn pane(&self, id: ViewId) -> Option<&Pane> {
        self.panes.iter().find(|pane| pane.id == id)
    }

    pub fn pane_mut(&mut self, id: ViewId) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|pane| pane.id == id)
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
        self.focused_pane_mut().camera = camera;
    }

    pub fn set_camera_of(&mut self, id: ViewId, camera: Camera) {
        if let Some(pane) = self.pane_mut(id) {
            pane.camera = camera;
        }
    }

    /// The focused pane's frame: the one markers project against when there is one pane.
    pub fn frame(&self) -> Option<crate::map::geometry::Rendered> {
        self.focused_pane().frame
    }

    /// A frame for a scene or a test, on the focused pane. A wall's frames come from the map thread.
    pub fn set_frame(&mut self, frame: crate::map::geometry::Rendered) {
        self.focused_pane_mut().frame = Some(frame);
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
        view::panes_on(&self.layout, self.pane_viewport(), self.settings.pane_gap)
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
        let rect = self
            .layout
            .rect_of(self.pane_viewport(), target)
            .unwrap_or(Rect {
                x: 0.0,
                y: 0.0,
                w: self.pane_viewport().w as f32,
                h: self.pane_viewport().h as f32,
            });
        let id = self.layout.split(target, rect.longer_edge())?;
        self.panes.push(Pane::new(id, camera));
        #[cfg(feature = "map")]
        if !self.headless {
            // The new pane's own thread, at its share of the panel: the same path the first pane takes,
            // so a pane is not a special case of a pane (`#68`).
            let viewport = self.pane_viewport();
            let half = self
                .layout
                .rect_of(viewport, id)
                .map(|rect| (rect.w, rect.h))
                .unwrap_or((viewport.w as f32, viewport.h as f32));
            if let Some(pane) = self.pane_mut(id) {
                pane.map = Some(crate::map::thread::MapThread::spawn(
                    camera,
                    (
                        half.0.round().max(1.0) as u32,
                        half.1.round().max(1.0) as u32,
                    ),
                ));
            }
        }
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
            self.panes
                .iter()
                .map(|pane| pane.map.as_ref().map(|map| map.frames()).unwrap_or(0))
                .collect()
        }
        #[cfg(not(feature = "map"))]
        {
            Vec::new()
        }
    }

    /// Tell every pane's map thread how often to redraw: the focused pane at the wall's cadence, the
    /// others at a quarter of it (#71).
    #[cfg(feature = "map")]
    pub fn apply_cadence(&self) {
        let focused = self.layout.focused();
        for pane in &self.panes {
            if let Some(map) = &pane.map {
                map.set_cadence(if pane.id == focused {
                    crate::map::thread::IDLE_INTERVAL
                } else {
                    crate::map::thread::UNFOCUSED_INTERVAL
                });
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
        for pane in &mut self.panes {
            // Dropping the handle stops the thread and joins it.
            pane.map = None;
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
        self.selected_in(self.layout.focused())
    }

    /// The hull **one pane** is asking about.
    pub fn selected_in(&self, pane: ViewId) -> Option<i64> {
        self.pane(pane).and_then(|pane| pane.view.selected)
    }

    /// One pane's card, if it has one.
    pub fn card_in(&self, pane: ViewId) -> Option<&crate::units::card::Card> {
        self.pane(pane).and_then(|pane| pane.view.card.as_ref())
    }

    /// Select a hull **in one pane**: build that pane's card from the picture and ask the reads thread for
    /// the rest.
    ///
    /// Selecting the same hull twice in the same pane does not re-ask: a card that re-fetches on every press
    /// is a card that flickers between answers. Two panes selecting the same hull ask twice, which is correct
    /// and cheap — they are two questions, and the reads thread is one thread with a queue.
    pub fn select_in(&mut self, pane: ViewId, id_unit: i64) {
        if self.selected_in(pane) == Some(id_unit) {
            return;
        }
        if let Some(pane) = self.pane_mut(pane) {
            pane.view.selected = Some(id_unit);
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
    pub fn press_marker(&mut self, pane: ViewId, id_unit: i64) {
        self.marker_press = true;
        if self.selected_in(pane) == Some(id_unit) {
            self.clear_selection(pane);
        } else {
            self.select_in(pane, id_unit);
        }
    }

    /// A press on the map itself: clear that pane, unless the press was a marker's.
    pub fn press_background(&mut self, pane: ViewId) {
        if !self.marker_press {
            self.clear_selection(pane);
        }
    }

    /// Clear one pane's selection. A pane has one card, so clearing it is clearing the only one there is.
    pub fn clear_selection(&mut self, pane: ViewId) {
        if let Some(pane) = self.pane_mut(pane) {
            pane.view.selected = None;
            pane.view.card = None;
            pane.view.last_age.clear();
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
    Menu { pane: ViewId, at: (f32, f32) },
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
    let units = if state.panes.len() > 1 {
        format!("{units} · panes={}", state.panes.len())
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
        .child(
            text(TITLE)
                .key("wordmark")
                .font(tokens::WORDMARK)
                .color(tokens::accent()),
        )
        .child(spacer())
        .child(
            text(link)
                .key("picture-status")
                .font(tokens::LABEL)
                .color(link_colour),
        )
        // The one piece of chrome on the wall that is a control: everything else is the picture. It is a
        // word because there is room for one here, and a word needs no catalogue to read (#82). The pane
        // chrome's icons say their meaning in help text for the same reason (#111).
        .child(
            text(SETTINGS_LABEL)
                .key("settings-open")
                .font(tokens::LABEL)
                .color(tokens::ink_dim())
                .on_press(|state: &mut State| {
                    state.begin(Begin::Settings);
                    state.layout_dirty = true;
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
        .child(text(RENAME_TITLE).font(tokens::TITLE).color(tokens::ink()))
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

/// What the filter panel is called, and the one layer the wall has until #112.
pub const FILTER_TITLE: &str = "Filter";
pub const FILTER_HULLS: &str = "Hulls";
pub const FILTER_ON: &str = "on";

/// The filter panel (#111): a popover anchored to a pane's filter toggle, **not a modal**.
///
/// A filter is a choice about the picture, so the picture has to stay visible while it is made — the
/// scrim a sheet draws would hide the water the operator is choosing for. The panel is nevertheless a
/// hold: a transparent catcher behind it makes the press outside dismiss it and do nothing else, which
/// is what `Interaction::Filtering` records and `tests/transitions.rs` checks.
///
/// It is attached at the root rather than inside the pane because a pane clips its picture; a panel
/// hanging outside its pane's right edge would be cut off, and the right-hand pane is exactly where the
/// toggle sits nearest the window's edge.
fn filter_panel(state: &State, body: El) -> El {
    let Some(pane) = state.filtering() else {
        return body;
    };
    let (w, h) = tokens::FILTER_SIZE;
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
        // One layer, in words. The layers and their toggles arrive with #112; this is the shell.
        .child(
            row()
                .key("filter-row-hulls")
                .h(tokens::MENU_ROW)
                .w_full()
                .items_center()
                .gap(tokens::GAP)
                .child(
                    text(FILTER_HULLS)
                        .font(tokens::LABEL)
                        .color(tokens::ink())
                        .grow(1.0),
                )
                .child(
                    text(FILTER_ON)
                        .key("filter-hulls-value")
                        .font(tokens::LABEL)
                        .color(tokens::accent()),
                ),
        );
    body.child(
        // The catcher is the panel's parent and the press that dismisses it. It is transparent and not
        // `modal`: the wall is dimmed by nothing and inert only for the pointer, which is the hold the
        // filter claims.
        el().key("filter-catcher")
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .on_press(|state: &mut State| {
                state.end();
                state.layout_dirty = true;
            })
            .child(panel),
    )
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
    state.layout.focus(pane);
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
    // On a wall the sheet shows the pane's own pixels, live: not a snapshot, not a second thread. A scene
    // has no map and draws the markers on the panel, which is the same picture minus the sea.
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
    if let Some(plate) = name_plate(pane) {
        sheet = sheet.child(plate);
    }
    // The same chrome row, and the maximise toggle says "back to the wall" because that is what the
    // press will do (#111). It is the pane's own toggle, not a second control invented for this shape.
    sheet = sheet.child(pane_chrome(state, id, rect));
    // The way back in words, below the icons: a wall has no title bar to close, and a word beside an
    // arrow is the meaning the arrow cannot say on its own.
    sheet = sheet.child(
        text(MAX_CLOSE)
            .key("max-close")
            .font(tokens::LABEL)
            .color(tokens::ink())
            // **Anchored to the top-right**, so the words fit whatever they say: an absolute x derived from
            // the label's own width is a plate that clips itself the first time the words change. Below the
            // chrome row, because the icons own the corner.
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
            .on_press(|state: &mut State| restore_full_size(state)),
    );
    // The wall behind, dimmed but still there: the other panes keep their cameras and their cadence, and
    // a reader can see what the full-size pane is one of.
    body.child(
        el().key("max-scrim")
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .fill(tokens::scrim())
            .child(sheet),
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

    let mut body = if state.layout.is_single() {
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
    for border in state.layout.borders(state.pane_viewport()) {
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
    let body = picture_content(state, id, rect, scale, true, body);
    let Some(pane) = state.pane(id) else {
        return body;
    };
    let Some(frame) = pane.frame else {
        return body;
    };
    let mut layer = body.child(projections(state, id, &frame, scale));
    // The name is chrome on the wall as it was, too: a one-pane wall with a name is a pane somebody named.
    if let Some(plate) = state.pane(id).and_then(name_plate) {
        layer = layer.child(plate);
    }
    // The controls are chrome too, and on a one-pane wall they are its whole chrome: the wall the
    // operator's screenshot marked is not necessarily a tiled one (#111).
    layer.child(pane_chrome(state, id, rect))
}

/// One pane: its picture, its markers, its own frame, the handle that moves it, and the ring that says
/// the keys are here.
fn pane(state: &State, id: ViewId, rect: Rect, scale: f32) -> El {
    let focused = state.layout.focused() == id;
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

    body = picture_content(state, id, rect, scale, false, body);
    // Without the map engine the pane itself is the surface under the pointer, so the pane carries the
    // gesture. With it, the map does — and a second handler here would be unreachable.
    #[cfg(not(feature = "map"))]
    {
        body = body
            .on_drag(move |state: &mut State, drag, _tx| drag_pane(state, id, rect, drag, scale));
    }

    // Markers project against THIS pane's frame (`INV-04`): its camera, its size, and nothing else. A
    // pane whose map has not drawn yet draws no markers — and still has its handle.
    let mut layer = body;
    if let Some(frame) = state.pane(id).and_then(|pane| pane.frame) {
        layer = layer.child(projections(state, id, &frame, scale));
    }
    // The plate is chrome over the picture, and the handle goes on top of it: a grab must never be a press
    // on the label either (#83).
    if let Some(plate) = state.pane(id).and_then(name_plate) {
        layer = layer.child(plate);
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
        .on_press(move |state: &mut State| toggle_full_size(state, pane));
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
fn toggle_full_size(state: &mut State, pane: ViewId) {
    if state.maximised() == Some(pane) {
        restore_full_size(state);
    } else {
        full_size_from_menu(state, pane);
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
    let viewport = state.pane_viewport();
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
            state.layout.focus(id);
        }
        rune::Phase::Move => {
            let drop = state.layout.drop_for(viewport, id, at);
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
                && state.layout.apply(drop)
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
    let Some(landing) = view::landing_rect(&state.layout, drop, state.pane_viewport(), drag.pane)
    else {
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
    let placed = markers::place(
        &state.picture,
        frame,
        state.now(),
        state.stale_after,
        range,
        &sizes,
    );
    let pictures = markers::Pictures {
        images: &state.pictures,
    };
    let mut layer = markers::layer(&placed, frame, scale, &pictures, id);
    if let Some(card) = state.card_in(id)
        && let Some(marker) = placed
            .iter()
            .find(|marker| marker.id_unit == card.unit.id_unit)
    {
        let points = if scale > 0.0 { f64::from(scale) } else { 1.0 };
        let anchor = (marker.x / points, marker.y / points);
        let viewport = (
            f64::from(frame.width) / points,
            f64::from(frame.height) / points,
        );
        let placement = crate::units::card::place(anchor, viewport);
        layer = layer.child(unitcard::card(card, placement, state.now()));
    }
    layer
}

/// A pane's name, on its own border (#83): a plate in the top-left corner, left of the handle.
///
/// **Inside the pane, not straddling its edge**: a pane clips its picture (`clip_rounded`), so a plate
/// half outside would be cut in half — and a name is chrome, which belongs *on* the picture rather than
/// hanging off it. Unnamed panes draw nothing at all: a placeholder is a promise of a name nobody made.
fn name_plate(pane: &Pane) -> Option<El> {
    let shown = shown_name(pane.name.as_deref().unwrap_or_default())?;
    Some(
        row()
            .key(format!("pane-name-{}", pane.id))
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
            .child(text(shown).font(tokens::LABEL).color(tokens::ink_dim())),
    )
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
fn pane_laid_out(state: &mut State, id: ViewId, width: f32, height: f32, scale: f32) {
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
    if state.layout.is_single() {
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
    let Some(rect) = state.layout.rect_of(viewport, menu.pane).map(|rect| {
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
        .fill(tokens::bg());
    for item in items {
        // Each row is its own target, so a press says **which** item it hit. The key is the row's, not the
        // surface's, which is what lets a capture assert that a second row is drawn at all.
        surface = surface.child(
            row()
                .key(item.key)
                .h(tokens::MENU_ROW)
                .w_full()
                .items_center()
                // A label touching the menu's own border reads as clipped, whatever the box says.
                .px(tokens::GAP)
                .child(text(item.label).font(tokens::LABEL).color(tokens::ink()))
                .on_press(move |state: &mut State| item.run(state, menu.pane)),
        );
    }
    body.child(surface)
}

/// `Name this pane…`: open the rename sheet on this pane, with its current name in the field (#83).
fn begin_rename(state: &mut State, pane: ViewId) {
    let current = state.pane(pane).and_then(|pane| pane.name.clone());
    state.begin(Begin::Rename(Rename::new(pane, current.as_deref())));
}

/// The maximise action: the pane is drawn over the wall and its map renders at the window's size (#85).
///
/// **It is still in its slot.** The tiling is untouched, the other panes keep their cameras and their
/// cadence, and restoring puts the picture back exactly as it was — this is "one pane for a moment", not
/// "make this the only pane", which is what a tiled wall's operator actually wants when the traffic gets
/// dense. The name says "from menu" because that is where #85 put it; since #111 the chrome's maximise
/// toggle calls the same function, in one press.
fn full_size_from_menu(state: &mut State, pane: ViewId) {
    state.end();
    if !state.layout.maximise(pane) {
        return;
    }
    // The keys and the highest cadence belong to the pane being looked at (`#71`).
    state.layout.focus(pane);
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

fn restore_full_size(state: &mut State) {
    let Some(pane) = state.maximised() else {
        return;
    };
    state.layout.restore();
    // The map goes back to the pane's own rectangle, not to whatever it was before: the tiling may have
    // been dragged while the pane was full size.
    if let Some(rect) = state
        .layout
        .rect_of(state.pane_viewport(), pane)
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

/// Enter in the rename sheet: the name is the field's text, trimmed, and an empty field removes it.
fn commit_rename(state: &mut State) {
    // The sheet is the only thing that renames, so taking its text and ending the state in one step is
    // unambiguous — and `end()` is what clears it, rather than a second way to leave the state.
    let Some((pane, text)) = state
        .rename()
        .map(|rename| (rename.pane, rename.edit.text.clone()))
    else {
        return;
    };
    state.end();
    if let Some(pane) = state.pane_mut(pane) {
        pane.name = shown_name(&text);
    }
    state.layout_dirty = true;
}

/// One row of the pane menu: what it says, what a capture calls it, and what a press does.
#[derive(Clone, Copy)]
pub struct MenuItem {
    pub label: &'static str,
    pub key: &'static str,
    pub action: fn(&mut State, ViewId),
}

impl MenuItem {
    /// Run the row's action on the pane the menu was asked on. The menu closes in the action, not here:
    /// what a row does is the row's, and one of them has to close the menu *before* it changes the layout.
    fn run(self, state: &mut State, pane: ViewId) {
        (self.action)(state, pane);
    }
}

/// The menu a pane offers. Ordered: the thing that grows the wall first, the thing that shrinks it last —
/// where the destructive action is not the one under the pointer by accident.
///
/// **Full size left the menu in #111.** It is the pane chrome's one-press toggle now; a row here was a
/// right-click, a row read, and a press — three actions for a thing an operator does constantly.
pub fn menu_items(state: &State, pane: ViewId) -> Vec<MenuItem> {
    let mut items = vec![MenuItem {
        label: MENU_SPLIT,
        key: "menu-split",
        action: split_from_menu,
    }];
    items.push(MenuItem {
        label: MENU_RENAME,
        key: "menu-rename",
        action: begin_rename,
    });
    if state.layout.len() > 1 {
        items.push(MenuItem {
            label: MENU_CLOSE,
            key: "menu-close",
            action: close_from_menu,
        });
    }
    let _ = pane;
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
pub const MENU_RENAME: &str = "Name this pane…";
pub const MENU_CLOSE: &str = "Close pane";
/// What the maximised pane's own close plate says, and the header's note about it (#85).
pub const MAX_CLOSE: &str = "back to the wall";
/// What the rename sheet is called, and what a name longer than `NAME_MAX_CHARS` ends with.
pub const RENAME_TITLE: &str = "Name this pane";
pub const ELLIPSIS: char = '…';

/// A right-click in a pane: focus it, and open the menu where the pointer is.
#[cfg(feature = "map")]
fn open_menu(state: &mut State, id: ViewId, event: &rune::PointerEvent, scale: f32) {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    state.layout.focus(id);
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
    state.layout.focus(id);
    // **A press on a pane's water clears that pane** (#99): the map underneath is that pane's, and the
    // press must not walk over to the pane that happens to have the keys.
    state.press_background(id);
    // The press was not for the menu (the menu's own rows take their presses), so the menu goes.
    if state.menu().is_some() {
        state.end();
    }
}

/// #81 `Close pane`: the pane goes, its slot collapses into its sibling, and its map thread is joined
/// before it is forgotten — a wall that leaks a render thread per closed pane is a wall that dies at the
/// third split.
fn close_from_menu(state: &mut State, id: ViewId) {
    state.end();
    if !state.layout.close(id) {
        // The last pane: nothing happened, and the menu said so by not offering it. A redraw keeps the
        // menu from being left open on a pane that is still there.
        state.layout_dirty = true;
        return;
    }
    // Dropping the pane's `LiveMap` stops its thread and joins it (the rule `close_maps` relies on).
    state.panes.retain(|pane| pane.id != id);
    state.layout_dirty = true;
}

/// `Focus on new pane`: the clicked ground becomes a pane of its own, same zoom.
///
/// The geographic point is the pane's **own** camera and frame, unprojected at the click — the inverse
/// of the projection every marker is drawn with, which is why it lives in `map::geometry` beside it.
fn split_from_menu(state: &mut State, id: ViewId) {
    let Some(menu) = state.menu() else {
        return;
    };
    state.end();
    let Some(pane) = state.pane(id) else { return };
    let Some(frame) = pane.frame else {
        // No frame yet means no camera to unproject against: a pane opened at a guessed point would
        // be a pane showing the wrong water, which is the failure `INV-04` exists to prevent.
        state.layout_dirty = true;
        return;
    };
    // `menu.at` is already in the frame's own pixels: element points × the display scale, taken when
    // the menu opened.
    let camera = crate::map::geometry::Camera::new(
        crate::map::geometry::unproject(
            frame.camera,
            frame.viewport(),
            f64::from(menu.at.0),
            f64::from(menu.at.1),
        ),
        frame.camera.zoom,
    );
    state.split_pane(id, camera);
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
    state.set_camera_of(id, moved);
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
            let origin = state
                .pane(id)
                .and_then(|pane| pane.drag_origin)
                .unwrap_or(camera);
            let moved = crate::map::geometry::pan(
                origin,
                frame.viewport(),
                f64::from(drag.dx),
                f64::from(drag.dy),
            );
            state.set_camera_of(id, moved);
            if let Some(map) = state.pane(id).and_then(|pane| pane.map.as_ref()) {
                map.set_camera(moved);
            }
        }
        rune::Phase::Up => {
            if let Some(pane) = state.pane_mut(id) {
                pane.drag_origin = None;
            }
            // One render now rather than at the next idle tick: the gesture just ended, and the
            // operator is looking at where it ended.
            if let Some(map) = state.pane(id).and_then(|pane| pane.map.as_ref()) {
                map.render_now();
            }
        }
    }
}

/// A drag on a tiled pane: a border grip resizes the split, anywhere else pans that pane's map (#69).
#[cfg(feature = "map")]
fn drag_pane(state: &mut State, id: ViewId, rect: Rect, drag: &mut rune::Drag, scale: f32) {
    let viewport = state.pane_viewport();
    match drag.phase {
        rune::Phase::Down => {
            let at = view::viewport_point(rect, (drag.start_x, drag.start_y));
            begin_resize(state, viewport, at);
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
                if state.layout.set_ratio(&border, border.ratio_for(pointer)) {
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
fn begin_resize(state: &mut State, viewport: crate::map::geometry::Viewport, at: (f32, f32)) {
    let Some(border) = state.layout.border_at(viewport, at, state.pane_grip()) else {
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
    let viewport = state.pane_viewport();
    match drag.phase {
        rune::Phase::Down => {
            let at = view::viewport_point(rect, (drag.start_x, drag.start_y));
            begin_resize(state, viewport, at);
        }
        rune::Phase::Move => {
            let Some(border) = state.resizing() else {
                return;
            };
            // A point, not a delta: the ratio follows the line the operator is holding, and a gesture
            // that leaves the gutter and comes back lands where the pointer is.
            let pointer = view::viewport_point(rect, (drag.x, drag.y));
            if state
                .layout
                .set_ratio(&border, border.ratio_for(border.axis_of(pointer)))
            {
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
    let viewport = state.pane_viewport();
    match drag.phase {
        rune::Phase::Down => {
            let at = view::viewport_point(rect, (drag.start_x, drag.start_y));
            begin_resize(state, viewport, at);
        }
        rune::Phase::Move => {
            if let Some(border) = state.resizing() {
                let pointer = view::viewport_point(rect, (drag.x, drag.y));
                let pointer = match border.dir {
                    Dir::Horizontal => pointer.0,
                    Dir::Vertical => pointer.1,
                };
                if state.layout.set_ratio(&border, border.ratio_for(pointer)) {
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
        for pane in state.panes.iter_mut() {
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
    for pane in state.panes.iter_mut() {
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
/// 3. the focused pane's card, which is the pane's own state (#99) and so goes last and only there.
fn on_key(state: &mut State, event: &rune::KeyEvent, _tx: &mut rune::Tx) -> bool {
    if event.key != rune::Key::Escape {
        return false;
    }
    if !matches!(state.interaction, Interaction::Idle) {
        state.end();
        return true;
    }
    if state.maximised().is_some() {
        restore_full_size(state);
        return true;
    }
    let focused = state.layout.focused();
    if state.selected_in(focused).is_none() {
        return false;
    }
    state.clear_selection(focused);
    true
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
                .panes
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
        // **One pane keeps the frame it has always had.** A capture with one pane is the wall as it was
        // before Multi View, and its goldens are byte-identical because of this line; a scene with more
        // panes gets each frame from that pane's own layout (`pane_laid_out`), which is where a wall
        // gets its frames too. `#71`'s golden pass unifies the two.
        if state.layout.is_single() {
            let camera = state.camera();
            state.set_frame(crate::map::geometry::Rendered {
                camera,
                width: tokens::WINDOW_SIZE.0 as u32,
                height: tokens::WINDOW_SIZE.1 as u32,
            });
        }
        return;
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
    #[cfg(feature = "map")]
    {
        // The first frame's layout corrects this size; 1280x720 is the window it opens at, so the map
        // is already rendering the right ground while the layout is being computed. Every pane gets its
        // own thread through the same door (`State::split_pane`).
        let camera = state.camera();
        if let Some(pane) = state.pane_mut(state.layout.focused()) {
            pane.map = Some(crate::map::thread::MapThread::spawn(
                camera,
                (tokens::WINDOW_SIZE.0 as u32, tokens::WINDOW_SIZE.1 as u32),
            ));
        }
    }
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
    // systemd stops a wall with `SIGTERM`. Quitting through the runtime's own path drops the GL
    // context in order, instead of a hard exit that leaves the driver complaining.
    if crate::platform::terminated() {
        tracing::info!(
            "terminating on SIGTERM/SIGINT after {} ms",
            crate::platform::since_start()
        );
        // **Every pane's map thread is joined here, not by the drop of a `State` the runtime owns.**
        // The engine asserts in `~RunLoop` when a process with a live map walks out, and with N panes
        // there are N contexts to close (#71).
        state.close_maps();
        tx.quit();
        return false;
    }

    let mut changed = drain_socket(state);
    changed |= drain_card_reads(state);
    changed |= tick_card_age(state);

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
                panes = state.panes.len(),
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

    // **The focused pane is the one that renders at the wall's cadence; the others are there, not
    // live** (#71). Told to the threads when the focus changes, so a wall at rest does not send a
    // message a frame.
    #[cfg(feature = "map")]
    if !state.headless && state.cadenced != Some(state.layout.focused()) {
        state.cadenced = Some(state.layout.focused());
        state.apply_cadence();
    }
    // The frame each pane's map last handed over is what that pane's markers project against
    // (`INV-04`). Reading it here, rather than in the view, keeps the view a function of the state.
    #[cfg(feature = "map")]
    {
        for index in 0..state.panes.len() {
            let rendered = state.panes[index]
                .map
                .as_ref()
                .and_then(|map| map.rendered());
            if let Some(frame) = rendered
                && state.panes[index].frame != Some(frame)
            {
                state.panes[index].frame = Some(frame);
            }
            if state.panes[index].frame != state.panes[index].frame_seen {
                state.panes[index].frame_seen = state.panes[index].frame;
                changed = true;
            }
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
    for pane in &mut state.panes {
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
        .panes
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

    // A picture still decoding is a reason to draw the next frame, and the frame a decode *lands* on is a
    // reason to draw one more. Both matter, and the second is the one that was missing (`#51`): a decode
    // that completes between two polls changes nothing the loop would otherwise notice, so a wall that
    // only redrew "while pending" kept the last tree — the symbol — forever. The waker now wakes the app
    // too, but a headless render is not waiting on one, and this costs two comparisons per frame.
    changed |= pictures_need_a_frame(&mut state.pictures_pending, &state.pictures);
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
            || State::default().with_saved_settings(),
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
        state.card_in(state.layout.focused())
    }

    /// The age text the focused pane's card drew last, which is what `tick_card_age` compares.
    fn card_age(state: &State) -> String {
        state
            .pane(state.layout.focused())
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
        state.select_in(state.layout.focused(), 8);
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
        let first = state.layout.focused();
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
        let first = state.layout.focused();
        let camera = state.camera_of(first);
        let second = state.split_pane(first, camera).expect("a second pane");
        state.select_in(second, 8);
        assert!(state.card_in(second).is_some());

        assert!(state.layout.close(second));
        state.panes.retain(|pane| pane.id != second);
        assert!(
            state.panes.iter().all(|pane| pane.view.selected.is_none()),
            "no pane is left holding the closed pane's selection"
        );
    }

    /// The card is a view of one hull, so that hull's fix has to reach it.
    #[test]
    fn a_fix_updates_the_open_card() {
        let mut state = state_with_two_hulls();
        state.select_in(state.layout.focused(), 8);
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
        state.select_in(state.layout.focused(), 8);
        assert_eq!(card_of(&state).map(|card| card.unit.id_unit), Some(8));
        state.select_in(state.layout.focused(), 9);
        assert_eq!(
            card_of(&state).map(|card| card.unit.id_unit),
            Some(9),
            "the second selection replaces the first"
        );
        assert_eq!(state.selection(), Some(9));
        let focused = state.layout.focused();
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
        state.press_marker(state.layout.focused(), 8);
        state.press_background(state.layout.focused());
        assert_eq!(
            state.selection(),
            Some(8),
            "the map's handler stood down for the marker's press"
        );
        assert!(card_of(&state).is_some());

        let mut state = state_with_two_hulls();
        state.press_marker(state.layout.focused(), 8);
        state.marker_press = false; // as the end of that frame does
        state.press_marker(state.layout.focused(), 8);
        assert!(
            card_of(&state).is_none(),
            "the second press on the same hull closes its card"
        );

        state.marker_press = false;
        state.press_background(state.layout.focused());
        assert_eq!(state.selection(), None, "a press on the map clears");
    }

    #[test]
    fn the_card_is_a_panel_with_a_real_height() {
        // The reported bug: `.size(WIDTH, 0.0)` painted the fill into a zero-height box, so the card
        // was loose text over the map. A drawn height is the proof, and the panel's own colour at its
        // centre is the second half of it.
        let mut state = state_with_two_hulls();
        state.select_in(state.layout.focused(), 8);
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
        let focused = state.layout.focused();
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
        let focused = state.layout.focused();
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
        state.select_in(state.layout.focused(), 8);
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
        let pane_key = rt.state.layout.focused();
        let (x, y) = rt
            .find(&format!("hit-{pane_key}-8"))
            .expect("the marker has a hit target");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.invalidate();
        rt.frame(1.0 / 60.0);
        assert!(
            rt.state.card_in(rt.state.layout.focused()).is_some(),
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
        rt.state.clear_selection(rt.state.layout.focused());
        assert!(rt.state.card_in(rt.state.layout.focused()).is_none());
    }

    #[test]
    fn a_card_for_a_hull_without_a_hull_number_has_no_hull_row() {
        // An empty row is worse than a missing one: it claims the field exists and is blank.
        let mut state = state_with_two_hulls();
        state.select_in(state.layout.focused(), 9);
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
        assert!(state.layout.is_single());
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
        let first = state.layout.ids()[0];
        // The first pane's thread is the one `setup` starts on a wall; `split_pane` starts the others.
        if let Some(pane) = state.pane_mut(first) {
            pane.map = Some(crate::map::thread::MapThread::spawn(
                Camera::START,
                (640, 360),
            ));
        }
        let mut ids = vec![first];
        for index in 1..4 {
            let from = ids[index - 1];
            let id = state
                .split_pane(from, Camera::START)
                .expect("a split spawns a pane");
            ids.push(id);
        }
        assert_eq!(
            state.panes.iter().filter(|pane| pane.map.is_some()).count(),
            4,
            "four panes, four map threads"
        );
        // Wait for each pane to publish a frame, so this test is about closing live contexts.
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(60)
            && state.panes.iter().any(|pane| {
                pane.map
                    .as_ref()
                    .is_some_and(|map| map.rendered().is_none())
            })
        {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let drawn = state
            .panes
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
            state.panes.iter().all(|pane| pane.map.is_none()),
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
        use crate::map::thread::{IDLE_INTERVAL, MapThread, UNFOCUSED_INTERVAL};
        use std::time::Duration;

        let mut state = State {
            headless: false,
            panel: Some((1280.0, 720.0)),
            ..State::default()
        };
        let first = state.layout.ids()[0];
        if let Some(pane) = state.pane_mut(first) {
            pane.map = Some(MapThread::spawn(Camera::START, (640, 360)));
        }
        let mut ids = vec![first];
        for index in 1..4 {
            let from = ids[index - 1];
            ids.push(state.split_pane(from, Camera::START).expect("a split"));
        }
        state.layout.focus(ids[0]);
        state.apply_cadence();
        for pane in &state.panes {
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
            && state.panes.iter().any(|pane| {
                pane.map
                    .as_ref()
                    .is_some_and(|map| map.frames() == 0 && map.error().is_none())
            })
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        println!(
            "panes: {} panes, {} with a map thread, first frames after {:?}",
            state.panes.len(),
            state.panes.iter().filter(|p| p.map.is_some()).count(),
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
        assert!(state.layout.is_single(), "the wall this test is about");
        let pane = state.layout.focused();
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
        let pane = state.layout.focused();
        state.begin(Begin::Menu {
            pane,
            at: (10.0, 10.0),
        });
        assert!(card_of(&state).is_some(), "the scene has a card open");
        let rt = frame_once(state);
        assert!(rt.find("pane-menu").is_some());

        // What `on_key` does on Escape: the menu first, the selection untouched.
        let mut state = crate::scene::state_for("card-open");
        let pane = state.layout.focused();
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
        let focused = state.layout.focused();
        state.select_in(focused, 8);
        assert!(state.layout.maximise(focused), "the pane goes full size");
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
        let tiles = state.layout.rects(state.pane_viewport());
        let cameras: Vec<_> = state.panes.iter().map(|pane| pane.camera).collect();
        let id = tiles[1].0;

        full_size_for_scene(&mut state, id);
        assert_eq!(state.maximised(), Some(id));
        assert_eq!(
            state.layout.focused(),
            id,
            "the keys go to the pane being looked at"
        );
        assert_eq!(
            state.layout.rects(state.pane_viewport()),
            tiles,
            "the tiling is untouched: a full-size pane is a bigger rectangle, not a new layout"
        );

        restore_full_size(&mut state);
        assert_eq!(state.maximised(), None);
        assert_eq!(state.layout.rects(state.pane_viewport()), tiles);
        assert_eq!(
            state
                .panes
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
    #[test]
    fn the_rename_sheet_names_a_pane_and_abandons_cleanly() {
        let mut state = crate::scene::state_for("panes");
        let id = state.layout.focused();
        begin_rename(&mut state, id);
        let rename = state.rename().expect("the sheet opens");
        assert_eq!(rename.pane, id);
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
        assert!(
            state.pane(id).and_then(name_plate).is_none(),
            "an unnamed pane draws no plate"
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
        let cells = state.layout.rects(state.pane_viewport());
        let before = state.pane_rects();
        assert_eq!(before, cells, "at gap 0 the panes are the cells");

        // The middle of the border, where the gutter will be: 4 pt from the line once the gap is on.
        let mid = (cells[0].1.right(), cells[0].1.y + cells[0].1.h / 2.0);
        assert!(
            state
                .layout
                .border_at(state.pane_viewport(), mid, state.pane_grip())
                .is_some(),
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
            state
                .layout
                .border_at(state.pane_viewport(), mid, state.pane_grip())
                .is_some(),
            "a pointer in the middle of the gutter still grabs the border"
        );

        // And back off again, which is what the second press of the row does.
        toggle_pane_gap(&mut state);
        assert_eq!(state.settings.pane_gap, 0.0);
        assert_eq!(state.pane_rects(), cells);
    }

    /// #81/#111: a wall with one pane offers two items, a tiled wall three, and the menu is as tall as the
    /// items it has — the surface used to be `size()`d for one row, which clips the second. `Full size`
    /// left the menu for the pane chrome, so the row count is one lower than it was.
    #[test]
    fn the_menu_offers_close_only_when_there_is_something_to_close() {
        let one = crate::scene::state_for("pane-menu");
        let pane = one.layout.focused();
        assert_eq!(
            crate::ui::menu_items(&one, pane)
                .iter()
                .map(|item| item.label)
                .collect::<Vec<_>>(),
            vec![MENU_SPLIT, MENU_RENAME],
            "the last pane cannot be closed, so the menu does not offer it"
        );

        let tiled = crate::scene::state_for("panes-menu");
        let pane = tiled.layout.focused();
        let items = crate::ui::menu_items(&tiled, pane);
        assert_eq!(
            items.iter().map(|item| item.label).collect::<Vec<_>>(),
            vec![MENU_SPLIT, MENU_RENAME, MENU_CLOSE]
        );
        assert_eq!(
            tokens::menu_size(items.len()),
            (tokens::MENU_WIDTH, tokens::MENU_ROW * 3.0),
            "three rows, not one row's box clipped"
        );
    }

    /// #81: closing a pane takes its slot out of the layout, keeps the focus on a live pane, and refuses the
    /// last one — the three things the menu row relies on.
    #[test]
    fn closing_a_pane_collapses_its_slot_and_keeps_a_focused_pane() {
        let mut state = crate::scene::state_for("panes-menu");
        let closed = state.layout.focused();
        let before = state.layout.rects(state.pane_viewport()).len();
        close_from_menu(&mut state, closed);
        let rects = state.layout.rects(state.pane_viewport());
        assert_eq!(rects.len(), before - 1, "one pane fewer");
        assert!(
            !rects.iter().any(|(id, _)| *id == closed),
            "the closed pane is gone from the layout"
        );
        assert!(
            !state.panes.iter().any(|pane| pane.id == closed),
            "and gone from the wall, so its map thread is joined"
        );
        assert!(
            rects.iter().any(|(id, _)| *id == state.layout.focused()),
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
        while state.layout.len() > 1 {
            let next = state.layout.focused();
            close_from_menu(&mut state, next);
        }
        assert_eq!(state.panes.len(), 1, "one pane left, and one pane object");
        let last = state.layout.focused();
        close_from_menu(&mut state, last);
        assert_eq!(
            state.layout.rects(state.pane_viewport()).len(),
            1,
            "the last pane is not closeable: a wall with no panes draws nothing"
        );
        assert_eq!(state.panes.len(), 1);
    }
}
