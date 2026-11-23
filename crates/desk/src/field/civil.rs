//! **A date as one number** — proleptic Gregorian arithmetic with no clock, no zone and no crate.
//!
//! Exactness is the whole point. The calendar grid decides which day cell a click lands on, and a pasted
//! `20/11/2026` decides which day the operator meant; if those two paths disagree by one at a month
//! boundary, the field reports the wrong day and nothing on screen looks wrong. A **day number** — an
//! `i64` counting days from 1970-01-01 — is the one coordinate both can share, because it has no clock in
//! it to disagree about. Everything else here is a total function of that number.
//!
//! The algorithms are Howard Hinnant's `days_from_civil` / `civil_from_days`, which are exact over the
//! whole range we can represent: they use only integer division and a 400-year era, so there is no
//! table to get wrong and no rounding at a century. The calendar is **proleptic Gregorian** — the
//! Gregorian rules extended backwards before 1582 — because that is what an ISO-8601 timestamp means and
//! because a calendar that changes rules mid-range would give a day two numbers depending on how it was
//! reached.
//!
//! Years are `i32` and days are `u32`, matching the field's segments: `year` is at most four digits and a
//! month is never zero. Callers validate first (see [`super::segment::Segments::commit`]); these
//! functions are the arithmetic beneath that check, not a second one.

/// True when `year` has 366 days in the proleptic Gregorian calendar.
///
/// The century rule is not decoration: 1900 was not a leap year and 2000 was, and a wall clock holding
/// dates from old logs is exactly where the difference bites. The remainder is taken on the
/// mathematical value, so a negative year behaves the same as its positive twin.
pub fn is_leap(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// The number of days in `month` of `year`.
///
/// A month outside 1–12 has no length and returns 0 rather than a plausible-looking 31: the callers that
/// can see an out-of-range month (a typed `13`, a pasted one) refuse it before asking, and a caller that
/// forgot to gets a number that cannot pass a day check instead of a silent 31 January.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// The day number of `year-month-day`: 1970-01-01 is 0, the day before is -1.
///
/// Days outside the month's range are not refused — the formula counts linearly, so day 32 of January is
/// 1 February. That is deliberate: the only values that reach here have already been through a commit or
/// a parse, and a linear formula is what makes the inverse total.
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    // Shift January and February to the end of the previous year so the leap day is the last day of the
    // shifted year. That is what lets one 400-year era arithmetic cover every month without a table.
    let year = i64::from(year) - i64::from(month <= 2);
    let month = i64::from(month);
    let day = i64::from(day);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400); // 0..=399
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date `days` counts to, as `(year, month, day)`.
///
/// Total over every `i64` day number, and the exact inverse of [`days_from_civil`] for every date those
/// years can hold. The grid and a paste both use it, so a day cell and a typed date cannot drift apart by
/// rounding through a month length.
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097); // 0..=146096
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153; // 0..=11, March is 0
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

/// The weekday of a day number, with **Monday = 0** and Sunday = 6.
///
/// Monday-first is the grid's choice (the operator's week starts on Monday, as the field is laid out), so
/// the number is produced here rather than adjusted at the view. Day 0, 1970-01-01, was a Thursday, which
/// is why the offset is 3 and not 0.
pub fn weekday(days: i64) -> u32 {
    (days + 3).rem_euclid(7) as u32
}
