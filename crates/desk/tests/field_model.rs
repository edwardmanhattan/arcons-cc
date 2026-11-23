//! The date/time field's decisions, tested without a window (ADR-0009, decision 5).
//!
//! Each name is the claim the test makes. The calendar arithmetic is checked against itself over three
//! centuries, the segment rules against a literal sequence of keystrokes, paste against the four forms
//! decision 3 names, and `zoned` against a synthetic fixed rule — no OS zone, no `TZ`, no `chrono`.

use desk::field::CivilTime;
use desk::field::calendar::Month;
use desk::field::civil::{civil_from_days, days_from_civil, days_in_month, is_leap, weekday};
use desk::field::paste::{PasteError, parse};
use desk::field::segment::{Field, FieldError, Segments};
use desk::field::zoned::{Resolution, at_offset, epoch_ms_of, resolve, utc_civil};

fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> CivilTime {
    CivilTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// 2026-11-20 14:03:22, the afternoon this ticket was written.
fn sample() -> CivilTime {
    at(2026, 11, 20, 14, 3, 22)
}

// ---------------------------------------------------------------------------
// civil: day numbers
// ---------------------------------------------------------------------------

#[test]
fn the_epoch_is_day_zero_and_a_thursday() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(
        weekday(0),
        3,
        "Monday is 0, so Thursday 1 January 1970 is 3"
    );
}

#[test]
fn a_day_before_the_epoch_is_a_negative_number() {
    assert_eq!(days_from_civil(1969, 12, 31), -1);
    assert_eq!(civil_from_days(-1), (1969, 12, 31));
    assert_eq!(weekday(-1), 2, "31 December 1969 was a Wednesday");
}

#[test]
fn every_day_of_three_centuries_round_trips_through_its_number() {
    let first = days_from_civil(1600, 1, 1);
    let last = days_from_civil(2400, 12, 31);
    for days in first..=last {
        let (year, month, day) = civil_from_days(days);
        assert_eq!(
            days_from_civil(year, month, day),
            days,
            "{year}-{month:02}-{day:02} did not keep its day number"
        );
        assert!(
            (1..=12).contains(&month),
            "{year}-{month:02}-{day:02} came back with month {month}"
        );
        assert!(
            day >= 1 && day <= days_in_month(year, month),
            "{year}-{month:02}-{day:02} is not a day of its month"
        );
    }
}

#[test]
fn leap_years_are_the_gregorian_ones() {
    assert!(is_leap(2024), "2024 is a leap year");
    assert!(
        is_leap(2024),
        "and so was 2000, a century that is divisible by 400"
    );
    assert!(is_leap(2000));
    assert!(
        !is_leap(1900),
        "1900 was not: a century not divisible by 400"
    );
    assert!(!is_leap(2100));
    assert!(!is_leap(2023));
    assert!(
        is_leap(0),
        "year 0 is divisible by 400 in the proleptic calendar"
    );
    assert!(is_leap(-4), "a negative year follows the same rule");
}

#[test]
fn february_has_twenty_nine_days_only_in_a_leap_year() {
    assert_eq!(days_in_month(2024, 2), 29);
    assert_eq!(days_in_month(2023, 2), 28);
    assert_eq!(days_in_month(1900, 2), 28);
    assert_eq!(days_in_month(2000, 2), 29);
    assert_eq!(days_in_month(2026, 4), 30);
    assert_eq!(days_in_month(2026, 12), 31);
    assert_eq!(
        days_in_month(2026, 13),
        0,
        "a month that does not exist has no length, so a day check cannot pass by accident"
    );
}

#[test]
fn the_day_the_calendar_skipped_is_not_a_gap_in_the_numbers() {
    assert_eq!(
        days_from_civil(2000, 2, 29) - days_from_civil(2000, 2, 28),
        1,
        "29 February 2000 is the day after the 28th"
    );
    assert_eq!(
        civil_from_days(days_from_civil(1900, 2, 28) + 1),
        (1900, 3, 1),
        "1900 had no 29 February, and the next day number must still be the 1st of March"
    );
}

#[test]
fn weekdays_run_in_order_across_a_month_and_a_year() {
    let start = days_from_civil(2026, 12, 30);
    for days in start..start + 5 {
        assert_eq!(
            weekday(days + 1),
            (weekday(days) + 1) % 7,
            "day numbers {} and {} must be consecutive weekdays",
            days,
            days + 1
        );
    }
    assert_eq!(weekday(days_from_civil(2026, 11, 20)), 4, "a Friday");
}

