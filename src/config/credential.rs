//! `credentials.toml` — the one file that holds a secret.
//!
//! Three rules, and each has a failure that costs a deployment:
//!
//! 1. **Mode `0600`, or refused.** A world-readable credential is a credential that has been read by
//!    someone. The refusal names the fix (`chmod 600 <path>`) because the operator reads one line.
//! 2. **Exactly one mode.** Both a refresh token and a password in the same file is a configuration
//!    with two answers, and the wrong one wins silently.
//! 3. **Nothing here is printed, ever.** The parsed value is [`Secret`], whose `Debug` is
//!    `<redacted>`, and no `Display` exists at all (`INV-08`).

use std::path::{Path, PathBuf};

use crate::config::{Credential, Secret};

/// Why a credential file was refused. Each says what to do about it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("no credential file at {path}: write one with mode 0600, see docs/runbook.md")]
    Missing { path: PathBuf },
    #[error("cannot read {path}: {error}")]
    Unreadable { path: PathBuf, error: String },
    #[error(
        "{path} is readable by group or others (mode {mode:04o}); it holds a secret: \
         chmod 600 {path}"
    )]
    Permissions { path: PathBuf, mode: u32 },
    #[error("{path}: {error}")]
    Shape { path: PathBuf, error: String },
    #[error(
        "cannot tell whether {path} is private on this platform (it has no POSIX file modes), and a \
         credential file nobody can check is a credential file to refuse: keep the secret in the \
         environment (MINOS_REFRESH_TOKEN, or MINOS_IDENTIFIER and MINOS_PASSWORD) instead"
    )]
    Unverifiable { path: PathBuf },
}

impl Refused {
    pub fn path(&self) -> &Path {
        match self {
            Refused::Missing { path }
            | Refused::Unreadable { path, .. }
            | Refused::Permissions { path, .. }
            | Refused::Shape { path, .. }
            | Refused::Unverifiable { path } => path,
        }
    }
}

/// What a credential file may say. Absent means unset.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    refresh_token: Option<String>,
    identifier: Option<String>,
    password: Option<String>,
}

/// Read and admit a credential file.
///
/// The mode is checked **before** the contents are read: a file we are not allowed to trust is a
/// file whose contents we should not have in memory either.
pub fn read(path: &Path) -> Result<Credential, Refused> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Refused::Missing {
                path: path.to_path_buf(),
            });
        }
        Err(error) => {
            return Err(Refused::Unreadable {
                path: path.to_path_buf(),
                error: error.to_string(),
            });
        }
    };
    admit_mode(path, &metadata)?;

    let text = std::fs::read_to_string(path).map_err(|error| Refused::Unreadable {
        path: path.to_path_buf(),
        error: error.to_string(),
    })?;
    parse(path, &text)
}

/// Refuse a file another user can read. Split out so the test does not have to create a real 0644
/// file *and* a real credential to prove the rule.
pub fn admit_mode(path: &Path, metadata: &std::fs::Metadata) -> Result<(), Refused> {
    mode_verdict(path, mode_from(metadata))
}

/// The mode a platform can state for a file, or `None` when it has no such notion.
fn mode_from(metadata: &std::fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(metadata.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        // Windows has ACLs, not modes. Reading them is a dependency and a policy of its own; until
        // that is decided, the wall says it cannot verify rather than pretending it did (`#31`).
        let _ = metadata;
        None
    }
}

/// The rule, with the mode as a value: **every** branch is testable on any platform, including the one
/// where the platform itself has no modes to check.
///
/// A missing mode is a refusal, not a pass. The gap this closes was found by the cross-platform matrix
/// (`#31`): `cfg(not(unix))` used to `let _ = (path, metadata)` and return `Ok`, so a wall on Windows
/// would have read a credential file whose permissions nothing had looked at — silently, because the
/// code that would have said so was compiled out.
pub fn mode_verdict(path: &Path, mode: Option<u32>) -> Result<(), Refused> {
    match mode {
        Some(mode) if mode & 0o077 != 0 => Err(Refused::Permissions {
            path: path.to_path_buf(),
            mode,
        }),
        Some(_) => Ok(()),
        None => Err(Refused::Unverifiable {
            path: path.to_path_buf(),
        }),
    }
}

/// The contents, checked. Public so a test can prove the shapes without touching the filesystem.
pub fn parse(path: &Path, text: &str) -> Result<Credential, Refused> {
    let file: File = toml::from_str(text).map_err(|error| Refused::Shape {
        path: path.to_path_buf(),
        error: error.to_string(),
    })?;
    let shape = |error: &str| Refused::Shape {
        path: path.to_path_buf(),
        error: error.to_string(),
    };
    fn present(value: &Option<String>) -> Option<&str> {
        value.as_deref().filter(|v| !v.trim().is_empty())
    }

    let refresh = present(&file.refresh_token);
    let identifier = present(&file.identifier);
    let password = present(&file.password);

    match (refresh, identifier, password) {
        (Some(token), None, None) => Ok(Credential::RefreshToken(Secret::new(token))),
        (None, Some(identifier), Some(password)) => Ok(Credential::Password {
            identifier: identifier.to_string(),
            password: Secret::new(password),
        }),
        // Two answers, or one and a half: both are a configuration that cannot be read the way it
        // looks. The password mode needs BOTH keys; a lone identifier is not a credential.
        (Some(_), _, _) => Err(shape(
            "refresh_token is set, so identifier/password must not be: pick one mode",
        )),
        (None, Some(_), None) => Err(shape("identifier needs a password beside it")),
        (None, None, Some(_)) => Err(shape("password needs an identifier beside it")),
        (None, None, None) => Err(shape(
            "no credential in it: set refresh_token, or identifier and password",
        )),
    }
}

