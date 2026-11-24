//! One row's trip, and a batch of trips (#278): the machine, with a fake transport that answers in recorded bytes.
//!
//! The transport is a trait so these can prove the trip without a socket. The answers here are the ones
//! `fixtures/desk/report/` recorded from the deployment, plus the `201` derived from Minos `496ae25`.

use std::collections::VecDeque;

use desk::batch::{Batch, Fix, Row};
use desk::report::{Answer, Body};
use desk::send::{Failure, Fate, State, Transport, send_all};
use desk::ui::WIB_OFFSET_MS;

const WIB: i64 = 1_795_158_202_000;

fn row(id_unit: u64) -> Row {
    Row {
        id_unit,
        name: format!("hull {id_unit}"),
        fix: Fix {
            recorded_at_ms: WIB,
            offset_ms: WIB_OFFSET_MS,
            latitude: Some(-6.10),
            longitude: Some(106.87),
            speed_kn: Some(8.1),
            course_deg: Some(91.0),
            accuracy_m: Some(7.2),
        },
    }
}

/// The body of a recorded failure, as the deployment answered it.
fn recorded(name: &str) -> Answer {
    let doc: serde_json::Value = serde_json::from_str(match name {
        "no-commander" => include_str!("../../../fixtures/desk/report/no-commander.json"),
        "no-such-vessel" => include_str!("../../../fixtures/desk/report/no-such-vessel.json"),
        "age" => include_str!("../../../fixtures/desk/report/age.json"),
        "created" => include_str!("../../../fixtures/desk/report/created.json"),
        other => panic!("no fixture called {other}"),
    })
    .expect("a fixture is JSON");
    Answer::new(
        doc["response"]["status"].as_u64().expect("a status") as u16,
        doc["response"]["body"].to_string(),
    )
}

/// A transport that answers from a queue and remembers what it was handed.
struct Fake {
    answers: VecDeque<Result<Answer, Failure>>,
    seen: Vec<Body>,
}

impl Fake {
    fn answering(answers: Vec<Result<Answer, Failure>>) -> Self {
        Self {
            answers: answers.into(),
            seen: Vec::new(),
        }
    }
}

impl Transport for Fake {
    fn post(&mut self, body: &Body) -> Result<Answer, Failure> {
        self.seen.push(body.clone());
        self.answers
            .pop_front()
            .unwrap_or_else(|| Err(Failure::new("the fake had no answer left")))
    }
}

#[test]
fn a_draft_becomes_validated_and_hands_its_body_over_once() {
    let plan = Batch::new().plan(None);
    assert!(plan.sendable.is_empty());

    let mut batch = Batch::new();
    batch.insert(row(18));
    let plan = batch.plan(Some(WIB));
    let sendable = &plan.sendable[0];

    let mut state = State::Draft
        .validated(sendable)
        .expect("a full row validates");
    let first = state.begin().expect("a validated row hands its body over");
    assert_eq!(first.id_unit, 18);
    assert!(
        state.begin().is_none(),
        "a row that is already out has nothing to hand over, so it cannot be in flight twice"
    );
    assert_eq!(state.body(), Some(&first), "the body is still the row's");
}

#[test]
fn an_answer_settles_the_row() {
    let mut batch = Batch::new();
    batch.insert(row(18));
    let plan = batch.plan(Some(WIB));

    let mut state = State::Draft
        .validated(&plan.sendable[0])
        .expect("a full row validates");
    state.begin().expect("in flight");
    state.settle(&recorded("created"));
    let State::Recorded { latest, .. } = state else {
        panic!("a 201 records the row, got {state:?}");
    };
    assert!(
        !latest,
        "the recorded 201 says this report did not move the picture"
    );
}

#[test]
fn a_transport_failure_puts_the_row_back_where_it_was() {
    let mut batch = Batch::new();
    batch.insert(row(18));
    let plan = batch.plan(Some(WIB));

    let mut state = State::Draft
        .validated(&plan.sendable[0])
        .expect("a full row validates");
    let body = state.begin().expect("in flight");
    state.failed();
    let State::Validated(back) = state else {
        panic!("no answer arrived, so the row is sendable again, got {state:?}");
    };
    assert_eq!(back, body, "nothing about the fix changed");
}

#[test]
fn one_batch_sees_one_answer_per_vessel_in_the_order_it_was_built() {
    let now = WIB;
    let mut batch = Batch::new();
    for id_unit in 18..21 {
        batch.insert(row(id_unit));
    }
    let plan = batch.plan(Some(now));
    assert_eq!(plan.sendable.len(), 3);

    let mut fake = Fake::answering(vec![
        Ok(recorded("created")),
        Ok(recorded("no-commander")),
        Ok(recorded("no-such-vessel")),
    ]);
    let trips = send_all(&plan, &mut fake);

    assert_eq!(
        trips.len(),
        3,
        "three vessels, three requests, three answers"
    );
    assert_eq!(
        trips.iter().map(|trip| trip.id_unit).collect::<Vec<_>>(),
        [18, 19, 20]
    );
    assert!(matches!(trips[0].fate, Fate::Recorded { latest: false }));
    assert!(matches!(trips[1].fate, Fate::Refused(_)));
    assert!(matches!(trips[2].fate, Fate::Refused(_)));
    assert_eq!(
        fake.seen
            .iter()
            .map(|body| body.id_unit)
            .collect::<Vec<_>>(),
        [18, 19, 20],
        "one body per vessel, and each names its own hull"
    );
}

#[test]
fn one_vessel_s_refusal_does_not_touch_the_next() {
    let now = WIB;
    let mut batch = Batch::new();
    for id_unit in 18..20 {
        batch.insert(row(id_unit));
    }
    let plan = batch.plan(Some(now));
    let mut fake = Fake::answering(vec![Ok(recorded("no-commander")), Ok(recorded("created"))]);
    let trips = send_all(&plan, &mut fake);

    assert!(
        matches!(trips[0].fate, Fate::Refused(_)),
        "the first hull has no commander"
    );
    assert!(
        matches!(trips[1].fate, Fate::Recorded { .. }),
        "the second hull's answer is its own: {trips:?}"
    );
    assert_eq!(fake.seen.len(), 2, "a refusal did not stop the loop");
}

#[test]
fn a_row_with_no_coordinates_never_leaves() {
    // The plan is a **snapshot**, and the row can be edited between the plan and its turn — so the second fence is
    // not redundant: a body that cannot be built stops the row before the transport sees it.
    let mut incomplete = row(18);
    incomplete.fix.latitude = None;
    let sendable = desk::batch::Sendable {
        id_unit: incomplete.id_unit,
        name: incomplete.name.clone(),
        fix: incomplete.fix,
    };
    let refusal = State::Draft
        .validated(&sendable)
        .expect_err("a relay without a position is not a position");
    assert_eq!(refusal.field(), "latitude");
    assert!(refusal.sentence().contains("place the vessel"));
}

#[test]
fn the_loop_does_not_retry_a_transport_failure_by_itself() {
    let now = WIB;
    let mut batch = Batch::new();
    batch.insert(row(18));
    let plan = batch.plan(Some(now));
    let mut fake = Fake::answering(vec![
        Err(Failure::new("connection refused")),
        Ok(recorded("created")),
    ]);
    let trips = send_all(&plan, &mut fake);

    assert_eq!(
        trips[0].fate,
        Fate::Failed(Failure::new("connection refused"))
    );
    assert!(
        trips[0].fate.retryable(),
        "the same fix may be sent again: nothing about it changed"
    );
    assert_eq!(
        fake.seen.len(),
        1,
        "the loop hands the row back rather than trying again with a clock that has moved"
    );
}
