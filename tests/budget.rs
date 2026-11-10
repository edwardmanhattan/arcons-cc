//! **Budgets are numbers in tests** (`docs/verification.md` rule 5).
//!
//! "Feels smooth" is not a result. This measures the frame CPU of a 200-marker scene while the camera
//! moves — the work the wall does per frame when an operator pans or zooms — and fails with the
//! measured number beside the limit (`INV-12`).
//!
//! **The number is measured on the reference machine, whatever machine runs the test** (`#48`). Wall-clock
//! milliseconds do not travel: the same commit measured 4.1 ms on the laptop the budget was set on and
//! 9.6 ms on a shared CI runner, so the gate passed or failed on the runner's mood. Every run therefore
//! times a fixed reference workload too (`boxes_view`) and divides by how much slower this machine is
//! than the reference — so a slower *runner* is allowed to be slower, and a slower *wall* is not.
//!
//! It is `#[ignore]` so the inner loop stays fast, and `scripts/verify.sh` runs it by name:
//! `cargo test --all-targets -- --include-ignored budget`. It measures **CPU**, not GPU: layout,
//! projection and element building. The map's own frame cost is MapLibre's, and the probe in #5 is
//! where that number lives.

use std::time::Instant;

use arcons_cc::map::geometry::{Camera, Rendered};
use arcons_cc::ui::{State, tokens};
use arcons_cc::units::Picture;
use chrono::{TimeZone, Utc};

/// The wall's limit for camera pan/zoom at 1080p with 200 units, in milliseconds of frame CPU.
const LIMIT_MS: f64 = 8.0;

/// The Multi View limit (#71): four panes at 960×540, 200 markers each, while ONE pane is panned.
///
/// **Measured, then set**: 10.5 ms on the reference machine for this workload, against 3.8 ms for one
/// pane — a tiled wall rebuilds every pane's marker layer when any pane's frame changes, so it is ~2.7×
/// one pane and not 4×. The limit is 14 ms: a third above the measurement, and **under the 16.6 ms frame
/// clock**, because a budget that allows a missed frame is not one. `docs/research/multi-view.md` has the
/// map thread's half of the arithmetic; this is the markers' half.
const PANES_LIMIT_MS: f64 = 14.0;
/// The pane count that limit is for, named so the failure says which layout it measured.
const PANES: usize = 4;
/// The size of one pane in that layout: four of them tile a 1080p wall.
const PANE_SIZE: (u32, u32) = (960, 540);

/// What that same machine measures for a fixed reference workload, in milliseconds per frame.
///
/// This is the measurement that makes the budget a budget instead of a lottery (`#48`). A shared CI
/// runner is **2.3× slower** than the laptop the 8 ms was set on: the same commit measured 4.1 ms here and
/// 8.7 and 9.6 ms there, so the gate failed and passed on the machine's mood. Wall-clock milliseconds do
/// not travel between machines; a *ratio* to a workload that machine also runs does.
const CALIBRATION_MS: f64 = 0.585;

fn picture_with(units: usize) -> Picture {
    let entries: Vec<_> = (0..units)
        .map(|index| {
            // A grid across the camera's ground, so every marker is on screen and none is trivially
            // off-viewport (which would make the measurement a measurement of nothing).
            let lat = -6.30 + (index % 20) as f64 * 0.02;
            let lon = 106.70 + (index / 20) as f64 * 0.02;
            arcons_cc::data::types::FeedPosition {
                id_unit: index as i64 + 1,
                name: format!("KRI {index}"),
                hull_number: None,
                position: Some(arcons_cc::data::types::FeedFix {
                    latitude: lat,
                    longitude: lon,
                    speed_kn: Some(12.0),
                    speed_ms: None,
                    course_deg: Some((index % 360) as f64),
                    accuracy_m: Some(80.0),
                    recorded_at: Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap(),
                    received_at: Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap(),
                    age_seconds: 5.0,
                    backfilled: false,
                }),
            }
        })
        .collect();
    let mut picture = Picture::default();
    picture.replace_from_feed(&entries);
    picture
}