// ---------------------------------------------------------------------------
// calendar: a month of weeks
// ---------------------------------------------------------------------------

#[test]
fn a_month_zero_or_thirteen_is_refused() {
    let zero = Month::new(2026, 0).expect_err("there is no month 0");
    assert_eq!(zero.month(), 0);
    let thirteen = Month::new(2026, 13).expect_err("there is no month 13");
    assert_eq!(thirteen.month(), 13);
    assert!(Month::new(2026, 1).is_ok());
    assert!(Month::new(2026, 12).is_ok());
}

#[test]
fn a_week_begins_on_monday() {
    let weeks = Month::new(2026, 11).expect("November exists").weeks();
    assert_eq!(
        weeks[0][6],
        Some(1),
        "1 November 2026 is a Sunday, so it is the last cell of the first row"
    );
    assert!(
        weeks[0][..6].iter().all(Option::is_none),
        "the six cells before it belong to October"
    );
}

#[test]
fn every_day_appears_once_in_its_own_column() {
    for (year, month) in [(2026, 1), (2026, 2), (2024, 2), (2026, 11), (2025, 12)] {
        let month_value = Month::new(year, month).expect("the month exists");
        let weeks = month_value.weeks();
        let days: Vec<u32> = weeks.iter().flatten().flatten().copied().collect();
        let expected: Vec<u32> = (1..=days_in_month(year, month)).collect();
        assert_eq!(days, expected, "{year}-{month:02} lost or reordered a day");
        for row in &weeks {
            if let Some(first) = row.iter().flatten().next() {
                let cell = row
                    .iter()
                    .position(|day| day == &Some(*first))
                    .expect("a cell");
                assert_eq!(
                    weekday(days_from_civil(year, month, *first)),
                    cell as u32,
                    "{year}-{month:02}-{first:02} sits under the wrong weekday"
                );
            }
        }
    }
}

#[test]
fn a_month_that_ends_on_sunday_adds_no_empty_row() {
    // February 2021 begins on a Monday and has 28 days: four full rows, and a fifth would be a row of
    // nothing. The grid is built from the days that exist, so it stops at the last one.
    let weeks = Month::new(2021, 2).expect("February exists").weeks();
    assert_eq!(weeks.len(), 4);
    assert!(
        weeks.iter().all(|row| row.iter().all(Option::is_some)),
        "every cell of February 2021 belongs to February"
    );
}

#[test]
fn a_short_month_is_five_rows_not_a_padded_six() {
    let weeks = Month::new(2026, 2).expect("February exists").weeks();
    assert_eq!(
        weeks.len(),
        5,
        "February 2026 starts on a Sunday and needs five rows, not a fixed six"
    );
    assert!(
        weeks.last().expect("a row").iter().flatten().count() > 0,
        "the last row holds at least one day"
    );
}

#[test]
fn shifting_a_month_crosses_the_year_in_both_directions() {
    let december = Month::new(2026, 12).expect("December exists");
    let january = december.shift(1);
    assert_eq!((january.year(), january.month()), (2027, 1));
    let january = Month::new(2026, 1).expect("January exists");
    let december = january.shift(-1);
    assert_eq!((december.year(), december.month()), (2025, 12));
    assert_eq!(
        january.shift(-13),
        Month::new(2024, 12).expect("December exists")
    );
    assert_eq!(
        january.shift(25),
        Month::new(2028, 2).expect("February exists")
    );
    assert_eq!(january.shift(0), january);
}

#[test]
fn a_month_can_be_named_by_any_day_number_inside_it() {
    for days in [
        days_from_civil(2026, 11, 1),
        days_from_civil(2026, 11, 20),
        days_from_civil(2026, 11, 30),
    ] {
        let month = Month::of_days(days);
        assert_eq!((month.year(), month.month()), (2026, 11));
    }
    assert_eq!(
        (Month::of_days(0).year(), Month::of_days(0).month()),
        (1970, 1)
    );
}

// ---------------------------------------------------------------------------
// segment: the six pieces
// ---------------------------------------------------------------------------

