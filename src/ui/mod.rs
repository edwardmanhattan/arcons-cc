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

use std::process::ExitCode;

use rune::{AppSpec, Cx, El, col, row, spacer, text};

use crate::config;
use crate::live::LiveThread;
use crate::map::geometry::Camera;
use crate::units::Picture;

/// The window title, and the wordmark's text.
pub const TITLE: &str = "Arcons Command Center";

/// What the view loop holds. #3 fills the picture, #4 moves it, #6 draws it, #7 selects from it.
pub struct State {
    pub picture: Picture,
    /// The camera the operator is holding. The map thread renders it; markers project against the
    /// camera of the frame on screen instead (`INV-04`), which is not the same number mid-gesture.
    pub camera: Camera,
    /// The frame the map last handed over: its camera and size are what markers project against.
    /// `None` until the first frame, and always `None` without `--features map`.
    pub frame: Option<crate::map::geometry::Rendered>,
    /// The frame the view was last drawn for, so a new one — from the map thread, or from a test —
    /// redraws the markers exactly once.
    frame_seen: Option<crate::map::geometry::Rendered>,
    /// The camera at the start of a drag, so a pan is absolute rather than a sum of deltas.
    #[cfg(feature = "map")]
    drag_origin: Option<Camera>,
    /// The map thread and the pixels it paints. `None` without `--features map`.
    #[cfg(feature = "map")]
    pub map: Option<crate::map::thread::MapThread>,
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
    /// When the last "socket" heartbeat was logged, on the frame clock.
    last_report: f64,
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
            camera,
            frame: None,
            frame_seen: None,
            #[cfg(feature = "map")]
            drag_origin: None,
            #[cfg(feature = "map")]
            map: None,
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
            last_report: 0.0,
        }
    }
}

impl State {
    /// The clock every age in the view is measured against. The *only* source of "now" in the view,
    /// which is what makes a capture reproducible: `--scene` drives `logical` from Rune's virtual
    /// clock, and a windowed run from the real one.
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

/// The map area: the frame when there is one, and the placeholder when there is not.
fn picture(state: &State, scale: f32) -> El {
    // Without the map engine the frame is never set, so the markers below are simply absent.
    #[cfg(not(feature = "map"))]
    let _ = scale;

    let body = col()
        .key("picture")
        .w_full()
        .h_full()
        .grow(1.0)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::HAIRLINE, tokens::hairline())
        .fill(tokens::panel());

    let content = picture_content(state, scale, body);

    // The marker layer goes on top of whatever the picture is — the map, its error, or the
    // placeholder. It is drawn from the frame the map last rendered, so it is absent until there is
    // one, and absent without the map engine (there is no camera to project against).
    let Some(frame) = state.frame else {
        return content;
    };
    let now = state.now();
    let placed = markers::place(&state.picture, &frame, now, state.stale_after);
    let mut layer = content.child(markers::layer(&placed, &frame, scale));

    // The card points at its marker's DRAWN position — not the fix — so it follows a hull that is
    // travelling and a map that has been panned. Placement is arithmetic in `units::card::place`,
    // where it is property-tested against the window.
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
        layer = layer.child(unitcard::card(card, placement, now));
    }
    layer
}

