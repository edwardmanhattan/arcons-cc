//! **Who asserted a fix** (#288): the contract's optional `manual`, `reported_by` and `reported_for`.
//!
//! `fixtures/scenes/feed-manual.json` is `feed.json` with unit 3's fix entered on the vessel commander's
//! behalf. It is derived, not recorded — the dev feed's only fix is a device fix — and its field names are
//! pinned to `docs/spec/minos-api.yaml` @ `496ae25` (`FeedFix` and `FeedActor`).
//!
//! The last test is the cache rule (#284): the wall's `FeedFix` is serialised for the desk's picture cache,
//! and a username has no business in that file, so the actor fields are read but never written.

use std::time::Instant;

use arcons_cc::data::types::{Envelope, FeedActor, FeedPosition};
use arcons_cc::units::card::Card;
use arcons_cc::units::{Picture, STALE_AFTER};

fn manual_feed() -> Vec<FeedPosition> {
    let envelope: Envelope<Vec<FeedPosition>> =
        serde_json::from_str(include_str!("../fixtures/scenes/feed-manual.json"))
            .expect("the manual scene feed");
    envelope.data
}

fn name(actor: &Option<FeedActor>) -> Option<&str> {
    actor.as_ref().map(|actor| actor.name.as_str())
}

#[test]
fn the_manual_fixture_carries_the_author_and_the_commander_it_claims() {
    let feed = manual_feed();
    let unit = feed
        .iter()
        .find(|position| position.id_unit == 3)
        .expect("the manual hull");
    let fix = unit.position.as_ref().expect("the manual hull has a fix");
    assert_eq!(fix.manual, Some(true));
    assert_eq!(name(&fix.reported_by), Some("Operator Sari"));
    assert_eq!(name(&fix.reported_for), Some("Commander Yani"));

    let mut picture = Picture::default();
    picture.replace_from_feed(&feed);
    let view = picture
        .units()
        .iter()
        .find(|unit| unit.unit.id_unit == 3)
        .expect("the hull");
    assert!(view.manual);
    assert_eq!(name(&view.reported_by), Some("Operator Sari"));
    assert_eq!(name(&view.reported_for), Some("Commander Yani"));

    let card = Card::new(view, Instant::now(), STALE_AFTER);
    assert!(
        card.badges().contains(&"relayed"),
        "the card wears the relayed badge: {:?}",
        card.badges()
    );
    assert_eq!(
        card.provenance_line().as_deref(),
        Some("reported by Operator Sari for Commander Yani"),
        "the card names both people, in words"
    );
}

#[test]
fn a_device_fix_has_no_author_and_no_relayed_badge() {
    let envelope: Envelope<Vec<FeedPosition>> =
        serde_json::from_str(include_str!("../fixtures/scenes/feed.json")).expect("the scene feed");
    let feed = envelope.data;
    let unit = feed
        .iter()
        .find(|position| position.id_unit == 3)
        .expect("the hull");
    let fix = unit.position.as_ref().expect("the hull has a fix");
    assert_eq!(fix.manual, None);
    assert!(fix.reported_by.is_none() && fix.reported_for.is_none());

    let mut picture = Picture::default();
    picture.replace_from_feed(&feed);
    let view = picture
        .units()
        .iter()
        .find(|unit| unit.unit.id_unit == 3)
        .expect("the hull");
    assert!(!view.manual);
    let card = Card::new(view, Instant::now(), STALE_AFTER);
    assert!(!card.badges().contains(&"relayed"));
    assert_eq!(card.provenance_line(), None);
}

#[test]
fn the_live_event_carries_the_same_provenance() {
    let body = r#"{"id_unit":3,"name":"KRI Ardadedali","latitude":-6.1,"longitude":106.9,"recorded_at":"2026-11-06T12:00:00Z","received_at":"2026-11-06T12:00:00Z","backfilled":false,"manual":true,"reported_by":{"id":7,"name":"Operator Sari","username":"operator.sari"},"reported_for":{"id":3,"name":"Commander Yani","username":"commander.yani"}}"#;
    let fix = arcons_cc::live::wire::parse(body.as_bytes()).expect("the relayed event parses");
    assert!(fix.manual);

    let mut picture = Picture::default();
    picture.apply_fix(&fix);
    let view = &picture.units()[0];
    assert!(view.manual);
    assert_eq!(name(&view.reported_by), Some("Operator Sari"));
    assert_eq!(name(&view.reported_for), Some("Commander Yani"));
}

#[test]
fn the_cache_shape_drops_the_actors_usernames() {
    let feed = manual_feed();
    let fix = feed
        .iter()
        .find(|position| position.id_unit == 3)
        .and_then(|position| position.position.as_ref())
        .expect("the manual fix");

    let text = serde_json::to_string(fix).expect("the wall's Fix serialises for the desk cache");
    for forbidden in [
        "reported_by",
        "reported_for",
        "Operator Sari",
        "Commander Yani",
        "operator.sari",
        "commander.yani",
    ] {
        assert!(
            !text.contains(forbidden),
            "the cache shape contains {forbidden}: {text}"
        );
    }
    assert!(
        text.contains("\"manual\":true"),
        "the manual flag is not a username; it stays: {text}"
    );
}
