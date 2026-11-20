//! **A vessel in the standing picture**, and how old its last position is.
//!
//! The picker lists every hull Minos returns, not a filtered reportable set: the contract publishes no such
//! set for a shore account, so a list that hid a vessel would be a guess wearing a permission's clothes
//! (ADR-0009, decision 4). A hull with `last: None` is one that has never reported; it stays in the list,
//! because on a desk whose job is to end silence, hiding it would make silence and non-existence look alike.

/// One hull's last position, as `GET /live-feed/positions` reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct LastKnown {
    /// Epoch milliseconds, in UTC.
    pub recorded_at_ms: i64,
    /// **Age at the moment the response was read**, not at the moment of the search.
    ///
    /// The list must not own a clock: a view that derived age from `recorded_at_ms` and `now` would make
    /// every redraw a new fact, and two panes would disagree about the same hull. The response's own age is
    /// the one Minos counted, and it is carried unchanged. It is `f64` because the contract's sample is;
    /// the unit is still seconds.
    pub age_seconds: f64,
    /// True when Minos kept the reported time rather than re-stamping it live. The operator's first
    /// question is *how stale*, and this is the other half of the answer.
    pub backfilled: bool,
    pub latitude: f64,
    pub longitude: f64,
    pub speed_kn: Option<f64>,
    pub course_deg: Option<f64>,
    pub accuracy_m: Option<f64>,
}

/// A hull the desk may report, whether or not it has ever reported.
#[derive(Debug, Clone, PartialEq)]
pub struct Vessel {
    pub id_unit: u64,
    pub name: String,
    /// The hull number as the picture gives it. Kept apart from the name because a hull number is the
    /// operator's strongest handle on a ship, and matching it well is how the list earns its speed.
    pub hull_number: String,
    /// `None` is a hull that has never reported — a visible state, never an empty position.
    pub last: Option<LastKnown>,
}

impl Vessel {
    /// Whether this hull's last known position is older than `max_age_seconds`.
    ///
    /// A **never-reported hull is not stale; it is silent**. There is no position whose age could have
    /// crossed the line, and calling silence "stale" would put it in the same bucket as a hull whose fix
    /// merely aged. Callers that ask "does this need attention" must ask both
    /// `last.is_none() || is_stale(...)`, because the two states need different sentences on screen.
    ///
    /// Exactly at the line is not stale: the comparison is strict, so a caller can name a window and get
    /// the same answer for the same age twice.
    pub fn is_stale(&self, max_age_seconds: f64) -> bool {
        self.last
            .as_ref()
            .is_some_and(|last| last.age_seconds > max_age_seconds)
    }
}
