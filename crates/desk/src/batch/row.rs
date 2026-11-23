//! **A row** — one vessel, its last observation, and whether that can be sent.
//!
//! There is exactly one row per hull (ADR-0009, decision 2). The row never decides a regime on its own: it
//! asks [`crate::time::verdict`], the same function the field and the send ask, because two places deciding
//! live versus backfill is the failure the ADR names.

use crate::time::{Regime, verdict};

use super::fix::{Fix, FixError};

/// Why a row cannot be sent, or that it can.
///
/// One reason, not a list: the operator's next action is chosen by the first thing that is wrong. `Sendable`
/// is a verdict about this moment, not a promise — the clock can move before the request leaves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Readiness {
    Sendable,
    Blocked(Reason),
}

impl Readiness {
    pub fn is_sendable(self) -> bool {
        matches!(self, Self::Sendable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reason {
    /// Latitude or longitude is absent. A vessel that never reported opens this way, and so does a source
    /// that gave a time but not a place.
    NoPosition,
    /// Older than thirty days. No retry fixes an age.
    RefusedForAge,
    /// Ahead of the server by more than the measured skew, so the server would replace the time.
    FutureTime,
    /// The server's clock is not known, so no verdict about the time is honest.
    UnknownClock,
    /// The value itself cannot be a position, speed, course or accuracy.
    BadValue(FixError),
}

impl Reason {
    /// **Why the row stays, in the operator's words.** One sentence per reason, beside the reason itself, so
    /// the tray and a test cannot describe "no position" two different ways. The field is named rather than
    /// the whole value: the operator has one row to fix and the name says where to look.
    pub fn sentence(self) -> String {
        match self {
            Self::NoPosition => "both coordinates are needed".to_string(),
            Self::RefusedForAge => "older than 30 days, so Minos refuses it".to_string(),
            Self::FutureTime => "ahead of the server's clock".to_string(),
            Self::UnknownClock => "the server's clock has not answered".to_string(),
            Self::BadValue(error) => error.to_string(),
        }
    }
}

/// A vessel's one row in the batch.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id_unit: u64,
    /// Carried so the tray and the result can say which ship, not just which number.
    pub name: String,
    pub fix: Fix,
}

impl Row {
    /// What will happen if this row is sent now.
    ///
    /// The checks are ordered by what outranks what: a value that cannot mean anything outranks the
    /// clock, an unknown clock outranks the position — with no verdict every row is blocked, whatever else
    /// is wrong — and a missing position outranks an age, because a row with no coordinates has nothing to
    /// send even if its time is perfect.
    pub fn readiness(&self, server_now: Option<i64>) -> Readiness {
        if let Err(error) = self.fix.check() {
            return Readiness::Blocked(Reason::BadValue(error));
        }
        let regime = verdict(self.fix.recorded_at_ms, server_now).regime;
        if regime == Regime::Unknown {
            return Readiness::Blocked(Reason::UnknownClock);
        }
        if self.fix.latitude.is_none() || self.fix.longitude.is_none() {
            return Readiness::Blocked(Reason::NoPosition);
        }
        match regime {
            Regime::Live | Regime::Backfill => Readiness::Sendable,
            Regime::Refused => Readiness::Blocked(Reason::RefusedForAge),
            Regime::Future => Readiness::Blocked(Reason::FutureTime),
            Regime::Unknown => Readiness::Blocked(Reason::UnknownClock),
        }
    }
}