#[test]
fn typing_two_then_zero_into_an_empty_day_gives_twenty() {
    let mut segments = Segments::new(sample());
    segments.clear();
    segments.push_digit(2);
    assert_eq!(
        segments.value(Field::Day),
        Some(2),
        "one digit is a day of 2, not a refusal of '20' being impossible"
    );
    segments.push_digit(0);
    assert_eq!(segments.value(Field::Day), Some(20));
    assert_eq!(
        segments.active(),
        Field::Month,
        "a full segment moves the cursor on"
    );
    assert_eq!(segments.date_text(), "20 / -- / ----");
}

#[test]
fn typing_all_six_segments_produces_the_time_that_was_typed() {
    let mut segments = Segments::new(sample());
    segments.clear();
    for digit in [2, 0, 1, 1, 2, 0, 2, 6] {
        segments.push_digit(digit);
    }
    assert_eq!(
        segments.date_text(),
        "20 / 11 / 2026",
        "the date is complete"
    );
    assert_eq!(
        segments.active(),
        Field::Hour,
        "the cursor waits at the first time segment"
    );
    for digit in [1, 4, 0, 3, 2, 2] {
        segments.push_digit(digit);
    }
    assert_eq!(segments.time_text(), "14:03:22");
    assert_eq!(segments.text(), "20 / 11 / 2026 14:03:22");
    assert_eq!(segments.commit(), Ok(sample()));
}

#[test]
fn a_digit_typed_into_a_full_segment_starts_a_new_entry() {
    let mut segments = Segments::new(sample());
    segments.push_digit(5);
    assert_eq!(
        segments.value(Field::Day),
        Some(5),
        "typing over day 20 begins at 5, not at a patched 25 or 05"
    );
    assert_eq!(
        segments.active(),
        Field::Day,
        "one digit is not a finished segment, so the cursor stays"
    );
    segments.push_digit(0);
    assert_eq!(
        segments.value(Field::Day),
        Some(50),
        "the newest digit is the least significant one, so 5 then 0 is 50"
    );
    assert_eq!(segments.active(), Field::Month);
}

#[test]
fn a_key_that_is_not_a_digit_changes_nothing() {
    let mut segments = Segments::new(sample());
    let before = segments.text();
    segments.push_digit(10);
    assert_eq!(segments.text(), before);
    assert_eq!(segments.commit(), Ok(sample()));
}

#[test]
fn stepping_the_hour_wraps_it_without_carrying_the_day() {
    let mut segments = Segments::new(at(2026, 1, 31, 23, 59, 59));
    segments.right();
    segments.right();
    segments.right();
    assert_eq!(segments.active(), Field::Hour);
    segments.step(1);
    let committed = segments.commit().expect("the date is valid");
    assert_eq!(committed.hour, 0);
    assert_eq!(
        (committed.year, committed.month, committed.day),
        (2026, 1, 31),
        "stepping the hour must not change the day it sits in"
    );
    segments.step(-1);
    assert_eq!(segments.value(Field::Hour), Some(23));
}

#[test]
fn stepping_the_day_wraps_at_the_months_length() {
    let mut segments = Segments::new(at(2026, 1, 31, 12, 0, 0));
    segments.step(1);
    assert_eq!(
        segments.value(Field::Day),
        Some(1),
        "after the 31st of January the day segment wraps to its first value, not into February"
    );
    assert_eq!(segments.commit().expect("valid").month, 1);

    let mut february = Segments::new(at(2023, 2, 28, 12, 0, 0));
    february.step(1);
    assert_eq!(
        february.value(Field::Day),
        Some(1),
        "February 2023 has 28 days, so its day wraps there"
    );

    let mut leap = Segments::new(at(2024, 2, 29, 12, 0, 0));
    leap.step(1);
    assert_eq!(
        leap.value(Field::Day),
        Some(1),
        "29 February is the last day of a leap February"
    );
}

#[test]
fn stepping_the_month_wraps_inside_the_year_it_sits_in() {
    let mut segments = Segments::new(at(2026, 12, 15, 12, 0, 0));
    segments.right();
    segments.step(1);
    let committed = segments.commit().expect("valid");
    assert_eq!(committed.month, 1);
    assert_eq!(
        committed.year, 2026,
        "a step touches the segment it points at; a carry would silently change the year too"
    );
    segments.step(-1);
    assert_eq!(segments.value(Field::Month), Some(12));
}

