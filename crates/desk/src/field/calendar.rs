//! **A month laid out as weeks** — the calendar's shape, decided without a clock (ADR-0009, decision 5).
//!
//! There is no "today" in this module and no default month. A calendar that picks its own date when it
//! is opened cannot be tested, and the one thing the operator must see is the month they chose. So
//! [`Month`] is only ever constructed from a year and a month that already exist, or from a day number.
//!
//! Two layout rules are decisions rather than consequences:
//!
//! - **Monday first**, because that is the week the operator reads and the one the sheet prints.
//! - **No row that contains no day.** A five-row February and a four-row one are the truth; padding to a
//!   fixed six-row grid would draw two empty weeks and make a short month look like a broken one. The
//!   grid is built only from the days that are there, so a trailing empty row cannot occur — the test
//!   gives February 2021 (Monday start, twenty-eight days) as the witness that ends on a Sunday and
//!   still occupies exactly four rows.
//!
//! Months are `u32` in the same 1–12 range the segment field uses; `Month::new` refuses 0 and 13 instead
//! of storing a month the arithmetic below would quietly normalise. `shift` is the one operation that is
//! allowed to leave the range, and it wraps through the year in both directions.

use std::fmt;

use super::civil;

/// A year and a month, with the month guaranteed to be 1–12.
///
/// The fields are private because the guarantee is the type's reason to exist: a `Month { month: 13 }`
/// built by hand would make [`Month::weeks`] return an empty grid with nothing to say why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Month {
    year: i32,
    month: u32,
}

/// A month number outside 1–12, kept so the message can say which one was tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthError {
    month: u32,
}

impl MonthError {
    /// The month that was refused — `0` or `13`, or anything else outside the range.
    pub fn month(self) -> u32 {
        self.month
    }
}

impl fmt::Display for MonthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "month {} does not exist: a year has months 1 to 12",
            self.month
        )
    }
}

impl std::error::Error for MonthError {}

impl Month {
    /// The month starting at `year` and `month` — or a refusal, because there is no thirteenth month to
    /// show and no sensible grid for a month zero.
    pub fn new(year: i32, month: u32) -> Result<Self, MonthError> {
        if (1..=12).contains(&month) {
            Ok(Self { year, month })
        } else {
            Err(MonthError { month })
        }
    }

    /// The month containing a day number. Every day number names exactly one month, so this cannot fail;
    /// it is how a clicked cell becomes the month to open.
    pub fn of_days(days: i64) -> Self {
        let (year, month, _) = civil::civil_from_days(days);
        Self { year, month }
    }

    pub fn year(&self) -> i32 {
        self.year
    }

    pub fn month(&self) -> u32 {
        self.month
    }

    /// The month's days as rows of seven, Monday first. `None` is a cell before the first or after the
    /// last day of the month, and there is never a whole row of them.
    ///
    /// The rows are generated from the first day's weekday and the month's length, so the grid cannot
    /// contain a day that does not exist and cannot omit one. A month whose last day is a Sunday ends on
    /// the row that holds it.
    pub fn weeks(&self) -> Vec<[Option<u32>; 7]> {
        let first = civil::days_from_civil(self.year, self.month, 1);
        let leading = civil::weekday(first) as usize;
        let length = civil::days_in_month(self.year, self.month) as usize;
        let rows = (leading + length).div_ceil(7);
        let mut weeks = Vec::with_capacity(rows);
        let mut row = [None; 7];
        let mut column = leading;
        for day in 1..=length {
            row[column] = Some(day as u32);
            column += 1;
            if column == 7 {
                weeks.push(row);
                row = [None; 7];
                column = 0;
            }
        }
        if column != 0 {
            weeks.push(row);
        }
        weeks
    }

    /// Move `months` months forward (or backward, for a negative number), crossing year boundaries in
    /// either direction.
    ///
    /// The day is not part of a `Month`, so nothing needs clamping here: shifting January to February
    /// cannot turn the 31st into an invalid day because no day is stored. A caller holding a day decides
    /// what to do with it, and the segment field's commit is where "31 February" is refused.
    pub fn shift(self, months: i32) -> Self {
        let total = i64::from(self.year) * 12 + i64::from(self.month) - 1 + i64::from(months);
        let year = total.div_euclid(12);
        let month = total.rem_euclid(12) + 1;
        Self {
            year: year as i32,
            month: month as u32,
        }
    }
}