/// The file mode, for the self-check line. Never the contents.
pub fn mode_of(path: &Path) -> Option<u32> {
    mode_from(&std::fs::metadata(path).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Minos;

    fn path() -> PathBuf {
        PathBuf::from("/cfg/credentials.toml")
    }

    #[test]
    fn a_platform_that_cannot_state_a_mode_refuses_the_file_instead_of_passing_it() {
        // `#31`: the non-unix branch used to discard the metadata and return `Ok`, so a wall on Windows
        // read a credential nobody had checked. The rule is now a value, and this is the branch that runs
        // there — testable here because it takes `None` rather than a real Windows filesystem.
        let refused = mode_verdict(&path(), None).unwrap_err();
        assert!(
            matches!(refused, Refused::Unverifiable { .. }),
            "a missing mode must refuse: {refused:?}"
        );
        let text = refused.to_string();
        assert!(
            text.contains("MINOS_REFRESH_TOKEN"),
            "the refusal names the way out: {text}"
        );
        assert_eq!(refused.path(), path());
    }

    #[test]
    fn the_mode_rule_is_about_group_and_other_bits_only() {
        // 0700 is private to its owner and fine; anything a group or a stranger can read is not.
        assert!(mode_verdict(&path(), Some(0o600)).is_ok());
        assert!(mode_verdict(&path(), Some(0o700)).is_ok());
        for mode in [0o640, 0o604, 0o660, 0o644, 0o777] {
            let refused = mode_verdict(&path(), Some(mode)).unwrap_err();
            assert!(
                matches!(refused, Refused::Permissions { mode: m, .. } if m == mode),
                "{mode:04o} must be refused: {refused:?}"
            );
            assert!(
                refused.to_string().contains("chmod 600"),
                "the refusal names the fix"
            );
        }
    }

    #[test]
    fn a_refresh_token_is_one_shape_and_a_password_is_the_other() {
        let token = Minos::new(
            "https://x.test",
            parse(&path(), "refresh_token = \"canary-token\"\n").unwrap(),
        );
        assert_eq!(token.refresh_token(), Some("canary-token"));
        assert_eq!(token.password(), None);
        assert_eq!(token.kind(), "refresh token");

        let pair = Minos::new(
            "https://x.test",
            parse(
                &path(),
                "identifier = \"wall@example.test\"\npassword = \"canary-password\"\n",
            )
            .unwrap(),
        );
        assert_eq!(
            pair.password(),
            Some(("wall@example.test", "canary-password"))
        );
        assert_eq!(pair.refresh_token(), None);
        assert_eq!(pair.kind(), "identifier and password");
    }

    #[test]
    fn two_modes_one_and_a_half_or_none_are_all_refused_with_a_reason() {
        for (text, needle) in [
            (
                "refresh_token = \"t\"\nidentifier = \"i\"\npassword = \"p\"\n",
                "pick one mode",
            ),
            ("identifier = \"i\"\n", "needs a password"),
            ("password = \"p\"\n", "needs an identifier"),
            ("", "no credential in it"),
            ("refresh_token = \"\"\n", "no credential in it"),
            ("refresh_tokn = \"t\"\n", "unknown field"),
        ] {
            let refused = parse(&path(), text).expect_err(text);
            let message = refused.to_string();
            assert!(
                message.contains(needle),
                "{needle:?} is not in {message:?} for {text:?}"
            );
        }
    }

    #[test]
    fn a_group_readable_file_is_refused_and_the_message_names_the_chmod() {
        let dir = std::env::temp_dir().join(format!("arcons-cc-creds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("credentials.toml");
        std::fs::write(&file, "refresh_token = \"canary-token\"\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for (mode, refused) in [(0o644, true), (0o604, true), (0o640, true), (0o600, false)] {
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
                let got = read(&file);
                assert_eq!(
                    got.is_err(),
                    refused,
                    "mode {mode:04o}: {got:?} (canary must never be echoed: {got:?})"
                );
                if let Err(Refused::Permissions { mode: got, .. }) = &got {
                    assert_eq!(*got, mode, "the refusal names the mode it found");
                }
            }

            // The refusal itself must not contain the secret.
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            let message = read(&file).unwrap_err().to_string();
            assert!(
                !message.contains("canary"),
                "the secret was printed: {message}"
            );
            assert!(message.contains("chmod 600"), "the fix is named: {message}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_says_so_instead_of_panicking() {
        let missing = std::env::temp_dir().join("arcons-cc-no-such-credential.toml");
        let refused = read(&missing).unwrap_err();
        assert!(matches!(refused, Refused::Missing { .. }));
        assert!(refused.to_string().contains("0600"));
    }
}
