//! The engine, when it is built: does MapLibre's thread actually hand the chart a frame — and does the
//! desk hear about it?
//!
//! Behind `--features map`, so the gate (which has no C++ toolchain) never builds it and CI's map jobs,
//! plus a laptop with the prebuilt core, do. These are the claims a compile cannot make.

#![cfg(feature = "map")]

use std::time::{Duration, Instant};

use desk::chart::Chart;
use desk::chart::engine;

/// Wait for the next frame the engine draws, up to `limit`.
fn wait_for_frames(at_least: u64, limit: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if engine::frames() >= at_least {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn the_engine_hands_the_chart_a_frame_and_the_desk_hears_about_it() {
    let chart = Chart::new((-6.10, 106.88), 8.5, (400.0, 300.0));
    let _ = engine::pixels(chart.camera(), (400, 300));

    assert!(
        wait_for_frames(1, Duration::from_secs(30)),
        "no frame after 30 s, and the engine says {:?}",
        engine::error()
    );

    // **The frame the screen has not drawn is a change** — #277's bug. Rune runs the view once and then
    // leaves a static screen alone, so a map drawn asynchronously is never asked for: the chart said
    // `the chart is opening` with a finished picture in the handle it was not drawing. This is the
    // question the desk's `poll` asks, and it must say yes exactly once per frame.
    assert!(
        engine::frame_arrived(),
        "the first frame arrived and nobody has drawn it: the desk would never redraw"
    );
    assert!(
        !engine::frame_arrived(),
        "asking twice about the same frame is not a change, or the screen would redraw forever"
    );

    // And a frame drawn later is a change again, not a one-shot latch.
    let drawn = engine::frames();
    assert!(
        wait_for_frames(drawn + 1, Duration::from_secs(10)) || engine::error().is_some(),
        "the thread stopped after {} frames without an error",
        engine::frames()
    );
    if engine::frames() > drawn {
        assert!(
            engine::frame_arrived(),
            "a second frame arrived and the desk called it old, so the chart would freeze on the first"
        );
    }

    engine::shutdown();
}
