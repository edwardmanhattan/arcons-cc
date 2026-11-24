//! **The standing picture's cache and its label** (#284), without a window and without a socket.
//!
//! `fixtures/rest/positions.json` is the recorded `GET /live-feed/positions` answer: 127 hulls, one with a
//! fix. The cache round trip, the cold start, a corrupt file, a failed refresh, the age order and the
//! "no secret, host or username" rule are claims about that same recorded picture.
//!
//! The read itself is tested over loopback in `http.rs` (`Client::positions`); a test here never signs in
//! and never writes a real vessel.

use std::path::PathBuf;

use arcons_cc::data::types::{Envelope, FeedPosition};
use desk::batch::{Fix, Row};
use desk::picker::Picker;
use desk::picture::{Picture, Status, load_from, path_in, save_to};
use desk::ui::{State, WIB_OFFSET_MS};

/// 2026-11-20 14:03:22 WIB — the sample's instant, and what the label must read.
const READ_AT_MS: i64 = 1_795_158_202_000;

/// A directory of this test's own, removed when the test ends.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("desk-picture-{name}-{}", std::process::id()));
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

/// The recorded picture, parsed through the same envelope the read uses.
fn positions() -> Vec<FeedPosition> {
    let envelope: Envelope<Vec<FeedPosition>> =
        serde_json::from_str(include_str!("../../../fixtures/rest/positions.json"))
            .expect("the recorded standing picture");
    envelope.data
}

fn row() -> Row {
    Row {
        id_unit: 18,
        name: "KRI DIPONEGORO".to_string(),
        fix: Fix {
            recorded_at_ms: READ_AT_MS,
            offset_ms: WIB_OFFSET_MS,
            latitude: Some(-6.2),
            longitude: Some(106.8),
            speed_kn: None,
            course_deg: None,
            accuracy_m: None,
        },
    }
}

#[test]
fn the_picture_round_trips_through_the_cache() {
    let dir = Temp::new("round-trip");
    let positions = positions();
    save_to(&dir.0, &positions, READ_AT_MS).expect("the picture is cached");

    let cached = load_from(&dir.0).expect("the cache is readable");
    assert_eq!(cached.read_at_ms, READ_AT_MS);
    assert_eq!(
        cached.positions, positions,
        "the cache is the same picture, field for field"
    );
    assert!(
        !path_in(&dir.0).with_extension("new").exists(),
        "the temp file was renamed, not left beside the cache"
    );
}

#[test]
fn a_corrupt_cache_is_ignored_not_turned_into_an_empty_picture() {
    let dir = Temp::new("corrupt");
    std::fs::write(path_in(&dir.0), "{ not the cache we wrote").expect("a corrupt file");

    assert!(
        load_from(&dir.0).is_none(),
        "a cache that is not ours is ignored, never decoded into a picture"
    );
    let state = State::open_with(Some(&dir.0));
    assert_eq!(
        state.picture_status,
        Status::None,
        "a corrupt cache is 'no picture read yet', not an empty standing picture"
    );
    assert!(state.desk.picker().is_empty());
    assert_eq!(
        state.picture_status.line(WIB_OFFSET_MS),
        "no picture read yet"
    );
}

#[test]
fn offline_start_paints_the_cache_before_any_read() {
    let dir = Temp::new("offline");
    let positions = positions();
    save_to(&dir.0, &positions, READ_AT_MS).expect("the picture is cached");

    let state = State::open_with(Some(&dir.0));
    assert_eq!(
        state.desk.picker().len(),
        positions.len(),
        "the first frame drew 0 vessels with a cache on disk"
    );
    assert_eq!(
        state.picture_status,
        Status::Cached {
            read_at_ms: READ_AT_MS
        },
        "the first frame says the picture is cached, not live"
    );
    assert!(
        state
            .picture_status
            .line(WIB_OFFSET_MS)
            .starts_with("cached · read at 14:03"),
        "the label carries the read's time: {}",
        state.picture_status.line(WIB_OFFSET_MS)
    );
}

#[test]
fn a_failed_refresh_keeps_the_picture_the_operator_is_reading() {
    let dir = Temp::new("failed");
    save_to(&dir.0, &positions(), READ_AT_MS).expect("the picture is cached");
    let mut state = State::open_with(Some(&dir.0));
    let before = state.desk.picker().len();

    state.refresh_failed("the request timed out".to_string());

    assert_eq!(
        state.desk.picker().len(),
        before,
        "a refresh failure emptied the list"
    );
    assert_eq!(
        state.picture_status,
        Status::Failed {
            message: "the request timed out".to_string(),
            cached: Some(READ_AT_MS),
        }
    );
    assert!(
        state
            .picture_status
            .line(WIB_OFFSET_MS)
            .contains("showing the picture read at 14:03"),
        "the screen still says which picture it is showing: {}",
        state.picture_status.line(WIB_OFFSET_MS)
    );
}