/// A fixed reference frame: 200 plain boxes, no text, no markers, no shapes. It measures what this
/// machine charges for a frame of layout and scene, which is the part of the budget that is not ours.
fn boxes_view(_state: &State, _cx: &mut rune::Cx<State>) -> rune::El {
    use rune::el;
    let mut root = el().size(1920.0, 1080.0);
    for index in 0..200 {
        root = root.child(
            el().key(format!("box-{index}"))
                .absolute((index % 20) as f32 * 90.0, (index / 20) as f32 * 100.0)
                .size(24.0, 24.0),
        );
    }
    root
}

/// How much slower this machine is than the one the budget was set on, as a factor ≥ 0.
///
/// Never below 0.25 or above 10: a calibration that ran on a machine so odd it fell outside that range
/// would scale the budget into meaninglessness, and a refused measurement is better than a wrong one.
fn machine_factor() -> f64 {
    const FRAMES: u32 = 60;
    let mut runtime = rune::Runtime::new(
        State::default(),
        boxes_view,
        (1920.0, 1080.0),
        rune::Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    runtime.frame(0.0);
    let started = Instant::now();
    for _ in 0..FRAMES {
        runtime.frame(1.0 / 60.0);
    }
    let per_frame = started.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES);
    (per_frame / CALIBRATION_MS).clamp(0.25, 10.0)
}

/// **`INV-12`, restated for a tiled wall** (#71): four panes, 200 markers each, one of them panning.
///
/// The cadence this measures is the wall's own: the focused pane's camera moves every frame (so every
/// pane's marker layer is rebuilt — Rune rebuilds the tree whole), and the unfocused panes' *map threads*
/// idle at 1 Hz while the focused one keeps its 4 Hz. The marker cost is the one that does not follow the
/// cadence, which is exactly why it needs a number.
#[test]
#[ignore = "budget: scripts/verify.sh runs it by name"]
fn budget_four_panes_pan_frame_cpu() {
    let mut state = State::new(picture_with(200));
    let camera = Camera::new((-6.108, 106.910), 11.0);
    let (w, h) = PANE_SIZE;
    // Four panes, tiled like a wall: split twice, then the right half again.
    let first = state.layout.ids()[0];
    let mut ids = vec![first];
    for index in 1..PANES {
        let from = ids[index - 1];
        let id = state.split_pane(from, camera).expect("a split");
        ids.push(id);
    }
    // Every pane's frame is its own rectangle — the rule the view applies — and every pane is centred
    // on the same water so all 200 hulls are inside every pane.
    state.panel = Some((f64::from(w) as f32 * 2.0, f64::from(h) as f32 * 2.0));
    for (id, rect) in state.pane_rects() {
        let frame = arcons_cc::ui::view::synthetic_frame(camera, rect, 1.0);
        if let Some(pane) = state.pane_mut(id) {
            pane.camera = camera;
            pane.frame = Some(frame);
        }
    }
    state.layout.focus(ids[0]);
    assert_eq!(state.panes.len(), PANES, "four panes");

    let mut runtime = rune::Runtime::new(
        state,
        arcons_cc::ui::view,
        (f64::from(w) as f32 * 2.0, f64::from(h) as f32 * 2.0),
        rune::Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    runtime.frame(0.0);
    assert!(
        runtime.find("marker-1").is_some(),
        "the marker layer did not render: the budget below would measure an empty window"
    );
    assert!(runtime.find("pane-4").is_some(), "four panes in the tree");

    const FRAMES: u32 = 120;
    let started = Instant::now();
    for frame in 0..FRAMES {
        let step = f64::from(frame) * 0.0004;
        let moved = Camera::new((-6.108, 106.910 + step), 11.0);
        // A pan on the FOCUSED pane only: the other three keep their cameras, which is the workload a
        // tiled wall actually has while the operator works one pane.
        let focused = runtime.state.layout.focused();
        if let Some(pane) = runtime.state.pane_mut(focused) {
            pane.camera = moved;
            if let Some(frame) = pane.frame {
                pane.frame = Some(arcons_cc::map::geometry::Rendered {
                    camera: moved,
                    ..frame
                });
            }
        }
        runtime.frame(1.0 / 60.0);
    }
    let per_frame_ms = started.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES);
    let factor = machine_factor();
    let reference_ms = per_frame_ms / factor;
    println!(
        "INV-12: {per_frame_ms:.3} ms frame CPU for {PANES} panes at {w}x{h} with 200 markers each, one \
         panning = {reference_ms:.3} ms on the reference machine (machine ×{factor:.2}; limit \
         {PANES_LIMIT_MS} ms; focused pane at 4 Hz idle, unfocused at 1 Hz)"
    );
    assert!(
        reference_ms < PANES_LIMIT_MS,
        "INV-12: {PANES} panes at {w}x{h} with 200 markers each cost {reference_ms:.3} ms of frame CPU \
         on the reference machine (×{factor:.2}), over the {PANES_LIMIT_MS} ms the tiled wall allows"
    );
}

