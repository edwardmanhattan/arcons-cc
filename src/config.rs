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
    --probe-map         render one map frame to a PNG with no window (needs --features map)
    --out PATH          where --probe-map writes (default out/probe.png)
    --probe-at L,L,Z    the camera to probe (default the wall's start camera)
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
    /// Render one map frame to a PNG with no window. CI's map witness.
    ProbeMap,
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
    /// Where `--probe-map` writes its PNG.
    pub out: std::path::PathBuf,
    /// The camera `--probe-map` renders, when it is not the default.
    pub probe_camera: Option<crate::map::geometry::Camera>,
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
            out: std::path::PathBuf::from("out/probe.png"),
            probe_camera: None,
            passthrough: Vec::new(),
        };

        let mut argv = argv.into_iter();
        while let Some(arg) = argv.next() {
            match arg.as_str() {
                "--version" | "-V" => args.command = Command::Version,
                "--help" | "-h" => args.command = Command::Help,
                "--self-check" => args.command = Command::SelfCheck,
                "--probe-map" => args.command = Command::ProbeMap,
                "--out" => {
                    let value = argv
                        .next()
                        .ok_or("--out needs a path, for example out/probe.png")?;
                    args.out = std::path::PathBuf::from(value);
                }
                "--probe-at" => {
                    let value = argv
                        .next()
                        .ok_or("--probe-at needs LAT,LON,ZOOM, for example -6.108,106.91,11")?;
                    args.probe_camera = Some(parse_camera(&value)?);
                }
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

/// `LAT,LON,ZOOM`, in degrees and zoom levels.
pub fn parse_camera(value: &str) -> Result<crate::map::geometry::Camera, String> {
    let error = || format!("--probe-at is LAT,LON,ZOOM (got {value:?})");
    let mut parts = value.split(',');
    let lat = parts
        .next()
        .ok_or_else(error)?
        .trim()
        .parse::<f64>()
        .map_err(|_| error())?;
    let lon = parts
        .next()
        .ok_or_else(error)?
        .trim()
        .parse::<f64>()
        .map_err(|_| error())?;
    let zoom = parts
        .next()
        .ok_or_else(error)?
        .trim()
        .parse::<f64>()
        .map_err(|_| error())?;
    if parts.next().is_some() || !lat.is_finite() || !lon.is_finite() || !zoom.is_finite() {
        return Err(error());
    }
    Ok(crate::map::geometry::Camera::new((lat, lon), zoom))
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

/// A configuration value that must never print. `Debug` is written by hand, so `{:?}` on the whole
/// configuration is safe by construction — not by remembering to filter it somewhere downstream.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Hand the value over. Named `expose` so that every use reads like one.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// How the wall proves who it is. `Debug` never shows the secret: [`Secret`] redacts itself.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Credential {
    /// Preferred for a deployment: no password at rest, and a wall can hold one for a long time.
    RefreshToken(Secret),
    /// Identifier and password, exchanged at startup. What the dev account uses.
    Password {
        identifier: String,
        password: Secret,
    },
}

/// Where Minos is, and how to sign in to it.
#[derive(Clone, PartialEq, Eq)]
pub struct Minos {
    base_url: String,
    credential: Credential,
}

impl Minos {
    pub fn new(base_url: impl Into<String>, credential: Credential) -> Self {
        let mut base_url: String = base_url.into();
        while base_url.ends_with('/') {
            base_url.pop();
        }
        Self {
            base_url,
            credential,
        }
    }

    /// The REST base, including its path prefix (`…/api/v1`).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `(identifier, password)`, when the wall signs in that way.
    pub fn password(&self) -> Option<(&str, &str)> {
        match &self.credential {
            Credential::Password {
                identifier,
                password,
            } => Some((identifier, password.expose())),
            Credential::RefreshToken(_) => None,
        }
    }

    /// The Centrifugo endpoint, derived from the REST base.
    ///
    /// Two shapes, and both are in the README because getting them wrong is a connect that hangs:
    /// `https://host` → `wss://host/connection/websocket`, and `http://host:8080` →
    /// `ws://host:8000/connection/websocket` (the REST port and the socket port differ in
    /// development, and only the host is shared). A path prefix on the REST base (`…/api/v1`) is
    /// dropped: the socket lives at the host root, not under the API.
    pub fn socket_url(&self) -> String {
        let rest = self.base_url();
        let (scheme, rest) = match rest.split_once("://") {
            Some((scheme, rest)) => (scheme, rest),
            None => ("https", rest),
        };
        let host = rest.split('/').next().unwrap_or(rest);
        if scheme == "https" {
            format!("wss://{host}/connection/websocket")
        } else {
            // A development host: the REST port in the base is 8080, the socket is 8000.
            let hostname = host.split(':').next().unwrap_or(host);
            format!("ws://{hostname}:8000/connection/websocket")
        }
    }

    /// The refresh token, when the wall holds one.
    pub fn refresh_token(&self) -> Option<&str> {
        match &self.credential {
            Credential::RefreshToken(token) => Some(token.expose()),
            Credential::Password { .. } => None,
        }
    }

    /// Read the configuration from the environment, naming what is missing and never what is set.
    ///
    /// #8 adds the file-based credential (`$XDG_CONFIG_HOME/arcons-cc/`); the environment vars are
    /// the developer's route and the same names `.env.example` documents.
    pub fn from_env() -> Result<Minos, Vec<String>> {
        let mut missing = Vec::new();
        let base_url = std::env::var("MINOS_BASE_URL").unwrap_or_default();
        if base_url.trim().is_empty() {
            missing.push("MINOS_BASE_URL".to_string());
        }

        let refresh = std::env::var("MINOS_REFRESH_TOKEN").unwrap_or_default();
        let identifier = std::env::var("MINOS_IDENTIFIER").unwrap_or_default();
        let password = std::env::var("MINOS_PASSWORD").unwrap_or_default();

        let credential = if !refresh.trim().is_empty() {
            Credential::RefreshToken(Secret::new(refresh))
        } else if !identifier.trim().is_empty() && !password.is_empty() {
            Credential::Password {
                identifier,
                password: Secret::new(password),
            }
        } else {
            missing.push("MINOS_REFRESH_TOKEN or MINOS_IDENTIFIER + MINOS_PASSWORD".to_string());
            return Err(missing);
        };

        if missing.is_empty() {
            Ok(Minos::new(base_url, credential))
        } else {
            Err(missing)
        }
    }
}

/// The base URL is not a secret (it is a hostname in a deployment); the credential beside it is.
impl std::fmt::Debug for Minos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Minos")
            .field("base_url", &self.base_url)
            .field("credential", &self.credential)
            .finish()
    }
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

    let seed = map::seed_path();
    if map::seed_present() {
        println!("  map seed     present  {}", seed.display());
    } else {
        println!("  map seed     MISSING  {}", seed.display());
        missing.push("the offline map seed (#5)".to_string());
    }

    // The live checks: sign in, read the picture, parse it, and look at the token's life. Nothing
    // here prints a value from the credential — only what the server said (INV-08).
    match Minos::from_env() {
        Ok(minos) => {
            println!("  REST         {}", minos.base_url());
            match crate::data::live_check(&minos) {
                Ok(check) => {
                    println!(
                        "  picture      ok  {} hulls, {} never reported",
                        check.vessels, check.silent
                    );
                    match check.token_remaining {
                        Some(left) if left.as_secs() > 0 => {
                            println!("  token        ok  {} s left", left.as_secs());
                        }
                        Some(_) => {
                            println!("  token        EXPIRED at sign-in");
                            missing.push("a token that is still valid (#3)".to_string());
                        }
                        None => {
                            println!("  token        no lifetime stated by the server");
                        }
                    }
                }
                Err(error) => {
                    println!("  REST         FAILED  {error}");
                    missing.push(format!("a working read (#3): {error}"));
                }
            }
        }
        Err(names) => {
            println!(
                "  REST         not configured  (needs {})",
                names.join(" and ")
            );
            missing.push("Minos configuration (#8)".to_string());
        }
    }
    if let Ok(minos) = Minos::from_env() {
        println!("  socket       {}", minos.socket_url());
        match crate::live::actor::probe(&minos) {
            Ok(probe) if probe.connected && probe.subscribed => {
                println!("  subscription ok  {}", probe.channel);
            }
            Ok(probe) => {
                println!(
                    "  subscription FAILED  connected={} subscribed={}",
                    probe.connected, probe.subscribed
                );
                missing.push("a socket subscription (#4)".to_string());
            }
            Err(error) => {
                println!("  subscription FAILED  {error}");
                missing.push(format!("a socket subscription (#4): {error}"));
            }
        }
    } else {
        println!("  socket       not checked (needs a Minos configuration: #8)");
        missing.push("socket subscription (#4)".to_string());
    }

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
    fn a_probe_camera_is_three_numbers_and_nothing_else() {
        let camera = parse(&["--probe-map", "--probe-at", "-6.108,106.91,11"])
            .unwrap()
            .probe_camera
            .unwrap();
        assert!((camera.center.0 - -6.108).abs() < 1e-9 && (camera.zoom - 11.0).abs() < 1e-9);
        for bad in ["-6.108,106.91", "-6.108,106.91,11,2", "a,b,c", "", "1,,3"] {
            assert!(parse_camera(bad).is_err(), "expected {bad:?} to be refused");
        }
    }

    #[test]
    fn the_probe_has_a_default_output_and_accepts_another() {
        assert_eq!(parse(&["--probe-map"]).unwrap().command, Command::ProbeMap);
        assert_eq!(
            parse(&["--probe-map"]).unwrap().out,
            std::path::PathBuf::from("out/probe.png")
        );
        assert_eq!(
            parse(&["--probe-map", "--out", "/tmp/x.png"]).unwrap().out,
            std::path::PathBuf::from("/tmp/x.png")
        );
        assert!(parse(&["--out"]).is_err(), "--out needs a path");
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
    fn the_socket_url_follows_the_base_url() {
        // https → wss on the same host, path prefix dropped. http on a dev port → ws on :8000,
        // because the REST and socket ports differ in development and only the host is shared.
        let wall = Minos::new(
            "https://api.example.test/api/v1",
            Credential::RefreshToken(Secret::new("t")),
        );
        assert_eq!(
            wall.socket_url(),
            "wss://api.example.test/connection/websocket"
        );

        let dev = Minos::new(
            "http://minos.local:8080/api/v1",
            Credential::RefreshToken(Secret::new("t")),
        );
        assert_eq!(
            dev.socket_url(),
            "ws://minos.local:8000/connection/websocket"
        );

        // A bare host with no scheme is treated as https, not as a relative path.
        let bare = Minos::new(
            "api.example.test",
            Credential::RefreshToken(Secret::new("t")),
        );
        assert_eq!(
            bare.socket_url(),
            "wss://api.example.test/connection/websocket"
        );
    }

    #[test]
    fn a_secret_never_prints_and_neither_does_the_configuration() {
        // INV-08 for the configuration: `{:?}` on the whole thing is safe by construction.
        let minos = Minos::new(
            "https://api.example.test/api/v1",
            Credential::Password {
                identifier: "wall".into(),
                password: Secret::new("hunter2-not-a-real-one"),
            },
        );
        let printed = format!("{minos:?}");
        assert!(
            !printed.contains("hunter2"),
            "the password printed: {printed}"
        );
        assert!(
            !printed.contains("REDACTED"),
            "redaction must be <redacted>, got {printed}"
        );
        assert!(printed.contains("<redacted>"));
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
