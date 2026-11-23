//! One vessel's report and the answer it comes back with (#278), against bytes recorded from the wire.
//!
//! The five refusals are `fixtures/desk/report/`, written by `scripts/record-desk-report.sh` against the
//! development deployment on 2026-11-23. None of them wrote anything. The `201` is **not** a recorded exchange —
//! recording it stores a fix for a real vessel — so the test that reads one says where its shape comes from.

use desk::batch::{Batch, Row};
use desk::batch::{Fix, FixError, Sendable};
use desk::report::{Answer, Body, Refusal, Verdict, iso_utc};
use desk::ui::WIB_OFFSET_MS;

/// 2026-11-20 14:03:22 WIB, the sample the scenes use — and 07:03:22 UTC, which is the instant the wire takes.
const WIB: i64 = 1_795_158_202_000;

fn sendable(id_unit: u64, fix: Fix) -> Sendable {
    Sendable {
        id_unit,
        name: "KRI DIPONEGORO".to_string(),
        fix,
    }
}

fn measured() -> Fix {
    Fix {
        recorded_at_ms: WIB,
        offset_ms: WIB_OFFSET_MS,
        latitude: Some(-6.1023),
        longitude: Some(106.8712),
        speed_kn: Some(8.1),
        course_deg: Some(91.0),
        accuracy_m: Some(7.2),
    }
}

fn fixture(name: &str) -> Answer {
    let doc: serde_json::Value = serde_json::from_str(match name {
        "required-fields" => include_str!("../../../fixtures/desk/report/required-fields.json"),
        "no-commander" => include_str!("../../../fixtures/desk/report/no-commander.json"),
        "age" => include_str!("../../../fixtures/desk/report/age.json"),
        "no-such-vessel" => include_str!("../../../fixtures/desk/report/no-such-vessel.json"),
        "unreadable" => include_str!("../../../fixtures/desk/report/unreadable.json"),
        "created" => include_str!("../../../fixtures/desk/report/created.json"),
        other => panic!("no fixture called {other}"),
    })
    .expect("a fixture is JSON");
    Answer::new(
        doc["response"]["status"]
            .as_u64()
            .expect("a recorded status") as u16,
        doc["response"]["body"].to_string(),
    )
}

#[test]
fn the_body_of_a_measured_fix_is_the_bytes_the_route_expects() {
    let body = Body::of(&sendable(18, measured())).expect("a measured fix is sendable");
    assert_eq!(
        body.json(),
        r#"{"id_unit":18,"recorded_at":"2026-11-20T07:03:22.000Z","latitude":-6.1023,"longitude":106.8712,"speed_kn":8.1,"course_deg":91.0,"accuracy_m":7.2}"#,
        "the body is flat, in the order the route's struct declares, and the time is UTC"
    );
}

#[test]
fn an_unmeasured_field_is_absent_rather_than_zero() {
    let fix = Fix {
        speed_kn: None,
        course_deg: None,
        accuracy_m: None,
        ..measured()
    };
    let json = Body::of(&sendable(18, fix))
        .expect("a position is enough")
        .json();
    for absent in ["speed_kn", "course_deg", "accuracy_m"] {
        assert!(
            !json.contains(absent),
            "{absent} was not measured, so it must not be in the body: {json}"
        );
    }
    assert!(
        !json.contains(",0"),
        "absence is not a zero measurement: {json}"
    );
}

