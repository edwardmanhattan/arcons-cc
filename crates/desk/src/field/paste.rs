//! **Four forms for a pasted time** — what the operator reads off a report, and nothing else.
//!
//! Decision 3 is explicit that paste is a first-class input, and explicit about which shapes qualify:
//!
//! - `2026-11-20T14:03:22Z` — an RFC 3339 UTC stamp, what a log or an email header carries;
//! - `2026-11-20T14:03:22+07:00` — the same with an offset, what another system's clock says;
//! - `20/11/2026 14:03` — the day-first form people write on a plot;
//! - `14:03` — a bare time, meaning **today**, because that is what a signal report sounds like.
//!
//! The list is closed rather than generous. A parser that also accepts `20-11-2026`, `Nov 20 2026` or
//! `2:03 pm` has to guess which of day and month came first, and a wrong guess is a fix at the wrong time
//! that looks entirely plausible. Anything outside the four forms is refused with a message that repeats
//! them, so the operator learns the shape instead of hunting for the one the parser wanted.
//!
//! **The civil fields are kept as they were written.** `14:03:22+07:00` becomes 14:03:22 plus an offset,
//! not 07:03:22: the operator read 14:03:22 on the clock in front of them (WIB, per decision 3), and the
//! offset is what turns it into an instant. Seconds absent from the text are zero, not "now" — a report
//! that says 14:03 did not observe a second.
//!
//! RFC 3339's lowercase `t` and `z` are accepted as the same stamp; the standard allows them, and refusing
//! them would be our strictness rather than a decision. Fractional seconds are not accepted: none of the
//! sources here has that precision, and silently dropping a fraction would be a rounding the operator
//! never saw.

use std::fmt;

use super::CivilTime;
use super::civil;

/// The four forms, spelled once so every refusal can repeat them.
const FORMS: &str = concat!(
    "accepted forms: 2026-11-20T14:03:22Z, 2026-11-20T14:03:22+07:00, ",
    "20/11/2026 14:03, 14:03"
);

/// A pasted time: the civil fields as written, and the offset if the text carried one.
///
/// `offset_ms` is `None` for the two forms that carry no zone. That is not zero — `14:03` pasted on a desk
/// in Jakarta is not 14:03 UTC, and the caller resolves it through [`super::zoned`] rather than assuming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pasted {
    /// The wall-clock fields the operator read.
    pub civil: CivilTime,
    /// Milliseconds east of UTC, when the form carried one. `Some(0)` is an explicit `Z`, not an absence.
    pub offset_ms: Option<i32>,
}

/// Why a paste was refused. Every message ends with the four forms, because the fix is almost always
/// "paste it the other way" rather than "the date is wrong".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteError {
    /// The text has none of the four shapes.
    NotAForm,
    /// The shape is right but a slot that must be digits is not.
    NotANumber { field: &'static str },
    /// The fields are numbers but not a date in the proleptic Gregorian calendar.
    NoSuchDate { year: i32, month: u32, day: u32 },
    /// A clock or offset field is outside its range.
    OutOfRange {
        field: &'static str,
        value: u32,
        min: u32,
        max: u32,
    },
}

impl fmt::Display for PasteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAForm => write!(f, "not a time I can read; {FORMS}"),
            Self::NotANumber { field } => write!(f, "the {field} is not a number; {FORMS}"),
            Self::NoSuchDate { year, month, day } => {
                write!(f, "{day:02}/{month:02}/{year} is not a date; {FORMS}")
            }
            Self::OutOfRange {
                field,
                value,
                min,
                max,
            } => write!(f, "the {field} {value} is outside {min}–{max}; {FORMS}"),
        }
    }
}

impl std::error::Error for PasteError {}

/// Read one of the four forms, using `today` for the date of a bare time.
///
/// `today` is passed in rather than read from a clock: the desk's "today" is already decided by the
/// caller, and a parser that consults the OS is a parser whose result depends on the wall clock at the
/// instant of the keystroke.
pub fn parse(text: &str, today: CivilTime) -> Result<Pasted, PasteError> {
    let text = text.trim();
    if text.is_empty() || !text.is_ascii() {
        return Err(PasteError::NotAForm);
    }
    let bytes = text.as_bytes();
    // Shape first: a date in RFC 3339 has its fourth byte a hyphen and a `T` at ten, and those are the
    // only two positions that cannot be confused with the day-first form's slashes or a bare time.
    if bytes.len() > 10 && bytes[4] == b'-' && bytes[7] == b'-' && is_t(bytes[10]) {
        parse_rfc3339(text)
    } else if bytes.contains(&b'/') {
        parse_day_first(text)
    } else if bytes.contains(&b':') {
        parse_time_only(text, today)
    } else {
        Err(PasteError::NotAForm)
    }
}

