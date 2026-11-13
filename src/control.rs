//! **The control plane's wire** — the constants, and the socket it lives in (#171, epic #162).
//!
//! `docs/adr/0005-control-plane.md` is this module's decision: a client sends **intents** and the wall that owns
//! the pixels renders them, over a unix socket whose permissions are the authentication. The document and the
//! code agree because `tests/doc_claims.rs` compares them: the socket's file name, its mode and the protocol
//! version are named here, once, and quoted there.
//!
//! Nothing here changes the wall yet. #170 adds the intent vocabulary and `apply` (the one door), #165 adds the
//! listener and the `ctl` CLI, and #167 and #164 add the cursor. This module exists first because a decision
//! nobody can compile against is a paragraph, and because the three values below are the ones a document most
//! easily gets wrong.

use std::path::PathBuf;

/// **The protocol's version**, on every message in both directions (`{"v": 1, …}`).
///
/// A client that sends another number is refused by name rather than guessed at: an intent vocabulary is not
/// something to negotiate, and a wall that silently accepted yesterday's verbs would be a wall two clients
/// disagree about.
pub const PROTOCOL: u32 = 1;

/// The socket's file name, inside the run directory. Named rather than spelled out at the call site so the ADR,
/// the CLI and the listener cannot drift apart.
pub const SOCKET_NAME: &str = "control.sock";

/// The socket's mode: owner read and write, nothing for anyone else.
///
/// **This is the credential** (`INV-08`). There is no token, no handshake and no secret to log, so the file's
/// permissions are what decide who may drive the wall — created at this mode, never chmod'ed after the fact
/// (a window between `create` and `chmod` is a window a local process can win).
pub const SOCKET_MODE: u32 = 0o600;

/// The socket's path: `$XDG_RUNTIME_DIR/arcons-cc/control.sock`, or the configuration directory when the run
/// directory is missing.
///
/// The run directory is the right home for it — it is per-user, it is the machine's own (not a network mount),
/// and it is cleared at logout, so a stale socket does not survive a reboot. A container or a systemd unit
/// without `XDG_RUNTIME_DIR` would otherwise have no wall to drive, so the fallback keeps the plane reachable
/// there; either way the directory is created `0700` before the socket is bound (#165).
pub fn socket_path() -> PathBuf {
    socket_path_in(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
}

/// The decision above, with the environment passed in: which directory the socket belongs in.
///
/// Split out so the branch is a **test** rather than a machine's luck — a test that reads the environment can
/// only assert what this machine happens to have, and a relative `XDG_RUNTIME_DIR` must not resolve against the
/// working directory (a wall started from `/` would bind `./arcons-cc/control.sock`).
fn socket_path_in(run: Option<PathBuf>) -> PathBuf {
    match run.filter(|path| path.is_absolute()) {
        Some(dir) => dir.join("arcons-cc").join(SOCKET_NAME),
        None => crate::config::config_dir().join(SOCKET_NAME),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The run directory, when it is one, is where the socket goes: per-user, per-machine, cleared at logout.
    #[test]
    fn the_socket_lives_in_the_run_directory() {
        let path = socket_path_in(Some(PathBuf::from("/run/user/1000")));
        assert_eq!(
            path,
            Path::new("/run/user/1000/arcons-cc/control.sock"),
            "an absolute XDG_RUNTIME_DIR wins: the socket is that user's, on this machine (#171)"
        );
    }

    /// A wall in a container or a unit without a run directory still has a control plane; it just lives beside
    /// the configuration.
    #[test]
    fn a_missing_run_directory_falls_back_to_the_config_directory() {
        let path = socket_path_in(None);
        assert_eq!(path, crate::config::config_dir().join(SOCKET_NAME));
        assert!(
            path.ends_with(SOCKET_NAME),
            "the fallback is still the control socket, got {}",
            path.display()
        );
    }

    /// A relative `XDG_RUNTIME_DIR` is not a directory the wall may create: it would resolve against whatever
    /// the working directory happened to be.
    #[test]
    fn a_relative_run_directory_is_ignored() {
        let path = socket_path_in(Some(PathBuf::from("run/user/1000")));
        assert_eq!(
            path,
            crate::config::config_dir().join(SOCKET_NAME),
            "a relative XDG_RUNTIME_DIR must not resolve against the working directory (#171)"
        );
    }

    /// The three values the ADR quotes are the three values a client depends on, and none of them is optional.
    #[test]
    fn the_wire_constants_are_what_a_client_is_told() {
        assert_eq!(PROTOCOL, 1, "the protocol's version is on every message");
        assert_eq!(SOCKET_NAME, "control.sock");
        assert_eq!(
            SOCKET_MODE, 0o600,
            "the socket is the credential: owner read and write, nothing else"
        );
        // A mode that lets the group or the world write is a wall anyone on the machine can drive (`INV-08`).
        assert_eq!(
            SOCKET_MODE & 0o077,
            0,
            "no group or other bits on the control socket"
        );
    }
}
