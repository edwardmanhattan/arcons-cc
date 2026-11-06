//! **Budgets are numbers in tests** (`docs/verification.md` rule 5).
//!
//! "Feels smooth" is not a result. This measures the frame CPU of a 200-marker scene while the camera
//! moves — the work the wall does per frame when an operator pans or zooms — and fails with the
//! measured number beside the limit (`INV-12`).
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

#[test]
#[ignore = "budget: scripts/verify.sh runs it by name"]
fn budget_200_markers_pan_zoom_frame_cpu() {
    let camera = Camera::new((-6.108, 106.910), 11.0);
    let mut state = State::new(picture_with(200));
    state.camera = camera;
    state.frame = Some(Rendered {
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
        runtime.state.camera = moved;
        runtime.state.frame = Some(Rendered {
            camera: moved,
            width: 1920,
            height: 1080,
        });
        runtime.frame(1.0 / 60.0);
    }
    let per_frame_ms = started.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES);

    println!(
        "INV-12: {per_frame_ms:.3} ms frame CPU with 200 markers and a panning camera (limit {LIMIT_MS} ms, {}x{})",
        tokens::WINDOW_SIZE.0,
        tokens::WINDOW_SIZE.1
    );
    assert!(
        per_frame_ms < LIMIT_MS,
        "INV-12: {per_frame_ms:.3} ms per frame with 200 markers exceeds the {LIMIT_MS} ms budget"
    );
}
