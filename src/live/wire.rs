//! The `live-feed` publication, read at the depth that channel actually uses.
//!
//! Centrifugo adds **one** `data` level around every payload, and the channels are not uniform
//! beneath it (`realtime-protocol.md` §4):
//!
//! | channel | shape |
//! | --- | --- |
//! | `live-feed` | a **bare** event at `pub.data` — what this module parses |
//! | `game:<id>` | a `GameEvent` at `pub.data.type` with its payload at `pub.data.data.*` (two levels) |
//! | `game:<id>:positions` | bare, **no `type`**, at `pub.data.positions` |
//!
//! A client that assumes one shape reads `undefined` and gets no error — which is how Minos's own
//! probe once reported a correct plot as empty. So [`parse`] takes the bytes of `pub.data` and
//! **refuses** anything that is not a live-feed event; a `game:` payload is an error here, not a
//! silent `None`.
//!
//! There is **no `age_seconds`** on this event (`realtime-protocol.md` §5), and its absence is the
//! design: age is measured at the moment of rendering, so a value frozen at publish time is an
//! under-estimate dressed as a measurement the instant it arrives. [`LiveFix::age_seconds`] computes
//! it from `received_at`.

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// One `live-feed` publication: a unit's label and its newest fix, flat.
///
/// The three optional measurements are `Option` and the keys are **absent** when unmeasured — never
/// zero (`INV-01`). `backfilled` says the fix did not just happen (`INV-03`) and travels with it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct LiveFix {
    /// The join key against `GET /live-feed/positions`.
    pub id_unit: i64,
    pub name: String,
    #[serde(default)]
    pub hull_number: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    /// Knots, or `None` when unmeasured.
    #[serde(default)]
    pub speed_kn: Option<f64>,
    /// The same fact as `speed_kn`, in metres per second, absent exactly when it is.
    #[serde(default)]
    pub speed_ms: Option<f64>,
    #[serde(default)]
    pub course_deg: Option<f64>,
    #[serde(default)]
    pub accuracy_m: Option<f64>,
    /// When the fix was **taken**.
    pub recorded_at: DateTime<Utc>,
    /// When the server **accepted** it. The only safe anchor for age.
    pub received_at: DateTime<Utc>,
    /// Reconstructed from an offline backlog: real, the newest thing known, and it did not just
    /// happen.
    #[serde(default)]
    pub backfilled: bool,
}

impl LiveFix {
    /// How old this fix is at `now`, in seconds, never negative.
    ///
    /// `now` is the **render clock**, not the wall clock compared to a server timestamp: this
    /// machine's clock and Minos's differ (the dev feed's only fix was 38 days old by
    /// `age_seconds`), so comparing a local `now` with a server `received_at` would produce a fix
    /// younger than it is. The normal path measures elapsed **monotonic** time since receipt and
    /// adds it to this value (`INV-06`); the parameter exists so a test can pin the clock.
    pub fn age_seconds(&self, now: DateTime<Utc>) -> f64 {
        let age = (now - self.received_at).num_milliseconds() as f64 / 1000.0;
        age.max(0.0)
    }
}

/// Why a publication was not a live-feed event.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("the publication is not JSON: {0}")]
    NotJson(String),
    #[error(
        "the publication is not a live-feed event at pub.data: {0}. A game channel has two data levels (pub.data.data.*) and game:<id>:positions lives at pub.data.positions — reading the wrong depth is `undefined` with no error, which is why this refuses instead"
    )]
    NotLiveFeed(String),
}

