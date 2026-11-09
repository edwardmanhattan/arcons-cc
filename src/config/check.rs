//! `--self-check`: six questions, six exit codes, one line each.
//!
//! ```text
//! 0 ok · 1 config unusable · 2 credentials refused · 3 login failed
//! 4 REST read failed · 5 socket refused · 6 map seed invalid
//! ```
//!
//! The exit code is the **first** failure in that order, so a script can ask "is this ready?" with
//! `arcons-cc --self-check; echo $?` and get one answer rather than a paragraph. Every check still
//! runs and prints: a self-check that stops at the first problem hides the second one, and a wall
//! operator then fixes one thing per session.
//!
//! **Nothing here prints a credential** (`INV-08`). The checks describe what the server *said*
//! (`900 s left`, `127 hulls`, `subscribed to live-feed`), and the one thing taken from the file — its
//! mode — is not a secret. The output is written into a sink rather than straight to stdout so a test
//! can read it and assert that a canary credential is not in it.
//!
//! [`World`] is the seam: the real one signs in over HTTP and subscribes over a websocket, and a test
//! supplies answers without a server. The two checks that must be proven against something real —
//! login and the REST read — are proven against a loopback stub in this module's tests.

use std::sync::Mutex;
use std::time::Duration;

use crate::config::{self, Args, Config, Env, Problem, Source};

/// What a self-check can conclude, in the order the exit codes are numbered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    ConfigUnusable,
    CredentialsRefused,
    LoginFailed,
    ReadFailed,
    SocketRefused,
    SeedInvalid,
}

impl Outcome {
    pub fn code(self) -> u8 {
        match self {
            Outcome::Ok => 0,
            Outcome::ConfigUnusable => 1,
            Outcome::CredentialsRefused => 2,
            Outcome::LoginFailed => 3,
            Outcome::ReadFailed => 4,
            Outcome::SocketRefused => 5,
            Outcome::SeedInvalid => 6,
        }
    }

    /// The one word the summary line uses, for a log or a CI failure annotation.
    pub fn reason(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::ConfigUnusable => "config unusable",
            Outcome::CredentialsRefused => "credentials refused",
            Outcome::LoginFailed => "login failed",
            Outcome::ReadFailed => "REST read failed",
            Outcome::SocketRefused => "socket refused",
            Outcome::SeedInvalid => "map seed invalid",
        }
    }
}

/// What the checks ask of the world. The real implementation talks to Minos; a test answers.
pub trait World {
    /// Prove the credential is accepted, and say how long the access token lasts.
    fn sign_in(&self, config: &Config) -> Result<String, String>;
    /// Read the picture: the thing the wall actually draws.
    fn read(&self, config: &Config) -> Result<String, String>;
    /// Read the fleet register: the hulls and their picture URLs. A different subject from the feed, so a
    /// wall account can pass everything above and still draw no pictures (#53).
    fn register(&self, config: &Config) -> Result<String, String>;
    /// Subscribe to the live feed, with the connect frame's `data` token.
    fn subscribe(&self, config: &Config) -> Result<String, String>;
    /// The offline map seed, without which the first frame waits on a network that may not be there.
    fn seed(&self) -> bool;
}

