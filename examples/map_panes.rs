//! #64's two spikes: what N map panes cost, and whether one renderer can serve them.
//!
//! Not in the gate — it needs the `map` feature and a GPU, and the gate promises neither. Run it by
//! hand, one `N` per process (a crash must not take the smaller `N`'s numbers with it):
//!
//! ```sh
//! # Spike A: N `LiveMap`s in one process. `shared` = one SQLite cache file, `separate` = one each.
//! cargo run --features map --example map_panes -- a 1 1920x1080 shared
//! cargo run --features map --example map_panes -- a 4 960x540 shared
//! # Spike B: one `LiveMap`, N cameras, one viewport size at a time.
//! cargo run --features map --example map_panes -- b 4 960x540
//! ```
//!
//! **Debug build on purpose**, like `map_frame_cost`: the numbers are not the wall's release numbers
//! (`#26` measured 42.5 ms per 1080p frame in release), they are *ratios* — what a second pane adds
//! over the first, and what a shared cache costs in memory. `docs/research/multi-view.md` says so in
//! every table it prints from here.
//!
//! Both `main`s below: without the feature the example builds and says so rather than failing
//! `cargo clippy --all-targets`, which the gate runs without `map`.

#[cfg(not(feature = "map"))]
fn main() {
    eprintln!("map_panes: build with `--features map` to measure the map engine");
    std::process::exit(2);
}

