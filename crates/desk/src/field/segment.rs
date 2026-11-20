//! **Six segments edited as digits** — the typing rules, ported without a DOM (ADR-0009, decision 5).
//!
//! The field shows `dd / mm / yyyy hh:mm:ss` and the operator moves through six real segments, not a
//! free-text box that parses on submit. That distinction is why these rules exist: the value is never a
//! string that might mean something else, and the failure is named while it is being typed.
//!
//! Three decisions worth stating, because they are what the tests pin:
//!
//! - **Digits fill right to left within a field.** Typing `2` then `0` into an empty day gives **20**, not
//!   `02` followed by a refused `0`. The newest digit is the least significant one, which is how a
//!   numeric segment reads and what the UI SDK's date input does. A digit that fills the field moves the
//!   cursor to the next one.
//! - **A step wraps inside its own field and never carries.** Stepping the hour from 23 gives 0 and leaves
//!   the day alone; stepping the day past the end of January gives 1 January, not 1 February. Carrying
//!   would change a field the operator did not point at, and on a form where the day is the thing being
//!   corrected that is a silent edit of the date.
//! - **Nothing is validated until [`Segments::commit`].** `31 / 02 / 2026` is a state the field can hold,
//!   because it is what the operator typed; commit is the one place that says no, and it names the field
//!   that is wrong rather than the whole value.
//!
//! The calendar arithmetic underneath is [`super::civil`]; the value produced is [`CivilTime`], which
//! still has no zone. Turning that into an instant is [`super::zoned`]'s job, not the field's.

use std::fmt;

use super::CivilTime;
use super::civil;

/// One of the six editable segments, in the order the field displays them.
///
/// The variants are declared year-first because that is how a date is spoken, but the cursor and
/// left/right walk the **displayed** order — day, month, year, hour, minute, second — because the
/// operator's arrowing must follow what is on screen. The mismatch is deliberate and is why the order
/// lives in `index` rather than in the enum's declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

impl Field {
    /// The number of digit slots: years have four, everything else has two.
    ///
    /// This is also the field's maximum value plus one digit of headroom — a four-slot year can hold 9999
    /// and a two-slot hour can hold 24, which commit then refuses. The width is the input mask, not the
    /// range.
    pub fn width(self) -> u32 {
        match self {
            Self::Year => 4,
            _ => 2,
        }
    }

    /// The field's name in words, so an error message and a label cannot disagree.
    pub fn name(self) -> &'static str {
        match self {
            Self::Year => "year",
            Self::Month => "month",
            Self::Day => "day",
            Self::Hour => "hour",
            Self::Minute => "minute",
            Self::Second => "second",
        }
    }

    /// Position in the displayed order, 0 (day) through 5 (second).
    fn index(self) -> usize {
        match self {
            Self::Day => 0,
            Self::Month => 1,
            Self::Year => 2,
            Self::Hour => 3,
            Self::Minute => 4,
            Self::Second => 5,
        }
    }

    /// The inverse of [`Field::index`], with 5 as the saturating end.
    fn at(index: usize) -> Self {
        match index {
            0 => Self::Day,
            1 => Self::Month,
            2 => Self::Year,
            3 => Self::Hour,
            4 => Self::Minute,
            _ => Self::Second,
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a committed field is not a time, with the field that has to change.
///
/// The field is a separate value rather than a sentence so the UI can put the mark on the right segment;
/// the sentence exists so a log line or a test can read the same decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldError {
    /// A segment still has empty digit slots. This is the state after `clear`, and it is not "zero":
    /// a year of `----` is missing, not the year zero.
    Incomplete { field: Field },
    /// The number is outside what the field can hold (hour 24, month 0, day 0).
    OutOfRange {
        field: Field,
        value: u32,
        min: u32,
        max: u32,
    },
    /// The day is the right shape but not a day of that month — 31 February, 29 February 2023.
    NoSuchDay { year: i32, month: u32, day: u32 },
}

impl FieldError {
    /// The segment that has to change, so the caller never has to parse the message.
    pub fn field(&self) -> Field {
        match self {
            Self::Incomplete { field } | Self::OutOfRange { field, .. } => *field,
            Self::NoSuchDay { .. } => Field::Day,
        }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete { field } => write!(f, "the {field} is not finished"),
            Self::OutOfRange {
                field,
                value,
                min,
                max,
            } => write!(f, "the {field} {value} is outside {min}–{max}"),
            Self::NoSuchDay { year, month, day } => {
                let length = civil::days_in_month(*year, *month);
                write!(
                    f,
                    "{day:02}/{month:02}/{year} is not a date: that month has {length} days"
                )
            }
        }
    }
}