#[test]
fn stepping_the_year_wraps_across_the_four_digit_range() {
    let mut segments = Segments::new(at(9999, 6, 15, 12, 0, 0));
    segments.right();
    segments.right();
    assert_eq!(segments.active(), Field::Year);
    segments.step(1);
    assert_eq!(segments.value(Field::Year), Some(0));
    segments.step(-1);
    assert_eq!(segments.value(Field::Year), Some(9999));
}

#[test]
fn the_cursor_walks_the_displayed_order_and_stops_at_the_ends() {
    let mut segments = Segments::new(sample());
    assert_eq!(segments.active(), Field::Day);
    segments.left();
    assert_eq!(segments.active(), Field::Day, "nothing is left of the day");
    let mut walked = Vec::new();
    for _ in 0..7 {
        segments.right();
        walked.push(segments.active());
    }
    assert_eq!(
        walked,
        vec![
            Field::Month,
            Field::Year,
            Field::Hour,
            Field::Minute,
            Field::Second,
            Field::Second,
            Field::Second,
        ],
        "left to right follows the text, and the second is the last segment"
    );
}

#[test]
fn backspace_eats_one_digit_at_a_time_and_then_moves_left() {
    let mut segments = Segments::new(sample());
    segments.backspace();
    assert_eq!(segments.date_text(), "02 / 11 / 2026");
    segments.backspace();
    assert_eq!(
        segments.date_text(),
        "-- / 11 / 2026",
        "a cleared segment says it is missing rather than showing 00"
    );
    segments.backspace();
    assert_eq!(
        segments.active(),
        Field::Day,
        "backspace at an empty leftmost segment has nowhere to go"
    );

    let mut month = Segments::new(sample());
    month.right();
    month.backspace();
    month.backspace();
    assert_eq!(month.value(Field::Month), None);
    month.backspace();
    assert_eq!(month.active(), Field::Day);
}

#[test]
fn clearing_leaves_placeholders_not_zeroes() {
    let mut segments = Segments::new(sample());
    segments.right();
    segments.clear();
    assert_eq!(segments.text(), "-- / -- / ---- --:--:--");
    assert_eq!(segments.active(), Field::Day);
    let error = segments.commit().expect_err("an empty field is not a time");
    assert_eq!(error.field(), Field::Day);
    assert!(error.to_string().contains("not finished"));
}

#[test]
fn a_month_of_thirteen_names_the_month() {
    let mut segments = Segments::new(sample());
    segments.right();
    segments.push_digit(1);
    segments.push_digit(3);
    let error = segments.commit().expect_err("13 is not a month");
    assert_eq!(error.field(), Field::Month);
    assert!(
        error.to_string().contains("month 13"),
        "the message must name the offending value: {error}"
    );
}

#[test]
fn the_thirty_first_of_february_names_the_day() {
    let segments = Segments::new(at(2026, 2, 31, 12, 0, 0));
    let error = segments.commit().expect_err("February has no 31st");
    assert_eq!(error.field(), Field::Day);
    assert_eq!(
        error,
        FieldError::NoSuchDay {
            year: 2026,
            month: 2,
            day: 31
        }
    );
    assert!(
        error.to_string().contains("31/02/2026"),
        "the message must show the date that does not exist: {error}"
    );
}

#[test]
fn february_twenty_nine_exists_only_in_a_leap_year() {
    assert_eq!(
        Segments::new(at(2024, 2, 29, 12, 0, 0)).commit(),
        Ok(at(2024, 2, 29, 12, 0, 0))
    );
    let error = Segments::new(at(2023, 2, 29, 12, 0, 0))
        .commit()
        .expect_err("2023 has no 29 February");
    assert_eq!(error.field(), Field::Day);
}

#[test]
fn an_hour_of_twenty_four_names_the_hour() {
    let error = Segments::new(at(2026, 11, 20, 24, 0, 0))
        .commit()
        .expect_err("the clock reaches 23 and no further");
    assert_eq!(error.field(), Field::Hour);
}

#[test]
fn a_minute_or_second_of_sixty_names_that_segment() {
    assert_eq!(
        Segments::new(at(2026, 11, 20, 12, 60, 0))
            .commit()
            .expect_err("no minute 60")
            .field(),
        Field::Minute
    );
    assert_eq!(
        Segments::new(at(2026, 11, 20, 12, 0, 60))
            .commit()
            .expect_err("no second 60")
            .field(),
        Field::Second
    );
}

