//! **The desk's clock** (#290): the operator's now is the OS clock; the server's is estimated from a fix.
//!
//! `time.rs`'s rule is the one under test: **no server clock, no verdict.** The OS clock decides what day the
//! field opens on and what "Now" means; it is never substituted for the server's clock. A fix carries
//! `received_at + age_seconds`, which is the server's clock at the picture's read; the newest fix is the
//! estimate. The cache file is written by the test, not the desk, so these are fixed instants.

use std::path::PathBuf;

use desk::batch::{Readiness, Reason};
use desk::picture::{ServerClock, load_from, path_in};
use desk::scene;
use desk::time::Regime;
use desk::ui::{ClockSource, State};

/// 2026-11-20 07:03:45Z — the server's clock at the cache's read, and 14:03:45 WIB.
const READ_AT: i64 = 1_795_158_225_000;

/// 2026-12-01 07:03:45Z — the same clock eleven days later, for the "opens on now" test.
const DEC_1: i64 = 1_796_108_625_000;

/// One hull that reported: `received_at` 07:03:00Z + `age_seconds` 45 = 07:03:45Z.
const CACHE_FIX: &str = r#"{
  "read_at_ms": 1795158225000,
  "positions": [
    {
      "id_unit": 1,
      "name": "KRI TEST",
      "hull_number": "1",
      "position": {
        "latitude": -6.2,
        "longitude": 106.8,
        "recorded_at": "2026-11-20T07:02:15Z",
        "received_at": "2026-11-20T07:03:00Z",
        "age_seconds": 45.0,
        "backfilled": false
      }
    }
  ]
}"#;

/// A hull that has never reported: no fix, so no server clock.
const CACHE_NO_FIX: &str = r#"{
  "read_at_ms": 1795158225000,
  "positions": [
    { "id_unit": 2, "name": "KRI SILENT", "hull_number": "2" }
  ]
}"#;

/// Two fixes with different ages. The newest (15 s) is the estimate; its
/// `received_at + age` is 07:03:45Z, while the older one's is 07:02:45Z.
const CACHE_TWO: &str = r#"{
  "read_at_ms": 1795158225000,
  "positions": [
    {
      "id_unit": 1,
      "name": "OLD",
      "hull_number": "1",
      "position": {
        "latitude": -6.2,
        "longitude": 106.8,
        "recorded_at": "2026-11-20T07:01:00Z",
        "received_at": "2026-11-20T07:02:00Z",
        "age_seconds": 45.0,
        "backfilled": false
      }
    },
    {
      "id_unit": 2,
      "name": "NEW",
      "hull_number": "2",
      "position": {
        "latitude": -6.2,
        "longitude": 106.8,
        "recorded_at": "2026-11-20T07:03:00Z",
        "received_at": "2026-11-20T07:03:30Z",
        "age_seconds": 15.0,
        "backfilled": false
      }
    }
  ]
}"#;

/// A directory of this test's own, removed when the test ends.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("desk-clock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temporary directory");
        Self(path)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A window opened at `now_ms` with the given cache.
fn state_with(name: &str, cache: &str, now_ms: i64) -> (Temp, State) {
    let dir = Temp::new(name);
    std::fs::write(path_in(&dir.0), cache).expect("the cache");
    let state = State::open_at(Some(&dir.0), now_ms);
    (dir, state)
}

#[test]
fn the_window_opens_on_the_operators_now_not_the_sample() {
    let (_dir, state) = state_with("open", CACHE_FIX, DEC_1);

    assert_eq!(state.epoch_ms, DEC_1);
    assert_eq!(state.committed.year, 2026);
    assert_eq!(state.committed.month, 12, "the sample's November is gone");
    assert_eq!(state.committed.day, 1);
    assert_eq!(state.today, state.committed);
    assert_eq!(state.server_now_ms, Some(DEC_1));
    assert_eq!(state.clock_source, ClockSource::Server);
    assert_eq!(state.verdict.regime, Regime::Live);
    assert!(
        state.clock_line().contains("server clock · read at 14:03"),
        "the note says which clock and when it was read: {}",
        state.clock_line()
    );
}