impl std::error::Error for FieldError {}

/// The six segments and the cursor, holding digits rather than a parsed value.
///
/// Values are kept as numbers plus a count of digits typed, because that pair is exactly what the display
/// needs: `--` for an empty slot, zero-padded digits for a partial one. Holding a `String` would let a
/// non-digit in, and holding only a number would lose the difference between an empty day and `00`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segments {
    values: [u32; 6],
    digits: [u8; 6],
    active: Field,
}

impl Segments {
    /// Start from a civil time with every segment filled, cursor on the day.
    ///
    /// The day is first because it is the leftmost thing on screen and the part most often corrected; the
    /// value is not empty, so the operator can step from it without clearing it first.
    pub fn new(civil: CivilTime) -> Self {
        Self {
            values: [
                civil.day,
                civil.month,
                civil.year as u32,
                civil.hour,
                civil.minute,
                civil.second,
            ],
            digits: [
                Field::Day.width() as u8,
                Field::Month.width() as u8,
                Field::Year.width() as u8,
                Field::Hour.width() as u8,
                Field::Minute.width() as u8,
                Field::Second.width() as u8,
            ],
            active: Field::Day,
        }
    }

    /// Which segment the next digit goes into.
    pub fn active(&self) -> Field {
        self.active
    }

    /// The value a segment currently shows, or `None` when no digit has been typed into it.
    ///
    /// `None` is not zero: an empty day is a day the operator has not supplied, and treating it as the
    /// first of the month would silently commit a date they did not mean.
    pub fn value(&self, field: Field) -> Option<u32> {
        let index = field.index();
        (self.digits[index] > 0).then_some(self.values[index])
    }

    /// Type one digit into the active segment.
    ///
    /// Digits are placed from the right: the existing entry shifts left and its most significant digit
    /// falls off, so `2` then `0` reads 20. A segment that was already full is **replaced rather than
    /// patched**: its first keystroke starts a new entry. Patching would make a prefilled `20` impossible
    /// to type over as `31`, because the first digit would look like a finished field. Filling the segment
    /// moves the cursor to the next one. A value that is not a digit is ignored — the key handler is the
    /// only producer, and a panic there would take the window down for a keystroke.
    pub fn push_digit(&mut self, digit: u32) {
        if digit > 9 {
            return;
        }
        let field = self.active;
        let index = field.index();
        let width = field.width();
        if u32::from(self.digits[index]) == width {
            self.values[index] = 0;
            self.digits[index] = 0;
        }
        let shift = 10u32.pow(width - 1);
        self.values[index] = (self.values[index] % shift) * 10 + digit;
        self.digits[index] = (self.digits[index] + 1).min(width as u8);
        if u32::from(self.digits[index]) == width {
            self.right();
        }
    }

    /// Add `delta` to the active segment, wrapping inside that segment's own range.
    ///
    /// The day's range is the current month's length, so stepping February's day wraps at 28 or 29 rather
    /// than producing a day the month does not have. The wrapped value is always committed (the segment
    /// becomes full), because an operator who stepped is not midway through typing.
    pub fn step(&mut self, delta: i32) {
        let field = self.active;
        let index = field.index();
        let (min, max) = self.bounds(field);
        let span = i64::from(max - min + 1);
        let value = self.values[index].clamp(min, max);
        let offset = i64::from(value) - i64::from(min) + i64::from(delta);
        self.values[index] = (i64::from(min) + offset.rem_euclid(span)) as u32;
        self.digits[index] = field.width() as u8;
    }

    /// Move the cursor one segment left, stopping at the day.
    pub fn left(&mut self) {
        self.active = Field::at(self.active.index().saturating_sub(1));
    }

    /// Move the cursor one segment right, stopping at the second.
    pub fn right(&mut self) {
        self.active = Field::at((self.active.index() + 1).min(5));
    }

