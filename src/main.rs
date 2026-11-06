//! Arcons Command Center: the live maritime picture, on a wall.
//!
//! v0.1 is a map of every live vessel, moving as fixes arrive, where clicking a unit opens a
//! floating card with what is known about it. The plan lives in the issues — `#1` is the epic —
//! not in this file.
//!
//! The crate is a library (`src/lib.rs`) with this one binary on top of it, so the tests can name
//! what they test. Everything worth reading is documented there.

use std::process::ExitCode;

use std::path::PathBuf;

use arcons_cc::{config, ui};

/// Render one map frame with no window. Needs `--features map`: without the engine there is nothing
/// to render, and saying so is better than writing a black PNG.
fn probe_map(_camera: arcons_cc::map::geometry::Camera, out: PathBuf) -> ExitCode {
    #[cfg(feature = "map")]
    {
        match arcons_cc::map::probe(_camera, (1280, 720), &out) {
            Ok((width, height)) => {
                println!("map probe: {width}x{height} → {}", out.display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("map probe failed: {error}");
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(feature = "map"))]
    {
        let _ = out;
        eprintln!(
            "--probe-map needs the map engine: build with `cargo run --features map -- --probe-map`"
        );
        ExitCode::from(2)
    }
}

fn main() -> ExitCode {
    // Before Rune's threads exist: the Vulkan loader reads this once, process-wide, and on a hybrid
    // laptop MapLibre would otherwise render on the discrete GPU the panel is not wired to.
    #[cfg(feature = "map")]
    arcons_cc::map::pin_gpu_from_env();

    let args = match config::Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("arcons-cc: {message}\n\n{}", config::USAGE);
            return ExitCode::from(64); // EX_USAGE
        }
    };

    match args.command {
        config::Command::Run => ui::run(args.window, args.passthrough),
        config::Command::Version => {
            println!("arcons-cc {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        config::Command::Help => {
            println!("{}", config::USAGE);
            ExitCode::SUCCESS
        }
        config::Command::SelfCheck => config::self_check(),
        config::Command::ProbeMap => probe_map(
            args.probe_camera
                .unwrap_or(arcons_cc::map::geometry::Camera::START),
            args.out,
        ),
    }
}
