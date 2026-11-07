//! What one map frame costs: the numbers #10 puts in `docs/research/map-engines.md`.
//!
//! Not in the gate — this needs the `map` feature and a GPU, and the gate promises neither. Run it
//! by hand, on a **debug** build: `--release` currently segfaults inside MapLibre's Vulkan backend
//! (`#26`), and a measurement harness that cannot run is not a measurement.
//!
//! ```sh
//! cargo run --features map --example map_frame_cost            # 1280x720
//! cargo run --features map --example map_frame_cost 1920x1080
//! ```
//!
//! One size per process on purpose: a second `LiveMap` in the same process crashes glslang, which is
//! a fact about the engine rather than something this harness should paper over.
//!
//! Both `main`s below: without the feature the example builds and says so rather than failing
//! `cargo clippy --all-targets`, which the gate runs without `map`.

#[cfg(not(feature = "map"))]
fn main() {
    eprintln!("map_frame_cost: build with `--features map` to measure the map engine");
    std::process::exit(2);
}

#[cfg(feature = "map")]
fn main() {
    use std::time::Instant;

    // The same GPU choice the app makes (`#16`): MapLibre otherwise takes the discrete GPU while the
    // panel is on the integrated one, and on this hybrid laptop that path aborts.
    arcons_cc::map::pin_gpu_from_env();
    let cache = arcons_cc::map::cache_path();
    arcons_cc::map::render::prepare_cache(&cache).expect("seed");

    let (width, height) = match std::env::args().nth(1).as_deref() {
        Some("1920x1080") => (1920u32, 1080u32),
        _ => (1280u32, 720u32),
    };

    let camera = arcons_cc::map::geometry::Camera {
        center: (-7.88, 112.53),
        zoom: 9.0,
    };
    let mut map =
        arcons_cc::map::render::LiveMap::new(camera, width, height, &cache).expect("a map");

    // Warm: style, tiles and the glyph atlas are resident, which is the state a wall lives in. The
    // cold path is `--probe-map`'s and it is seconds, not frames.
    map.pump(24);
    // The style's own shaders compile on the first frames; every wall frame after that is warm.
    map.pump(8);

    let mut times = Vec::new();
    for _ in 0..30 {
        // One frame, then let the tile threads finish. Back-to-back `render_once` calls race
        // MapLibre's own render thread and segfault inside glslang (`RenderLineLayer`).
        let start = Instant::now();
        let rgba = map.frame_rgba();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(
            rgba.len(),
            (width as usize) * (height as usize) * 4,
            "a frame is RGBA at the wall's size"
        );
        std::thread::sleep(std::time::Duration::from_millis(120));
    }

    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let p95 = times[times.len() * 95 / 100];
    println!(
        "{width}x{height}: median {median:.2} ms, p95 {p95:.2} ms, min {:.2} ms (n={}, camera still, render + RGBA readback)",
        times[0],
        times.len()
    );
}