#[test]
#[ignore = "budget: scripts/verify.sh runs it by name"]
fn budget_200_markers_pan_zoom_frame_cpu() {
    let camera = Camera::new((-6.108, 106.910), 11.0);
    let mut state = State::new(picture_with(200));
    state.set_camera(camera);
    state.set_frame(Rendered {
        camera,
        width: 1920,
        height: 1080,
    });
    assert_eq!(
        state.picture.with_fix(),
        200,
        "the fixture must actually have 200 drawable hulls"
    );

    let mut runtime = rune::Runtime::new(
        state,
        arcons_cc::ui::view,
        (1920.0, 1080.0),
        rune::Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    // One warm-up frame: the first frame builds the text atlas and every track, and counting it would
    // measure startup rather than a steady wall.
    runtime.frame(0.0);
    // If the markers are not in the tree, this measures the empty window and proves nothing.
    assert!(
        runtime.find("marker-1").is_some(),
        "the marker layer did not render: the budget below would measure an empty window"
    );
    assert!(
        runtime.find("marker-200").is_some(),
        "only some markers rendered"
    );

    const FRAMES: u32 = 120;
    let started = Instant::now();
    for frame in 0..FRAMES {
        // A pan: the camera moves every frame, which is the expensive case (every marker reprojects
        // and every element retargets).
        let step = f64::from(frame) * 0.0004;
        let moved = Camera::new((-6.108, 106.910 + step), 11.0);
        // A pan publishes a NEW frame — that is what makes every marker reproject. Changing only the
        // live camera would leave the picture where it was and measure nothing.
        runtime.state.set_camera(moved);
        runtime.state.set_frame(Rendered {
            camera: moved,
            width: 1920,
            height: 1080,
        });
        runtime.frame(1.0 / 60.0);
    }
    let per_frame_ms = started.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES);

    // The budget is the wall's; the machine is whatever is running the test. `machine_factor` is how much
    // slower this one is than the reference, so the number compared against the limit is "milliseconds on
    // the reference machine" — a slower runner is allowed to be slower, a slower *wall* is not (#48).
    let factor = machine_factor();
    let reference_ms = per_frame_ms / factor;
    println!(
        "INV-12: {per_frame_ms:.3} ms frame CPU with 200 markers and a panning camera \
         = {reference_ms:.3} ms on the reference machine (machine ×{factor:.2}; limit {LIMIT_MS} ms, {}x{})",
        tokens::WINDOW_SIZE.0,
        tokens::WINDOW_SIZE.1
    );
    assert!(
        reference_ms < LIMIT_MS,
        "INV-12: {per_frame_ms:.3} ms per frame with 200 markers is {reference_ms:.3} ms on the reference \
         machine (×{factor:.2}), which exceeds the {LIMIT_MS} ms budget"
    );
}
