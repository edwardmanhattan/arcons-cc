//! **Where the desk sends, and who it says it is** — the deployment, resolved the way the wall resolves it.
//!
//! ADR-0009 says the desk's credential lives where the wall's does: `$XDG_CONFIG_HOME/arcons-cc/`, the same TOML
//! shapes, and **mode 0600 or refused**. So this file does not invent a second configuration language: it reads the
//! wall's own `config.toml` for `minos_url`, the wall's own credential reader (mode check included) for the secret,
//! and the environment first in both cases, because a deployment's setting is not the operator's.
//!
//! What it does **not** reuse is the wall's client. The wall's process must not hold a code path that can put a
//! vessel somewhere; the two applications share configuration, never a write.
//!
//! Nothing here prints a secret, and the refusal messages name a path and a mode, never a value (`INV-08`).

use std::path::{Path, PathBuf};

use arcons_cc::config::credential::Refused as CredentialRefused;
use arcons_cc::config::{Credential, Minos, Secret};

use crate::identity;

/// Why the desk cannot say where to send or who it is. Each sentence is an instruction.
#[derive(Debug)]
pub enum Refused {
    /// No `MINOS_BASE_URL` and no `minos_url` in the config file.
    NoDeployment { config: PathBuf },
    /// The credential file is missing, world-readable, or unreadable — the wall's own refusals, passed through.
    Credential(CredentialRefused),
    /// A `MINOS_IDENTIFIER` without a `MINOS_PASSWORD` (or the other way round): half a credential is none.
    HalfCredential,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDeployment { config } => write!(
                f,
                "no deployment: set MINOS_BASE_URL or minos_url in {}",
                config.display()
            ),
            Self::Credential(refused) => refused.fmt(f),
            Self::HalfCredential => f.write_str(
                "half a credential: set both MINOS_IDENTIFIER and MINOS_PASSWORD, or neither",
            ),
        }
    }
}

impl std::error::Error for Refused {}

/// The deployment a send goes to, and the credential it goes with.
pub struct Deployment {
    pub minos: Minos,
}

/// The base is printable; the credential is not. A `Debug` that printed the whole struct would put a password in a
/// panic message, and a panic is a log line (`INV-08`).
impl std::fmt::Debug for Deployment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deployment")
            .field("base", &self.minos.base_url())
            .finish_non_exhaustive()
    }
}

impl Deployment {
    /// Resolve from this process's environment and this installation's config directory.
    pub fn load() -> Result<Self, Refused> {
        let dir = identity::config_dir().ok_or_else(|| Refused::NoDeployment {
            config: PathBuf::from("$XDG_CONFIG_HOME/arcons-cc/config.toml"),
        })?;
        Self::resolve(|name| std::env::var(name).ok(), &dir)
    }