#[test]
fn a_successful_refresh_replaces_the_picture_and_keeps_the_batch() {
    let mut state = State::open_with(None);
    let held = row();
    state.desk.add_row(held.clone());

    state.apply_picture(Picture {
        read_at_ms: READ_AT_MS,
        positions: positions(),
    });

    assert_eq!(
        state.desk.picker().len(),
        127,
        "the new picture is the list"
    );
    assert_eq!(
        state.desk.batch().get(held.id_unit),
        Some(&held),
        "the batch is a draft, not a view of the picture"
    );
    assert_eq!(
        state.picture_status,
        Status::Ready {
            read_at_ms: READ_AT_MS
        }
    );
}

#[test]
fn a_selection_whose_hull_is_gone_is_cleared_and_one_still_listed_stays() {
    let dir = Temp::new("selection");
    save_to(&dir.0, &positions(), READ_AT_MS).expect("the picture is cached");
    let mut state = State::open_with(Some(&dir.0));
    state.select(8);
    assert_eq!(state.desk.selected(), Some(8));

    state.apply_picture(Picture {
        read_at_ms: READ_AT_MS,
        positions: positions(),
    });
    assert_eq!(
        state.desk.selected(),
        Some(8),
        "a hull still in the picture stays selected"
    );

    state.apply_picture(Picture {
        read_at_ms: READ_AT_MS,
        positions: positions()
            .into_iter()
            .filter(|position| position.id_unit == 1)
            .collect(),
    });
    assert_eq!(
        state.desk.selected(),
        None,
        "a selection whose hull is gone is cleared, not left naming nobody"
    );
}

#[test]
fn the_picture_is_in_age_order_not_the_wire_order() {
    let picker = Picker::from_positions(&positions());
    let first = &picker.vessels()[0];
    assert_eq!(
        first.id_unit, 8,
        "the one reported hull comes first, whatever the wire's id order"
    );
    assert!(first.last.is_some(), "and it carries its fix");
    assert!(
        picker.vessels()[1..]
            .iter()
            .all(|vessel| vessel.last.is_none()),
        "hulls that never reported come last, not mixed by id"
    );
    let silent: Vec<u64> = picker.vessels()[1..]
        .iter()
        .map(|vessel| vessel.id_unit)
        .collect();
    let mut sorted = silent.clone();
    sorted.sort_unstable();
    assert_eq!(silent, sorted, "ties among silent hulls break by id");
}

#[test]
fn the_cache_holds_no_token_host_or_username() {
    let dir = Temp::new("secret");
    save_to(&dir.0, &positions(), READ_AT_MS).expect("the picture is cached");
    let text = std::fs::read_to_string(path_in(&dir.0)).expect("the cache");
    let preview: String = text.chars().take(300).collect();

    for forbidden in [
        "access_token",
        "refresh_token",
        "authorization",
        "Bearer",
        "http://",
        "https://",
        "reported_by",
        "reported_for",
        "username",
        "password",
    ] {
        assert!(
            !text.contains(forbidden),
            "the cache contains {forbidden}: {preview}"
        );
    }
}

#[test]
fn the_status_line_says_which_picture_is_on_screen() {
    assert_eq!(Status::None.line(WIB_OFFSET_MS), "no picture read yet");
    assert_eq!(
        Status::Cached {
            read_at_ms: READ_AT_MS
        }
        .line(WIB_OFFSET_MS),
        "cached · read at 14:03"
    );
    assert_eq!(
        Status::Ready {
            read_at_ms: READ_AT_MS
        }
        .line(WIB_OFFSET_MS),
        "read at 14:03"
    );
    assert_eq!(
        Status::Reading {
            cached: Some(READ_AT_MS)
        }
        .line(WIB_OFFSET_MS),
        "reading… showing the picture read at 14:03"
    );
    assert_eq!(
        Status::Reading { cached: None }.line(WIB_OFFSET_MS),
        "reading the standing picture…"
    );
    assert_eq!(
        Status::Failed {
            message: "the request timed out".to_string(),
            cached: Some(READ_AT_MS),
        }
        .line(WIB_OFFSET_MS),
        "refresh failed — showing the picture read at 14:03 (the request timed out)"
    );
    assert_eq!(
        Status::Failed {
            message: "the request timed out".to_string(),
            cached: None,
        }
        .line(WIB_OFFSET_MS),
        "refresh failed — no picture read (the request timed out)"
    );
}
