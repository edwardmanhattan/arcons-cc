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

use arcons_cc::{config, platform, selfcheck, ui};

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

/// **`--record`'s tiling half** (#220): one file per case, from the wall's own functions, so a client in
/// another language reads numbers instead of guessing them.
fn record_tiling() -> bool {
    let dir = arcons_cc::ui::tiling::directory();
    match arcons_cc::ui::tiling::record(&dir) {
        Ok(written) => {
            for path in written {
                println!("recorded {}", path.display());
            }
            true
        }
        Err(error) => {
            eprintln!("arcons-cc: {error}");
            false
        }
    }
}

/// **`--record`'s corpus half** (#224): the exchange stream, from the wall's own session loop.
#[cfg(unix)]
fn record_corpus() -> bool {
    let dir = arcons_cc::corpus::directory();
    match arcons_cc::corpus::record(&dir) {
        Ok(path) => {
            println!("recorded {}", path.display());
            true
        }
        Err(error) => {
            eprintln!("arcons-cc: {error}");
            false
        }
    }
}

/// **`--conformance FILE`** (#224): replay a corpus through this wall and print what it answered. The first
/// answer that differs from the file is the failure, with the line and both lines' JSON, because a corpus
/// that has gone stale is a client reading a contract the wall no longer keeps.
#[cfg(unix)]
fn conformance(path: &std::path::Path) -> ExitCode {
    match arcons_cc::corpus::replay(path) {
        Ok(answered) => {
            for (number, (exchange, answer)) in answered.iter().enumerate() {
                println!(
                    "ok    {:>3}  {}\n            {}",
                    number + 1,
                    arcons_cc::corpus::line_of(&exchange.send),
                    answer
                );
            }
            println!(
                "{}: {} exchanges answered as recorded",
                path.display(),
                answered.len()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("arcons-cc: {error}");
            ExitCode::FAILURE
        }
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
///
/// The counting and the sentence are [`arcons_cc::verify::compare`]'s (#276): the desk compares its
/// own goldens with the same function, so a bound that moves moves for both, and the two binaries
/// cannot disagree about what "the same picture" means.
fn compare_captures(args: &config::Args) -> ExitCode {
    let (Some(candidate), Some(golden)) = (args.image.as_deref(), args.compare_with.as_deref())
    else {
        eprintln!("arcons-cc: --compare needs a golden and --image PATH for the candidate");
        return ExitCode::from(64);
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
    match arcons_cc::verify::compare(golden, candidate, bounds) {
        Ok(comparison) => match comparison.verdict() {
            Ok(()) => {
                println!("{}", comparison.pass_line());
                ExitCode::SUCCESS
            }
            Err(reason) => {
                eprintln!("{}", comparison.fail_line(&reason));
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("arcons-cc: {error}");
            ExitCode::FAILURE
        }
    }
}

fn self_check(args: &config::Args) -> ExitCode {
    let mut out = String::new();
    let outcome = selfcheck::run(
        args,
        &config::Env::from_process(),
        &selfcheck::Live::new(),
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
        // **The control plane's contract, recorded** (#220, #224). One file per tiling case and one
        // exchange stream, from the wall's own functions and its own session loop, so a client in another
        // language reads numbers and answers instead of guessing them.
        config::Command::Record => {
            let tiling = record_tiling();
            #[cfg(unix)]
            let corpus = record_corpus();
            #[cfg(not(unix))]
            let corpus = true;
            if tiling && corpus {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        // **A corpus, replayed outside the gate** (#224): the same file the phone's CI reads, answered by
        // this wall through the same session loop the socket serves.
        #[cfg(not(unix))]
        config::Command::Conformance(_) => {
            eprintln!("arcons-cc: the control plane is a unix socket, and this platform has none");
            ExitCode::from(2)
        }
        #[cfg(unix)]
        config::Command::Conformance(path) => conformance(&path),
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
        #[cfg(not(unix))]
        config::Command::Ctl(_) => {
            eprintln!(
                "arcons-cc ctl: the control plane is a unix socket, and this platform has none"
            );
            ExitCode::from(2)
        }
        #[cfg(unix)]
        config::Command::Ctl(words) => match arcons_cc::control::ctl::run(&words) {
            Ok(outcome) => {
                if outcome == arcons_cc::control::ctl::Outcome::Unchanged {
                    println!("unchanged");
                }
                // **A device key is the one thing a `ctl` prints for a script to keep** (#194): the socket is the
                // local credential, so the caller is the operator at the wall. Nothing else goes to stdout.
                if let arcons_cc::control::ctl::Outcome::Paired(key) = &outcome {
                    println!("{key}");
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
