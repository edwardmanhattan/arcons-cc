//! The Minos REST client: the standing picture, one hull, and the identity a card needs.
//!
//! **Nothing here runs on the UI thread.** [`thread::Backend`] owns a current-thread Tokio runtime
//! and this module's client on a thread of its own; the view loop sends a [`thread::Command`] and
//! drains [`thread::Reply`]s between frames. A blocking `GET` in `view` is how a wall display freezes
//! and stays frozen.
//!
//! | need | call |
//! | --- | --- |
//! | the initial picture | `GET /live-feed/positions` — not paginated, includes hulls with no `position` |
//! | one hull | `GET /live-feed/positions/{id}` |
//! | identity | `GET /units/{id}` |
//! | who commands it | `GET /units/{id}/commander` — newest first; the current one has no `to_at` |
//! | sign in, refresh | `POST /auth/login` (`return_refresh_token: true`), `POST /auth/refresh` |
//!
//! Permissions, from the pinned spec and **confirmed against the dev server on 2026-11-06**: the
//! picture needs the `read` action on `/system/live-feed`, and `/units/…` (identity, commander) needs
//! `read` on `/system/units`. A read-only wall account therefore holds two `read` actions and nothing
//! else — it never posts a fix, an order or a message.
//!
//! Facts the code must not re-derive per call site, because they are the traps:
//!
//! - An absent or `null` measurement is **unmeasured**, never zero (`INV-01`). `Option<f64>`, never a
//!   default.
//! - `backfilled: true` is real, current-as-of-whenever-it-arrived, and **did not just happen**
//!   (`INV-03`).
//! - `live-feed` has no history: after a reconnect, re-read before trusting anything (`INV-09`).
//! - Unknown JSON fields must parse: the server adds fields without ceremony.
//! - Age comes from the server (`age_seconds`), never from this machine's clock against a timestamp.
//!
//! `401`/`403` are failures to surface, not to retry; `429` is real and backs off. A second `401`
//! after a successful refresh is [`Error::Unauthorized`] — the account is disabled, or the token is
//! for another server, and retrying it all night burns a rate limit for nothing.

pub mod auth;
pub mod http;
pub mod thread;
pub mod types;
pub mod wire;

pub use http::Http;
pub use thread::{Backend, Command, Reply};

/// Everything that can go wrong between here and the wire.
///
/// **No variant carries a token, a header or a password** (`INV-08`): the transport variant carries
/// the URL from `reqwest`'s own message, which never includes request headers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// No credential is configured, or it was consumed by a failed refresh.
    #[error("no credential is configured")]
    NoCredential,
    /// The request never completed.
    #[error("could not reach Minos: {0}")]
    Transport(String),
    /// `401` — and a second `401` after a refresh is exactly this, not a reason to retry.
    #[error("not authorised (401): the credential was refused")]
    Unauthorized,
    /// `403` — authenticated, but not allowed, or the account is not `Active`.
    #[error("forbidden (403): {message}")]
    Forbidden { message: String },
    /// `429` — the server is asking for less traffic. Back off.
    #[error("rate limited (429)")]
    RateLimited,
    /// Any other failure status, with `data.errors` flattened into `field: reason` when present.
    #[error("HTTP {status}: {message}")]
    Status { status: u16, message: String },
    /// The body was not the envelope the spec describes.
    #[error("could not decode the response: {0}")]
    Decode(String),
}

/// What `--self-check` learned from the live server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveCheck {
    /// Hulls in the standing picture — including the ones that have never reported.
    pub vessels: usize,
    /// Hulls with no fix. On the dev feed this is almost all of them.
    pub silent: usize,
    /// How long the access token still has. `None` if the server did not state a lifetime.
    pub token_remaining: Option<std::time::Duration>,
}

/// A signed-in session, kept open for the CLI paths that ask two questions of one wall.
///
/// `start` is the login check and `read` is the read check, deliberately separate: "the credential is
/// refused" (exit 3) and "the credential works and the read fails" (exit 4) are different problems
/// with different fixes, and one function that does both cannot tell them apart.
///
/// This is the CLI path, not the display: it is allowed to block, because nothing is drawing yet.
pub struct Session {
    http: http::Http,
}

