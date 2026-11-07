//! Arcons Command Center: the live maritime picture, on a wall.
//!
//! v0.1 is a map of every live vessel, moving as fixes arrive, where clicking a unit opens a
//! floating card with what is known about it. The plan lives in the issues — `#1` is the epic —
//! not in this file.
//!
//! The crate is a library (`src/lib.rs`) with this one binary on top of it, so the tests can name
//! what they test. Everything worth reading is documented there.
//!
//! The order here is the order a wall needs:
//!
//! 1. parse the command line (so `--help` never touches the network or the log),
//! 2. start the log (so every line after this one is in the file too),
//! 3. pin the GPU (before Rune's threads exist — the Vulkan loader reads the environment once),
//! 4. read the configuration (`--self-check` prints exactly this and stops),
//! 5. watch for `SIGTERM`, and open the window.

use std::path::PathBuf;
use std::process::ExitCode;

use arcons_cc::{config, platform, ui};

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

/// `--self-check`: run every check, print one line each, and exit with the first failure's code.
fn self_check(args: &config::Args) -> ExitCode {
    let mut out = String::new();
    let outcome = config::check::run(
        args,
        &config::Env::from_process(),
        &config::check::Live::new(),
        &mut out,
    );
    print!("{out}");
    ExitCode::from(outcome.code())
}

fn main() -> ExitCode {
    platform::started();

    let args = match config::Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("arcons-cc: {message}\n\n{}", config::USAGE);
            return ExitCode::from(64); // EX_USAGE
        }
    };

    // `--version` and `--help` answer without opening a log file, a config, or a socket.
    if matches!(args.command, config::Command::Version) {
        println!("arcons-cc {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if matches!(args.command, config::Command::Help) {
        println!("{}", config::USAGE);
        return ExitCode::SUCCESS;
    }

    // Before Rune's threads exist: the Vulkan loader reads this once, process-wide, and on a hybrid
    // laptop MapLibre would otherwise render on the discrete GPU the panel is not wired to.
    platform::init_logging(args.log_file.as_deref());
    #[cfg(feature = "map")]
    arcons_cc::map::pin_gpu_from_env();

    match args.command {
        config::Command::SelfCheck => self_check(&args),
        config::Command::ProbeMap => probe_map(
            args.probe_camera
                .unwrap_or(arcons_cc::map::geometry::Camera::START),
            args.out.clone(),
        ),
        config::Command::Run => match config::Config::load(&args, &config::Env::from_process()) {
            Ok(config) => {
                config::set_current(&config);
                tracing::info!(
                    "minos {} ({} from {}) · socket {} · stale after {} s",
                    config.minos.base_url(),
                    config.minos.kind(),
                    config.credential_source.label(),
                    config.socket_url,
                    config.stale_after.as_secs()
                );
                // The flag is read by the view loop's poll hook; installing it before the window
                // opens means a `SIGTERM` during startup is not lost.
                platform::watch_for_termination();
                ui::run(args.window, args.passthrough.clone())
            }
            Err(problem) => {
                // The same codes `--self-check` uses, so a unit file that reports `status=2` says
                // "the credential is refused" without anyone having to read a log line.
                eprintln!("arcons-cc: {problem}");
                ExitCode::from(problem.code())
            }
        },
        config::Command::Version | config::Command::Help => ExitCode::SUCCESS,
    }
}
