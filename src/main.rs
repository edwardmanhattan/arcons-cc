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
/// `--assert`: pixels with numbers and a non-zero exit. `scripts/assert.sh` is a wrapper for it.
fn assert_capture(args: &config::Args) -> ExitCode {
    let (Some(scene), Some(image)) = (args.assert_scene.as_deref(), args.image.as_deref()) else {
        eprintln!("arcons-cc: --assert needs a scene name and --image PATH");
        return ExitCode::from(64);
    };
    match arcons_cc::verify::check(scene, image) {
        Ok(lines) => {
            for line in lines {
                println!("  ok    {line}");
            }
            println!("{scene}: assertions passed");
            ExitCode::SUCCESS
        }
        Err(failures) => {
            for failure in failures {
                eprintln!("  FAIL  {failure}");
            }
            eprintln!("{scene}: {} assertion(s) failed", 1);
            ExitCode::FAILURE
        }
    }
}

/// `--compare`: two captures, one difference, and whether it is a change or a rounding step.
///
/// `scripts/capture.sh` runs this against a golden on **every** machine, which is the point: a byte
/// compare can only be the reference machine's, so CI used to skip goldens altogether. The numbers are
/// printed on success too — a tolerance nobody can see the margin of is a tolerance that silently
/// widens.
fn compare_captures(args: &config::Args) -> ExitCode {
    let (Some(candidate), Some(golden)) = (args.image.as_deref(), args.compare_with.as_deref())
    else {
        eprintln!("arcons-cc: --compare needs a golden and --image PATH for the candidate");
        return ExitCode::from(64);
    };
    let read = |path: &std::path::Path| arcons_cc::verify::Image::read(path);
    let (candidate_image, golden_image) = match (read(candidate), read(golden)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("arcons-cc: {error}");
            return ExitCode::FAILURE;
        }
    };
    let difference = match arcons_cc::verify::difference(&candidate_image, &golden_image) {
        Ok(difference) => difference,
        Err(error) => {
            eprintln!("arcons-cc: {error}");
            return ExitCode::FAILURE;
        }
    };
    // **The scene's bounds** (#130): derived from the golden's name, which is the scene's identity by
    // construction (`fixtures/golden/<scene>.png`). A per-scene measurement lives in one place,
    // `verify::golden_bounds`; the flags are the human's override, and `ARCONS_GOLDENS=exact` passes
    // zeroes.
    let scene = golden
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let mut bounds = arcons_cc::verify::golden_bounds(scene);
    if let Some(tolerance) = args.tolerance {
        bounds.tolerance = tolerance;
    }
    if let Some(changed) = args.budget {
        bounds.changed = changed;
    }
    if let Some(rounding) = args.rounding {
        bounds.rounding = rounding;
    }
    let verdict = arcons_cc::verify::within(difference, bounds);
    match verdict {
        Ok(()) if difference.is_identical() => {
            println!("identical to {} — byte for byte", golden.display());
            ExitCode::SUCCESS
        }
        Ok(()) => {
            println!(
                "matches {} within bounds: {} px differ ({} changed, {} rounding), max Δ{}/255 \
                 (tolerance {}, change {}, rounding {})",
                golden.display(),
                difference.differing,
                difference.changed,
                difference.rounding,
                difference.max_delta,
                bounds.tolerance,
                bounds.changed,
                bounds.rounding
            );
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!(
                "DIFFERS from {} — {reason} (tolerance {}, change {}, rounding {})",
                golden.display(),
                bounds.tolerance,
                bounds.changed,
                bounds.rounding
            );
            ExitCode::FAILURE
        }
    }
}

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

/// Whether Rune has decided this process is a headless render (`--scene`, `--snap`, `--tree`,
/// `--script`, …). A capture runs on a fixture and opens no window, so it neither needs nor should
/// have a configuration.
fn arcons_cc_headless() -> bool {
    rune::is_headless()
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
        config::Command::Scenes => {
            for scene in arcons_cc::scene::names() {
                println!("{scene}");
            }
            ExitCode::SUCCESS
        }
        config::Command::Assert => assert_capture(&args),
        config::Command::Compare => compare_captures(&args),
        config::Command::ProbeMap => probe_map(
            args.probe_camera
                .unwrap_or(arcons_cc::map::geometry::Camera::START),
            args.out.clone(),
        ),
        // A headless scene renders a fixture: it has no server to talk to and must not need one.
        // `--scene`/`--snap`/`--tree` are Rune's own flags, so this asks Rune rather than guessing.
        config::Command::Run if arcons_cc_headless() => {
            ui::run(args.window, args.passthrough.clone())
        }
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
        // **The second client** (#165): one intent over the control socket, and an exit code a script can
        // branch on. It never opens a window and never reads the configuration — a client should not need the
        // wall's credentials to press a button on the wall.
        config::Command::Ctl(words) => match arcons_cc::control::ctl::run(&words) {
            Ok(outcome) => {
                if outcome == arcons_cc::control::ctl::Outcome::Unchanged {
                    println!("unchanged");
                }
                ExitCode::from(outcome.code())
            }
            // A refused intent and a wall that is not there are both failures a caller acts on; the reason
            // goes to stderr so `ctl … | grep` stays useful.
            Err(failure) => {
                eprintln!("arcons-cc ctl: {}", failure.reason());
                ExitCode::from(failure.code())
            }
        },
        config::Command::Version | config::Command::Help => ExitCode::SUCCESS,
    }
}
