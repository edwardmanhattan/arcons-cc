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
//!
//! Without `--features map` the map element is absent and the panel says so: the engine is optional
//! so the gate stays free of C++ and of a network. `--probe-map` is the headless witness.

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
            #[cfg(feature = "map")]
            drag_origin: None,
        }
    }
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
    /// The open context menu, if any: where it was asked for, and on which pane.
    pub menu: Option<Menu>,
    /// Set when a layout pass changed something the drawing depends on (the panel's size, a pane's
    /// frame), so `poll` redraws once instead of every frame.
    pub layout_dirty: bool,
    /// Whether the first map frame's camera has been reported. One log line per process, so a wall showing
    /// the wrong sea leaves a trace rather than a screenshot to argue with (`#56`).
    pub camera_reported: bool,
    /// The socket actor, when a credential was configured. `None` means the wall is read-only.
    pub live: Option<LiveThread>,
    /// The reads thread: the card asks it for identity, commander and specification.
    pub backend: Option<crate::data::Backend>,
    /// The selected hull. One at a time, always (`INV-10`).
    pub selected: Option<i64>,
    /// The open card, built when the selection or its fix changes — never per frame.
    pub card: Option<crate::units::card::Card>,
    /// The age text shown last frame, so a second passing redraws the card and nothing else.
    last_age: String,
    /// What the socket last said about itself.
    pub link: Link,
    /// How many fixes the socket has delivered **and the picture applied**, for the header's
    /// "is anything happening" question.
    pub fixes: u64,
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
    /// Whether the wall is drawing symbols or pictures, latched with hysteresis on the frame's zoom
    /// (`units::mark`). One latch for the wall: the decision is about the zoom, not about a hull.
    pub range: crate::units::mark::Range,
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
            panel: None,
            menu: None,
            layout_dirty: false,
            live: None,
            backend: None,
            selected: None,
            card: None,
            last_age: String::new(),
            link: Link::default(),
            fixes: 0,
            stale_after: config
                .map(|config| config.stale_after)
                .unwrap_or(crate::units::STALE_AFTER),
            origin: std::time::Instant::now(),
            logical: 0.0,
            headless: false,
            marker_press: false,
            last_report_ms: 0,
            range: crate::units::mark::Range::default(),
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

    /// Where each pane is drawn, from the same function `verify` checks a capture with.
    pub fn pane_rects(&self) -> Vec<(ViewId, Rect)> {
        self.layout.rects(self.pane_viewport())
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
                self.range,
                &|unit| self.sizes.get(&unit).copied(),
            )
            .into_iter()
            .find(|marker| marker.id_unit == id_unit)?;
            let _ = marker;
            Some(id)
        })
    }

    /// Select a hull: build the card from the picture and ask the reads thread for the rest.
    ///
    /// Selecting the same hull twice does not re-ask: a card that re-fetches on every press is a card
    /// that flickers between answers.
    pub fn select(&mut self, id_unit: i64) {
        if self.selected == Some(id_unit) {
            return;
        }
        self.selected = Some(id_unit);
        self.rebuild_card();
        if let Some(backend) = &self.backend {
            backend.request(crate::data::Command::Unit(id_unit));
            backend.request(crate::data::Command::Commander(id_unit));
            backend.request(crate::data::Command::Specifications(id_unit));
        }
    }

    /// A press on a hull's marker: select it, or close its card if it is already open.
    ///
    /// One click, one meaning — which is why the marker's press and the map's "press the background
    /// to clear" handler do not both decide: pressing a marker records that this press was a
    /// marker's, and the map's handler stands down for it.
    pub fn press_marker(&mut self, id_unit: i64) {
        self.marker_press = true;
        if self.selected == Some(id_unit) {
            self.clear_selection();
        } else {
            self.select(id_unit);
        }
    }

    /// A press on the map itself: clear, unless that press was a marker's.
    pub fn press_background(&mut self) {
        if !self.marker_press {
            self.clear_selection();
        }
    }

    /// Clear the selection. One card at a time means clearing the only one there is.
    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.card = None;
        self.last_age.clear();
    }

    /// Rebuild the card from the current picture — on selection, and when its fix changes.
    fn rebuild_card(&mut self) {
        let Some(id_unit) = self.selected else {
            self.card = None;
            return;
        };
        let Some(view) = self
            .picture
            .units()
            .iter()
            .find(|unit| unit.unit.id_unit == id_unit)
        else {
            // A selection for a hull the picture no longer names: the card goes with it.
            self.card = None;
            return;
        };
        let now = self.now();
        let previous = self.card.take();
        let mut card = crate::units::card::Card::new(view, now, self.stale_after);
        // Keep what the reads already said for THIS hull: re-asking on every fix would make the
        // commander row flicker between answers every few seconds.
        if let Some(previous) = previous
            && previous.unit.id_unit == id_unit
        {
            card.identity = previous.identity;
            card.commander = previous.commander;
            card.specification = previous.specification;
        }
        self.card = Some(card);
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
    root.child(picture(state, scale))
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
fn picture(state: &State, scale: f32) -> El {
    let body = col()
        .key("picture")
        .w_full()
        .h_full()
        .grow(1.0)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::HAIRLINE, tokens::hairline())
        .fill(tokens::panel())
        .on_layout(|state: &mut State, [_, _, width, height]| panel_laid_out(state, width, height));

    if state.layout.is_single() {
        let Some((id, _)) = state.pane_rects().into_iter().next() else {
            return body;
        };
        return single_pane(state, id, scale, body);
    }

    let mut body = body;
    for (id, rect) in state.pane_rects() {
        body = body.child(pane(state, id, rect, scale));
    }
    // The context menu is drawn last, over every pane, at the point it was asked for: a menu under the
    // pane it belongs to would be clipped by the next split.
    body = menu_view(state, scale, body);
    body
}

