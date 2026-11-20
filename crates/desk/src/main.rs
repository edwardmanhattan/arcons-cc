//! `cargo run -p desk` — the desk's command line.
//!
//! The order is the wall's: answer `--version`, `--self-check`, `--new-device-id` and `--scenes`
//! before anything opens a log, a configuration or a window; then hand Rune its own flags.
//!
//! **`--snap` is Rune's flag, not ours.** `--snap out/desk-time.png` is recognised by Rune's
//! tooling arguments, which also carry `--scene`, `--at` and the rest; the desk passes every
//! argument it does not own straight through, the way the wall's `passthrough` does (#9). Parsing
//! `--snap` here would be a second renderer's flag for a renderer we do not have.

use std::process::ExitCode;

use desk::identity::{self, DeviceId};

const USAGE: &str = "\
  --self-check      what is configured, and this installation's device id
  --new-device-id   mint a new id (a cloned disk image carries the old one)
  --version         the version, no window
  --scenes          the scene names this binary can render, one per line
  --scene NAME      render a named scene (Rune's flag; `time-field` is ours)
  --snap PATH       render a scene to a PNG with no window (Rune's flag)";

fn main() -> ExitCode {
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
        // Rune's own flags, and anything else, go through untouched: `--snap` and `--scene` are
        // how a capture happens, and swallowing them would make `--snap` succeed at producing
        // nothing. With no arguments the desk opens its window, like the wall's `cargo run`.
        Some(flag) if flag.starts_with('-') => desk::ui::run(args),
        Some(flag) => {
            eprintln!("desk: unknown argument {flag}\n\n{USAGE}");
            ExitCode::from(64)
        }
        None => desk::ui::run(args),
    }
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
