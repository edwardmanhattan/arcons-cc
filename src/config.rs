//! Flags, paths, and the `--self-check` story.
//!
//! #8 owns the credential file, the Minos base URL and the real checks (REST reachable, socket
//! subscribed, map seed present). What lives here today is the part that must not move later:
//! **where** configuration comes from, and the rule that a credential is never printed.
//!
//! Configuration is read from the environment or `$XDG_CONFIG_HOME/arcons-cc/`. Never from the
//! source tree: `INV-08` forbids a token in any log line, and the surest way to leak one is to
//! check it in. `--self-check` therefore reports *presence*, never contents.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ui::tokens;

pub mod check;
pub mod credential;
pub mod file;
pub mod layout;
pub mod settings;

pub const USAGE: &str = "\
arcons-cc — the live maritime picture on a wall

USAGE:
    arcons-cc [OPTIONS]

OPTIONS:
    --window-size WxH   open at this size, in points (default 1280x720)
    --fullscreen        cover the primary output, no frame
    --no-decorations    no title bar, no resize grip: nothing to click on a wall
    --config PATH       read this config file instead of $XDG_CONFIG_HOME/arcons-cc/config.toml
    --credentials PATH  read this credential file instead of the one config.toml names
    --log-file PATH     also write the log there (stderr always gets it)
    --assert SCENE      check a capture's pixels and tree (scripts/assert.sh calls this)
    --image PATH        the capture --assert reads, and the candidate --compare compares
    --compare PATH      compare --image against this golden, within the scene's golden bounds, and
                        exit 1 if the difference is a change rather than a rounding difference
    --tolerance N       how far one channel may differ, in 255ths (default: the scene's bound, 2)
    --budget N          how many pixels may change by more than one step (default: the scene's, 200)
    --rounding N        how many pixels may differ by exactly one step (default: the scene's, 600)
    --tree PATH         the element tree to check too
    --reconnect-after S drop the socket once, S seconds after connecting (the soak's deliberate gap)
    --self-check        check config, credential, login, REST, socket and map seed, and exit
                        with 0 ok · 1 config · 2 credential · 3 login · 4 REST · 5 socket · 6 seed
    --scenes            list the scenes a capture can render, one per line
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
    /// Print the names of the scenes a capture can render, one per line, and exit 0. `scripts/verify.sh`
    /// reads this rather than keeping its own list: a scene the gate does not render is a scene nothing
    /// checks, which is how `photo` spent ten days drawing the wrong thing behind a green gate (`#54`).
    Scenes,
    /// Check a capture: pixels with numbers, and the element tree for structure.
    Assert,
    /// Compare two captures within a stated tolerance, and exit 1 if the difference is a change rather
    /// than a rounding difference. `scripts/capture.sh` uses it against a golden.
    Compare,
}

/// How the window should open. `--fullscreen` wins over `--window-size`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub fullscreen: bool,
    /// Logical size in points.
    pub size: (f32, f32),
    /// No title bar and no resize grip. A wall has no window manager affordances to click, and a
    /// title bar is one more thing that can be dragged out of place.
    pub decorations: bool,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            fullscreen: false,
            size: tokens::WINDOW_SIZE,
            decorations: true,
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
    /// `--config`: where the configuration is. `None` means the default under `XDG_CONFIG_HOME`.
    pub config: Option<PathBuf>,
    /// `--credentials`: the credential file, overriding the one `config.toml` names.
    pub credentials: Option<PathBuf>,
    /// `--log-file`: a second place the log goes, for a wall with no terminal attached.
    pub log_file: Option<PathBuf>,
    /// `--assert`: the scene whose assertions to run.
    pub assert_scene: Option<String>,
    /// `--image`: the capture `--assert` reads, and the candidate `--compare` compares.
    pub image: Option<PathBuf>,
    /// `--compare`: the golden to compare `--image` against.
    pub compare_with: Option<PathBuf>,
    /// How far any one channel may differ, in steps of 255, before it is a different picture.
    pub tolerance: Option<u8>,
    /// How many pixels may differ at all before it is a different picture.
    pub budget: Option<usize>,
    pub rounding: Option<usize>,
    /// `--reconnect-after`: drop the socket once, this many seconds after connecting. A soak aid: the
    /// reconnect path is otherwise only exercised by a network that happens to blink.
    pub reconnect_after: Option<std::time::Duration>,
    /// `--tree`: the element tree to check beside the capture.
    pub tree: Option<PathBuf>,
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
            config: None,
            credentials: None,
            log_file: None,
            assert_scene: None,
            image: None,
            compare_with: None,
            // Measured on two rasterisers (lavapipe and this laptop's iGPU, 2026-11-08): at most 23
            // pixels of 921,600 differ, by at most 1/255. Ten times that is still far below a real
            // change, and it leaves room for another driver's rounding. `tests/verify.rs` pins the
            // behaviour, and `docs/verification.md` says why the numbers are these.
            tolerance: None,
            budget: None,
            rounding: None,
            reconnect_after: None,
            tree: None,
            passthrough: Vec::new(),
        };

        let mut argv = argv.into_iter();
        while let Some(arg) = argv.next() {
            match arg.as_str() {
                "--version" | "-V" => args.command = Command::Version,
                "--help" | "-h" => args.command = Command::Help,
                "--assert" => {
                    let value = argv.next().ok_or("--assert needs a scene name")?;
                    args.assert_scene = Some(value);
                    args.command = Command::Assert;
                }
                "--image" => {
                    let value = argv.next().ok_or("--image needs a path")?;
                    args.image = Some(PathBuf::from(value));
                }
                "--compare" => {
                    let value = argv.next().ok_or("--compare needs the golden's path")?;
                    args.compare_with = Some(PathBuf::from(value));
                    args.command = Command::Compare;
                }
                "--tolerance" => {
                    let value = argv.next().ok_or("--tolerance needs a number")?;
                    args.tolerance = Some(
                        value
                            .parse()
                            .map_err(|_| "--tolerance is a number of 255ths, 0 to 255")?,
                    );
                }
                "--budget" => {
                    let value = argv.next().ok_or("--budget needs a number")?;
                    args.budget = Some(
                        value
                            .parse()
                            .map_err(|_| "--budget is a number of pixels")?,
                    );
                }
                "--rounding" => {
                    let value = argv.next().ok_or("--rounding needs a number")?;
                    args.rounding = Some(
                        value
                            .parse()
                            .map_err(|_| "--rounding is a number of pixels")?,
                    );
                }
                "--reconnect-after" => {
                    let value = argv.next().ok_or("--reconnect-after needs seconds")?;
                    let seconds: u64 = value
                        .parse()
                        .map_err(|_| "--reconnect-after is a whole number of seconds")?;
                    args.reconnect_after = Some(std::time::Duration::from_secs(seconds));
                }
                "--tree" => {
                    let value = argv.next().ok_or("--tree needs a path")?;
                    args.tree = Some(PathBuf::from(value));
                }
                "--self-check" => args.command = Command::SelfCheck,
                "--scenes" => args.command = Command::Scenes,
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
                "--no-decorations" => args.window.decorations = false,
                "--config" => {
                    let value = argv.next().ok_or("--config needs a path")?;
                    args.config = Some(PathBuf::from(value));
                }
                "--credentials" => {
                    let value = argv.next().ok_or("--credentials needs a path")?;
                    args.credentials = Some(PathBuf::from(value));
                }
                "--log-file" => {
                    let value = argv.next().ok_or("--log-file needs a path")?;
                    args.log_file = Some(PathBuf::from(value));
                }
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

/// **The wall as the operator arranged it** (#153): workspaces, panes, cameras, names.
pub fn layout_file() -> PathBuf {
    layout::layout_file()
}

/// Write a file **atomically and privately**: a temp file in the same directory, mode 0600, a rename.
///
/// The rename is what makes a wall that loses power come back with the previous file instead of half a
/// new one, and the mode is what stops a habit: a wall's config directory is where the credential lives,
/// and a file everyone can read there is a habit that will write a secret into one eventually. Both
/// writers in this module need exactly this, so it is one function rather than two copies that can drift.
pub fn write_atomically(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let directory = path
        .parent()
        .ok_or_else(|| format!("{} has no directory", path.display()))?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    // A temp file **in the same directory**, so the rename is a rename and not a copy across
    // filesystems: `layout.toml.tmp` next to `layout.toml` is atomic; `/tmp/...` is not.
    let temp = path.with_extension("toml.tmp");
    let mut file =
        std::fs::File::create(&temp).map_err(|error| format!("{}: {error}", temp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // 0600 **before** the content is in it: a window in which the file exists, readable and empty, is
        // shorter than one where it is readable and full, but it is free to close.
        if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("{}: {error}", temp.display()));
        }
    }
    if let Err(error) = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("{}: {error}", temp.display()));
    }
    drop(file);
    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        format!("{}: {error}", path.display())
    })
}