#[test]
fn a_partly_typed_segment_is_not_read_as_its_digits() {
    let mut segments = Segments::new(sample());
    segments.clear();
    segments.push_digit(2);
    let error = segments.commit().expect_err("one digit is half a day");
    assert_eq!(error.field(), Field::Day);
    assert_eq!(segments.value(Field::Month), None);
}

#[test]
fn a_segment_can_be_chosen_without_walking_to_it() {
    let mut segments = Segments::new(sample());
    segments.select(Field::Second);
    assert_eq!(segments.active(), Field::Second);
    segments.select(Field::Year);
    segments.step(1);
    assert_eq!(segments.value(Field::Year), Some(2027));
}

#[test]
fn a_segment_can_be_given_a_whole_value_and_reads_zero_padded() {
    let mut segments = Segments::new(sample());
    segments.set(Field::Day, 5);
    segments.set(Field::Month, 3);
    segments.set(Field::Year, 2027);
    assert_eq!(segments.date_text(), "05 / 03 / 2027");
    assert_eq!(
        segments.segment_text(Field::Day),
        "05",
        "a segment is one slot per digit, and the display draws it that way"
    );
    assert_eq!(segments.commit().expect("valid"), at(2027, 3, 5, 14, 3, 22));
}

#[test]
fn an_empty_segment_reads_as_placeholders_not_zeroes() {
    let mut segments = Segments::new(sample());
    segments.clear();
    assert_eq!(segments.segment_text(Field::Year), "----");
    assert_eq!(segments.segment_text(Field::Minute), "--");
}

#[test]
fn a_month_is_named_in_words() {
    assert_eq!(Month::new(2026, 11).expect("November").name(), "November");
    assert_eq!(Month::new(2026, 1).expect("January").name(), "January");
    assert_eq!(Month::new(2026, 12).expect("December").name(), "December");
}

// ---------------------------------------------------------------------------
// paste: the four forms, and nothing else
// ---------------------------------------------------------------------------

#[test]
fn an_rfc3339_zulu_time_is_utc() {
    let pasted = parse("2026-11-20T14:03:22Z", sample()).expect("the form is accepted");
    assert_eq!(pasted.civil, sample());
    assert_eq!(
        pasted.offset_ms,
        Some(0),
        "an explicit Z is an offset of zero, not an absent one"
    );
}

#[test]
fn an_offset_keeps_the_wall_clock_it_was_read_in() {
    let pasted = parse("2026-11-20T14:03:22+07:00", sample()).expect("the form is accepted");
    assert_eq!(
        pasted.civil,
        sample(),
        "the operator read 14:03:22 on the +07:00 clock"
    );
    assert_eq!(pasted.offset_ms, Some(7 * 3_600_000));
}

#[test]
fn a_negative_offset_is_kept_with_its_sign() {
    let pasted = parse("2026-11-20T14:03:22-05:00", sample()).expect("the form is accepted");
    assert_eq!(pasted.offset_ms, Some(-5 * 3_600_000));
    let half = parse("2026-11-20T14:03:22+05:30", sample()).expect("half-hour zones exist");
    assert_eq!(half.offset_ms, Some(5 * 3_600_000 + 1_800_000));
}

#[test]
fn the_day_first_form_carries_no_offset_and_no_seconds() {
    let pasted = parse("20/11/2026 14:03", sample()).expect("the form is accepted");
    assert_eq!(pasted.civil, at(2026, 11, 20, 14, 3, 0));
    assert_eq!(
        pasted.offset_ms, None,
        "the text named no zone, so the caller must resolve one"
    );
}

#[test]
fn a_bare_time_is_today() {
    let today = at(2026, 11, 20, 23, 59, 0);
    let pasted = parse("14:03", today).expect("the form is accepted");
    assert_eq!(pasted.civil, at(2026, 11, 20, 14, 3, 0));
    assert_eq!(pasted.offset_ms, None);
}

#[test]
fn surrounding_whitespace_is_not_part_of_the_time() {
    assert_eq!(
        parse("  14:03  ", sample()).expect("trimmed"),
        parse("14:03", sample()).expect("untrimmed")
    );
}

#[test]
fn lowercase_rfc3339_markers_are_the_same_stamp() {
    assert_eq!(
        parse("2026-11-20t14:03:22z", sample()).expect("RFC 3339 allows lowercase"),
        parse("2026-11-20T14:03:22Z", sample()).expect("the canonical form")
    );
}

