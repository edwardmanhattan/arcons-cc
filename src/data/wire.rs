//! The field lists this client models, one per schema it reads from the pinned spec.
//!
//! These are **the model contract**, not a decoder: the types in [`crate::data::types`] implement
//! them, and `tests/spec_drift.rs` fails when the snapshot makes required a field that is not listed
//! here, or drops a property that is. The lists are deliberately written by hand — a generated list
//! would agree with the generator, not with the server.
//!
//! Two schemas the display consumes are *not* JSON objects in the spec and so are absent here: the
//! `live-feed` publication is a bare `FeedPositionEvent` (described in `realtime-protocol.md` §5),
//! and the connect-frame credential is `{"token": "…"}`.

/// `GET /live-feed/positions` → one entry per hull, `position` absent when it never reported.
pub const FEED_POSITION: &[&str] = &["id_unit", "name", "hull_number", "position"];

/// The fix inside a `FeedPosition`. The four nullable measurements are `Option` (`INV-01`), and
/// `backfilled` is the flag that must never set the "here now" glyph (`INV-03`).
pub const FEED_FIX: &[&str] = &[
    "latitude",
    "longitude",
    "speed_kn",
    "speed_ms",
    "course_deg",
    "accuracy_m",
    "recorded_at",
    "received_at",
    "age_seconds",
    "backfilled",
];

/// The envelope `GET /live-feed/positions` returns.
pub const FEED_POSITION_LIST_ENVELOPE: &[&str] = &["status_code", "message", "data"];

/// `POST /auth/login` and `/auth/refresh`.
pub const AUTH_ENVELOPE: &[&str] = &["status_code", "message", "data"];

/// The pair inside [`AUTH_ENVELOPE`]. `refresh_token` is present only because the wall asks for it
/// (`return_refresh_token: true`).
pub const TOKEN_PAIR: &[&str] = &["access_token", "token_type", "expires_in", "refresh_token"];

/// Every 4xx/5xx body.
pub const ERROR_RESPONSE: &[&str] = &["status_code", "message", "data"];

/// The `game:<id>:positions` payload — bare, no `type`, at `pub.data.positions` (v0.2, #11). The
/// ticket that specified `tests/spec_drift.rs` called this `GamePositionEvent`; there is no such
/// schema in the snapshot, and this is the object that carries `positions`.
pub const GAME_POSITION_LIST: &[&str] = &["assumed_time", "positions", "from_unit"];

/// `GET /units/{id}`.
pub const UNIT_DETAIL: &[&str] = &["id", "name", "hull_number", "unit_class", "unit_status"];

/// A lookup row (`unit_status`, `service_branch`, a category).
pub const HELPER: &[&str] = &["id", "name", "id_name", "is_system", "description_en"];

/// A hull's class, with the category it belongs to.
pub const UNIT_CLASS_SUMMARY: &[&str] = &["id", "name", "id_name", "unit_type", "category"];

/// `GET /units/{id}/commander` — one assignment; the current one has no `to_at`.
pub const UNIT_COMMANDER_SUMMARY: &[&str] = &[
    "id_unit",
    "id_user",
    "user_name",
    "from_at",
    "to_at",
    "assigned_by",
    "note",
];

/// `GET /units/{id}/specifications` — an immutable, append-only version.
pub const UNIT_SPECIFICATION: &[&str] = &[
    "version",
    "is_current",
    "loa_m",
    "beam_m",
    "draft_m",
    "displacement_standard_t",
    "displacement_full_t",
    "range_nm",
    "endurance_days",
    "crew_personnel",
    "embarked_personnel",
    "turn_rate_max_deg_s",
    "turn_rate_is_derived",
    "spec",
];

/// Every schema this client models, by the name it has in `docs/spec/minos-api.yaml`.
pub const MODELLED: &[(&str, &[&str])] = &[
    ("FeedPosition", FEED_POSITION),
    ("FeedFix", FEED_FIX),
    ("FeedPositionListEnvelope", FEED_POSITION_LIST_ENVELOPE),
    ("AuthEnvelope", AUTH_ENVELOPE),
    ("TokenPair", TOKEN_PAIR),
    ("ErrorResponse", ERROR_RESPONSE),
    ("GamePositionList", GAME_POSITION_LIST),
    ("UnitDetail", UNIT_DETAIL),
    ("Helper", HELPER),
    ("UnitClassSummary", UNIT_CLASS_SUMMARY),
    ("UnitCommanderSummary", UNIT_COMMANDER_SUMMARY),
    ("UnitSpecification", UNIT_SPECIFICATION),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_modelled_schema_names_at_least_one_field() {
        // A typo that empties a list would make the drift test pass vacuously.
        assert!(!MODELLED.is_empty());
        for (schema, fields) in MODELLED {
            assert!(
                !fields.is_empty(),
                "{schema} models no fields, so its drift check proves nothing"
            );
        }
    }

    #[test]
    fn the_measurements_that_may_be_absent_are_not_in_a_required_list() {
        // INV-01 is a type fact before it is a rendering fact: these four names exist and are never
        // required, because "unmeasured" has to be representable.
        for (schema, fields) in [("FEED_FIX", FEED_FIX)] {
            for measurement in ["speed_kn", "speed_ms", "course_deg", "accuracy_m"] {
                assert!(
                    fields.contains(&measurement),
                    "{measurement} must be modelled in {schema}"
                );
            }
        }
    }
}