/// Run every check, print one line each, and conclude the first failure in code order.
pub fn run(args: &Args, env: &Env, world: &dyn World, out: &mut dyn std::fmt::Write) -> Outcome {
    let _ = writeln!(out, "arcons-cc {} self-check", env!("CARGO_PKG_VERSION"));

    let mut verdict = Outcome::Ok;
    let fail = |outcome: Outcome, verdict: &mut Outcome| {
        if *verdict == Outcome::Ok {
            *verdict = outcome;
        }
    };

    let config = match Config::load(args, env) {
        Ok(config) => {
            let _ = writeln!(
                out,
                "  {:<11} ok  {} ({} · minos from {})",
                "config",
                config.config_path.display(),
                if config.config_path.exists() {
                    "file"
                } else {
                    "defaults"
                },
                config.minos_source.label()
            );
            Some(config)
        }
        Err(problem) => {
            let _ = writeln!(out, "  {:<11} FAILED  {problem}", "config");
            fail(
                match problem {
                    Problem::Unusable(_) => Outcome::ConfigUnusable,
                    Problem::Credentials(_) => Outcome::CredentialsRefused,
                },
                &mut verdict,
            );
            None
        }
    };

    let Some(config) = config else {
        // Everything below needs the configuration. Say so instead of printing four identical
        // "skipped" lines that look like four separate problems.
        let _ = writeln!(
            out,
            "  {:<11} skipped  (the configuration is unusable)",
            "rest"
        );
        return conclude(out, verdict);
    };

    // Credentials: already read, or the config would have failed. The line states which mode and the
    // file's mode — never a value from it.
    let mode = config::credential::mode_of(&config.credentials_path);
    let _ = writeln!(
        out,
        "  {:<11} ok  {} · {} (from {})",
        "credentials",
        match config.credential_source {
            // A mode is a fact about a file. An environment credential has no file to state one for,
            // and printing a path with no mode would suggest one was checked.
            Source::Env => "no file".to_string(),
            _ => match mode {
                Some(mode) => format!("{} mode {mode:04o}", config.credentials_path.display()),
                None => config.credentials_path.display().to_string(),
            },
        },
        config.minos.kind(),
        config.credential_source.label()
    );

    match world.sign_in(&config) {
        Ok(detail) => {
            let _ = writeln!(out, "  {:<11} ok  {detail}", "login");
            match world.read(&config) {
                Ok(detail) => {
                    let _ = writeln!(out, "  {:<11} ok  {detail}", "picture");
                }
                Err(error) => {
                    let _ = writeln!(out, "  {:<11} FAILED  {error}", "picture");
                    fail(Outcome::ReadFailed, &mut verdict);
                }
            }
            // A separate line with the same exit code (4, "a read failed"): the code says a read failed,
            // the line says which one. The register is its own read with its own permission, and a wall
            // that cannot read it spends its life drawing symbols for hulls that have photographs.
            match world.register(&config) {
                Ok(detail) => {
                    let _ = writeln!(out, "  {:<11} ok  {detail}", "register");
                }
                Err(error) => {
                    let _ = writeln!(out, "  {:<11} FAILED  {error}", "register");
                    fail(Outcome::ReadFailed, &mut verdict);
                }
            }
        }
        Err(error) => {
            // The read cannot be honest without a session, so it is skipped rather than reported as
            // a second failure: one cause, one line.
            let _ = writeln!(out, "  {:<11} FAILED  {error}", "login");
            let _ = writeln!(out, "  {:<11} skipped  (no session)", "picture");
            let _ = writeln!(out, "  {:<11} skipped  (no session)", "register");
            fail(Outcome::LoginFailed, &mut verdict);
        }
    }

    // The socket check needs a token of its own; a login failure has already been reported, so this
    // is skipped rather than made into a second symptom of the same cause.
    if verdict == Outcome::Ok || verdict == Outcome::ReadFailed {
        match world.subscribe(&config) {
            Ok(detail) => {
                let _ = writeln!(out, "  {:<11} ok  {detail}", "socket");
            }
            Err(error) => {
                let _ = writeln!(out, "  {:<11} FAILED  {error}", "socket");
                fail(Outcome::SocketRefused, &mut verdict);
            }
        }
    } else {
        let _ = writeln!(out, "  {:<11} skipped  (no session)", "socket");
    }

    if world.seed() {
        let _ = writeln!(
            out,
            "  {:<11} ok  {}",
            "map seed",
            crate::map::seed_path().display()
        );
    } else {
        let _ = writeln!(
            out,
            "  {:<11} FAILED  missing {}",
            "map seed",
            crate::map::seed_path().display()
        );
        fail(Outcome::SeedInvalid, &mut verdict);
    }

    conclude(out, verdict)
}