#[test]
fn anything_else_is_refused_with_the_four_forms_in_the_message() {
    for text in [
        "",
        "yesterday",
        "2026-11-20",
        "20-11-2026 14:03",
        "2026-11-20T14:03",
        "14:03:22",
        "2026-11-20T14:03:22.5Z",
        "2026-11-20T14:03:22+0700",
    ] {
        let error = parse(text, sample()).expect_err("this is not one of the four forms");
        let message = error.to_string();
        assert!(
            matches!(error, PasteError::NotAForm),
            "{text:?} should be refused as a shape: {error:?}"
        );
        assert!(
            message.contains("2026-11-20T14:03:22Z")
                && message.contains("2026-11-20T14:03:22+07:00")
                && message.contains("20/11/2026 14:03")
                && message.contains("14:03"),
            "the refusal must name the four forms: {message}"
        );
    }
}

#[test]
fn an_impossible_date_is_refused_although_the_shape_is_right() {
    for (text, day, month, year) in [
        ("2026-02-30T10:00:00Z", 30, 2, 2026),
        ("31/04/2026 10:00", 31, 4, 2026),
        ("29/02/2023 10:00", 29, 2, 2023),
    ] {
        let error = parse(text, sample()).expect_err("that date does not exist");
        assert_eq!(error, PasteError::NoSuchDate { year, month, day });
        assert!(
            error.to_string().contains("is not a date"),
            "the refusal must say it is the date: {error}"
        );
    }
}

#[test]
fn an_impossible_clock_or_offset_is_refused() {
    for text in [
        "2026-11-20T24:00:00Z",
        "2026-11-20T14:60:00Z",
        "2026-11-20T14:03:60Z",
        "20/11/2026 24:00",
        "2026-11-20T14:03:22+24:00",
    ] {
        assert!(
            parse(text, sample()).is_err(),
            "{text:?} is not a time on a clock"
        );
    }
    assert!(
        parse("2026-00-20T14:03:22Z", sample()).is_err(),
        "there is no month 0"
    );
}

// ---------------------------------------------------------------------------
// zoned: a wall clock back to an instant
// ---------------------------------------------------------------------------

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;
const WINTER_OFFSET_MS: i32 = 3_600_000;
const SUMMER_OFFSET_MS: i32 = 7_200_000;

fn instant(year: i32, month: u32, day: u32, hour: u32) -> i64 {
    days_from_civil(year, month, day) * DAY_MS + i64::from(hour) * HOUR_MS
}

/// A fixed synthetic rule with one spring transition and one autumn one, both at 01:00 UTC:
/// +01:00 before 2026-03-29, +02:00 until 2026-10-25, +01:00 after. It is the shape of a DST zone
/// without a zone database — and the same function the OS would answer through `rune::LocalTime`.
fn synthetic(epoch_ms: i64) -> i32 {
    if (instant(2026, 3, 29, 1)..instant(2026, 10, 25, 1)).contains(&epoch_ms) {
        SUMMER_OFFSET_MS
    } else {
        WINTER_OFFSET_MS
    }
}

fn only(resolution: Resolution) -> (i64, i32) {
    match resolution {
        Resolution::Unique {
            epoch_ms,
            offset_ms,
        } => (epoch_ms, offset_ms),
        other => panic!("expected one instant, got {other:?}"),
    }
}

#[test]
fn an_ordinary_wall_clock_names_one_instant() {
    let (epoch_ms, offset_ms) = only(resolve(at(2026, 11, 20, 14, 3, 22), synthetic));
    assert_eq!(offset_ms, WINTER_OFFSET_MS);
    assert_eq!(
        epoch_ms,
        instant(2026, 11, 20, 14) - i64::from(WINTER_OFFSET_MS) + 3 * 60_000 + 22 * 1_000,
        "14:03:22 at +01:00 is 13:03:22 UTC"
    );
}

#[test]
fn a_spring_forward_gap_has_no_answer() {
    // At 01:00 UTC the clock jumps from 02:00 to 03:00, so local 02:30 never happened.
    assert_eq!(
        resolve(at(2026, 3, 29, 2, 30, 0), synthetic),
        Resolution::Gap
    );
}

