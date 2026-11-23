//! **The time field, without a window** — the parts of #276 that are decisions rather than pixels.
//!
//! ADR-0009, decision 5: the calendar and date-input semantics come from Vercel Labs' UI SDK as *ideas*
//! (`plain-date.ts`, `calendar-model.ts`, `date-input-model.ts`), ported to Rust. Nothing here knows what
//! a `TextInput` is, so every rule below is testable without a frame.
//!
//! The modules are the seams the UI SDK found, because those are the seams a change of renderer does not
//! move:
//!
//! - [`civil`] — a date as a day number, the arithmetic a grid and a paste both stand on;
//! - [`calendar`] — a month as rows of weeks, with no clock of its own;
//! - [`segment`] — the six pieces edited as digits, and the one place a typed time becomes a value;
//! - [`paste`] — the four forms an operator reads off a report;
//! - [`zoned`] — a wall clock back to an instant, where a DST gap has no answer and a fold has two
//!   (decision 3).
//!
//! [`CivilTime`] is the value they share: wall-clock fields with **no zone in them**. It is deliberately
//! not an instant. Two desks in two zones can hold the same `CivilTime` and mean two different moments,
//! which is exactly why the offset is part of the report (decision 3) and why the conversion lives in
//! [`zoned`] instead of in the value. Nothing in this module reads a clock; "now" and "today" are always
//! arguments, so a test can say what day it is.

use std::fmt;

pub mod calendar;
pub mod civil;
pub mod paste;
pub mod segment;
pub mod zoned;

pub use calendar::{Month, MonthError};
pub use paste::{PasteError, Pasted};
pub use segment::{Field, FieldError, Segments};
pub use zoned::{Resolution, resolve};

/// A wall-clock reading: the fields the operator sees, with no zone and no instant behind them.
///
/// This is what the field edits and what a paste produces. It does not validate its own fields — the two
/// places a human's input becomes a value ([`Segments::commit`] and [`paste::parse`]) are where an
/// impossible date is refused, so that the refusal can name the segment at fault. Constructing one
/// directly is for tests and for values those two already accepted.
///
/// The derived ordering is the calendar's: year, then month, then day, then clock. It is safe because
/// every field is compared in the order it is declared, and valid civil times compare chronologically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CivilTime {
    /// Proleptic Gregorian year; 0 is 1 BC, as ISO 8601 counts.
    pub year: i32,
    /// 1–12.
    pub month: u32,
    /// 1–31, but only up to the month's length when it came from a commit or a paste.
    pub day: u32,
    /// 0–23.
    pub hour: u32,
    /// 0–59.
    pub minute: u32,
    /// 0–59.
    pub second: u32,
}

impl fmt::Display for CivilTime {
    /// `dd/mm/yyyy hh:mm:ss`, the field's own order, which is what an operator reads back to check a
    /// paste. Not ISO 8601: that is the wire's format, produced by the sender, not by this display.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02}/{:02}/{:04} {:02}:{:02}:{:02}",
            self.day, self.month, self.year, self.hour, self.minute, self.second
        )
    }
}
