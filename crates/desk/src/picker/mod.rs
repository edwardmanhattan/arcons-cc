//! **The picker, without a window** — finding a hull in the standing picture, locally.
//!
//! The list is every vessel `GET /live-feed/positions` returns, including hulls that never reported
//! (ADR-0009, decision 4). No reportable-set filter exists here: Minos publishes no such set for a shore
//! account, so the desk lists every hull and learns about a `403` at send. A local filter would hide a
//! vessel the operator can actually report.
//!
//! Search is local and needs no network after the picture is read, so a lost connection does not empty the
//! list on the one screen where the operator is trying to write into silence. Age is carried from the
//! response rather than derived in the view: the same hull must not look older every time a pane redraws.

mod fold;
mod rank;
mod search;
mod vessel;

pub use rank::Rank;
pub use search::{Hit, Picker};
pub use vessel::{LastKnown, Vessel};