/// One pane filling the panel: the whole wall, exactly as it was before Multi View.
fn single_pane(state: &State, id: ViewId, scale: f32, body: El) -> El {
    let body = picture_content(state, id, scale, true, body);
    let Some(pane) = state.pane(id) else {
        return body;
    };
    let Some(frame) = pane.frame else {
        return body;
    };
    let sizes = |id_unit: i64| state.sizes.get(&id_unit).copied();
    let placed = markers::place(
        &state.picture,
        &frame,
        state.now(),
        state.stale_after,
        state.range,
        &sizes,
    );
    let pictures = markers::Pictures {
        images: &state.pictures,
    };
    let mut layer = body.child(markers::layer(&placed, &frame, scale, &pictures));
    if let Some(card) = &state.card
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

/// One pane: its picture, its markers, its own frame, and the ring that says the keys are here.
fn pane(state: &State, id: ViewId, rect: Rect, scale: f32) -> El {
    let focused = state.layout.focused() == id;
    let single = state.layout.is_single();
    let mut body = el()
        .key(format!("pane-{id}"))
        .absolute(rect.x, rect.y)
        .size(rect.w.max(1.0), rect.h.max(1.0))
        .on_layout(move |state: &mut State, [_, _, width, height]| {
            pane_laid_out(state, id, width, height, scale)
        });
    // The ring means "the keys are here rather than there", so a wall with one pane has none: the
    // panel's own hairline is that pane's frame, and a second edge drawn on it is a line that says
    // nothing.
    if !single {
        // **A pane is a window on its own water**: a marker or a picture that overflows must not appear
        // in the pane beside it. Only when there *is* a pane beside it — a clip costs the marker layer
        // about a third of its frame CPU (`INV-12`: 5.07 → 3.78 ms reference for 200 markers), and a
        // wall with one pane has nothing to clip against. `#71` measures the multi-pane case, where the
        // clip is doing work.
        body = body.clip_rounded(0.0).border(
            tokens::HAIRLINE,
            if focused {
                tokens::focus_edge()
            } else {
                tokens::hairline()
            },
        );
    }

    body = picture_content(state, id, scale, false, body);

    let Some(pane) = state.pane(id) else {
        return body;
    };
    // Markers project against THIS pane's frame (`INV-04`): its camera, its size, and nothing else.
    let Some(frame) = pane.frame else {
        return body;
    };
    let sizes = |id_unit: i64| state.sizes.get(&id_unit).copied();
    let placed = markers::place(
        &state.picture,
        &frame,
        state.now(),
        state.stale_after,
        state.range,
        &sizes,
    );
    let pictures = markers::Pictures {
        images: &state.pictures,
    };
    let mut layer = body.child(markers::layer(&placed, &frame, scale, &pictures));

    // The card points at its marker's DRAWN position inside its own pane, so it follows a hull that is
    // travelling and a map that has been panned. Placement is arithmetic in `units::card::place`, where
    // it is property-tested against the window.
    if let Some(card) = &state.card
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

/// One pane's picture: the map's pixels, its failure, or the placeholder.
#[cfg(feature = "map")]
fn picture_content(state: &State, id: ViewId, scale: f32, single: bool, body: El) -> El {
    if let Some(map) = state.pane(id).and_then(|pane| pane.map.as_ref()) {
        if map.error().is_none() {
            return body.child(
                rune::el()
                    .key("map")
                    .w_full()
                    .h_full()
                    .pixels(map.pixels())
                    .on_wheel(move |state: &mut State, event: rune::WheelEvent, _tx| {
                        wheel(state, id, &event)
                    })
                    .on_drag(move |state: &mut State, drag, _tx| drag_map(state, id, drag))
                    .on_secondary_press_local(
                        move |state: &mut State, event: rune::PointerEvent, _tx| {
                            open_menu(state, id, &event, scale)
                        },
                    )
                    .on_press(move |state: &mut State| press_in(state, id)),
            );
        }
        if single {
            // One pane: the map element reports its own size, as it always has, so a wall with no
            // panes is the wall it was. With panes, the pane's wrapper does it — one `on_layout` per
            // frame is a measurable cost when 200 markers are inside it (`INV-12`).
            return body.child(
                rune::el()
                    .key("map")
                    .w_full()
                    .h_full()
                    .pixels(map.pixels())
                    .on_layout(move |state: &mut State, [_, _, width, height]| {
                        pane_laid_out(state, id, width, height, scale)
                    })
                    .on_wheel(move |state: &mut State, event: rune::WheelEvent, _tx| {
                        wheel(state, id, &event)
                    })
                    .on_drag(move |state: &mut State, drag, _tx| drag_map(state, id, drag))
                    .on_secondary_press_local(
                        move |state: &mut State, event: rune::PointerEvent, _tx| {
                            open_menu(state, id, &event, scale)
                        },
                    )
                    .on_press(move |state: &mut State| press_in(state, id)),
            );
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
fn picture_content(state: &State, _id: ViewId, _scale: f32, _single: bool, body: El) -> El {
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

/// The context menu, at the point it was asked for: one item, and it says what it does.
fn menu_view(state: &State, scale: f32, body: El) -> El {
    let Some(menu) = state.menu else { return body };
    let Some(rect) = state.layout.rect_of(state.pane_viewport(), menu.pane) else {
        return body;
    };
    let points = if scale > 0.0 { scale } else { 1.0 };
    let item = text(MENU_SPLIT)
        .key("menu-split")
        .font(tokens::LABEL)
        .color(tokens::ink());
    let surface = col()
        .key("pane-menu")
        .absolute(rect.x + menu.at.0 / points, rect.y + menu.at.1 / points)
        .padding(tokens::GAP)
        .gap(tokens::GAP)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::HAIRLINE, tokens::card_edge())
        .fill(tokens::bg())
        .child(item)
        .on_press(move |state: &mut State| split_from_menu(state, menu.pane));
    body.child(surface)
}

/// What the pane menu offers. One item for now: #69 and #70 add the border drag and the swap, and a
/// `Close pane` belongs with them.
pub const MENU_SPLIT: &str = "Focus on new pane";

/// A right-click in a pane: focus it, and open the menu where the pointer is.
#[cfg(feature = "map")]
fn open_menu(state: &mut State, id: ViewId, event: &rune::PointerEvent, scale: f32) {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    state.layout.focus(id);
    state.menu = Some(Menu {
        pane: id,
        at: (event.x * scale, event.y * scale),
    });
    state.layout_dirty = true;
}

/// A primary press in a pane: close the menu (the click was not for it) and focus the pane.
#[cfg(feature = "map")]
fn press_in(state: &mut State, id: ViewId) {
    state.layout.focus(id);
    if state.menu.take().is_some() {
        state.layout_dirty = true;
    }
}

/// `Focus on new pane`: the clicked ground becomes a pane of its own, same zoom.
///
/// The geographic point is the pane's **own** camera and frame, unprojected at the click — the inverse
/// of the projection every marker is drawn with, which is why it lives in `map::geometry` beside it.
fn split_from_menu(state: &mut State, id: ViewId) {
    let Some(menu) = state.menu.take() else {
        return;
    };
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

/// How the window opens: sized from the command line, or covering the primary output.
///
/// Rune at this rev exposes no fullscreen API, so `--fullscreen` is the closest honest thing —
/// borderless, at the primary output's own size and position. #8 revisits it once the wall's output
/// is chosen, and records what it decided.
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
    let Some(card) = state.card.as_mut() else {
        return changed;
    };
    for reply in replies {
        match reply {
            crate::data::Reply::Unit(id, result) if Some(id) == state.selected => {
                card.identity = match result {
                    Ok(unit) => crate::units::card::Fetch::Ready(*unit),
                    Err(error) => crate::units::card::Fetch::Failed(error.to_string()),
                };
                changed = true;
            }
            crate::data::Reply::Commander(id, result) if Some(id) == state.selected => {
                card.commander = match result {
                    Ok(entries) => crate::units::card::Fetch::Ready(entries),
                    Err(error) => crate::units::card::Fetch::Failed(error.to_string()),
                };
                changed = true;
            }
            crate::data::Reply::Specifications(id, result) if Some(id) == state.selected => {
                card.specification = match result {
                    Ok(versions) => crate::units::card::Fetch::Ready(versions),
                    Err(error) => crate::units::card::Fetch::Failed(error.to_string()),
                };
                changed = true;
            }
            _ => {}
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
    let Some(card) = &state.card else {
        return false;
    };
    // The age advances on the **frame** clock, not the wall clock: `poll`'s answer decides when Rune
    // rebuilds the element tree, so a real-clock comparison here makes *when* the card is redrawn
    // depend on how fast the machine is — which is exactly how a capture stops being reproducible
    // (#35). Same rule as every other `now` in the view: `State::now()` and nothing else.
    let text = card.age_text(state.now());
    if text == state.last_age {
        return false;
    }
    state.last_age = text;
    true
}

/// Escape closes the card. A key press the focused control did not take comes here.
fn on_key(state: &mut State, event: &rune::KeyEvent, _tx: &mut rune::Tx) -> bool {
    if !closes_on_escape(event, state.selected) {
        return false;
    }
    state.clear_selection();
    true
}

/// The decision, without a `Tx`: Escape closes an open card and is consumed, and with nothing open it
/// is left for whatever else wants it.
fn closes_on_escape(event: &rune::KeyEvent, selected: Option<i64>) -> bool {
    event.key == rune::Key::Escape && selected.is_some()
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
        Update::Fix(fix) => {
            // **This is the wall.** A publication is the only thing that makes the picture newer than
            // the last REST read (#28): counting it and keeping the old position is a photograph with
            // a counter on it.
            state.picture.apply_fix(&fix);
            state.fixes += 1;
            // A card is a view of one unit, so it follows that unit's fix. Rebuilt here rather than
            // every frame: a fix for another hull does not change this card's numbers.
            if state.selected == Some(fix.id_unit) {
                state.rebuild_card();
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
    // systemd stops a wall with `SIGTERM`. Quitting through the runtime's own path drops the GL
    // context in order, instead of a hard exit that leaves the driver complaining.
    if crate::platform::terminated() {
        tracing::info!(
            "terminating on SIGTERM/SIGINT after {} ms",
            crate::platform::since_start()
        );
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
    // The range follows the frame's zoom, not the camera's: mid-gesture they disagree, and a hull must
    // not switch representation while it is still projected against the previous frame.
    if let Some(frame) = state.frame() {
        let latched = state.range.update(frame.camera.zoom);
        if latched != state.range {
            state.range = latched;
            changed = true;
        }
    }
    // In picture range the wall needs two things the feed does not carry: the register (picture URLs,
    // one read for the fleet) and each hull's dimensions (what the picture is scaled by). Both are
    // asked for a bounded number per frame, off the UI thread, and neither is asked in symbol range
    // where nothing uses them.
    if state.range == crate::units::mark::Range::Picture
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
        ..AppSpec::new(TITLE, window.size, State::default, view)
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
        state.select(8);
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
            state.last_age, "5 s old",
            "the fixture's fix is 4 s old at logical 0"
        );

        state.logical = 2.0;
        assert!(tick_card_age(&mut state));
        assert_eq!(state.last_age, "6 s old");
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
            crate::live::Update::Fix(crate::live::wire::LiveFix {
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
            }),
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

    /// The card is a view of one hull, so that hull's fix has to reach it.
    #[test]
    fn a_fix_updates_the_open_card() {
        let mut state = state_with_two_hulls();
        state.select(8);
        assert_eq!(
            state.card.as_ref().map(|card| card.speed_kn),
            Some(Some(0.0))
        );

        apply_update(
            &mut state,
            crate::live::Update::Fix(crate::live::wire::LiveFix {
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
            }),
        );

        assert_eq!(
            state.card.as_ref().map(|card| card.speed_kn),
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
        state.select(8);
        assert_eq!(state.card.as_ref().map(|card| card.unit.id_unit), Some(8));
        state.select(9);
        assert_eq!(
            state.card.as_ref().map(|card| card.unit.id_unit),
            Some(9),
            "the second selection replaces the first"
        );
        assert_eq!(state.selected, Some(9));
        state.clear_selection();
        assert!(
            state.card.is_none(),
            "INV-10: a cleared selection has no card"
        );
        assert_eq!(state.selected, None);
    }

    #[test]
    fn a_press_on_a_hull_toggles_it_and_a_press_on_the_map_does_not_steal_it() {
        // The two handlers fire for one click. Whichever runs first, the marker's press wins and the
        // map's does not undo it — and pressing the same hull again closes the card.
        let mut state = state_with_two_hulls();
        state.press_marker(8);
        state.press_background();
        assert_eq!(
            state.selected,
            Some(8),
            "the map's handler stood down for the marker's press"
        );
        assert!(state.card.is_some());

        let mut state = state_with_two_hulls();
        state.press_marker(8);
        state.marker_press = false; // as the end of that frame does
        state.press_marker(8);
        assert!(
            state.card.is_none(),
            "the second press on the same hull closes its card"
        );

        state.marker_press = false;
        state.press_background();
        assert_eq!(state.selected, None, "a press on the map clears");
    }

    #[test]
    fn the_card_is_a_panel_with_a_real_height() {
        // The reported bug: `.size(WIDTH, 0.0)` painted the fill into a zero-height box, so the card
        // was loose text over the map. A drawn height is the proof, and the panel's own colour at its
        // centre is the second half of it.
        let mut state = state_with_two_hulls();
        state.select(8);
        let rt = frame_once(state);
        let (rect, _) = rt.presented("card").expect("the card is drawn");
        assert!(
            rect[3] > 100.0,
            "the card panel has a real height, got {} (zero means the fill is invisible)",
            rect[3]
        );
        assert!(rect[2] > 200.0, "and a width, got {}", rect[2]);
    }

    #[test]
    fn escape_closes_the_card_only_when_there_is_one() {
        let mut state = state_with_two_hulls();
        let escape = rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        };
        assert!(
            !closes_on_escape(&escape, state.selected),
            "Escape with nothing open is not consumed"
        );
        state.select(8);
        assert!(
            closes_on_escape(&escape, state.selected),
            "Escape closes the open card"
        );
        state.clear_selection();
        assert!(state.card.is_none());
        // And a key that is not Escape never closes it, however the state looks.
        let tab = rune::KeyEvent {
            key: rune::Key::Tab,
            mods: rune::Mods::default(),
        };
        assert!(!closes_on_escape(&tab, Some(8)));
    }

    #[test]
    fn the_open_card_is_drawn_inside_the_window_and_keeps_its_marker() {
        // INV-10 at the render level: the card exists, is on screen, and stays inside the window for
        // a marker at the very edge (where a careless card would clip).
        let mut state = state_with_two_hulls();
        state.select(8);
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
        let (x, y) = rt.find("hit-8").expect("the marker has a hit target");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.invalidate();
        rt.frame(1.0 / 60.0);
        assert!(
            rt.state.card.is_some(),
            "a press on the marker opens its card"
        );
        let before = rt.presented("card").expect("card drawn").0;
        let marker_before = rt.find("marker-8").expect("marker placed");

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

        let marker_after = rt.find("marker-8").expect("marker still placed");
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
        rt.state.clear_selection();
        assert!(rt.state.card.is_none());
    }

    #[test]
    fn a_card_for_a_hull_without_a_hull_number_has_no_hull_row() {
        // An empty row is worse than a missing one: it claims the field exists and is blank.
        let mut state = state_with_two_hulls();
        state.select(9);
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
}