/// What the display itself remembers: the preferences someone changed on the wall (#82).
pub fn settings_file() -> PathBuf {
    settings::settings_file()
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

    /// `"refresh token"` or `"identifier and password"`: what the wall is doing about its identity,
    /// in words that never include it.
    pub fn kind(&self) -> &'static str {
        match &self.credential {
            Credential::RefreshToken(_) => "refresh token",
            Credential::Password { .. } => "identifier and password",
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

/// The environment, snapshotted once.
///
/// Read through this rather than `std::env` so a test can describe an environment instead of
/// mutating the process's — two tests that both set `MINOS_BASE_URL` are a flaky test, not a test.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Env(BTreeMap<String, String>);

impl Env {
    pub fn from_process() -> Self {
        let mut vars = BTreeMap::new();
        for name in [
            "MINOS_BASE_URL",
            "MINOS_SOCKET_URL",
            "MINOS_REFRESH_TOKEN",
            "MINOS_IDENTIFIER",
            "MINOS_PASSWORD",
        ] {
            if let Ok(value) = std::env::var(name) {
                vars.insert(name.to_string(), value);
            }
        }
        Self(vars)
    }

    pub fn new<'a, I>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        Self(
            pairs
                .into_iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        )
    }

    /// A set, non-blank value. Blank is unset: an empty `MINOS_PASSWORD` in a `.env` is a password
    /// nobody meant to send.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
    }
}