impl Session {
    /// Sign in, or exchange the refresh token once. **This is the only call that proves a
    /// credential**, so a failure here is exit 3 and nothing else.
    pub fn start(minos: &crate::config::Minos) -> Result<Session, Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| Error::Transport(format!("could not start a runtime: {error}")))?;
        let http = runtime.block_on(async {
            match minos.refresh_token() {
                // A refresh token is exchanged once at startup rather than held unproven: without
                // this, "signed in" would mean "the file has a string in it".
                Some(token) => {
                    let http = http::Http::with_refresh_token(minos, token.to_string()).await?;
                    http.access_token().await?;
                    Ok::<_, Error>(http)
                }
                None => http::Http::sign_in(minos).await,
            }
        })?;
        Ok(Session { http })
    }

    /// How long the access token still has. `None` when the server stated no lifetime.
    pub fn token_remaining(&self) -> Option<std::time::Duration> {
        self.http.token_remaining()
    }

    /// The picture as the server has it. A read, not a shape check: a `200` with an empty list would
    /// prove the credential works and nothing else.
    pub fn read(&mut self) -> Result<LiveCheck, Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| Error::Transport(format!("could not start a runtime: {error}")))?;
        let token_remaining = runtime.block_on(async {
            let picture = self.http.positions().await?;
            let silent = picture
                .iter()
                .filter(|hull| hull.position.is_none())
                .count();
            Ok::<_, Error>(LiveCheck {
                vessels: picture.len(),
                silent,
                token_remaining: self.http.token_remaining(),
            })
        })?;
        let _ = token_remaining.token_remaining;
        Ok(token_remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::types::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/rest")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
    }

    fn picture(name: &str) -> Vec<FeedPosition> {
        let envelope: Envelope<Vec<FeedPosition>> =
            serde_json::from_str(&fixture(name)).expect("the recorded picture must decode");
        envelope.data
    }

    #[test]
    fn an_absent_speed_is_none_and_never_zero() {
        // INV-01: the measurement is missing, not zero. A `Some(0.0)` here would draw a stopped hull
        // with all the confidence of a measured one.
        let hulls = picture("positions-speed-absent.json");
        let fix = hulls[0].position.as_ref().expect("this fixture has a fix");
        assert_eq!(fix.speed_kn, None, "absent speed_kn must decode as None");
        assert_eq!(
            fix.course_deg,
            Some(255.0),
            "course_deg is still there and must survive: absence is per field"
        );
        assert_ne!(
            fix.speed_kn,
            Some(0.0),
            "an absent measurement is not a measured zero"
        );
    }

    #[test]
    fn a_null_speed_is_none_and_never_zero() {
        // The API encodes the same thing both ways; both must mean "unmeasured".
        let hulls = picture("positions-speed-null.json");
        let fix = hulls[0].position.as_ref().expect("this fixture has a fix");
        assert_eq!(fix.speed_kn, None, "null speed_kn must decode as None");
        assert_ne!(fix.speed_kn, Some(0.0));
    }

    #[test]
    fn a_measured_zero_survives_as_zero() {
        // The other half of INV-01: `0.0` on the wire is a measured stop and must not become "unknown".
        let body =
            fixture("positions-speed-null.json").replace("\"speed_kn\": null", "\"speed_kn\": 0.0");
        let envelope: Envelope<Vec<FeedPosition>> =
            serde_json::from_str(&body).expect("valid after the edit");
        let fix = envelope.data[0].position.as_ref().expect("a fix");
        assert_eq!(fix.speed_kn, Some(0.0), "a measured zero is a measurement");
    }

    #[test]
    fn hulls_that_never_reported_are_in_the_picture_without_a_fix() {
        // INV-02 starts here: 126 of the 127 hulls on the dev feed have never reported. They are in
        // the list and they have no position; neither fact may be inferred from the other.
        let hulls = picture("positions.json");
        assert!(
            hulls.len() > 100,
            "the recorded picture should be the whole fleet, got {}",
            hulls.len()
        );
        let silent = hulls.iter().filter(|hull| hull.position.is_none()).count();
        assert_eq!(
            silent + 1,
            hulls.len(),
            "the recording has exactly one reported hull; if that changed, re-read the fixture before trusting this test"
        );
        let reported = hulls
            .iter()
            .find(|hull| hull.position.is_some())
            .expect("one hull has reported");
        let fix = reported.position.as_ref().unwrap();
        assert!(!fix.backfilled, "the recorded live fix is not a backfill");
        assert!(
            fix.age_seconds > 0.0,
            "the server states the age; it does not send zero for an old fix"
        );
    }

    #[test]
    fn a_hull_number_may_be_missing_or_null() {
        let body = r#"{"status_code":200,"message":"Successfull","data":[{"id_unit":1,"name":"KRI Cakra"},{"id_unit":2,"name":"KRI Nagapasa","hull_number":null},{"id_unit":3,"name":"KRI Bima Suci","hull_number":""}]}"#;
        let envelope: Envelope<Vec<FeedPosition>> = serde_json::from_str(body).expect("decodes");
        assert_eq!(envelope.data[0].hull_number, None, "absent is None");
        assert_eq!(envelope.data[1].hull_number, None, "null is None");
        assert_eq!(
            envelope.data[2].hull_number.as_deref(),
            Some(""),
            "an empty hull number is still a value the server sent; we do not invent one and we do not delete one"
        );
    }

    #[test]
    fn unknown_fields_do_not_break_parsing() {
        // docs/verification.md rule 8. The recorded login carries `must_change_password`, which the
        // pinned TokenPair does not have: the server adds fields without ceremony.
        let pair: Envelope<TokenPair> = serde_json::from_str(&fixture("auth-login.json"))
            .expect("unknown fields must be tolerated");
        assert_eq!(pair.data.expires_in(), 900);
        assert!(
            pair.data.access_token().starts_with("REDACTED"),
            "the fixture's tokens are redacted on purpose"
        );
    }

    #[test]
    fn a_unit_and_its_specification_decode() {
        let unit: Envelope<UnitDetail> =
            serde_json::from_str(&fixture("unit.json")).expect("unit decodes");
        assert!(unit.data.id > 0);
        assert!(
            !unit.data.unit_class.category.name.is_empty(),
            "the class carries its category"
        );

        let spec: Envelope<Vec<UnitSpecification>> =
            serde_json::from_str(&fixture("specifications.json")).expect("specification decodes");
        assert!(
            spec.data.iter().any(|version| version.is_current),
            "one version is current"
        );
    }

    #[test]
    fn a_commander_list_may_be_empty() {
        // "Nobody has ever commanded this hull" is an empty list, not a 404.
        let envelope: Envelope<Vec<UnitCommander>> =
            serde_json::from_str(&fixture("commander-empty.json")).expect("empty list decodes");
        assert!(envelope.data.is_empty());
        assert_eq!(envelope.status_code, 200);
    }

    #[test]
    fn the_current_commander_is_the_entry_with_no_to_at() {
        let body = r#"{"status_code":200,"message":"Successfull","data":[
            {"id_unit":1,"id_user":9,"user_name":"A","from_at":"2026-11-01T00:00:00Z","to_at":null,"assigned_by":1,"note":""},
            {"id_unit":1,"id_user":7,"user_name":"B","from_at":"2026-10-01T00:00:00Z","to_at":"2026-11-01T00:00:00Z","assigned_by":1,"note":""}]}"#;
        let envelope: Envelope<Vec<UnitCommander>> = serde_json::from_str(body).expect("decodes");
        let current: Vec<&UnitCommander> = envelope
            .data
            .iter()
            .filter(|entry| entry.is_current())
            .collect();
        assert_eq!(
            current.len(),
            1,
            "exactly one open assignment, by definition"
        );
        assert_eq!(current[0].user_name, "A");
    }

    #[test]
    fn an_error_envelope_carries_its_status_and_no_token() {
        let error: ErrorResponse =
            serde_json::from_str(&fixture("error-401.json")).expect("error decodes");
        assert_eq!(error.status_code, 401);
        assert_eq!(error.message, "Unauthorized");
        assert!(error.data.is_none(), "a 401 names no field");
        let not_found: ErrorResponse =
            serde_json::from_str(&fixture("error-404.json")).expect("error decodes");
        assert_eq!(not_found.status_code, 404);
    }
}