fn conclude(out: &mut dyn std::fmt::Write, verdict: Outcome) -> Outcome {
    if verdict == Outcome::Ok {
        let _ = writeln!(out, "ok");
    } else {
        let _ = writeln!(
            out,
            "FAILED: {} (exit {})",
            verdict.reason(),
            verdict.code()
        );
    }
    verdict
}

/// The live world: sign in, read, subscribe, and look at the seed.
#[derive(Default)]
pub struct Live {
    /// Sign-in is kept, so the read reuses the session the login check proved instead of signing in
    /// twice (which, on a password account, is also two failed attempts when the password is wrong).
    session: Mutex<Option<crate::data::Session>>,
}

impl Live {
    pub fn new() -> Self {
        Self::default()
    }
}

impl World for Live {
    fn sign_in(&self, config: &Config) -> Result<String, String> {
        let session =
            crate::data::Session::start(&config.minos).map_err(|error| error.to_string())?;
        let remaining = session.token_remaining();
        if let Ok(mut held) = self.session.lock() {
            *held = Some(session);
        }
        Ok(match remaining {
            Some(left) => format!(
                "{} accepted, access token valid for {} s",
                config.minos.kind(),
                left.as_secs()
            ),
            None => format!("{} accepted", config.minos.kind()),
        })
    }

    fn read(&self, config: &Config) -> Result<String, String> {
        let mut held = self
            .session
            .lock()
            .map_err(|_| "the session lock was poisoned".to_string())?;
        let session = match held.as_mut() {
            Some(session) => session,
            // No session means the login check was skipped, which only happens in a fake. Sign in
            // then, so the read is still a real read.
            None => {
                let mut session = crate::data::Session::start(&config.minos)
                    .map_err(|error| error.to_string())?;
                let check = session.read().map_err(|error| error.to_string())?;
                *held = Some(session);
                return Ok(format!(
                    "{} hulls, {} never reported",
                    check.vessels, check.silent
                ));
            }
        };
        let check = session.read().map_err(|error| error.to_string())?;
        Ok(format!(
            "{} hulls, {} never reported",
            check.vessels, check.silent
        ))
    }

    fn register(&self, config: &Config) -> Result<String, String> {
        let mut held = self
            .session
            .lock()
            .map_err(|_| "the session lock was poisoned".to_string())?;
        let session = match held.as_mut() {
            Some(session) => session,
            None => {
                let mut session = crate::data::Session::start(&config.minos)
                    .map_err(|error| error.to_string())?;
                let check = session.register().map_err(|error| error.to_string())?;
                *held = Some(session);
                return Ok(format!("{} hulls, {} with a picture", check.0, check.1));
            }
        };
        let check = session.register().map_err(|error| error.to_string())?;
        Ok(format!("{} hulls, {} with a picture", check.0, check.1))
    }

    fn subscribe(&self, config: &Config) -> Result<String, String> {
        let probe = crate::live::actor::probe_url(&config.minos, &config.socket_url)?;
        if probe.connected && probe.subscribed {
            Ok(format!("subscribed to {}", probe.channel))
        } else {
            Err(format!(
                "connected={} subscribed={}",
                probe.connected, probe.subscribed
            ))
        }
    }

    fn seed(&self) -> bool {
        crate::map::seed_present()
    }
}

/// A self-check that never touches the network, for `--self-check` in a place with no Minos: it
/// reports configuration and the seed, and says the live checks were not run. Not used by default;
/// it exists so a boot script can ask a narrower question.
pub fn offline(args: &Args, env: &Env, out: &mut dyn std::fmt::Write) -> Outcome {
    struct Offline;
    impl World for Offline {
        fn sign_in(&self, _: &Config) -> Result<String, String> {
            Err("not attempted (--offline)".to_string())
        }
        fn read(&self, _: &Config) -> Result<String, String> {
            Err("not attempted (--offline)".to_string())
        }
        fn register(&self, _: &Config) -> Result<String, String> {
            Err("not attempted (--offline)".to_string())
        }
        fn subscribe(&self, _: &Config) -> Result<String, String> {
            Err("not attempted (--offline)".to_string())
        }
        fn seed(&self) -> bool {
            crate::map::seed_present()
        }
    }
    run(args, env, &Offline, out)
}