    /// The resolution itself, with the environment passed in so it can be tested without touching this process's.
    ///
    /// Order: **environment, then file**, and nothing else. A deployment set in the environment wins over the file,
    /// because a machine that runs the desk against a different deployment for one session should not have to edit a
    /// file another installation owns.
    pub fn resolve(env: impl Fn(&str) -> Option<String>, dir: &Path) -> Result<Self, Refused> {
        let config = dir.join("config.toml");
        let file = arcons_cc::config::file::File::read(&config).unwrap_or_default();
        let base = env("MINOS_BASE_URL")
            .or(file.minos_url.clone())
            .ok_or_else(|| Refused::NoDeployment {
                config: config.clone(),
            })?;

        let credential = if let Some(identifier) = env("MINOS_IDENTIFIER") {
            let password = env("MINOS_PASSWORD").ok_or(Refused::HalfCredential)?;
            Credential::Password {
                identifier,
                password: Secret::new(password),
            }
        } else if let Some(token) = env("MINOS_REFRESH_TOKEN") {
            Credential::RefreshToken(Secret::new(token))
        } else {
            let path = file
                .credentials
                .as_deref()
                .map_or_else(|| dir.join("credentials.toml"), |named| dir.join(named));
            arcons_cc::config::credential::read(&path).map_err(Refused::Credential)?
        };

        Ok(Self {
            minos: Minos::new(base, credential),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A directory of this test's own, removed when the test ends.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("desk-deployment-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("a temporary directory");
            Self(path)
        }

        fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.0.join(name);
            let mut file = std::fs::File::create(&path).expect("a file");
            file.write_all(text.as_bytes()).expect("written");
            path
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_environment_wins_over_the_file_in_both_places() {
        let temp = Temp::new("env");
        temp.write(
            "config.toml",
            "minos_url = \"https://from-the-file/api/v1\"\n",
        );
        let env = |name: &str| match name {
            "MINOS_BASE_URL" => Some("https://from-the-environment/api/v1".to_string()),
            "MINOS_IDENTIFIER" => Some("operator".to_string()),
            "MINOS_PASSWORD" => Some("secret".to_string()),
            _ => None,
        };
        let deployment = Deployment::resolve(env, &temp.0).expect("resolved");
        assert_eq!(
            deployment.minos.base_url(),
            "https://from-the-environment/api/v1"
        );
        assert_eq!(deployment.minos.password(), Some(("operator", "secret")));
    }

    #[cfg(unix)]
    #[test]
    fn the_file_answers_when_the_environment_does_not() {
        use std::os::unix::fs::PermissionsExt;
        let temp = Temp::new("file");
        temp.write(
            "config.toml",
            "minos_url = \"https://from-the-file/api/v1\"\n",
        );
        let credentials = temp.write(
            "credentials.toml",
            "identifier = \"operator\"\npassword = \"secret\"\n",
        );
        std::fs::set_permissions(&credentials, std::fs::Permissions::from_mode(0o600))
            .expect("mode 0600");

        let deployment = Deployment::resolve(|_| None, &temp.0).expect("resolved");
        assert_eq!(deployment.minos.base_url(), "https://from-the-file/api/v1");
        assert_eq!(deployment.minos.password(), Some(("operator", "secret")));
    }

    #[cfg(unix)]
    #[test]
    fn a_world_readable_credential_is_refused_before_it_is_read() {
        use std::os::unix::fs::PermissionsExt;
        let temp = Temp::new("mode");
        temp.write("config.toml", "minos_url = \"https://example/api/v1\"\n");
        let credentials = temp.write(
            "credentials.toml",
            "identifier = \"operator\"\npassword = \"hunter2-do-not-print\"\n",
        );
        std::fs::set_permissions(&credentials, std::fs::Permissions::from_mode(0o644))
            .expect("mode 0644");

        let refused = Deployment::resolve(|_| None, &temp.0).expect_err("0644 is refused");
        let sentence = refused.to_string();
        assert!(
            sentence.contains("0644") && sentence.contains("chmod 600"),
            "the refusal names the mode it found and the command that fixes it: {sentence}"
        );
        assert!(
            !sentence.contains("hunter2-do-not-print"),
            "a refusal never repeats the value it refused: {sentence}"
        );
    }

    #[test]
    fn no_deployment_anywhere_says_which_file_to_write() {
        let temp = Temp::new("empty");
        let refused = Deployment::resolve(|_| None, &temp.0).expect_err("nothing configured");
        let sentence = refused.to_string();
        assert!(
            sentence.contains("MINOS_BASE_URL") && sentence.contains("config.toml"),
            "the sentence says both places: {sentence}"
        );
    }

    #[test]
    fn half_a_credential_is_no_credential() {
        let env = |name: &str| match name {
            "MINOS_BASE_URL" => Some("https://example/api/v1".to_string()),
            "MINOS_IDENTIFIER" => Some("operator".to_string()),
            _ => None,
        };
        let temp = Temp::new("half");
        let refused = Deployment::resolve(env, &temp.0).expect_err("a password is missing");
        assert!(
            matches!(refused, Refused::HalfCredential),
            "an identifier with no password is refused as such, got {refused:?}"
        );
    }
}