#[test]
fn the_minutes_on_either_side_of_a_gap_still_have_answers() {
    let (before, before_offset) = only(resolve(at(2026, 3, 29, 1, 59, 0), synthetic));
    assert_eq!(before_offset, WINTER_OFFSET_MS);
    assert_eq!(
        before + i64::from(before_offset),
        instant(2026, 3, 29, 1) + 59 * 60_000
    );

    let (after, after_offset) = only(resolve(at(2026, 3, 29, 3, 0, 0), synthetic));
    assert_eq!(after_offset, SUMMER_OFFSET_MS);
    assert_eq!(after + i64::from(after_offset), instant(2026, 3, 29, 3));
}

#[test]
fn an_autumn_fold_has_two_answers_earliest_first() {
    match resolve(at(2026, 10, 25, 2, 30, 0), synthetic) {
        Resolution::Ambiguous {
            earlier_ms,
            later_ms,
        } => {
            assert_eq!(
                earlier_ms,
                instant(2026, 10, 25, 0) + 1_800_000,
                "the first 02:30 was on the summer clock"
            );
            assert_eq!(
                later_ms,
                instant(2026, 10, 25, 1) + 1_800_000,
                "the second was on the winter clock"
            );
            assert!(earlier_ms < later_ms, "earlier must mean earlier");
            assert_eq!(synthetic(earlier_ms), SUMMER_OFFSET_MS);
            assert_eq!(synthetic(later_ms), WINTER_OFFSET_MS);
        }
        other => panic!("expected two instants, got {other:?}"),
    }
}

#[test]
fn the_two_answers_of_a_fold_are_one_hour_apart() {
    if let Resolution::Ambiguous {
        earlier_ms,
        later_ms,
    } = resolve(at(2026, 10, 25, 2, 15, 0), synthetic)
    {
        assert_eq!(
            later_ms - earlier_ms,
            HOUR_MS,
            "the fold is an hour wide, and the difference must be exactly that"
        );
    } else {
        panic!("02:15 on the fold day happened twice");
    }
}

#[test]
fn a_zone_without_a_transition_always_answers_once() {
    // WIB: +07:00 all year, as Indonesia has no DST. The inverse is plain subtraction.
    let jakarta = |_: i64| 7 * HOUR_MS as i32;
    let (epoch_ms, offset_ms) = only(resolve(sample(), jakarta));
    assert_eq!(offset_ms, 7 * HOUR_MS as i32);
    assert_eq!(
        epoch_ms + i64::from(offset_ms),
        instant(2026, 11, 20, 14) + 3 * 60_000 + 22 * 1_000
    );
}

#[test]
fn a_half_hour_zone_is_offset_by_a_half_hour() {
    let kolkata = |_: i64| 5 * HOUR_MS as i32 + 1_800_000;
    let (epoch_ms, _) = only(resolve(sample(), kolkata));
    assert_eq!(
        epoch_ms,
        instant(2026, 11, 20, 14) + 3 * 60_000 + 22 * 1_000 - 5 * HOUR_MS - 1_800_000,
        "an offset is not assumed to be a whole number of hours"
    );
}

#[test]
fn the_answer_does_not_depend_on_a_clock_being_read() {
    // The same civil time resolved twice with the same rule is the same answer, and a rule that counts
    // its calls still returns a time because `resolve` is a function of its arguments only.
    let before = only(resolve(at(2026, 6, 1, 12, 0, 0), synthetic));
    let after = only(resolve(at(2026, 6, 1, 12, 0, 0), synthetic));
    assert_eq!(before, after);
}

#[test]
fn an_instant_reads_as_its_utc_fields() {
    let recorded = at(2026, 11, 20, 14, 3, 22);
    let instant = epoch_ms_of(recorded, 7 * 3_600_000);
    assert_eq!(utc_civil(instant), at(2026, 11, 20, 7, 3, 22));
    assert_eq!(
        utc_civil(-1),
        at(1969, 12, 31, 23, 59, 59),
        "an instant before the epoch must floor, not truncate toward zero"
    );
}

#[test]
fn an_offset_moves_a_pasted_time_between_zones() {
    let in_india = at(2026, 11, 20, 14, 3, 22);
    let instant = epoch_ms_of(in_india, 5 * 3_600_000 + 1_800_000);
    assert_eq!(
        at_offset(instant, 5 * 3_600_000 + 1_800_000),
        in_india,
        "a time read at its own offset comes back unchanged"
    );
    assert_eq!(
        at_offset(instant, 7 * 3_600_000),
        at(2026, 11, 20, 15, 33, 22),
        "14:03:22 at +05:30 is 15:33:22 on the WIB clock the field shows"
    );
}