/// Seconds, for the token line in a runbook.
pub fn dur(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// Where a setting came from, in the runbook's words.
pub fn source_label(source: Source) -> &'static str {
    source.label()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A `World` that answers from a script, so the check's own logic — ordering, skipping, exit
    /// codes — can be proven without a server.
    struct Scripted {
        login: Result<String, String>,
        read: Result<String, String>,
        register: Result<String, String>,
        socket: Result<String, String>,
        seed: bool,
    }

    impl Scripted {
        fn good() -> Self {
            Self {
                login: Ok("refresh token accepted, access token valid for 900 s".into()),
                read: Ok("127 hulls, 126 never reported".into()),
                register: Ok("127 hulls, 62 with a picture".into()),
                socket: Ok("subscribed to live-feed".into()),
                seed: true,
            }
        }
    }

    impl World for Scripted {
        fn sign_in(&self, _: &Config) -> Result<String, String> {
            self.login.clone()
        }
        fn read(&self, _: &Config) -> Result<String, String> {
            self.read.clone()
        }
        fn register(&self, _: &Config) -> Result<String, String> {
            self.register.clone()
        }
        fn subscribe(&self, _: &Config) -> Result<String, String> {
            self.socket.clone()
        }
        fn seed(&self) -> bool {
            self.seed
        }
    }

    /// A directory of configuration files for one test, removed on drop.
    struct Sandbox(std::path::PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("arcons-cc-check-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, text: &str, mode: u32) -> std::path::PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, text).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            }
            // Windows has no mode to set, and the tests that pass one are about the POSIX rule: the
            // parameter is still checked for "is it sayable here", not silently dropped (#31 caught this
            // as an `unused variable` error, which is the matrix doing its job).
            #[cfg(not(unix))]
            {
                let _ = mode;
            }
            path
        }

        /// A wall whose configuration lives here, with a refresh token in the credential file.
        fn args(&self, token: &str) -> Args {
            let _credentials = self.write(
                "credentials.toml",
                &format!("refresh_token = \"{token}\"\n"),
                0o600,
            );
            let config = self.write(
                "config.toml",
                "minos_url = \"https://api.example.test/api/v1\"\ncredentials = \"credentials.toml\"\n",
                0o644,
            );
            Args {
                config: Some(config),
                ..Args::parse(std::iter::empty::<String>()).unwrap()
            }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn args() -> Args {
        Args::parse(std::iter::empty::<String>()).unwrap()
    }

    /// A Minos on loopback: `POST /auth/login`, `POST /auth/refresh`, and a queue of `GET` answers.
    /// Real sockets and real `reqwest` — the checks are the thing under test, not a stub of them.
    struct Stub {
        base: String,
        logins: Arc<AtomicUsize>,
        paths: Arc<Mutex<Vec<String>>>,
    }

    impl Stub {
        #[allow(dead_code)]
        fn paths(&self) -> Vec<String> {
            self.paths
                .lock()
                .map(|paths| paths.clone())
                .unwrap_or_default()
        }

        fn env(&self) -> Env {
            Env::new([
                ("MINOS_BASE_URL", self.base.as_str()),
                ("MINOS_IDENTIFIER", "wall@example.test"),
                ("MINOS_PASSWORD", "canary-password-never-print"),
            ])
        }
    }

    /// Minos wraps every answer, success or failure, in `{status_code, message, data}` — the pinned
    /// contract's envelope, which is why a bare `{"data": …}` is a decode failure and not a token.
    const TOKEN_PAIR: &str = r#"{"status_code":200,"message":"Successful","data":{"access_token":"stub-access","refresh_token":"stub-refresh","expires_in":900}}"#;

    fn stub(login_ok: bool, get_status: u16) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let base = format!(
            "http://127.0.0.1:{}/api/v1",
            listener.local_addr().unwrap().port()
        );
        let logins = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&logins);
        let paths = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&paths);
        let gets = Arc::new(Mutex::new(VecDeque::from(vec![get_status])));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let path = request.split(' ').nth(1).unwrap_or("/").to_string();
                if let Ok(mut seen) = seen.lock() {
                    seen.push(path.clone());
                }
                let (status, body) = if path.starts_with("/api/v1/auth/login") {
                    counted.fetch_add(1, Ordering::SeqCst);
                    if login_ok {
                        (200, TOKEN_PAIR.to_string())
                    } else {
                        (
                            401,
                            r#"{"status_code":401,"message":"identifier or password is wrong"}"#
                                .to_string(),
                        )
                    }
                } else if path.starts_with("/api/v1/auth/refresh") {
                    (200, TOKEN_PAIR.to_string())
                } else {
                    let status = gets
                        .lock()
                        .ok()
                        .and_then(|mut queue| queue.pop_front())
                        .unwrap_or(200);
                    if status == 200 {
                        (status, r#"{"status_code":200,"message":"Successful","data":[{"id_unit":1,"name":"KRI Example"}]}"#.to_string())
                    } else {
                        (
                            status,
                            format!(r#"{{"status_code":{status},"message":"stub refused"}}"#),
                        )
                    }
                };
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Stub {
            base,
            logins,
            paths,
        }
    }

    #[test]
    fn a_healthy_wall_exits_zero_and_says_so_in_one_line() {
        let sandbox = Sandbox::new("ok");
        let mut out = String::new();
        let outcome = run(
            &sandbox.args("canary-token"),
            &Env::default(),
            &Scripted::good(),
            &mut out,
        );
        assert_eq!(outcome, Outcome::Ok);
        assert_eq!(outcome.code(), 0);
        assert!(
            out.trim_end().ends_with("ok"),
            "last line is the verdict:\n{out}"
        );
        for line in [
            "config",
            "credentials",
            "login",
            "picture",
            "socket",
            "map seed",
        ] {
            assert!(out.contains(line), "{line:?} has no line:\n{out}");
        }
    }

    #[test]
    fn an_unusable_configuration_exits_one_and_skips_the_rest() {
        let sandbox = Sandbox::new("bad-config");
        let config = sandbox.write("config.toml", "map_cetner = [1, 2]\n", 0o644);
        let mut args = args();
        args.config = Some(config);
        let mut out = String::new();
        let outcome = run(&args, &Env::default(), &Scripted::good(), &mut out);
        assert_eq!(outcome, Outcome::ConfigUnusable);
        assert_eq!(outcome.code(), 1);
        assert!(out.contains("map_cetner"), "the key is named:\n{out}");
        // A configuration we cannot read is not a reason to print four more failures.
        assert!(!out.contains("socket"));
    }

    #[test]
    fn a_group_readable_credential_exits_two_and_names_the_chmod() {
        let sandbox = Sandbox::new("mode");
        let credentials = sandbox.write("credentials.toml", "refresh_token = \"canary\"\n", 0o644);
        let config = sandbox.write(
            "config.toml",
            "minos_url = \"https://api.example.test/api/v1\"\ncredentials = \"credentials.toml\"\n",
            0o644,
        );
        let mut args = args();
        args.config = Some(config);
        let mut out = String::new();
        let outcome = run(&args, &Env::default(), &Scripted::good(), &mut out);
        assert_eq!(outcome, Outcome::CredentialsRefused, "{out}");
        assert_eq!(outcome.code(), 2);
        assert!(out.contains("chmod 600"), "the fix is named:\n{out}");
        assert!(!out.contains("canary"), "the secret was printed:\n{out}");
        let _ = credentials;
    }

    #[test]
    fn a_refused_login_exits_three_against_a_real_server() {
        // A real HTTP server, a real sign-in, a real refusal: 4xx from the stub, exit 3 from us.
        let stub = stub(false, 200);
        let mut out = String::new();
        let outcome = run(&args(), &stub.env(), &Live::new(), &mut out);
        assert_eq!(outcome, Outcome::LoginFailed, "{out}");
        assert_eq!(outcome.code(), 3);
        assert_eq!(
            stub.logins.load(Ordering::SeqCst),
            1,
            "one attempt, not a retry loop"
        );
        assert!(
            !out.contains("canary-refresh-token"),
            "the token printed:\n{out}"
        );
    }

    #[test]
    fn a_failed_read_exits_four_after_a_successful_login() {
        let stub = stub(true, 500);
        let mut out = String::new();
        let outcome = run(&args(), &stub.env(), &Live::new(), &mut out);
        assert_eq!(outcome.code(), 4, "{out}");
        assert!(out.contains("login       ok"), "the login passed:\n{out}");
    }

    #[test]
    fn a_refused_socket_exits_five_and_a_bad_seed_six() {
        let sandbox = Sandbox::new("socket");
        let mut scripted = Scripted::good();
        scripted.socket = Err("refused 403: forbidden".into());
        let mut out = String::new();
        assert_eq!(
            run(&sandbox.args("t"), &Env::default(), &scripted, &mut out).code(),
            5
        );

        let mut scripted = Scripted::good();
        scripted.seed = false;
        let mut out = String::new();
        assert_eq!(
            run(&sandbox.args("t"), &Env::default(), &scripted, &mut out).code(),
            6
        );
    }

    #[test]
    fn the_first_failure_in_code_order_is_the_exit_code() {
        // Both the socket and the seed fail: the code names the earlier one, so the operator fixes
        // the thing that comes first in the list rather than a random one.
        let sandbox = Sandbox::new("order");
        let mut scripted = Scripted::good();
        scripted.socket = Err("refused".into());
        scripted.seed = false;
        let mut out = String::new();
        assert_eq!(
            run(&sandbox.args("t"), &Env::default(), &scripted, &mut out).code(),
            5
        );
    }

    #[test]
    fn a_canary_credential_never_reaches_the_output_of_any_failure() {
        // INV-08 over every path this module owns: the canary is in the credential file and in the
        // environment, and no failure may echo it. The World's errors are what a server says — a fake
        // that copied the canary into its own message would only be testing itself.
        const CANARY: &str = "canary-6d1f2c-never-print";
        let sandbox = Sandbox::new("canary");

        let mut outputs = Vec::new();
        for (scripted, env) in [
            (Scripted::good(), Env::default()),
            (
                Scripted {
                    login: Err("not authorised (401): the credential was refused".into()),
                    ..Scripted::good()
                },
                Env::default(),
            ),
            (
                Scripted {
                    read: Err("forbidden (403): the account may not read /system/live-feed".into()),
                    ..Scripted::good()
                },
                Env::default(),
            ),
            (
                Scripted {
                    socket: Err("refused 101: bad token".into()),
                    ..Scripted::good()
                },
                Env::default(),
            ),
            (
                Scripted::good(),
                Env::new([
                    ("MINOS_REFRESH_TOKEN", CANARY),
                    ("MINOS_BASE_URL", "https://x.test/api/v1"),
                ]),
            ),
            // A credential file that is refused for its mode still must not print its contents.
            (Scripted::good(), Env::default()),
        ] {
            let mut out = String::new();
            let _ = run(&sandbox.args(CANARY), &env, &scripted, &mut out);
            outputs.push(out);
        }

        // And the same canary through a real server that refuses the login.
        let stub = stub(false, 200);
        let mut out = String::new();
        let _ = run(&args(), &stub.env(), &Live::new(), &mut out);
        outputs.push(out);

        for out in outputs {
            assert!(
                !out.contains(CANARY),
                "INV-08: the canary reached the self-check output:\n{out}"
            );
        }
    }
}