fn is_t(byte: u8) -> bool {
    byte == b'T' || byte == b't'
}

fn parse_rfc3339(text: &str) -> Result<Pasted, PasteError> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 {
        return Err(PasteError::NotAForm);
    }
    let year = digits(bytes, 0..4, "year")?;
    let month = digits(bytes, 5..7, "month")?;
    let day = digits(bytes, 8..10, "day")?;
    let hour = digits(bytes, 11..13, "hour")?;
    let minute = digits(bytes, 14..16, "minute")?;
    let second = digits(bytes, 17..19, "second")?;
    let civil = checked(year as i32, month, day, hour, minute, second)?;
    let offset_ms = match bytes.len() {
        20 if bytes[19] == b'Z' || bytes[19] == b'z' => Some(0),
        25 if matches!(bytes[19], b'+' | b'-') && bytes[22] == b':' => {
            let hours = digits(bytes, 20..22, "offset hour")?;
            let minutes = digits(bytes, 23..25, "offset minute")?;
            if hours > 23 {
                return Err(PasteError::OutOfRange {
                    field: "offset hour",
                    value: hours,
                    min: 0,
                    max: 23,
                });
            }
            if minutes > 59 {
                return Err(PasteError::OutOfRange {
                    field: "offset minute",
                    value: minutes,
                    min: 0,
                    max: 59,
                });
            }
            let magnitude = (hours * 3_600 + minutes * 60) as i32 * 1_000;
            Some(if bytes[19] == b'-' {
                -magnitude
            } else {
                magnitude
            })
        }
        _ => return Err(PasteError::NotAForm),
    };
    Ok(Pasted { civil, offset_ms })
}

fn parse_day_first(text: &str) -> Result<Pasted, PasteError> {
    // Exactly `dd/mm/yyyy hh:mm`: two digits, a slash, two, a slash, four, a space, two, a colon, two.
    let bytes = text.as_bytes();
    if bytes.len() != 16
        || bytes[2] != b'/'
        || bytes[5] != b'/'
        || bytes[10] != b' '
        || bytes[13] != b':'
    {
        return Err(PasteError::NotAForm);
    }
    let day = digits(bytes, 0..2, "day")?;
    let month = digits(bytes, 3..5, "month")?;
    let year = digits(bytes, 6..10, "year")?;
    let hour = digits(bytes, 11..13, "hour")?;
    let minute = digits(bytes, 14..16, "minute")?;
    Ok(Pasted {
        civil: checked(year as i32, month, day, hour, minute, 0)?,
        offset_ms: None,
    })
}

fn parse_time_only(text: &str, today: CivilTime) -> Result<Pasted, PasteError> {
    let bytes = text.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' {
        return Err(PasteError::NotAForm);
    }
    let hour = digits(bytes, 0..2, "hour")?;
    let minute = digits(bytes, 3..5, "minute")?;
    Ok(Pasted {
        civil: checked(today.year, today.month, today.day, hour, minute, 0)?,
        offset_ms: None,
    })
}

/// The numeric value of one fixed-width slot, or `NotANumber` naming it.
fn digits(
    bytes: &[u8],
    range: std::ops::Range<usize>,
    field: &'static str,
) -> Result<u32, PasteError> {
    let slice = bytes.get(range).ok_or(PasteError::NotAForm)?;
    slice
        .iter()
        .try_fold(0u32, |value, &byte| {
            byte.is_ascii_digit()
                .then(|| value * 10 + u32::from(byte - b'0'))
        })
        .ok_or(PasteError::NotANumber { field })
}

/// The one range check all four forms share, so a pasted `31/02` and a typed one fail for the same
/// reason: the calendar, not the shape.
fn checked(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Result<CivilTime, PasteError> {
    if !(1..=12).contains(&month) {
        return Err(PasteError::OutOfRange {
            field: "month",
            value: month,
            min: 1,
            max: 12,
        });
    }
    if hour > 23 {
        return Err(PasteError::OutOfRange {
            field: "hour",
            value: hour,
            min: 0,
            max: 23,
        });
    }
    if minute > 59 {
        return Err(PasteError::OutOfRange {
            field: "minute",
            value: minute,
            min: 0,
            max: 59,
        });
    }
    if second > 59 {
        return Err(PasteError::OutOfRange {
            field: "second",
            value: second,
            min: 0,
            max: 59,
        });
    }
    if day < 1 || day > civil::days_in_month(year, month) {
        return Err(PasteError::NoSuchDate { year, month, day });
    }
    Ok(CivilTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    })
}