#[test]
fn the_newest_fix_estimates_the_server_clock() {
    let dir = Temp::new("newest");
    std::fs::write(path_in(&dir.0), CACHE_TWO).expect("the cache");
    let cached = load_from(&dir.0).expect("the cache is readable");

    let clock = ServerClock::from_positions(&cached.positions, cached.read_at_ms)
        .expect("a fix gives a clock");
    assert_eq!(
        clock.at_ms, READ_AT,
        "the 15 s fix is the estimate; the 45 s one would have said 07:02:45"
    );
    assert_eq!(clock.read_at_ms, READ_AT);
    assert_eq!(
        clock.now_ms(READ_AT + 5_000),
        READ_AT + 5_000,
        "the estimate is carried forward by the desk's elapsed time"
    );
}

#[test]
fn no_fix_means_no_server_clock_and_no_verdict() {
    let (_dir, mut state) = state_with("no-fix", CACHE_NO_FIX, READ_AT);

    assert_eq!(
        state.server_now_ms, None,
        "the OS clock is not the server's"
    );
    assert_eq!(state.clock_source, ClockSource::Unknown);
    assert_eq!(state.clock_line(), "server clock not known yet");
    assert_eq!(state.verdict.regime, Regime::Unknown);

    state.select(2);
    assert_eq!(
        state.readiness(),
        Some(Readiness::Blocked(Reason::UnknownClock)),
        "a send without a server clock would be a verdict the desk cannot give"
    );
}

#[test]
fn forty_five_seconds_is_live_and_thirty_one_days_is_refused() {
    let (_dir, mut state) = state_with("width", CACHE_FIX, READ_AT);

    state.paste("2026-11-20T14:03:00+07:00");
    state.enter();
    assert_eq!(state.verdict.regime, Regime::Live, "45 s old is live");

    state.paste("2026-10-20T14:03:45+07:00");
    state.enter();
    assert_eq!(
        state.verdict.regime,
        Regime::Refused,
        "31 days old is refused"
    );

    state.paste("2026-11-20T14:04:46+07:00");
    state.enter();
    assert_eq!(
        state.verdict.regime,
        Regime::Future,
        "61 s ahead is not a live time to send"
    );

    state.paste("2026-11-20T14:03:48+07:00");
    state.enter();
    assert_eq!(state.verdict.regime, Regime::Live);
    assert!(
        state.verdict.near_boundary,
        "3 s ahead is inside the hedge around the future line"
    );
}

#[test]
fn now_sets_the_operators_day_not_the_samples() {
    let (_dir, mut state) = state_with("today", CACHE_FIX, DEC_1);

    state.paste("20/11/2026 09:00");
    state.today();

    assert_eq!(state.today.year, 2026);
    assert_eq!(state.today.month, 12, "Now is the OS day, not the sample's");
    assert_eq!(state.today.day, 1);
    assert_eq!(
        state
            .segments
            .commit()
            .expect("the six segments read a time"),
        state.today
    );
    assert!(
        state.epoch_ms > 0,
        "the field holds the instant the operator's now names"
    );
    assert_eq!(state.epoch_ms, 1_796_108_625_000);
}

#[test]
fn the_clock_carries_forward_and_the_sample_does_not_tick() {
    let (_dir, mut state) = state_with("tick", CACHE_FIX, READ_AT);

    assert!(state.tick_clock_at(READ_AT + 60_000));
    assert_eq!(state.server_now_ms, Some(READ_AT + 60_000));
    assert_eq!(state.today.hour, 14);
    assert_eq!(state.today.minute, 4);
    assert_eq!(state.today.second, 45);

    let mut sample = scene::sample();
    let before = sample.server_now_ms;
    assert!(
        !sample.tick_clock_at(READ_AT + 60_000),
        "a scene's fixed clock must not move under a capture"
    );
    assert_eq!(sample.server_now_ms, before);
}

#[test]
fn the_clock_line_says_the_sample_the_estimate_or_nothing() {
    assert_eq!(
        scene::sample().clock_line(),
        "sample clock — a scene's fixed time"
    );

    let (_server_dir, server) = state_with("line-server", CACHE_FIX, READ_AT);
    assert_eq!(server.clock_line(), "server clock · read at 14:03");

    let (_unknown_dir, unknown) = state_with("line-unknown", CACHE_NO_FIX, READ_AT);
    assert_eq!(unknown.clock_line(), "server clock not known yet");
}