/// Why the configuration could not be used, and which exit code says so.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Problem {
    /// Exit 1: no URL, an unreadable or misspelled config file, a half-set credential in the env.
    #[error("{0}")]
    Unusable(String),
    /// Exit 2: the credential file is missing, readable by others, or says two things at once.
    #[error("{0}")]
    Credentials(credential::Refused),
}

impl Problem {
    pub fn code(&self) -> u8 {
        match self {
            Problem::Unusable(_) => 1,
            Problem::Credentials(_) => 2,
        }
    }
}

/// Where a setting came from. The self-check prints it, because "it works on my machine" is usually
/// a different `config.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Flag,
    Env,
    File,
    Default,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Flag => "flag",
            Source::Env => "env",
            Source::File => "file",
            Source::Default => "default",
        }
    }
}

/// The resolved configuration: what the wall actually uses, and where each part came from.
#[derive(Clone, Debug)]
pub struct Config {
    pub minos: Minos,
    /// The Centrifugo endpoint. Named explicitly when the socket is not derivable from the REST base
    /// (a deployment behind a path-prefixing proxy), derived otherwise.
    pub socket_url: String,
    pub socket_source: Source,
    pub map_center: (f64, f64),
    pub map_zoom: f64,
    pub stale_after: Duration,
    pub config_path: PathBuf,
    pub credentials_path: PathBuf,
    pub minos_source: Source,
    pub credential_source: Source,
    /// A **soak aid**: drop the socket once, this long after connecting. Never set in a deployment; the
    /// flag exists so the reconnect path can be exercised on purpose instead of by luck.
    pub reconnect_after: Option<Duration>,
}

