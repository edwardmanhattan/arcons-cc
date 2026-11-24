//! **What a typed time will do** — one function, and every screen asks it (ADR-0009, decision 3).
//!
//! Minos decides by its own clock what a `recorded_at` means:
//!
//! | age against the server's clock | the fix is | the time |
//! | --- | --- | --- |
//! | within 60 s either side | **live** | replaced by the server's |
//! | 60 s to 30 days old | **backfill** | kept as recorded |
//! | older than 30 days | **refused** | counted, permanently |
//! | more than a few seconds ahead | **live, and logged** | replaced — so the desk blocks it |
//!
//! The last row is ours rather than Minos's: the server would accept a future time and overwrite it, which
//! silently turns "this is when we saw it" into "this is when we sent it". Blocking is the only honest
//! answer, and it is the one place the desk is stricter than the contract.
//!
//! Two details that are not decoration:
//!
//! - **No server clock, no verdict.** Until the server's clock is known — from a fix's
//!   `received_at + age_seconds` (or a response's `Date` header) — the verdict is [`Regime::Unknown`]; the
//!   device's own clock is not evidence about the server's.
//! - **A verdict near a boundary can flip** between the moment the line is drawn and the moment the request
//!   leaves. [`Verdict::near_boundary`] is true there, so the line says so instead of asserting.

use std::fmt;

/// Within this either side of the server's clock, a fix is live and its time is replaced.
pub const LIVE_WINDOW_MS: i64 = 60_000;

/// Older than this, a fix is refused, permanently.
pub const BACKFILL_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// How far ahead of the server's clock the desk tolerates without calling a time "the future". A few
/// seconds of clock skew is normal; a minute is a wrong clock.
pub const FUTURE_TOLERANCE_MS: i64 = 5_000;

/// How close to a boundary the verdict is worth hedging.
pub const NEAR_BOUNDARY_MS: i64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regime {
    /// The server replaces the typed time with its own. An honest "45 seconds ago" is lost here.
    Live,
    /// The typed time is kept, and the track it belongs to keeps the shape it was sailed.
    Backfill,
    /// Older than thirty days. No retry can fix an age.
    Refused,
    /// Ahead of the server. The server would re-stamp it as live, so the desk refuses to send it.
    Future,
    /// The server's clock is not known yet; no verdict is honest.
    Unknown,
}

impl Regime {
    /// What the line under the field says. One string per verdict, so the screen and a test agree.
    pub fn sentence(self) -> &'static str {
        match self {
            Self::Live => "live — the server replaces this time with its own",
            Self::Backfill => "backfill — kept as recorded",
            Self::Refused => "refused — older than 30 days",
            Self::Future => "in the future — fix the clock",
            Self::Unknown => "clock not checked — no verdict until the server answers",
        }
    }

    /// What a **card** says, where the sentence would be four lines: the verdict's name and nothing else.
    ///
    /// The field keeps [`Regime::sentence`] because it is read before a time is sent; a tray card is a
    /// glance, and a glance that has to be read left to right is not one.
    pub fn word(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Backfill => "backfill",
            Self::Refused => "refused",
            Self::Future => "in the future",
            Self::Unknown => "clock not checked",
        }
    }
}

impl fmt::Display for Regime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.sentence())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub regime: Regime,
    /// True within [`NEAR_BOUNDARY_MS`] of a line where the verdict changes. The line hedges: the answer can
    /// move between drawing it and sending it.
    pub near_boundary: bool,
}

impl Verdict {
    pub fn of(regime: Regime, age_ms: i64) -> Self {
        let near_boundary = regime != Regime::Unknown
            && [-FUTURE_TOLERANCE_MS, LIVE_WINDOW_MS, BACKFILL_WINDOW_MS]
                .iter()
                .any(|boundary| (age_ms - boundary).abs() < NEAR_BOUNDARY_MS);
        Self {
            regime,
            near_boundary,
        }
    }
}

/// What Minos will do with `recorded_at_ms`, read against the server's clock.
///
/// `age_ms` is `server_now_ms - recorded_at_ms`: positive is in the past, negative is in the future. A
/// `None` clock is not "now" — it is no answer at all.
pub fn verdict(recorded_at_ms: i64, server_now_ms: Option<i64>) -> Verdict {
    let Some(now) = server_now_ms else {
        return Verdict {
            regime: Regime::Unknown,
            near_boundary: false,
        };
    };
    let age = now - recorded_at_ms;
    let regime = if age < -FUTURE_TOLERANCE_MS {
        Regime::Future
    } else if age <= LIVE_WINDOW_MS {
        Regime::Live
    } else if age <= BACKFILL_WINDOW_MS {
        Regime::Backfill
    } else {
        Regime::Refused
    };
    Verdict::of(regime, age)
}
