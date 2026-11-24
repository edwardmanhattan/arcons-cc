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

use arcons_cc::data::types::FeedPosition;

impl Picker {
    /// **The picture as the wire gave it, in the order the desk draws it.**
    ///
    /// Minos returns `/live-feed/positions` ordered by id; ADR-0009 decision 2 asks the list for **age
    /// order**, so reported hulls come first (youngest first), silent hulls last, and ties break by id.
    /// The age is the wire's own `age_seconds`, carried unchanged; the sort only chooses the order.
    ///
    /// A hull the server names with a non-positive id is not a vessel this desk can report; it is skipped
    /// rather than wrapped into a `u64` that names somebody else. `hull_number` is optional in the
    /// snapshot, and a missing one is drawn as `—`, never as an empty string that reads "Hull ".
    pub fn from_positions(positions: &[FeedPosition]) -> Self {
        let mut vessels: Vec<Vessel> = positions
            .iter()
            .filter_map(|position| {
                let id_unit = u64::try_from(position.id_unit).ok()?;
                let last = position.position.as_ref().map(|fix| LastKnown {
                    recorded_at_ms: fix.recorded_at.timestamp_millis(),
                    age_seconds: fix.age_seconds,
                    backfilled: fix.backfilled,
                    latitude: fix.latitude,
                    longitude: fix.longitude,
                    speed_kn: fix.speed_kn,
                    course_deg: fix.course_deg,
                    accuracy_m: fix.accuracy_m,
                });
                Some(Vessel {
                    id_unit,
                    name: position.name.clone(),
                    hull_number: position
                        .hull_number
                        .clone()
                        .filter(|number| !number.trim().is_empty())
                        .unwrap_or_else(|| "—".to_string()),
                    last,
                })
            })
            .collect();
        vessels.sort_by(|a, b| {
            a.last
                .is_none()
                .cmp(&b.last.is_none())
                .then_with(|| match (&a.last, &b.last) {
                    (Some(a), Some(b)) => a.age_seconds.total_cmp(&b.age_seconds),
                    _ => std::cmp::Ordering::Equal,
                })
                .then_with(|| a.id_unit.cmp(&b.id_unit))
        });
        Self::new(vessels)
    }
}