impl Config {
    /// Resolve flags, then the environment, then the file, then the default — in that order, and
    /// saying which one answered.
    pub fn load(args: &Args, env: &Env) -> Result<Config, Problem> {
        let config_path = args.config.clone().unwrap_or_else(config_file);
        let file = file::File::read(&config_path).map_err(Problem::Unusable)?;

        let (price, minos_url, minos_source) = match env.get("MINOS_BASE_URL") {
            Some(url) => (None, url.to_string(), Source::Env),
            None => match file.minos_url.clone() {
                Some(url) => (None, url, Source::File),
                None => (Some(()), String::new(), Source::Default),
            },
        };
        if price.is_some() {
            return Err(Problem::Unusable(format!(
                "minos_url is not configured: set it in {} or in MINOS_BASE_URL",
                config_path.display()
            )));
        }

        let credential_path = args
            .credentials
            .clone()
            .or_else(|| file.credential_path(&config_path))
            .unwrap_or_else(credential_file);
        let (credential, credential_source) = resolve_credential(args, env, &credential_path)?;
        let minos = Minos::new(minos_url, credential);

        let (socket_url, socket_source) = match env.get("MINOS_SOCKET_URL") {
            Some(url) => (url.to_string(), Source::Env),
            None => match file.centrifugo_url.clone() {
                Some(url) => (url, Source::File),
                None => (minos.socket_url(), Source::Default),
            },
        };

        let start = crate::map::geometry::Camera::START;
        Ok(Config {
            socket_url,
            socket_source,
            map_center: file
                .map_center
                .map(|[lat, lon]| (lat, lon))
                .unwrap_or(start.center),
            map_zoom: file.map_zoom.unwrap_or(start.zoom),
            stale_after: Duration::from_secs_f64(
                file.stale_after_s
                    .unwrap_or_else(|| crate::units::STALE_AFTER.as_secs_f64()),
            ),
            config_path,
            credentials_path: credential_path,
            minos_source,
            credential_source,
            reconnect_after: args.reconnect_after,
            minos,
        })
    }
}

/// The credential: a flag-named file, then the environment, then the file the config names.
fn resolve_credential(
    args: &Args,
    env: &Env,
    path: &Path,
) -> Result<(Credential, Source), Problem> {
    // An explicit `--credentials` is an instruction: read that file even if the environment could
    // answer, because the operator asked for *this* credential and a silent substitution is how a
    // wall signs in as the wrong account.
    if args.credentials.is_some() {
        return credential::read(path)
            .map(|credential| (credential, Source::Flag))
            .map_err(Problem::Credentials);
    }
    if let Some(token) = env.get("MINOS_REFRESH_TOKEN") {
        return Ok((Credential::RefreshToken(Secret::new(token)), Source::Env));
    }
    match (env.get("MINOS_IDENTIFIER"), env.get("MINOS_PASSWORD")) {
        (Some(identifier), Some(password)) => Ok((
            Credential::Password {
                identifier: identifier.to_string(),
                password: Secret::new(password),
            },
            Source::Env,
        )),
        // One without the other is a configuration that looks set and is not.
        (Some(_), None) | (None, Some(_)) => Err(Problem::Unusable(
            "MINOS_IDENTIFIER and MINOS_PASSWORD are set one at a time: set both, or neither"
                .to_string(),
        )),
        (None, None) => credential::read(path)
            .map(|credential| (credential, Source::File))
            .map_err(Problem::Credentials),
    }
}

/// The configuration the running process uses.
///
/// A `static` because Rune's `AppSpec::init` is a plain `fn() -> State`: a state cannot carry the
/// configuration *in* to the app, so the process installs it once, before the first frame, and
/// everything that needs it reads it from here. Tests never call [`set_current`], so `current()` is
/// `None` there and the defaults apply — which is what a test wants.
static CURRENT: std::sync::OnceLock<Config> = std::sync::OnceLock::new();

/// Install the configuration for this process. Idempotent: a second call is ignored, because two
/// configurations in one process is a bug, not a feature.
pub fn set_current(config: &Config) {
    let _ = CURRENT.set(config.clone());
}

/// The configuration, once [`set_current`] has run.
pub fn current() -> Option<&'static Config> {
    CURRENT.get()
}

/// What the wall is doing about its identity, in words that never include it.
pub fn credential_kind(credential: &Credential) -> &'static str {
    match credential {
        Credential::RefreshToken(_) => "refresh token",
        Credential::Password { .. } => "identifier and password",
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
            "--at",
            "2",
            "--scene",
            "picture",
        ])
        .unwrap();
        assert!(args.window.fullscreen);
        assert_eq!(
            args.passthrough,
            vec![
                "--snap",
                "out/picture.png",
                "--at",
                "2",
                "--scene",
                "picture"
            ]
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
