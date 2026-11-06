//! Flags, paths, and the `--self-check` story.
//!
//! #8 owns the credential file, the Minos base URL and the real checks (REST reachable, socket
//! subscribed, map seed present). What lives here today is the part that must not move later:
//! **where** configuration comes from, and the rule that a credential is never printed.
//!
//! Configuration is read from the environment or `$XDG_CONFIG_HOME/arcons-cc/`. Never from the
//! source tree: `INV-08` forbids a token in any log line, and the surest way to leak one is to
//! check it in. `--self-check` therefore reports *presence*, never contents.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::map;
use crate::ui::tokens;

pub const USAGE: &str = "\
arcons-cc — the live maritime picture on a wall

USAGE:
    arcons-cc [OPTIONS]

OPTIONS:
    --window-size WxH   open at this size, in points (default 1280x720)
    --fullscreen        cover the primary output, no frame
    --self-check        report what is configured, and what is missing (exit 2 if anything is)
    --version           print the version
    -h, --help          this text
";

/// What the process was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Open the display.
    Run,
    /// Print the version and exit 0.
    Version,
    /// Print [`USAGE`] and exit 0.
    Help,
    /// Report configuration and exit 0, or exit 2 naming what is missing.
    SelfCheck,
}

/// How the window should open. `--fullscreen` wins over `--window-size`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub fullscreen: bool,
    /// Logical size in points.
    pub size: (f32, f32),
}

impl Default for Window {
    fn default() -> Self {
        Self {
            fullscreen: false,
            size: tokens::WINDOW_SIZE,
        }
    }
}

/// Parsed command line.
#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub command: Command,
    pub window: Window,
    /// Arguments this binary does not own, handed to Rune so its headless tooling still works:
    /// `--snap`, `--scene`, `--tree`, `--script`, `--frames`. #9's captures depend on that.
    pub passthrough: Vec<String>,
}

impl Args {
    /// Parse the process arguments (`argv[0]` already skipped).
    ///
    /// Unknown arguments are passed through rather than refused: Rune owns a long list of them and
    /// this binary must not have to track it.
    pub fn parse(argv: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = Args {
            command: Command::Run,
            window: Window::default(),
            passthrough: Vec::new(),
        };

        let mut argv = argv.into_iter();
        while let Some(arg) = argv.next() {
            match arg.as_str() {
                "--version" | "-V" => args.command = Command::Version,
                "--help" | "-h" => args.command = Command::Help,
                "--self-check" => args.command = Command::SelfCheck,
                "--fullscreen" => args.window.fullscreen = true,
                "--window-size" => {
                    let value = argv
                        .next()
                        .ok_or("--window-size needs WxH, for example 1280x720")?;
                    args.window.size = parse_size(&value)?;
                }
                other => args.passthrough.push(other.to_string()),
            }
        }

        Ok(args)
    }
}

/// `WxH`, both above zero, in points.
pub fn parse_size(value: &str) -> Result<(f32, f32), String> {
    let error = || format!("--window-size is WxH, both above 0 (got {value:?})");
    let (w, h) = value.split_once(['x', 'X']).ok_or_else(error)?;
    let parse = |s: &str| {
        s.trim()
            .parse::<f32>()
            .ok()
            .filter(|n| *n > 0.0)
            .ok_or_else(error)
    };
    Ok((parse(w)?, parse(h)?))
}

/// `$XDG_CONFIG_HOME/arcons-cc`, falling back to `$HOME/.config/arcons-cc`.
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("arcons-cc")
}

/// The non-secret configuration: base URL, channel, window mode. Written by #8.
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// The credential. Read, never printed, never logged, never committed (`INV-08`).
pub fn credential_file() -> PathBuf {
    config_dir().join("credentials.toml")
}

/// Report one file's presence, and remember the gap when it is absent.
fn report(label: &str, path: &std::path::Path, owner: &str, missing: &mut Vec<String>) {
    if path.exists() {
        println!("  {label:<12} present  {}", path.display());
    } else {
        println!("  {label:<12} MISSING  {} (ticket {owner})", path.display());
        missing.push(format!("{label} ({owner})"));
    }
}

/// Report what is configured and what is missing. Exit 0 when nothing is, else 2.
///
/// Scaffold-era truth: nothing is configured yet, so this exits 2 and names each gap with the
/// ticket that fills it. #8 keeps the shape and replaces the stubs.
pub fn self_check() -> ExitCode {
    println!("arcons-cc {} self-check", env!("CARGO_PKG_VERSION"));
    println!("  config dir   {}", config_dir().display());

    let mut missing: Vec<String> = Vec::new();

    report("config", &config_file(), "#8", &mut missing);
    // Presence only. Reading it is #8's job, and printing anything from it would be an INV-08 bug.
    report("credentials", &credential_file(), "#8", &mut missing);

    let seed = map::seed_dir();
    if map::seed_present() {
        println!("  map seed     present  {}", seed.display());
    } else {
        println!("  map seed     EMPTY    {} (ticket #5)", seed.display());
        missing.push("map seed (#5)".to_string());
    }

    println!("  REST         not checked (needs a base URL: ticket #3)");
    missing.push("REST base URL (#3)".to_string());
    println!("  socket       not checked (needs a credential: ticket #4)");
    missing.push("socket subscription (#4)".to_string());

    if missing.is_empty() {
        println!("ok");
        ExitCode::SUCCESS
    } else {
        eprintln!("self-check: missing {}", missing.join(", "));
        ExitCode::from(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Result<Args, String> {
        Args::parse(argv.iter().map(|s| s.to_string()))
    }

    #[test]
    fn a_bare_invocation_runs_the_display_at_the_token_size() {
        let args = parse(&[]).unwrap();
        assert_eq!(args.command, Command::Run);
        assert_eq!(args.window.size, tokens::WINDOW_SIZE);
        assert!(!args.window.fullscreen);
        assert!(args.passthrough.is_empty());
    }

    #[test]
    fn the_window_size_flag_needs_two_positive_numbers() {
        assert_eq!(
            parse(&["--window-size", "1920x1080"]).unwrap().window.size,
            (1920.0, 1080.0)
        );
        for bad in ["1920", "1920x", "x1080", "1920x0", "-1x1080", "bigxsmall"] {
            let got = parse(&["--window-size", bad]);
            assert!(got.is_err(), "expected {bad:?} to be refused, got {got:?}");
        }
        assert!(parse(&["--window-size"]).is_err());
    }

    #[test]
    fn runes_own_flags_pass_through_untouched() {
        // #9 captures with `cargo run -- --snap out/picture.png`; swallowing that flag would make
        // the capture silently produce nothing.
        let args = parse(&[
            "--fullscreen",
            "--snap",
            "out/picture.png",
            "--tree",
            "out/t.json",
        ])
        .unwrap();
        assert!(args.window.fullscreen);
        assert_eq!(
            args.passthrough,
            vec!["--snap", "out/picture.png", "--tree", "out/t.json"]
        );
    }

    #[test]
    fn the_config_dir_is_ours_and_absolute() {
        // A relative XDG_CONFIG_HOME must not resolve against the working directory: a wall
        // process started from `/` would otherwise look for `./arcons-cc`.
        let dir = config_dir();
        assert!(
            dir.is_absolute(),
            "config dir must be absolute, got {}",
            dir.display()
        );
        assert_eq!(dir.file_name().unwrap(), "arcons-cc");
        assert!(credential_file().starts_with(&dir));
        assert_ne!(credential_file(), config_file());
    }
}