    /// Remove the last digit of the active segment; at an empty segment, move left.
    ///
    /// Backspace never deletes a whole segment in one press, because the value on screen is zero-padded:
    /// deleting `20` to `0` and then to empty is the visible sequence, and the empty state says "not
    /// supplied" rather than "midnight".
    pub fn backspace(&mut self) {
        let index = self.active.index();
        if self.digits[index] == 0 {
            self.left();
            return;
        }
        self.digits[index] -= 1;
        self.values[index] /= 10;
    }

    /// Empty every segment and put the cursor back on the day.
    ///
    /// Clear is all or nothing on purpose: a per-segment clear would leave a value that looks complete
    /// while one part of it came from a previous time.
    pub fn clear(&mut self) {
        self.values = [0; 6];
        self.digits = [0; 6];
        self.active = Field::Day;
    }

    /// The date line, `dd / mm / yyyy`, with `--` where a segment is still empty.
    pub fn date_text(&self) -> String {
        format!(
            "{} / {} / {}",
            self.render(Field::Day),
            self.render(Field::Month),
            self.render(Field::Year)
        )
    }

    /// The time line, `hh:mm:ss`, with `--` where a segment is still empty.
    pub fn time_text(&self) -> String {
        format!(
            "{}:{}:{}",
            self.render(Field::Hour),
            self.render(Field::Minute),
            self.render(Field::Second)
        )
    }

    /// Both lines on one line, as the field reads left to right: `dd / mm / yyyy hh:mm:ss`.
    pub fn text(&self) -> String {
        format!("{} {}", self.date_text(), self.time_text())
    }

    /// The value, if the six segments name a real date and time.
    ///
    /// Every refusal names the one segment to change. The day is checked against the month it is in
    /// rather than against 31, so February is a month of 28 days rather than a month that cannot take a
    /// 31st; the message says which length applied.
    pub fn commit(&self) -> Result<CivilTime, FieldError> {
        for field in [
            Field::Day,
            Field::Month,
            Field::Year,
            Field::Hour,
            Field::Minute,
            Field::Second,
        ] {
            if u32::from(self.digits[field.index()]) < field.width() {
                return Err(FieldError::Incomplete { field });
            }
        }
        let year = self.values[Field::Year.index()] as i32;
        let month = self.values[Field::Month.index()];
        let day = self.values[Field::Day.index()];
        if !(1..=12).contains(&month) {
            return Err(FieldError::OutOfRange {
                field: Field::Month,
                value: month,
                min: 1,
                max: 12,
            });
        }
        let length = civil::days_in_month(year, month);
        if day < 1 {
            return Err(FieldError::OutOfRange {
                field: Field::Day,
                value: day,
                min: 1,
                max: length,
            });
        }
        if day > length {
            return Err(FieldError::NoSuchDay { year, month, day });
        }
        let hour = self.values[Field::Hour.index()];
        if hour > 23 {
            return Err(FieldError::OutOfRange {
                field: Field::Hour,
                value: hour,
                min: 0,
                max: 23,
            });
        }
        for field in [Field::Minute, Field::Second] {
            let value = self.values[field.index()];
            if value > 59 {
                return Err(FieldError::OutOfRange {
                    field,
                    value,
                    min: 0,
                    max: 59,
                });
            }
        }
        Ok(CivilTime {
            year,
            month,
            day,
            hour,
            minute: self.values[Field::Minute.index()],
            second: self.values[Field::Second.index()],
        })
    }

    /// The range a step wraps inside. The day reads the month and year it currently sits in; an invalid
    /// month gives a one-value range instead of underflowing, because a step must not panic on the state
    /// the operator just typed.
    fn bounds(&self, field: Field) -> (u32, u32) {
        match field {
            Field::Year => (0, 9999),
            Field::Month => (1, 12),
            Field::Day => (1, civil::days_in_month(self.year(), self.month()).max(1)),
            Field::Hour => (0, 23),
            Field::Minute | Field::Second => (0, 59),
        }
    }

    fn year(&self) -> i32 {
        self.values[Field::Year.index()] as i32
    }

    fn month(&self) -> u32 {
        self.values[Field::Month.index()]
    }

    /// A segment as the display shows it: zero-padded digits, or one `-` per empty slot.
    fn render(&self, field: Field) -> String {
        let index = field.index();
        let width = field.width() as usize;
        if self.digits[index] == 0 {
            "-".repeat(width)
        } else {
            format!("{:0width$}", self.values[index])
        }
    }
}
