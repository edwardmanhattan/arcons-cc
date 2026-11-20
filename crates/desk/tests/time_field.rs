//! The ±60 s and 30-day lines, the future the desk refuses, and the clock it will not guess at.
//!
//! The field's UI lands in the next commit; what it will render is decided here, once, by one function.

use desk::time::{
    BACKFILL_WINDOW_MS, FUTURE_TOLERANCE_MS, LIVE_WINDOW_MS, Regime, Verdict, verdict,
};

const NOW: i64 = 1_700_000_000_000;

fn at(age_ms: i64) -> Verdict {
    verdict(NOW - age_ms, Some(NOW))
}

#[test]
fn sixty_seconds_is_still_live_and_one_millisecond_more_is_backfill() {
    assert_eq!(at(59_000).regime, Regime::Live);
    assert_eq!(
        at(LIVE_WINDOW_MS).regime,
        Regime::Live,
        "the window is inclusive at 60 s, as the contract's table reads"
    );
    assert_eq!(
        at(LIVE_WINDOW_MS + 1).regime,
        Regime::Backfill,
        "one millisecond past the window is a backlog, and its time is kept"
    );
}

#[test]
fn thirty_days_is_still_backfill_and_one_millisecond_more_is_refused() {
    assert_eq!(at(BACKFILL_WINDOW_MS - 1).regime, Regime::Backfill);
    assert_eq!(
        at(BACKFILL_WINDOW_MS).regime,
        Regime::Backfill,
        "thirty days exactly is inside the window"
    );
    assert_eq!(
        at(BACKFILL_WINDOW_MS + 1).regime,
        Regime::Refused,
        "a refusal for age cannot be fixed by retrying"
    );
}

#[test]
fn a_few_seconds_ahead_is_tolerated_and_a_minute_is_not() {
    assert_eq!(at(0).regime, Regime::Live);
    assert_eq!(
        at(-FUTURE_TOLERANCE_MS).regime,
        Regime::Live,
        "clock skew of a few seconds is normal"
    );
    assert_eq!(
        at(-FUTURE_TOLERANCE_MS - 1).regime,
        Regime::Future,
        "beyond the tolerance the server would re-stamp the time as now"
    );
    assert_eq!(at(-90_000).regime, Regime::Future);
}

#[test]
fn without_the_servers_clock_there_is_no_verdict() {
    let unknown = verdict(NOW, None);
    assert_eq!(unknown.regime, Regime::Unknown);
    assert!(
        !unknown.near_boundary,
        "an unknown clock cannot be near a boundary"
    );
    assert!(
        unknown.regime.sentence().contains("clock not checked"),
        "the line must say why there is no verdict"
    );
}

#[test]
fn a_verdict_near_a_boundary_says_it_can_flip() {
    assert!(
        at(LIVE_WINDOW_MS - 1_000).near_boundary,
        "one second inside 60 s can cross before the request leaves"
    );
    assert!(
        at(LIVE_WINDOW_MS + 1_000).near_boundary,
        "one second outside is just as close"
    );
    assert!(
        at(BACKFILL_WINDOW_MS - 1_000).near_boundary,
        "the 30-day line flips the same way"
    );
    assert!(
        at(-FUTURE_TOLERANCE_MS + 500).near_boundary,
        "so does the future line"
    );
    assert!(
        !at(10_000).near_boundary,
        "ten seconds old is not near any line"
    );
    assert!(
        !at(86_400_000).near_boundary,
        "a day old is not near any line"
    );
}

#[test]
fn every_verdict_has_one_sentence() {
    for regime in [
        Regime::Live,
        Regime::Backfill,
        Regime::Refused,
        Regime::Future,
        Regime::Unknown,
    ] {
        assert!(!regime.sentence().is_empty(), "{regime:?} has no sentence");
        assert_eq!(regime.to_string(), regime.sentence());
    }
    assert_ne!(
        Regime::Live.sentence(),
        Regime::Backfill.sentence(),
        "live and backfill must not read the same: one keeps the time, one replaces it"
    );
}