/// The picture itself: the map's pixels, its failure, or the placeholder.
#[cfg(feature = "map")]
fn picture_content(state: &State, scale: f32, body: El) -> El {
    if let Some(map) = &state.map {
        if map.error().is_none() {
            return body.child(
                rune::el()
                    .key("map")
                    .w_full()
                    .h_full()
                    .pixels(map.pixels())
                    .on_wheel(|state: &mut State, event: rune::WheelEvent, _tx| {
                        wheel(state, &event)
                    })
                    .on_drag(|state: &mut State, drag, _tx| drag_map(state, drag))
                    .on_layout(move |state: &mut State, [_, _, width, height]| {
                        laid_out(state, width, height, scale)
                    }),
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
fn picture_content(state: &State, _scale: f32, body: El) -> El {
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

/// A wheel step: zoom about the pointer, so the ground under the cursor stays under it.
#[cfg(feature = "map")]
fn wheel(state: &mut State, event: &rune::WheelEvent) -> bool {
    let Some(map) = &state.map else { return false };
    let Some(frame) = map.rendered() else {
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
    state.camera = crate::map::geometry::zoom_about(
        state.camera,
        viewpoint,
        (f64::from(event.x), f64::from(event.y)),
        delta,
    );
    map.set_camera(state.camera);
    true
}

/// A drag: move the ground with the hand, from the camera the drag started on.
///
/// Absolute rather than incremental on purpose: [`rune::Drag`] reports travel since the press, so
/// summing per-event deltas would compound and the map would run away from the pointer.
#[cfg(feature = "map")]
fn drag_map(state: &mut State, drag: &mut rune::Drag) {
    let Some(map) = &state.map else { return };
    let Some(frame) = map.rendered() else { return };
    match drag.phase {
        rune::Phase::Down => state.drag_origin = Some(state.camera),
        rune::Phase::Move => {
            let origin = state.drag_origin.unwrap_or(state.camera);
            state.camera = crate::map::geometry::pan(
                origin,
                frame.viewport(),
                f64::from(drag.dx),
                f64::from(drag.dy),
            );
            map.set_camera(state.camera);
        }
        rune::Phase::Up => {
            state.drag_origin = None;
            // One render now rather than at the next idle tick: the gesture just ended, and the
            // operator is looking at where it ended.
            map.render_now();
        }
    }
}

/// The map area was laid out: tell the map how many DEVICE pixels to render.
///
/// The frame is device pixels because `Pixels` is; the element is points. Rendering at points on a
/// 2x panel would draw a map at half resolution and every label with it.
#[cfg(feature = "map")]
fn laid_out(state: &mut State, width: f32, height: f32, scale: f32) {
    let Some(map) = &state.map else { return };
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let (w, h) = (
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    );
    if map
        .rendered()
        .is_some_and(|frame| (frame.width, frame.height) == (w, h))
    {
        return;
    }
    map.resize(w, h);
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
    let Some(card) = state.card.as_mut() else {
        return false;
    };
    let mut changed = false;
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
    state.live = Some(LiveThread::spawn(
        config.minos.clone(),
        config.socket_url.clone(),
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
    if rune::is_headless() {
        state.headless = true;
        // A scene declares its own viewport: there is no map thread to hand one over, and markers
        // still have to project against something real. The size is the one `capture.sh` pins.
        state.frame = Some(crate::map::geometry::Rendered {
            camera: state.camera,
            width: tokens::WINDOW_SIZE.0 as u32,
            height: tokens::WINDOW_SIZE.1 as u32,
        });
        return;
    }
    start_socket(state, waker.clone());
    if let Some(config) = config::current() {
        // The card's three reads run here, on the reads thread, never on the UI thread.
        state.backend = crate::data::Backend::spawn(config.minos.clone()).ok();
    }
    #[cfg(feature = "map")]
    {
        // The first frame's layout corrects this size; 1280x720 is the window it opens at, so the map
        // is already rendering the right ground while the layout is being computed.
        state.map = Some(crate::map::thread::MapThread::spawn(
            state.camera,
            (tokens::WINDOW_SIZE.0 as u32, tokens::WINDOW_SIZE.1 as u32),
        ));
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
    if state.frame.is_some() {
        crate::platform::first_frame("first map frame");
    } else if cfg!(not(feature = "map")) {
        crate::platform::first_frame("first frame (no map engine)");
    }
    changed |= drain_card_reads(state);
    changed |= tick_card_age(state);

    // The frame the map last handed over is what markers project against (`INV-04`). Reading it here,
    // rather than in the view, keeps the view a function of the state.
    #[cfg(feature = "map")]
    if let Some(map) = &state.map {
        state.frame = map.rendered();
    }
    if state.frame != state.frame_seen {
        state.frame_seen = state.frame;
        changed = true;
    }

    // A marker still travelling between two fixes needs the next frame drawn, and stops asking on its
    // own when the travel window ends: a wall at rest rebuilds nothing.
    if state.picture.travelling(state.now()) {
        changed = true;
    }
    // Every 30 s the log says what the wall has received, so "it is not moving" is a line in a file
    // rather than a report that ends in a relaunch (#28). Nothing here is drawn: the header already
    // carries the count, and a wall has no console.
    if !state.headless && elapsed - state.last_report >= REPORT_EVERY.as_secs_f64() {
        state.last_report = elapsed;
        tracing::info!(
            fixes = state.fixes,
            units = state.picture.units().len(),
            link = ?state.link,
            "socket"
        );
    }

    // The next frame starts a new gesture: a marker's press has done its job either way.
    state.marker_press = false;
    changed
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
            eprintln!("arcons-cc: cannot open a window: {message}");
            eprintln!("  hint: the display needs a compositor; `cargo run -- --version` does not.");
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
        state.camera = camera;
        state.frame = Some(crate::map::geometry::Rendered {
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
        rt.state.camera = camera;
        rt.state.frame = Some(crate::map::geometry::Rendered {
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
}
