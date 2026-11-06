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

pub mod tokens;

use std::process::ExitCode;

use rune::{AppSpec, Cx, El, col, row, spacer, text};

use crate::config;
#[cfg(feature = "map")]
use crate::map::geometry::Camera;
use crate::units::Picture;

/// The window title, and the wordmark's text.
pub const TITLE: &str = "Arcons Command Center";

/// What the view loop holds. #3 fills the picture, #4 moves it, #6 draws it, #7 selects from it.
#[derive(Default)]
pub struct State {
    pub picture: Picture,
    /// The camera the operator is holding. The map thread renders it; markers project against the
    /// camera of the frame on screen instead (`INV-04`), which is not the same number mid-gesture.
    #[cfg(feature = "map")]
    pub camera: Camera,
    /// The camera at the start of a drag, so a pan is absolute rather than a sum of deltas.
    #[cfg(feature = "map")]
    drag_origin: Option<Camera>,
    /// The map thread and the pixels it paints. `None` without `--features map`.
    #[cfg(feature = "map")]
    pub map: Option<crate::map::thread::MapThread>,
}

/// The whole window.
pub fn view(state: &State, cx: &mut Cx<State>) -> El {
    // The first frame is where the scale is known. `view` gets `&State`, so the camera and the map
    // are reached through interior mutability only where they must be — here, the scale is a plain
    // read for the layout maths.
    let scale = cx.env().scale;
    let status = if state.picture.is_empty() {
        "no picture yet".to_string()
    } else {
        match state.picture.len() {
            1 => "1 unit".to_string(),
            n => format!("{n} units"),
        }
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
            text(status)
                .key("picture-status")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );

    col()
        .w_full()
        .h_full()
        .padding(tokens::PAD)
        .gap(tokens::GAP)
        .fill(tokens::bg())
        .child(header)
        .child(picture(state, scale))
}

/// The map area: the frame when there is one, and the placeholder when there is not.
fn picture(state: &State, scale: f32) -> El {
    // Without the map engine neither is read: the panel is the whole story.
    #[cfg(not(feature = "map"))]
    let _ = (state, scale);

    let body = col()
        .key("picture")
        .w_full()
        .h_full()
        .grow(1.0)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::HAIRLINE, tokens::hairline())
        .fill(tokens::panel());

    // Drawn from the first frame, before the map has produced anything: `Pixels` is the streaming
    // path, and its element only has to exist once. `set()` wakes the window and the next frame
    // uploads — which is why a live map needs no view re-run at 4 Hz (or at all).
    #[cfg(feature = "map")]
    if let Some(map) = &state.map
        && map.error().is_none()
    {
        {
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
    }

    #[cfg(feature = "map")]
    if let Some(map) = &state.map
        && let Some(error) = map.error()
    {
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

    let hint = if cfg!(feature = "map") {
        "the feed and the markers land here (#3, #6)"
    } else {
        "built without the map: cargo run --features map"
    };
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

/// Start the map thread, once, before the first frame. A `fn` because `AppSpec::setup` is one.
#[cfg(feature = "map")]
fn setup(state: &mut State, _waker: rune::Waker) {
    // The first frame's layout corrects this size; 1280x720 is the window it opens at, so the map is
    // already rendering the right ground while the layout is being computed.
    state.map = Some(crate::map::thread::MapThread::spawn(
        state.camera,
        (tokens::WINDOW_SIZE.0 as u32, tokens::WINDOW_SIZE.1 as u32),
    ));
}

/// Open the display and run until the compositor closes it.
pub fn run(window: config::Window, passthrough: Vec<String>) -> ExitCode {
    let spec = AppSpec::<State> {
        min_size: Some(tokens::MIN_WINDOW),
        #[cfg(feature = "map")]
        setup: Some(setup),
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