/// Parse one `live-feed` publication from the bytes of **`pub.data`**.
///
/// The caller passes what Centrifugo put at `pub.data`, not the whole envelope: the `data` level is
/// the transport's, and unwrapping it in two places is how the two depths get confused.
pub fn parse(pub_data: &[u8]) -> Result<LiveFix, WireError> {
    serde_json::from_slice::<LiveFix>(pub_data).map_err(|error| {
        if serde_json::from_slice::<serde_json::Value>(pub_data).is_err() {
            WireError::NotJson(error.to_string())
        } else {
            WireError::NotLiveFeed(error.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example from `realtime-protocol.md` §5, verbatim — the event as the wire sends it.
    const SPEC_EXAMPLE: &str = r#"{
      "id_unit": 13,
      "name": "KRI Ahmad Yani",
      "hull_number": "KRI-AH-YN",
      "latitude": -6.0888,
      "longitude": 106.9111,
      "speed_kn": 14.2,
      "speed_ms": 7.307333333333334,
      "course_deg": 87.5,
      "accuracy_m": 4.5,
      "recorded_at": "2026-09-15T09:56:28.923518653Z",
      "received_at": "2026-09-15T09:56:28.923518653Z",
      "backfilled": false
    }"#;

    #[test]
    fn the_documented_event_reads_at_pub_data() {
        let fix = parse(SPEC_EXAMPLE.as_bytes()).expect("the §5 example must parse");
        assert_eq!(fix.id_unit, 13);
        assert_eq!(fix.name, "KRI Ahmad Yani");
        assert_eq!(fix.speed_kn, Some(14.2));
        assert_eq!(fix.course_deg, Some(87.5));
        assert!(!fix.backfilled, "the documented example is a live fix");
    }

    #[test]
    fn an_unmeasured_speed_is_absent_and_never_zero() {
        // INV-01 on the socket path: the optional keys are omitted, not zeroed.
        let body = SPEC_EXAMPLE
            .replace("      \"speed_kn\": 14.2,\n", "")
            .replace("      \"speed_ms\": 7.307333333333334,\n", "")
            .replace("      \"course_deg\": 87.5,\n", "")
            .replace("      \"accuracy_m\": 4.5,\n", "");
        let fix =
            parse(body.as_bytes()).expect("a publication with no measurements is still a fix");
        assert_eq!(fix.speed_kn, None);
        assert_eq!(fix.course_deg, None);
        assert_eq!(fix.accuracy_m, None);
        assert!(fix.latitude.is_finite() && fix.longitude.is_finite());
    }

    #[test]
    fn a_game_publication_is_refused_at_this_depth() {
        // The wrong-depth failure the protocol warns about: a `game:` payload read as a live-feed
        // event. It must be an error, not a marker at latitude 0.
        let game = br#"{"type":"game.position","data":{"positions":[{"id_unit":1,"latitude":0.0,"longitude":0.0}]}}"#;
        let error = parse(game).expect_err("a two-level game payload is not a live-feed event");
        assert!(
            matches!(error, WireError::NotLiveFeed(_)),
            "expected a depth refusal, got {error:?}"
        );

        let positions = br#"{"assumed_time":"2026-09-15T09:56:28Z","positions":[]}"#;
        assert!(
            matches!(parse(positions), Err(WireError::NotLiveFeed(_))),
            "game:<id>:positions is not a live-feed event either"
        );
    }

    #[test]
    fn rubbish_is_reported_as_rubbish_not_as_a_wrong_depth() {
        let error = parse(b"not json at all").expect_err("garbage is an error");
        assert!(matches!(error, WireError::NotJson(_)), "got {error:?}");
    }

    #[test]
    fn a_backfilled_fix_keeps_its_flag() {
        // INV-03 begins here: the socket path must not lose the flag, or a days-old point becomes
        // "here now" one layer above.
        let body = SPEC_EXAMPLE.replace("\"backfilled\": false", "\"backfilled\": true");
        let fix = parse(body.as_bytes()).expect("parses");
        assert!(fix.backfilled);
    }

    #[test]
    fn age_comes_from_the_render_clock_and_is_never_negative() {
        let fix = parse(SPEC_EXAMPLE.as_bytes()).expect("parses");
        let received = fix.received_at;
        assert_eq!(fix.age_seconds(received), 0.0);
        assert!((fix.age_seconds(received + chrono::Duration::seconds(30)) - 30.0).abs() < 1e-6);
        assert_eq!(
            fix.age_seconds(received - chrono::Duration::seconds(30)),
            0.0,
            "a clock that jumps backwards must not make a fix younger than zero: still a clamp, never a negative age"
        );
    }
}
