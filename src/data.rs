//! The Minos REST client: the standing picture, one vessel, and the identity a card needs.
//!
//! **Nothing here runs on the UI thread.** #3 puts every call on the reads thread and hands the
//! result to the view loop as a message; a blocking `GET` in `view` is how a wall display freezes
//! and stays frozen.
//!
//! The endpoints this module will own (pinned in `docs/spec/`, at Minos `01f8fbe2ca88`):
//!
//! | need | call |
//! | --- | --- |
//! | the initial picture | `GET /live-feed/positions` — not paginated, includes hulls with no `position` |
//! | one vessel | `GET /live-feed/positions/{id}` |
//! | identity and specification | `GET /units/{id}`, `GET /units/{id}/specifications` |
//! | who commands it | `GET /units/{id}/commander` — newest first; the current one has no `to_at` |
//! | sign in, refresh | `POST /auth/login` (`return_refresh_token: true`), `POST /auth/refresh` |
//!
//! `GET /live-feed/positions` needs the `read` action on `/system/live-feed`. That permission *is*
//! the answer to "what account does the wall get".
//!
//! Facts the code must not re-derive per call site, because they are the traps:
//!
//! - An absent or `null` `speed_kn`/`speed_ms`/`course_deg`/`accuracy_m` is **unmeasured**, never
//!   zero (`INV-01`). `Option<f64>`, never `f64` with a default.
//! - `backfilled: true` is real, current-as-of-whenever-it-arrived, and **did not just happen**
//!   (`INV-03`).
//! - `live-feed` has no history: after a reconnect, re-read before trusting anything (`INV-09`).
//! - Unknown JSON fields must parse: the server adds fields without ceremony.
//!
//! No item exists yet on purpose — #3 lands the first one, and this module's shape must be argued
//! in that ticket rather than guessed here.

/// The field lists this client models, one per schema it reads from the pinned spec.
///
/// These are **the model contract**, not a decoder: #3 implements the structs to match, and
/// `tests/spec_drift.rs` fails when the snapshot makes required a field that is not listed here,
/// or drops a property that is. The lists are deliberately written by hand — a generated list would
/// agree with the generator, not with the server.
///
/// Two schemas the display consumes are *not* JSON objects in the spec and so are absent here:
/// the `live-feed` publication is a bare `FeedPositionEvent` (described in
/// `realtime-protocol.md` §5), and the connect-frame credential is `{"token": "…"}`.
pub mod wire {
    /// `GET /live-feed/positions` → one entry per hull, `position` absent when it never reported.
    pub const FEED_POSITION: &[&str] = &["id_unit", "name", "hull_number", "position"];

    /// The fix inside a `FeedPosition`. The four nullable measurements are `Option` (`INV-01`),
    /// and `backfilled` is the flag that must never set the "here now" glyph (`INV-03`).
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

    /// The pair inside [`AUTH_ENVELOPE`]. `refresh_token` is present only because the wall asks for
    /// it (`return_refresh_token: true`).
    pub const TOKEN_PAIR: &[&str] = &["access_token", "token_type", "expires_in", "refresh_token"];

    /// Every 4xx/5xx body.
    pub const ERROR_RESPONSE: &[&str] = &["status_code", "message", "data"];

    /// The `game:<id>:positions` payload — bare, no `type`, at `pub.data.positions` (v0.2, #11).
    /// The ticket that specified `tests/spec_drift.rs` called this `GamePositionEvent`; there is no
    /// such schema in the snapshot, and this is the object that carries `positions`.
    pub const GAME_POSITION_LIST: &[&str] = &["assumed_time", "positions", "from_unit"];

    /// Every schema this client models, by the name it has in `docs/spec/minos-api.yaml`.
    pub const MODELLED: &[(&str, &[&str])] = &[
        ("FeedPosition", FEED_POSITION),
        ("FeedFix", FEED_FIX),
        ("FeedPositionListEnvelope", FEED_POSITION_LIST_ENVELOPE),
        ("AuthEnvelope", AUTH_ENVELOPE),
        ("TokenPair", TOKEN_PAIR),
        ("ErrorResponse", ERROR_RESPONSE),
        ("GamePositionList", GAME_POSITION_LIST),
    ];
}

#[cfg(test)]
mod tests {
    use super::wire;

    #[test]
    fn every_modelled_schema_names_at_least_one_field() {
        // A typo that empties a list would make the drift test pass vacuously.
        assert!(!wire::MODELLED.is_empty());
        for (schema, fields) in wire::MODELLED {
            assert!(
                !fields.is_empty(),
                "{schema} models no fields, so its drift check proves nothing"
            );
        }
    }
}
