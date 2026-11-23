//! **The engine behind the chart** — MapLibre's thread, the offline seed and the RGBA handoff, behind
//! `--features map`.
//!
//! The wall owns the engine ([`arcons_cc::map::thread`]): one thread per map, a `rune::Pixels` handle the
//! UI draws, and a camera the UI sets. The desk does not have a second implementation of any of that, and
//! it must not: two threads opening two Vulkan contexts would fight for the same adapter, and two
//! definitions of "the picture" is the thing ADR-0009 exists to avoid.
//!
//! What the desk adds is **when** to ask: the camera is the UI's (decision 4), so every frame the chart is
//! drawn with the state's camera, and the thread coalesces the requests it does not need.
//!
//! A process-global rather than a field on `State`: a `MapThread` owns a thread and a GL context, and
//! `State` is rebuilt for every scene, test and capture. Spawning one per `State::sample` would leave
//! several Vulkan contexts open after a gate run. One process, one chart, one engine — and one **explicit
//! shutdown**, because a static is never dropped: MapLibre aborts when its context is torn down under a
//! thread that is still rendering (`Failed to set thread-specific storage`, then `SIGABRT`), which is what
//! the wall's join-on-drop exists to prevent and what a `static` silently skips.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use arcons_cc::map::geometry::Camera;
use arcons_cc::map::thread::MapThread;

static ENGINE: Mutex<Option<MapThread>> = Mutex::new(None);

/// What the thread was told last: the centre, the zoom, and the size in device pixels.
type Told = ((f64, f64), f64, (u32, u32));

/// What the thread was last told. **The UI draws sixty frames a second and the camera moves on almost none
/// of them**: sending a camera on every draw keeps the thread rendering instead of letting it finish a
/// frame, and the chart stays empty while it churns. One request per change, not per frame.
static TOLD: Mutex<Option<Told>> = Mutex::new(None);

/// The chart's engine, started on first use, asked for the camera and size of this frame.
///
/// `size` is **device pixels**: what the map must render, not the points the layout uses. It returns a
/// clone of the pixel handle rather than a reference, because the thread lives behind a lock: the UI must
/// not hold that lock while it draws a frame.
pub fn pixels(camera: Camera, size: (u32, u32)) -> Option<rune::Pixels> {
    let mut slot = ENGINE.lock().ok()?;
    let engine = slot.get_or_insert_with(|| MapThread::spawn(camera, size));
    let now = (camera.center, camera.zoom, size);
    let changed = {
        let mut told = TOLD.lock().ok()?;
        let changed = told.as_ref() != Some(&now);
        *told = Some(now);
        changed
    };
    if changed {
        engine.set_camera(camera);
        engine.resize(size.0, size.1);
    }
    if engine.rendered().is_none() {
        // **The first frame is asked for.** The thread renders on its idle tick, and while the window is
        // still opening that tick is not soon enough to be the picture the operator sees.
        engine.render_now();
    }
    Some(engine.pixels().clone())
}

/// Whether a frame has arrived. Without one the chart draws the graticule and no water, because a black
/// rectangle looks like the sea at night rather than like a map that is still opening.
pub fn has_frame() -> bool {
    ENGINE
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().and_then(|engine| engine.rendered()))
        .is_some()
}

/// How many frames the thread has rendered: the desk's probe while the picture is not on screen.
pub fn frames() -> u64 {
    ENGINE
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|engine| engine.frames()))
        .unwrap_or(0)
}

/// The frame number the screen last drew.
///
/// **Rune runs the view once and stops**: with nothing changing, a static screen costs no frames. A
/// map is the one thing on this screen that changes by itself — its thread finishes a frame while
/// nobody is looking — so somebody has to ask for the next pass. This is the number that tells the
/// desk whether a frame arrived that the operator has not seen (the wall's `frame_seen`, per pane).
static SEEN: AtomicU64 = AtomicU64::new(0);

/// Whether a frame has arrived since the last redraw. The desk's `poll` asks once per frame.
pub fn frame_arrived() -> bool {
    let now = frames();
    now != SEEN.swap(now, Ordering::SeqCst)
}

/// Why the engine is not drawing, when it is not. One line, for the chart's own corner.
pub fn error() -> Option<String> {
    ENGINE
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().and_then(|engine| engine.error()))
}

/// Stop the thread and join it. Called from `main` before the process exits, because nothing else will:
/// `MapThread`'s join-on-drop is the wall's protection, and a `static` is never dropped.
pub fn shutdown() {
    if let Ok(mut slot) = ENGINE.lock() {
        slot.take();
    }
}
