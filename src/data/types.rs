//! The wire types, and the two rules that make them safe to draw.
//!
//! **Absence is not zero** (`INV-01`). `speed_kn`, `speed_ms`, `course_deg` and `accuracy_m` are
//! omitted — or `null` — when unmeasured, and a `0.0` on the wire is a *measured* zero. Every one is
//! `Option<f64>`, `None` means "not measured", and nothing in this crate may `unwrap_or(0.0)` one.
//!
//! **Unknown fields must not break parsing** (`docs/verification.md` rule 8). The server adds fields
//! without ceremony; `deny_unknown_fields` is how a client breaks on a release that changed nothing
//! for it. Nothing here denies them, and `data/tests` proves it with a recorded payload carrying a
//! field the spec does not have.
//!
//! Types are hand-written and small on purpose. Codegen from the OpenAPI would add a build step for
//! five endpoints, and the thing that goes wrong with this API is not the shape of a struct.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// `{status_code, message, data}` — the envelope every Minos route returns, success or failure.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Envelope<T> {
    pub status_code: i32,
    pub message: String,
    pub data: T,
}

/// One hull in the standing picture, with its last known fix if it has ever reported.
///
/// A hull with `position: None` belongs in the picture and not on the map (`INV-02`). On the dev
/// feed that is 126 of 127 hulls: the common case, not the edge case.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FeedPosition {
    pub id_unit: i64,
    pub name: String,
    /// Omitted for hulls the source lists without one. Not an empty string, not a zero.
    #[serde(default)]
    pub hull_number: Option<String>,
    /// **Absent** (the key is missing, not `null`) when the hull has never reported.
    #[serde(default)]
    pub position: Option<FeedFix>,
}

/// One accepted position report.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FeedFix {
    pub latitude: f64,
    pub longitude: f64,
    /// Knots, or `None` when unmeasured. `Some(0.0)` is a measured stop.
    #[serde(default)]
    pub speed_kn: Option<f64>,
    /// Metres per second — **the same fact as `speed_kn`**, absent exactly when it is.
    ///
    /// The HTTP read does not send it; the `live-feed` event does (`realtime-protocol.md` §5).
    /// `speed_kn` is authoritative here: this field is carried so the socket path (#4) can use the
    /// same type, and nothing converts one into the other.
    #[serde(default)]
    pub speed_ms: Option<f64>,
    /// Degrees true, or `None` when unmeasured. `Some(0.0)` is due north.
    #[serde(default)]
    pub course_deg: Option<f64>,
    /// Metres, or `None` when unmeasured.
    #[serde(default)]
    pub accuracy_m: Option<f64>,
    pub recorded_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    /// How old the fix is, measured against the **server's** clock at response time.
    ///
    /// This is the only safe source for age: computing it locally would require trusting this
    /// machine's clock against the server's, and on these machines they differ by months. Between
    /// reads, age is this value plus local *monotonic* elapsed time (`INV-06`).
    pub age_seconds: f64,
    /// Reconstructed from an offline backlog: real, the newest thing known, and it did not just
    /// happen. It must never set the "here now" glyph (`INV-03`).
    pub backfilled: bool,
}

/// A lookup row: `name` is the canonical English value, `id_name` the Indonesian label to show.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Helper {
    pub id: i64,
    pub name: String,
    pub id_name: String,
    pub is_system: bool,
    #[serde(default)]
    pub description_en: Option<String>,
}

/// A hull's class, with the category the class belongs to so one payload draws the whole chain.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitClassSummary {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub id_name: Option<String>,
    #[serde(default)]
    pub unit_type: Option<Helper>,
    pub category: Helper,
}

/// `GET /units/{id}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitDetail {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub hull_number: Option<String>,
    pub unit_class: UnitClassSummary,
    pub unit_status: Helper,
}

/// One entry of `GET /units/{id}/commander`, newest first.
///
/// **The current commander is the entry with no `to_at`.** There is no "who commands it now" route,
/// so ordering and currency cannot disagree. Nobody having ever commanded a hull is an empty list,
/// which is a valid answer and not a 404.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitCommander {
    pub id_unit: i64,
    pub id_user: i64,
    pub user_name: String,
    pub from_at: DateTime<Utc>,
    #[serde(default)]
    pub to_at: Option<DateTime<Utc>>,
    pub assigned_by: i64,
    pub note: String,
}

impl UnitCommander {
    /// Whether this assignment is the open one.
    pub fn is_current(&self) -> bool {
        self.to_at.is_none()
    }
}

/// One version of a hull's specification. Append-only: editing a figure inserts a new version.
///
/// Every measurement is nullable, and **null means unknown, not zero** — the source data carries no
/// submerged speed for any submarine, and a client that reads a missing speed as zero freezes the
/// vessel while one that reads it as unbounded lets it outrun a frigate.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitSpecification {
    pub version: i64,
    pub is_current: bool,
    #[serde(default)]
    pub loa_m: Option<f64>,
    #[serde(default)]
    pub beam_m: Option<f64>,
    #[serde(default)]
    pub draft_m: Option<f64>,
    #[serde(default)]
    pub displacement_standard_t: Option<f64>,
    #[serde(default)]
    pub displacement_full_t: Option<f64>,
    #[serde(default)]
    pub range_nm: Option<f64>,
    #[serde(default)]
    pub endurance_days: Option<i64>,
    #[serde(default)]
    pub crew_personnel: Option<i64>,
    #[serde(default)]
    pub embarked_personnel: Option<i64>,
    #[serde(default)]
    pub turn_rate_max_deg_s: Option<f64>,
    /// True when the value was derived rather than supplied by an operator. `None` when the field is
    /// absent, which is not the same as "not derived".
    #[serde(default)]
    pub turn_rate_is_derived: Option<bool>,
    /// The free-form note block (`armament`, `propulsion`, …). Kept as JSON: it is operator prose
    /// with no schema, and inventing fields for it is how a client breaks on the next entry.
    #[serde(default)]
    pub spec: Option<serde_json::Value>,
}

/// A token pair. **The tokens never print** (`INV-08`): the fields are private, the accessors are
/// explicit, and `Debug` is written by hand.
#[derive(Clone, PartialEq, Deserialize)]
pub struct TokenPair {
    access_token: String,
    #[serde(default)]
    token_type: String,
    expires_in: i64,
    #[serde(default)]
    refresh_token: Option<String>,
}

impl TokenPair {
    /// The bearer token. Named `expose` so that every use reads like one.
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// The rotated refresh token, when the server sent one.
    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }

    /// Lifetime in seconds, as the server stated it.
    pub fn expires_in(&self) -> i64 {
        self.expires_in
    }
}

/// Redacted by hand, not by string filtering: there is no path from a token to a log line.
impl std::fmt::Debug for TokenPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenPair")
            .field("access_token", &"<redacted>")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// The body of a failed request: `{status_code, message, data}`.
///
/// Branch on `status_code` and on the field names in `data.errors`, **never on `message`** — the
/// message is prose for a human and is free to change. The ticket that specified this module called
/// the machine-readable field `code`; the pinned spec has no such field, so this models what the
/// server actually sends and says so instead of inventing one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ErrorResponse {
    pub status_code: i32,
    pub message: String,
    #[serde(default)]
    pub data: Option<ErrorData>,
}

/// The `data` of an [`ErrorResponse`]: `null` when the failure is not attributable to a named input.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ErrorData {
    #[serde(default)]
    pub errors: BTreeMap<String, String>,
}
