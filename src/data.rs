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
