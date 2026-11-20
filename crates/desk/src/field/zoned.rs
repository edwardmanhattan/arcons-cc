//! **A wall clock back to an instant** — the inverse `rune::LocalTime` does not give us (decision 3).
//!
//! `rune::LocalTime` answers "which fields and which offset does this instant read as here". The field
//! needs the other direction: the operator typed a wall clock, so which instants could it be? Usually
//! one. Twice a year the answer is none or two:
//!
//! - a **gap**: the clock jumped forward, so 02:30 on that date never happened and no instant wears it;
//! - a **fold**: the clock went back, so 02:30 happened twice and the two instants differ by the size of
//!   the jump.
//!
//! Both are facts about the zone, not errors the operator made, which is why the answer is a
//! [`Resolution`] with three shapes rather than an `Option`. A `None` would collapse "that time did not
//! exist" and "I could not work it out" into the same screen.
//!
//! The method is a **probe and a round trip**, not a zone database:
//!
//! 1. Ask the zone for its offset at a few instants near the civil time read as UTC: ±26 hours, and ±2
//!    hours. No real offset is more than 14 hours from UTC, so the civil time's true instant is inside
//!    ±14 h of that reading; ±26 h therefore samples the zone on both sides of any transition. The ±2 h
//!    probes are the second chance for a transition that sits between the far probes' answers.
//! 2. For each distinct offset, subtract it and ask the zone what offset that instant actually has. An
//!    offset that survives the round trip is a real reading of this wall clock.
//! 3. Zero survivors is a gap, one is `Unique`, two are `Ambiguous` ordered earliest first.
//!
//! `offset_at` is the zone, injected: epoch milliseconds in, milliseconds east of UTC out. That is what
//! makes this testable against a synthetic rule with no OS zone, no `TZ` environment and no `chrono-tz` —
//! and the same function is what `rune::LocalTime` will call once the crate can reach it. The convention
//! matches the wire: local = UTC + offset, so `instant = civil - offset`.

use super::CivilTime;
use super::civil;

const HOUR_MS: i64 = 3_600_000;

/// How far from the civil time read as UTC the zone is probed, in milliseconds.
///
/// Twenty-six hours is twice the largest offset in use (UTC+14), which is the guarantee that one probe is
/// before a transition and one after it for any transition the civil time could sit inside.
const FAR_PROBE_MS: i64 = 26 * HOUR_MS;

/// The near probes, in milliseconds. They catch a transition that falls between the far probes' answers
/// being equal — a zone whose rule changed twice in the window, or a jump narrower than the far probes
/// would resolve.
const NEAR_PROBE_MS: i64 = 2 * HOUR_MS;

/// What instants a civil time names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// The ordinary case: one wall clock reading, one instant.
    Unique { epoch_ms: i64, offset_ms: i32 },
    /// The clock jumped forward over this time, so it was never on a clock.
    Gap,
    /// The clock went back over this time, so it happened twice; `earlier_ms` is the first.
    Ambiguous { earlier_ms: i64, later_ms: i64 },
}

/// The instants that wear `civil` on a clock using `offset_at`.
///
/// `civil` is assumed to be a committed value — a date and time the field or a paste accepted. It is not
/// re-validated here: the only paths to it refuse 31 February, and a second validator would be a second
/// place for the calendar to be wrong. `offset_at` must be pure enough to answer the same instant the
/// same way twice; the round trip in step 2 depends on it.
pub fn resolve(civil: CivilTime, offset_at: impl Fn(i64) -> i32) -> Resolution {
    let civil_ms = civil_as_utc_ms(civil);
    let mut candidates: [i32; 4] = [0; 4];
    let mut found = 0;
    for probe in [-FAR_PROBE_MS, FAR_PROBE_MS, -NEAR_PROBE_MS, NEAR_PROBE_MS] {
        let offset = offset_at(civil_ms + probe);
        if !candidates[..found].contains(&offset) {
            candidates[found] = offset;
            found += 1;
        }
    }
    let mut answers: Vec<i64> = Vec::new();
    for &offset in &candidates[..found] {
        let epoch_ms = civil_ms - i64::from(offset);
        if offset_at(epoch_ms) == offset {
            answers.push(epoch_ms);
        }
    }
    answers.sort_unstable();
    answers.dedup();
    match answers.as_slice() {
        [] => Resolution::Gap,
        [only] => Resolution::Unique {
            epoch_ms: *only,
            offset_ms: offset_at(*only),
        },
        [earlier, later, ..] => Resolution::Ambiguous {
            earlier_ms: *earlier,
            later_ms: *later,
        },
    }
}

/// The civil fields read as if they were UTC. Not a claim about the zone — a coordinate to probe around.
fn civil_as_utc_ms(civil: CivilTime) -> i64 {
    civil::days_from_civil(civil.year, civil.month, civil.day) * 86_400_000
        + i64::from(civil.hour) * HOUR_MS
        + i64::from(civil.minute) * 60_000
        + i64::from(civil.second) * 1_000
}

/// The instant a civil time and a known offset name: `civil - offset`.
///
/// A paste that carried `+05:30` already says which offset it meant, so nothing has to be resolved
/// — but the conversion is still arithmetic, and it belongs beside [`resolve`] rather than in a
/// screen. `civil` is assumed committed, like [`resolve`]'s argument.
pub fn epoch_ms_of(civil: CivilTime, offset_ms: i32) -> i64 {
    civil_as_utc_ms(civil) - i64::from(offset_ms)
}

/// The civil time an instant reads as at `offset_ms` — [`epoch_ms_of`]'s inverse.
///
/// This is what turns a pasted `14:03:22+05:30` into the fields the field shows when the field's
/// zone is WIB, and what the readout beside the field would show for any other offset.
pub fn at_offset(epoch_ms: i64, offset_ms: i32) -> CivilTime {
    utc_civil(epoch_ms + i64::from(offset_ms))
}

/// The UTC fields of an instant.
///
/// Not a zone lookup: no offset is applied, and the result is the reading a UTC clock would show.
/// It is here, not in a view, because the derived readout `= 07:03:22Z` is the inverse of
/// [`epoch_ms_of`] and a screen that divided milliseconds itself would be a second calendar.
pub fn utc_civil(epoch_ms: i64) -> CivilTime {
    let days = epoch_ms.div_euclid(86_400_000);
    let in_day = epoch_ms.rem_euclid(86_400_000);
    let (year, month, day) = civil::civil_from_days(days);
    CivilTime {
        year,
        month,
        day,
        hour: (in_day / HOUR_MS) as u32,
        minute: (in_day % HOUR_MS / 60_000) as u32,
        second: (in_day % 60_000 / 1_000) as u32,
    }
}
