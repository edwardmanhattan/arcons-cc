//! `cargo run -p desk` — the desk's command line.
//!
//! The order is the wall's: answer `--version`, `--self-check`, `--new-device-id`, `--scenes` and
//! `--compare` before anything opens a log, a configuration or a window; then hand Rune its own flags.
//!
//! **`--snap` is Rune's flag, not ours.** `--snap out/desk-time.png` is recognised by Rune's
//! tooling arguments, which also carry `--scene`, `--at` and the rest; the desk passes every
//! argument it does not own straight through, the way the wall's `passthrough` does (#9). Parsing
//! `--snap` here would be a second renderer's flag for a renderer we do not have.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use desk::identity::{self, DeviceId};
use desk::scene;

const USAGE: &str = "\
  --self-check      what is configured, and this installation's device id
  --new-device-id   mint a new id (a cloned disk image carries the old one)
  --version         the version, no window
  --scenes          the scene names this binary can render, one per line
  --scene NAME      render a named scene (Rune's flag; `time-field` is ours)
  --snap PATH       render a scene to a PNG with no window (Rune's flag)
  --compare PATH    compare --image against this golden, within the scene's golden bounds, and
                    exit 1 if the difference is a change rather than a rounding difference
  --image PATH      the capture --compare compares
  --tolerance N     how far one channel may differ, in 255ths (default: the scene's bound, 2)
  --budget N        how many pixels may change by more than one step (default: the scene's, 200)
  --rounding N      how many pixels may differ by exactly one step (default: the scene's, 600)";

fn main() -> ExitCode {
    let code = run();
    // **The engine is stopped before the process is.** A static is never dropped, so MapThread's
    // join-on-drop would be skipped and MapLibre would abort tearing down a context under a thread that is
    // still rendering (`--features map` only; a no-op otherwise).
    #[cfg(feature = "map")]
    desk::chart::engine::shutdown();
    code
}

fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("arcons desk {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--self-check") => self_check(),
        Some("--new-device-id") => new_device_id(),
        Some("--scenes") => {
            for scene in desk::scene::names() {
                println!("{scene}");
            }
            ExitCode::SUCCESS
        }
        Some("--compare") => compare(&args[1..]),
        Some("--image") => {
            eprintln!(
                "desk: --image goes with --compare GOLDEN, e.g. \
                 --compare fixtures/golden/desk/time-field.png --image out/desk-time-field.png"
            );
            ExitCode::from(64)
        }
        // Rune's own flags, and anything else, go through untouched: `--snap` and `--scene` are
        // how a capture happens, and swallowing them would make `--snap` succeed at producing
        // nothing. With no arguments the desk opens its window, like the wall's `cargo run`.
        Some(flag) if flag.starts_with('-') => open_desk(args),
        Some(flag) => {
            eprintln!("desk: unknown argument {flag}\n\n{USAGE}");
            ExitCode::from(64)
        }
        None => open_desk(args),
    }
}

/// Open the desk's window. The capture state and the scene catalog are the desk's, not the screen's.
fn open_desk(args: Vec<String>) -> ExitCode {
    desk::ui::run(args, desk::scene::sample, desk::scene::SCENES)
}

fn self_check() -> ExitCode {
    let Some(dir) = identity::config_dir() else {
        eprintln!("desk: no config directory (set XDG_CONFIG_HOME or HOME)");
        return ExitCode::from(2);
    };
    println!("client       arcons desk {}", env!("CARGO_PKG_VERSION"));
    println!("config       {}", dir.display());
    match DeviceId::load_or_mint(&dir) {
        Ok(id) => {
            println!("device id    {id}");
            println!("device file  {}", DeviceId::path(&dir).display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("device id    {error}");
            ExitCode::from(2)
        }
    }
}

fn new_device_id() -> ExitCode {
    let Some(dir) = identity::config_dir() else {
        eprintln!("desk: no config directory (set XDG_CONFIG_HOME or HOME)");
        return ExitCode::from(2);
    };
    match DeviceId::rotate(&dir) {
        Ok(id) => {
            println!("device id    {id}");
            println!("device file  {}", DeviceId::path(&dir).display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("desk: {error}");
            ExitCode::from(2)
        }
    }
}

/// `--compare GOLDEN --image PATH`: one desk capture against one desk golden.
///
/// **The comparison is the wall's** — [`arcons_cc::verify::compare`] — and not a second one: two
/// implementations would drift, and a golden judged by a different function is two goldens. The bounds
/// come from [`desk::scene::bounds`]; today that is `DEFAULT_BOUNDS` for every desk scene, because the
/// wall's measured table does not know our scenes and must not learn their names (#276).
///
/// The exit codes are the wall's `--compare`'s: 0 inside the bounds, 1 when the difference is a change
/// or a file cannot be read, 64 when the flags are wrong. A missing golden is not a comparison at all:
/// it names the command that writes one, because a gate that fails with "cannot open" sends its reader
/// looking in the wrong place.
fn compare(args: &[String]) -> ExitCode {
    let Some(golden) = args.first() else {
        return usage_error("--compare needs the golden's path");
    };
    if golden.starts_with('-') {
        return usage_error("--compare needs the golden's path first");
    }
    let golden = Path::new(golden);

    let mut image: Option<PathBuf> = None;
    let mut tolerance: Option<u8> = None;
    let mut budget: Option<usize> = None;
    let mut rounding: Option<usize> = None;
    let mut argv = args[1..].iter();
    while let Some(flag) = argv.next() {
        match flag.as_str() {
            "--image" => {
                let Some(value) = argv.next() else {
                    return usage_error("--image needs a path");
                };
                image = Some(PathBuf::from(value));
            }
            "--tolerance" => {
                let Some(value) = argv.next() else {
                    return usage_error("--tolerance needs a number");
                };
                match value.parse::<u8>() {
                    Ok(number) => tolerance = Some(number),
                    Err(_) => return usage_error("--tolerance is a number of 255ths, 0 to 255"),
                }
            }
            "--budget" => {
                let Some(value) = argv.next() else {
                    return usage_error("--budget needs a number");
                };
                match value.parse::<usize>() {
                    Ok(number) => budget = Some(number),
                    Err(_) => return usage_error("--budget is a number of pixels"),
                }
            }
            "--rounding" => {
                let Some(value) = argv.next() else {
                    return usage_error("--rounding needs a number");
                };
                match value.parse::<usize>() {
                    Ok(number) => rounding = Some(number),
                    Err(_) => return usage_error("--rounding is a number of pixels"),
                }
            }
            other => return usage_error(&format!("unknown flag {other} after --compare")),
        }
    }
    let Some(image) = image else {
        return usage_error("--compare needs --image PATH for the capture");
    };

    // The golden's file stem is the scene's name, exactly as on the wall: `…/desk/time-field.png` is
    // the scene `time-field`.
    let scene = golden
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let mut bounds = scene::bounds(scene);
    if let Some(number) = tolerance {
        bounds.tolerance = number;
    }
    if let Some(number) = budget {
        bounds.changed = number;
    }
    if let Some(number) = rounding {
        bounds.rounding = number;
    }

    if !golden.is_file() {
        eprintln!(
            "desk: no golden at {}: run scripts/capture-desk.sh --update-goldens {scene}",
            golden.display()
        );
        return ExitCode::FAILURE;
    }
    match arcons_cc::verify::compare(golden, &image, bounds) {
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
            eprintln!("desk: {error}");
            ExitCode::FAILURE
        }
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("desk: {message}\n\n{USAGE}");
    ExitCode::from(64)
}