#[cfg(feature = "map")]
fn main() {
    use arcons_cc::map::geometry::Camera;
    use arcons_cc::map::render::prepare_cache;
    use std::path::PathBuf;

    // The same adapter choice the app makes (#16): MapLibre otherwise takes the discrete GPU while the
    // panel is on the integrated one, and on this hybrid laptop that path aborts.
    arcons_cc::map::pin_gpu_from_env();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("a");
    let panes: usize = args
        .get(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
        .clamp(1, 6);
    let (width, height) = match args.get(2).map(String::as_str) {
        Some("1920x1080") => (1920u32, 1080u32),
        Some("640x360") => (640u32, 360u32),
        _ => (960u32, 540u32),
    };
    let cache_mode = args.get(3).map(String::as_str).unwrap_or("shared");

    // Three cameras over the same archipelago, so every pane is drawing real tiles rather than ocean.
    let cameras: Vec<Camera> = (0..panes)
        .map(|index| Camera {
            center: (
                -7.88 + f64::from(index as u32) * 0.35,
                112.53 + f64::from(index as u32) * 0.4,
            ),
            zoom: 9.0,
        })
        .collect();

    let cache_dir = std::env::temp_dir().join(format!("arcons-cc-spike-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache_dir);
    std::fs::create_dir_all(&cache_dir).expect("a writable cache dir");
    let cache_for = |index: usize| -> PathBuf {
        let path = match cache_mode {
            "separate" => cache_dir.join(format!("map-cache-{index}.sqlite")),
            _ => cache_dir.join("map-cache.sqlite"),
        };
        prepare_cache(&path).expect("the seed copies to the cache");
        path
    };

    println!(
        "spike {mode}: panes={panes} size={width}x{height} cache={cache_mode} build=debug \
         rss_start={} MiB",
        rss_mib()
    );

    match mode {
        "b" => spike_b(&cameras, width, height, &cache_for(0)),
        _ => spike_a(&cameras, width, height, &cache_for),
    }
}

#[cfg(feature = "map")]
fn spike_a(
    cameras: &[arcons_cc::map::geometry::Camera],
    width: u32,
    height: u32,
    cache_for: &dyn Fn(usize) -> std::path::PathBuf,
) {
    use arcons_cc::map::render::{LiveMap, prepare_cache};
    use std::time::Instant;

    let baseline = rss_mib();
    let mut maps: Vec<Option<LiveMap>> = Vec::new();
    let mut load_ms = Vec::new();
    for (index, camera) in cameras.iter().enumerate() {
        let cache = cache_for(index);
        // `prepare_cache` again: a shared cache is copied once by the first pane, and the later ones
        // must not re-copy over a cache another renderer already has open.
        let _ = prepare_cache(&cache);
        let started = Instant::now();
        match LiveMap::new(*camera, width, height, &cache) {
            Ok(map) => {
                load_ms.push(started.elapsed().as_secs_f64() * 1000.0);
                println!(
                    "  pane {index}: loaded in {:.0} ms (rss {} MiB)",
                    load_ms[index],
                    rss_mib()
                );
                maps.push(Some(map));
            }
            Err(error) => {
                println!("  pane {index}: FAILED to load: {error}");
                maps.push(None);
            }
        }
    }
    let loaded = maps.iter().filter(|map| map.is_some()).count();
    println!(
        "  loaded {loaded}/{}, rss after load {} MiB (+{} MiB, {:.1} MiB/pane)",
        cameras.len(),
        rss_mib(),
        rss_mib().saturating_sub(baseline),
        if loaded == 0 {
            0.0
        } else {
            rss_mib().saturating_sub(baseline) as f64 / loaded as f64
        }
    );

    // A frame each, so shaders and tiles are warm before anything is timed.
    for map in maps.iter_mut().flatten() {
        let started = Instant::now();
        let rgba = map.frame_rgba();
        println!(
            "  pane warm frame {:.0} ms ({} bytes)",
            started.elapsed().as_secs_f64() * 1000.0,
            rgba.len()
        );
    }

    // One frame per pane per round, sequentially, with the gap the single-pane harness needs: two
    // `render_once` calls back to back race MapLibre's render thread (`map_frame_cost`, #26).
    let mut per_pane: Vec<Vec<f64>> = vec![Vec::new(); maps.len()];
    let mut round_totals = Vec::new();
    for _ in 0..6 {
        let round = Instant::now();
        for (index, map) in maps.iter_mut().enumerate() {
            let Some(map) = map else { continue };
            let started = Instant::now();
            let _ = map.frame_rgba();
            per_pane[index].push(started.elapsed().as_secs_f64() * 1000.0);
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
        round_totals.push(round.elapsed().as_secs_f64() * 1000.0);
    }
    for (index, times) in per_pane.iter_mut().enumerate() {
        if times.is_empty() {
            continue;
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  pane {index}: median {:.2} ms/frame, p95 {:.2} ms (n={})",
            times[times.len() / 2],
            times[times.len() * 95 / 100],
            times.len()
        );
    }
    round_totals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "  round of {loaded} panes: median {:.0} ms including the 120 ms gaps (rss_peak {} MiB)",
        round_totals[round_totals.len() / 2],
        rss_mib()
    );
    // Nothing is dropped here on purpose: the RSS line above is "N maps resident", and a
    // process-wide teardown of N Vulkan contexts is exactly the crash this spike is asking about. The
    // engine asserts in `~RunLoop` when a process with a live map exits (`Failed to close loop`), so
    // the harness leaves the way a measurement should: through `exit`, after the numbers are printed.
    std::mem::forget(maps);
    std::process::exit(0);
}

#[cfg(feature = "map")]
fn spike_b(
    cameras: &[arcons_cc::map::geometry::Camera],
    width: u32,
    height: u32,
    cache: &std::path::Path,
) {
    use std::time::Instant;

    let baseline = rss_mib();
    let started = Instant::now();
    let mut map = raw_map(cameras[0], width, height, cache);
    println!(
        "  one renderer: loaded in {:.0} ms, rss {} MiB (+{} MiB)",
        started.elapsed().as_secs_f64() * 1000.0,
        rss_mib(),
        rss_mib().saturating_sub(baseline)
    );

    // Pass 1 — one renderer, N cameras, **one viewport size**: what the API can do today. There is no
    // viewport rectangle anywhere in `ImageRenderer` (`set_map_size`, one camera, one
    // `read_still_image`), so "render N panes in one pass" is not a call that exists.
    let mut same = vec![Vec::new(); cameras.len()];
    for _ in 0..6 {
        for (index, camera) in cameras.iter().enumerate() {
            map.update_camera(&camera_update(*camera));
            let started = Instant::now();
            map.render_once();
            let frame = map.read_still_image();
            same[index].push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(
                frame.buffer().len(),
                (width as usize) * (height as usize) * 4,
                "a frame is RGBA at the renderer's size"
            );
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
    }
    for (index, times) in same.iter_mut().enumerate() {
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  same size {width}x{height}, camera {index}: median {:.2} ms/frame (n={})",
            times[times.len() / 2],
            times.len()
        );
    }

    // Pass 2 — the tiled case: panes of **different sizes**, so a single renderer must resize between
    // them. This is what a tiling layout actually asks for, and it is the pass that decides whether one
    // renderer is even usable for panes of unequal size.
    let sizes: Vec<(u32, u32)> = (0..cameras.len())
        .map(|index| {
            let half = index % 2 == 0;
            if half {
                (width, height)
            } else {
                (width / 2, height / 2)
            }
        })
        .collect();
    let mut resized = vec![Vec::new(); cameras.len()];
    for _ in 0..6 {
        for (index, camera) in cameras.iter().enumerate() {
            let (w, h) = sizes[index];
            let started = Instant::now();
            map.set_map_size(maplibre_native::Size {
                width: w.max(1),
                height: h.max(1),
            });
            map.update_camera(&camera_update(*camera));
            map.render_once();
            let frame = map.read_still_image();
            resized[index].push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(
                frame.buffer().len(),
                (w as usize) * (h as usize) * 4,
                "a resized frame is RGBA at the new size"
            );
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
    }
    for (index, times) in resized.iter_mut().enumerate() {
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let (w, h) = sizes[index];
        println!(
            "  resized to {w}x{h}, camera {index}: median {:.2} ms/frame (n={}) — includes set_map_size",
            times[times.len() / 2],
            times.len()
        );
    }

    let cycle: f64 = resized.iter().map(|times| times[times.len() / 2]).sum();
    println!(
        "  cycle of {} panes of unequal size on one renderer: {cycle:.1} ms, rss {} MiB",
        cameras.len(),
        rss_mib()
    );
    std::mem::forget(map);
    std::process::exit(0);
}

/// A bare renderer built the way `LiveMap` builds one, because spike B's question is about the API
/// underneath it: `set_map_size`, one camera, one readback. A research harness may duplicate thirty
/// lines; a `LiveMap` accessor added only for a spike would be a production seam with a spike for a
/// test.
#[cfg(feature = "map")]
fn raw_map(
    camera: arcons_cc::map::geometry::Camera,
    width: u32,
    height: u32,
    cache: &std::path::Path,
) -> maplibre_native::ImageRenderer<maplibre_native::Continuous> {
    use maplibre_native::{ImageRendererBuilder, ResourceOptions};
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    const STYLE_URL: &str = "https://tiles.openfreemap.org/styles/liberty";

    let nonzero = |value: u32| NonZeroU32::new(value.max(1)).expect("1 is not zero");
    let mut renderer = ImageRendererBuilder::new()
        .with_size(nonzero(width), nonzero(height))
        .with_resource_options(
            ResourceOptions::default()
                .with_cache_path(cache.to_path_buf())
                .with_maximum_cache_size(256 * 1024 * 1024),
        )
        .build_continuous_renderer();

    let loaded = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let observer = renderer.map_observer();
    observer.set_did_finish_loading_style_callback({
        let loaded = Arc::clone(&loaded);
        move || loaded.store(true, Ordering::SeqCst)
    });
    observer.set_did_fail_loading_map_callback({
        let failed = Arc::clone(&failed);
        move |_| failed.store(true, Ordering::SeqCst)
    });
    renderer.update_camera(&camera_update(camera));
    renderer.load_style_from_url(&STYLE_URL.parse().expect("a style url"));

    let deadline = Instant::now() + Duration::from_secs(30);
    while !loaded.load(Ordering::SeqCst) {
        if failed.load(Ordering::SeqCst) || Instant::now() > deadline {
            panic!("the style did not load");
        }
        renderer.render_once();
        std::thread::sleep(Duration::from_millis(20));
    }
    for _ in 0..24 {
        renderer.render_once();
        std::thread::sleep(Duration::from_millis(20));
    }
    renderer
}

/// Resident set size in MiB, from `/proc`. Linux-only, and this harness is a laptop harness.
#[cfg(feature = "map")]
fn rss_mib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.split_whitespace().next()?.parse::<u64>().ok())
        .map(|kib| kib / 1024)
        .unwrap_or(0)
}

/// The camera a `CameraUpdate` says, exactly as `map::render` builds one.
#[cfg(feature = "map")]
fn camera_update(camera: arcons_cc::map::geometry::Camera) -> maplibre_native::CameraUpdate {
    maplibre_native::CameraUpdate::new()
        .center(maplibre_native::LatLng {
            lat: camera.center.0,
            lng: camera.center.1,
        })
        .zoom(camera.zoom)
}
