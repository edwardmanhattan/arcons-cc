//! The batch's rules, without a window: one row per hull, one request per row, and one reason a row stays.
//!
//! These are ADR-0009's decisions 2, 4 and 6 as claims. The tray and the picker render them; they do not
//! decide them.

use std::collections::HashSet;

use desk::batch::{Batch, Fix, FixError, Readiness, Reason, Row};
use desk::time::BACKFILL_WINDOW_MS;

const NOW: i64 = 1_700_000_000_000;

/// WIB, the operator's clock, as the desk stores it.
const OFFSET_MS: i32 = 7 * 60 * 60 * 1000;

fn fix_at(recorded_at_ms: i64) -> Fix {
    Fix {
        recorded_at_ms,
        offset_ms: OFFSET_MS,
        latitude: Some(-6.2),
        longitude: Some(106.8),
        speed_kn: None,
        course_deg: None,
        accuracy_m: None,
    }
}

fn row(id_unit: u64, recorded_at_ms: i64) -> Row {
    Row {
        id_unit,
        name: format!("Hull {id_unit}"),
        fix: fix_at(recorded_at_ms),
    }
}

#[test]
fn inserting_the_same_hull_twice_leaves_one_row_and_returns_the_displaced_one() {
    let mut batch = Batch::new();
    let first = row(7, NOW);

    assert!(
        batch.insert(first.clone()).is_none(),
        "the first row for a hull displaces nothing"
    );

    let second = Row {
        name: "Renamed".to_string(),
        fix: fix_at(NOW - 1_000),
        ..row(7, NOW)
    };
    let displaced = batch
        .insert(second.clone())
        .expect("a hull's second row must displace the first, not become a second row");

    assert_eq!(displaced, first, "the displaced row is what was there");
    assert_eq!(batch.len(), 1, "a hull never contributes two rows");
    assert_eq!(batch.get(7), Some(&second), "the new row is the one held");
    assert_eq!(batch.rows(), std::slice::from_ref(&second));
}

#[test]
fn a_row_without_a_position_is_blocked_and_absent_from_the_plan() {
    let mut batch = Batch::new();
    let mut no_latitude = row(3, NOW);
    no_latitude.fix.latitude = None;
    let mut no_longitude = row(4, NOW);
    no_longitude.fix.longitude = None;
    batch.insert(no_latitude);
    batch.insert(no_longitude);

    for id_unit in [3, 4] {
        assert_eq!(
            batch
                .get(id_unit)
                .expect("the row is in the batch")
                .readiness(Some(NOW)),
            Readiness::Blocked(Reason::NoPosition),
            "half a position is no position"
        );
    }

    let plan = batch.plan(Some(NOW));
    assert!(
        plan.sendable.is_empty(),
        "a fix with no coordinate cannot be sent"
    );
    assert_eq!(
        plan.blocked,
        vec![(3, Reason::NoPosition), (4, Reason::NoPosition)],
        "both rows stay on the board with the reason they stay"
    );
}

#[test]
fn a_hull_that_never_reported_becomes_sendable_only_once_both_coordinates_and_a_time_exist() {
    let mut batch = Batch::new();
    let empty = Row {
        fix: Fix {
            latitude: None,
            longitude: None,
            ..fix_at(NOW)
        },
        ..row(11, NOW)
    };
    batch.insert(empty);
    assert_eq!(
        batch.get(11).expect("row").readiness(Some(NOW)),
        Readiness::Blocked(Reason::NoPosition),
        "a hull that never reported opens empty, and an empty row is not sendable"
    );

    let mut half = row(11, NOW);
    half.fix.longitude = None;
    batch.insert(half);
    assert_eq!(
        batch.get(11).expect("row").readiness(Some(NOW)),
        Readiness::Blocked(Reason::NoPosition),
        "one coordinate is not a position"
    );

    batch.insert(row(11, NOW));
    assert_eq!(
        batch.get(11).expect("row").readiness(Some(NOW)),
        Readiness::Sendable,
        "both coordinates and a live time are the whole requirement"
    );
    let plan = batch.plan(Some(NOW));
    assert_eq!(plan.sendable.len(), 1);
    assert_eq!(plan.sendable[0].id_unit, 11);
}

#[test]
fn thirty_days_exactly_is_sendable_while_thirty_days_and_a_millisecond_is_blocked_for_age() {
    let mut batch = Batch::new();
    batch.insert(row(21, NOW - BACKFILL_WINDOW_MS));
    assert_eq!(
        batch.get(21).expect("row").readiness(Some(NOW)),
        Readiness::Sendable,
        "the window is inclusive at thirty days, so the last reportable second is reportable"
    );

    batch.insert(row(22, NOW - BACKFILL_WINDOW_MS - 1));
    assert_eq!(
        batch.get(22).expect("row").readiness(Some(NOW)),
        Readiness::Blocked(Reason::RefusedForAge),
        "one millisecond older is a permanent refusal, and no retry fixes an age"
    );

    let plan = batch.plan(Some(NOW));
    assert_eq!(plan.sendable.len(), 1);
    assert_eq!(plan.sendable[0].id_unit, 21);
    assert_eq!(plan.blocked, vec![(22, Reason::RefusedForAge)]);
}

