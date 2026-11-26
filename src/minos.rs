//! **Where Minos is, and how the wall signs in to it** (#8): the base URL and the credential.
//!
//! A leaf: it names `std` and `serde_json`, and no other module of this crate. `config` reads it
//! from the flags, the environment or `credentials.toml`; `data`, `live` and `control` name it to
//! reach the server. Keeping it out of `config` removes the `data`/`live -> config` edges from the
//! module graph, and those edges are the cycle.
//!
//! Nothing here prints a secret: [`Secret`] redacts itself, and [`Minos`]'s `Debug` is written by
//! hand, so `{:?}` on the whole configuration is safe by construction (`INV-08`).

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

    /// **The credential this wall already holds, in the shape its own file has** (#196): what it authenticates *to
    /// its relay* with, so no wall-side secret is invented (`docs/mobile-remote.md` decision 2).
    ///
    /// The relay is the deployment's own service and exchanges it the way Minos does: `{"refresh_token": …}` for a
    /// refresh-token wall, `{"identifier": …, "password": …}` for a password one. The value is [`Secret`], which is
    /// why this is the one function that turns it into JSON — and why a `Debug` of anything derived from it, and a
    /// log line that names the relay, still cannot carry one (`INV-08`).
    pub fn relay_credential(&self) -> serde_json::Value {
        match &self.credential {
            Credential::RefreshToken(token) => {
                serde_json::json!({ "refresh_token": token.expose() })
            }
            Credential::Password {
                identifier,
                password,
            } => serde_json::json!({ "identifier": identifier, "password": password.expose() }),
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