#[test]
fn recorded_at_is_utc_with_milliseconds() {
    assert_eq!(iso_utc(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso_utc(WIB), "2026-11-20T07:03:22.000Z");
    assert_eq!(iso_utc(WIB + 123), "2026-11-20T07:03:22.123Z");
    assert_eq!(
        iso_utc(WIB - 1),
        "2026-11-20T07:03:21.999Z",
        "milliseconds are the instant's, never negative"
    );
}

#[test]
fn a_row_with_no_coordinates_is_not_a_report() {
    let fix = Fix {
        latitude: None,
        ..measured()
    };
    let refusal = Body::of(&sendable(18, fix)).expect_err("a relay without a position is nothing");
    assert_eq!(refusal.field(), "latitude");
    assert!(refusal.sentence().contains("place the vessel"));
}

#[test]
fn the_fence_refuses_five_thousand_six_hundred_sixty_six_knots() {
    let fix = Fix {
        speed_kn: Some(5666.0),
        ..measured()
    };
    let refusal = Body::of(&sendable(18, fix)).expect_err("5666 knots is not a vessel");
    assert_eq!(refusal.field(), "speed_kn");
    assert!(matches!(refusal, Refusal::Fenced(FixError::SpeedKn { .. })));
}

#[test]
fn a_course_of_a_full_turn_is_refused_because_the_route_refuses_it() {
    // `course_deg` is `lt=360` in `dto.FeedRelayRequest`: 360 and 0 are the same bearing, and the route takes 0.
    let fix = Fix {
        course_deg: Some(360.0),
        ..measured()
    };
    assert_eq!(
        Body::of(&sendable(18, fix))
            .expect_err("360 is a 400 on the wire")
            .field(),
        "course_deg"
    );
}

#[test]
fn the_required_field_answer_names_a_field_minos_refused() {
    let verdict = fixture("required-fields").verdict();
    let Verdict::Refused { field, .. } = verdict else {
        panic!("a 400 with field errors is a refusal, got {verdict:?}");
    };
    assert!(
        ["id_unit", "latitude", "longitude", "recorded_at"].contains(&field.as_str()),
        "the refusal names a field from the wire, got {field}"
    );
}

#[test]
fn a_vessel_with_no_commander_is_a_403_and_the_row_survives_it() {
    let verdict = fixture("no-commander").verdict();
    let Verdict::NoCommander { message } = &verdict else {
        panic!("a 403 is a per-vessel verdict, got {verdict:?}");
    };
    assert!(
        message.contains("no commander appointed"),
        "the row says what Minos said, got {message}"
    );
    assert!(
        verdict.retryable(),
        "an unappointed commander is not permanent: the row becomes sendable when one is assigned"
    );
    assert_eq!(
        verdict.sentence(),
        format!("no commander appointed, so there is no report to attribute: {message}")
    );
}

#[test]
fn a_time_older_than_thirty_days_is_a_400_on_recorded_at_not_a_count() {
    let verdict = fixture("age").verdict();
    assert_eq!(
        verdict,
        Verdict::Refused {
            status: 400,
            field: "recorded_at".to_string(),
            message: "That position is more than 30 days old, so it cannot be recorded."
                .to_string(),
        },
        "the age rule is a field refusal on this route, not a rejected count"
    );
    assert!(verdict.sentence().contains("Minos refused recorded_at"));
}

#[test]
fn a_missing_hull_is_a_404_whose_row_goes() {
    let verdict = fixture("no-such-vessel").verdict();
    assert_eq!(verdict, Verdict::NoSuchVessel);
    assert!(
        !verdict.retryable(),
        "a hull Minos does not have will not appear by retrying"
    );
    assert!(verdict.sentence().contains("no such vessel"));
}

#[test]
fn a_body_minos_could_not_read_is_a_422_and_is_our_bug() {
    let verdict = fixture("unreadable").verdict();
    let Verdict::Unreadable { message } = &verdict else {
        panic!("a 422 is an unreadable body, got {verdict:?}");
    };
    assert!(
        message.contains("could not be read"),
        "the answer's own words, got {message}"
    );
    assert!(
        !verdict.retryable(),
        "the same body would be refused again; the client is what has to change"
    );
}

#[test]
fn a_recorded_201_that_did_not_move_the_picture_says_so() {
    // **Not a wire exchange**, and the fixture says so: recording an accepted answer stores a fix for a real
    // vessel. The shape is `dto.FeedRelayedResult` in Minos 496ae25 — `{id_unit, recorded_at, received_at,
    // latest}` — and this is the `latest: false` half, the one an operator must not read as "the picture moved".
    let verdict = fixture("created").verdict();
    assert_eq!(
        verdict,
        Verdict::Created {
            id_unit: 18,
            recorded_at: "2026-11-20T07:03:22Z".to_string(),
            received_at: "2026-11-20T07:04:01.512Z".to_string(),
            latest: false,
        }
    );
    assert!(
        verdict.sentence().contains("did not move"),
        "a report older than what the vessel already sent is recorded and does not correct the picture: {}",
        verdict.sentence()
    );
    assert!(!verdict.retryable(), "a 201 is done");
}

#[test]
fn an_answer_nobody_has_seen_is_never_read_as_success() {
    for (status, body) in [
        (503, "<html>service unavailable</html>".to_string()),
        (201, "<html>a proxy's page</html>".to_string()),
        (
            200,
            r#"{"status_code":200,"message":"OK","data":null}"#.to_string(),
        ),
    ] {
        let verdict = Answer::new(status, body).verdict();
        assert!(
            matches!(verdict, Verdict::Unrecognised { .. }),
            "{status} is not a status this route documents, so it cannot mean success: {verdict:?}"
        );
        assert!(!verdict.retryable());
    }
}

#[test]
fn three_vessels_are_three_bodies_with_three_ids() {
    // ADR-0009 decision 2: one request per vessel, because `id_unit` is top-level.
    let now = WIB;
    let mut batch = Batch::new();
    for id_unit in 18..21 {
        batch.insert(Row {
            id_unit,
            name: format!("hull {id_unit}"),
            fix: Fix {
                recorded_at_ms: now,
                ..measured()
            },
        });
    }
    let plan = batch.plan(Some(now));
    assert_eq!(plan.sendable.len(), 3, "three vessels, three requests");
    let bodies: Vec<String> = plan
        .sendable
        .iter()
        .map(|sendable| Body::of(sendable).expect("each row is sendable").json())
        .collect();
    let ids: Vec<String> = bodies
        .iter()
        .map(|body| {
            body.split("\"id_unit\":")
                .nth(1)
                .and_then(|rest| rest.split(',').next())
                .expect("a body names its vessel")
                .to_string()
        })
        .collect();
    assert_eq!(ids, ["18", "19", "20"], "one id per body, and never two");
}