#[test]
fn two_seconds_ahead_is_sendable_while_ninety_seconds_ahead_is_blocked_as_future() {
    let mut batch = Batch::new();
    batch.insert(row(31, NOW + 2_000));
    assert_eq!(
        batch.get(31).expect("row").readiness(Some(NOW)),
        Readiness::Sendable,
        "a few seconds of clock skew is normal, and the server re-stamps it live"
    );

    batch.insert(row(32, NOW + 90_000));
    assert_eq!(
        batch.get(32).expect("row").readiness(Some(NOW)),
        Readiness::Blocked(Reason::FutureTime),
        "a minute and a half ahead is a wrong clock, and the server would overwrite the honest time"
    );

    let plan = batch.plan(Some(NOW));
    assert_eq!(plan.sendable.len(), 1);
    assert_eq!(plan.sendable[0].id_unit, 31);
    assert_eq!(plan.blocked, vec![(32, Reason::FutureTime)]);
}

#[test]
fn an_unknown_server_clock_blocks_everything_with_unknown_clock() {
    let mut batch = Batch::new();
    batch.insert(row(41, NOW));
    batch.insert(row(42, NOW - 40 * 24 * 60 * 60 * 1000));
    let mut no_position = row(43, NOW);
    no_position.fix.latitude = None;
    batch.insert(no_position);

    for id_unit in [41, 42, 43] {
        assert_eq!(
            batch.get(id_unit).expect("row").readiness(None),
            Readiness::Blocked(Reason::UnknownClock),
            "without the server's clock no verdict is honest, whatever else the row holds"
        );
    }

    let plan = batch.plan(None);
    assert!(plan.sendable.is_empty());
    assert_eq!(
        plan.blocked,
        vec![
            (41, Reason::UnknownClock),
            (42, Reason::UnknownClock),
            (43, Reason::UnknownClock),
        ]
    );
}

#[test]
fn five_sendable_rows_produce_five_sendables_with_five_distinct_id_units() {
    let mut batch = Batch::new();
    for id_unit in 51..=55 {
        batch.insert(row(id_unit, NOW));
    }

    let plan = batch.plan(Some(NOW));
    assert_eq!(
        plan.sendable.len(),
        5,
        "N vessels means N requests, one id_unit each"
    );
    assert!(plan.blocked.is_empty());

    let ids: HashSet<u64> = plan
        .sendable
        .iter()
        .map(|sendable| sendable.id_unit)
        .collect();
    assert_eq!(
        ids.len(),
        5,
        "each request names one vessel, and none repeats"
    );
    for sendable in &plan.sendable {
        assert_eq!(
            sendable.fix,
            batch.get(sendable.id_unit).expect("row").fix,
            "a sendable carries that vessel's one fix, not a copy of the batch"
        );
    }
}

#[test]
fn five_thousand_six_hundred_sixty_six_knots_is_refused_naming_speed_kn() {
    let mut fast = fix_at(NOW);
    fast.speed_kn = Some(5666.0);
    let error = fast.check().expect_err("5666 knots is not a vessel");
    assert_eq!(
        error.field(),
        "speed_kn",
        "a refusal names the field to fix"
    );
    assert!(matches!(error, FixError::SpeedKn { .. }));
    assert!(
        error.to_string().contains("speed_kn"),
        "the message says which number is impossible: {error}"
    );

    let mut batch = Batch::new();
    batch.insert(Row {
        id_unit: 61,
        name: "Implausible".to_string(),
        fix: fast,
    });
    assert_eq!(
        batch.get(61).expect("row").readiness(Some(NOW)),
        Readiness::Blocked(Reason::BadValue(error))
    );
    let plan = batch.plan(Some(NOW));
    assert!(
        plan.sendable.is_empty(),
        "a typo must not become a fact on the live picture"
    );
    assert_eq!(plan.blocked, vec![(61, Reason::BadValue(error))]);
}

#[test]
fn the_source_note_is_carried_on_the_batch_and_appears_nowhere_in_the_plan() {
    let note = "Plot 4 · phone call 14:03";
    let mut batch = Batch::new();
    batch.insert(row(71, NOW));
    batch.source_note = Some(note.to_string());

    assert_eq!(
        batch.source_note.as_deref(),
        Some(note),
        "the desk keeps how it knew; the payload has no field for it"
    );

    let plan = batch.plan(Some(NOW));
    assert_eq!(plan.sendable.len(), 1);
    let rendered = format!("{plan:?}");
    assert!(
        !rendered.contains(note),
        "the note is the desk's own record and is never sent: {rendered}"
    );
}
