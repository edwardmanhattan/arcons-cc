//! The hull, its fix, and what the wall is allowed to claim about them.
//!
//! **A Unit is a hull, not a position.** Half the fleet may never have reported; those units belong
//! in the picture and not on the map (`INV-02`). What the map draws is a [`Fix`], and a Fix is
//! absent until one arrives.
//!
//! The words, as `CONTEXT.md` defines them:
//!
//! - **Unit** — a hull: `id_unit`, `name`, `hull_number`. Never "vessel" or "ship" in code.
//! - **Fix** — one accepted position report. Never "ping", "update" or "location".
//! - **Feed** — the standing picture: every live unit, whether or not it has ever reported.
//! - **Picture** — what the wall shows: the feed, plus the client-side judgements drawn on it.
//!
//! Three states, and they are different facts rather than three shades of one:
//!
//! | state | what it means | what the wall draws |
//! | --- | --- | --- |
//! | [`Glyph::Live`] | a fresh fix, taken recently | a filled marker with its heading |
//! | [`Glyph::Stale`] | a fix older than [`STALE_AFTER`] | a dimmed marker, and it says how old |
//! | [`Glyph::History`] | `backfilled: true` — real, newest known, **did not just happen** | a hollow mark, never "here now" |
//!
//! and a fourth case that is not a state at all: **no fix**, which draws nothing. Latitude 0,
//! longitude 0 is a real place in the Gulf of Guinea, so a missing fix drawn at the origin is a wrong
//! fix, not an empty one.
//!
//! Age is the server's, carried forward: `age_seconds` from the HTTP read, plus local **monotonic**
//! elapsed time since that read ([`Timed`]). A clock that can jump is never consulted, because a fix
//! that gets younger is a lie the wall cannot take back (`INV-06`).

pub mod card;
pub mod motion;

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::data::types::{FeedFix, FeedPosition};
use crate::live::wire::LiveFix;

/// How old a fix may be before the wall stops calling it fresh.
///
/// A **client decision**, deliberately: the server sends `age_seconds` and refuses to decide, because
/// "stale" is a judgement about what the operator is looking at, not a property of the fix. Two
/// minutes is longer than a beacon gap and short enough that a hull that stopped reporting is visibly
/// different before an operator has to wonder.
pub const STALE_AFTER: Duration = Duration::from_secs(120);

/// A position on the globe, in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    pub lat: f64,
    pub lon: f64,
}

impl Pos {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    /// How far apart two fixes are, in degrees of arc — enough to decide "did it move".
    pub fn apart(self, other: Pos) -> f64 {
        let (dlat, dlon) = (self.lat - other.lat, self.lon - other.lon);
        (dlat * dlat + dlon * dlon).sqrt()
    }
}

/// A hull. `id_unit` is the join key for every other Minos endpoint, so it is never guessed.
///
/// `hull_number` is `Option` because the API types it `nullable` and omits it for hulls the source
/// lists without one. An absent hull number is not an empty string, and it is not a zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    /// `int64` in the pinned spec (`FeedPosition.id_unit`).
    pub id_unit: i64,
    pub name: String,
    pub hull_number: Option<String>,
}

/// What the wall knows about one hull: the view model, and the only thing the view may read.
///
/// Built once per fix — in the data layer, not in `view` — because the view runs on every frame and
/// a view model rebuilt per frame is a frame that can be wrong in a new way each time.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitView {
    pub unit: Unit,
    /// The newest fix, if one has ever arrived. `None` means this hull is in the picture and not on
    /// the map (`INV-02`).
    pub position: Option<Pos>,
    /// The fix before it, for display interpolation only. Never a second truth.
    pub previous: Option<Pos>,
    /// Where the marker was when the newest fix arrived, and when that was, so the drawn position can
    /// travel between them without the fix itself ever moving (`INV-05`).
    pub travel: Option<motion::Travel>,
    /// Age at the moment the fix was accepted here, in seconds.
    pub age_at_receipt: f64,
    /// When this fix was accepted by **this process**, on the monotonic clock. Age is this plus the
    /// elapsed time since; nothing else in the crate may compute age.
    pub receipt: Instant,
    /// `backfilled: true` — reconstructed from an offline backlog.
    pub backfilled: bool,
    /// The heading, when it was measured. `None` draws no arrow: a hull with no course does not point
    /// north.
    pub course_deg: Option<f64>,
    /// The reported accuracy, when it was measured: the ring's radius.
    pub accuracy_m: Option<f64>,
    /// Speed over ground, when it was measured. `Some(0.0)` is a measured stop.
    pub speed_kn: Option<f64>,
    /// When the server accepted the fix. Displayed, never used for arithmetic against a local clock.
    pub received_at: DateTime<Utc>,
}

impl UnitView {
    /// A hull that is in the picture and has never reported.
    pub fn silent(unit: Unit) -> Self {
        Self {
            unit,
            position: None,
            previous: None,
            travel: None,
            age_at_receipt: 0.0,
            receipt: Instant::now(),
            backfilled: false,
            course_deg: None,
            accuracy_m: None,
            speed_kn: None,
            received_at: DateTime::<Utc>::UNIX_EPOCH,
        }
    }

    /// How old the newest fix is, in seconds, `now` being the monotonic clock.
    ///
    /// Age only ever grows between fixes: it is a value fixed at receipt plus elapsed monotonic time,
    /// so no clock jump can make a fix younger (`INV-06`). A new fix resets it, which is the one
    /// legitimate way for it to fall.
    pub fn age_seconds(&self, now: Instant) -> Option<f64> {
        self.position?;
        Some(self.age_at_receipt + now.saturating_duration_since(self.receipt).as_secs_f64())
    }

    /// Where to draw the marker, now.
    ///
    /// This is a **drawing** position. It never enters the track, never re-stamps `received_at`, and
    /// nothing computes anything from it (`INV-05`).
    pub fn drawn(&self, now: Instant) -> Option<Pos> {
        let to = self.position?;
        match &self.travel {
            Some(travel) => Some(travel.at(now)),
            None => Some(to),
        }
    }

    /// Which glyph this hull earns, if any.
    pub fn glyph(&self, now: Instant) -> Option<Glyph> {
        self.position?;
        if self.backfilled {
            // INV-03: history, whatever its age. The flag and the age are different facts, and this
            // one wins: a backfilled fix is real, is the newest thing known, and did not just happen.
            return Some(Glyph::History);
        }
        match self.age_seconds(now) {
            Some(age) if age >= STALE_AFTER.as_secs_f64() => Some(Glyph::Stale),
            Some(_) => Some(Glyph::Live),
            None => None,
        }
    }
}

/// The marker a hull earns. One glyph, one fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// A fresh fix: filled marker, heading arrow when the course was measured.
    Live,
    /// Older than [`STALE_AFTER`]: dimmed, and the card says how old.
    Stale,
    /// `backfilled: true`: a hollow mark. Never the "here now" glyph.
    History,
}

/// The standing picture: every live unit, whether or not it has ever reported.
///
/// The type lives here, rather than in `data`, deliberately: the picture is the display's model, and
/// the transport is not allowed to decide its shape.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picture {
    units: Vec<UnitView>,
}

impl Picture {
    /// Every hull, in the order the server listed them.
    pub fn units(&self) -> &[UnitView] {
        &self.units
    }

    /// How many hulls the wall knows about — including the ones that have never reported.
    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// True before the first read completes, which is what tells "no units exist" apart from "not
    /// asked yet". `INV-09` is the same distinction one layer down.
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// How many hulls are currently drawable (have a fix).
    pub fn with_fix(&self) -> usize {
        self.units
            .iter()
            .filter(|unit| unit.position.is_some())
            .count()
    }

    /// Replace the picture with a completed read from `/live-feed/positions`.
    ///
    /// The read is authoritative for membership: a hull missing from it is missing from the fleet, not
    /// from our copy. It is also authoritative for age, which is the server's `age_seconds` measured
    /// at response time — the one place a server-sent age is correct, because it is measured when the
    /// answer is composed and read immediately.
    pub fn replace_from_feed(&mut self, feed: &[FeedPosition]) {
        let now = Instant::now();
        self.units = feed
            .iter()
            .map(|entry| {
                let unit = Unit {
                    id_unit: entry.id_unit,
                    name: entry.name.clone(),
                    hull_number: entry.hull_number.clone(),
                };
                match &entry.position {
                    None => UnitView::silent(unit),
                    Some(fix) => view_from_http(unit, fix, now),
                }
            })
            .collect();
    }

    /// Whether any marker is still travelling to its fix, so the caller knows to redraw now rather
    /// than at the next fix.
    pub fn travelling(&self, now: Instant) -> bool {
        self.units.iter().any(|unit| {
            unit.travel.as_ref().is_some_and(|travel| {
                now.saturating_duration_since(travel.started()) < motion::DURATION
            })
        })
    }

    /// Fold one live publication into the picture, by `id_unit`.
    ///
    /// A publication for a hull the picture has never heard of is **added**, not dropped: the socket
    /// is allowed to know about a hull the last read did not, and dropping it would make the two
    /// transports disagree about the fleet.
    pub fn apply_fix(&mut self, fix: &LiveFix) {
        let now = Instant::now();
        let position = Pos::new(fix.latitude, fix.longitude);
        let backfilled = fix.backfilled;

        if let Some(existing) = self
            .units
            .iter_mut()
            .find(|unit| unit.unit.id_unit == fix.id_unit)
        {
            let previous = existing.drawn(now).or(existing.position);
            travel_into(existing, previous, position, backfilled, now, 0.0);
            existing.course_deg = fix.course_deg;
            existing.accuracy_m = fix.accuracy_m;
            existing.speed_kn = fix.speed_kn;
            existing.received_at = fix.received_at;
            existing.unit.name = fix.name.clone();
            if fix.hull_number.is_some() {
                existing.unit.hull_number = fix.hull_number.clone();
            }
            return;
        }

        let unit = Unit {
            id_unit: fix.id_unit,
            name: fix.name.clone(),
            hull_number: fix.hull_number.clone(),
        };
        let mut view = UnitView::silent(unit);
        travel_into(&mut view, None, position, backfilled, now, 0.0);
        view.course_deg = fix.course_deg;
        view.accuracy_m = fix.accuracy_m;
        view.speed_kn = fix.speed_kn;
        view.received_at = fix.received_at;
        self.units.push(view);
    }
}

fn view_from_http(unit: Unit, fix: &FeedFix, now: Instant) -> UnitView {
    let position = Pos::new(fix.latitude, fix.longitude);
    UnitView {
        unit,
        position: Some(position),
        previous: None,
        // A read is a snapshot, not a movement: the marker appears where the server says it is,
        // rather than flying there from wherever it used to be. On a reconnect that would be a hull
        // crossing the ocean in a second, which is exactly the "teleport" the traps warn about.
        travel: None,
        age_at_receipt: fix.age_seconds,
        receipt: now,
        backfilled: fix.backfilled,
        course_deg: fix.course_deg,
        accuracy_m: fix.accuracy_m,
        speed_kn: fix.speed_kn,
        received_at: fix.received_at,
    }
}

/// Start the display travel from wherever the marker was to the new fix — or snap, when travelling
/// would be a lie.
fn travel_into(
    view: &mut UnitView,
    previous: Option<Pos>,
    to: Pos,
    backfilled: bool,
    now: Instant,
    gap_seconds: f64,
) {
    let travel = previous.and_then(|from| motion::travel(from, to, backfilled, gap_seconds, now));
    view.previous = previous;
    view.position = Some(to);
    view.travel = travel;
    view.backfilled = backfilled;
    view.age_at_receipt = gap_seconds.max(0.0) * 0.0; // a live publication is fresh on arrival
    view.receipt = now;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::FeedPosition;
    use chrono::TimeZone;

    fn http_entry(id: i64, fix: Option<FeedFix>) -> FeedPosition {
        FeedPosition {
            id_unit: id,
            name: format!("KRI {id}"),
            hull_number: None,
            position: fix,
        }
    }

    fn fix(lat: f64, lon: f64, age: f64, backfilled: bool) -> FeedFix {
        FeedFix {
            latitude: lat,
            longitude: lon,
            speed_kn: Some(12.0),
            speed_ms: None,
            course_deg: Some(90.0),
            accuracy_m: Some(5.0),
            recorded_at: Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap(),
            received_at: Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap(),
            age_seconds: age,
            backfilled,
        }
    }

    fn live_fix(id: i64, lat: f64, lon: f64, backfilled: bool) -> LiveFix {
        let body = format!(
            r#"{{"id_unit":{id},"name":"KRI {id}","latitude":{lat},"longitude":{lon},"recorded_at":"2026-11-06T12:00:00Z","received_at":"2026-11-06T12:00:00Z","backfilled":{backfilled}}}"#
        );
        crate::live::wire::parse(body.as_bytes()).expect("valid")
    }

    #[test]
    fn a_hull_that_never_reported_is_in_the_picture_and_has_no_glyph() {
        // INV-02: the hull is listed and not drawn. 126 of the dev feed's 127 hulls are this case.
        let mut picture = Picture::default();
        picture.replace_from_feed(&[
            http_entry(1, None),
            http_entry(8, Some(fix(-6.1, 106.9, 3.0, false))),
        ]);
        assert_eq!(picture.len(), 2);
        assert_eq!(picture.with_fix(), 1);
        let silent = &picture.units()[0];
        assert_eq!(silent.position, None);
        assert_eq!(
            silent.glyph(Instant::now()),
            None,
            "a hull with no fix earns no marker"
        );
        assert_eq!(silent.drawn(Instant::now()), None, "and nothing to draw");
    }

    #[test]
    fn a_fresh_fix_is_live_and_an_old_one_is_stale() {
        let mut picture = Picture::default();
        picture.replace_from_feed(&[
            http_entry(1, Some(fix(-6.1, 106.9, 2.0, false))),
            http_entry(2, Some(fix(-6.2, 106.8, 3.0 * 3600.0, false))),
        ]);
        let now = Instant::now();
        assert_eq!(picture.units()[0].glyph(now), Some(Glyph::Live));
        assert_eq!(
            picture.units()[1].glyph(now),
            Some(Glyph::Stale),
            "three hours old is not live"
        );
        // And the difference the ticket asks for is a number, not a shade: the stale one says how old.
        let age = picture.units()[1].age_seconds(now).unwrap();
        assert!(
            age >= STALE_AFTER.as_secs_f64(),
            "stale means at least {STALE_AFTER:?}, got {age} s"
        );
    }

    #[test]
    fn a_backfilled_fix_is_history_even_when_it_is_fresh() {
        // INV-03: the flag wins over the age. A backfilled fix is real, is the newest thing known, and
        // did not just happen — drawing it as "here now" teleports a hull across the map.
        let mut picture = Picture::default();
        picture.replace_from_feed(&[http_entry(3, Some(fix(-6.1, 106.9, 0.5, true)))]);
        assert_eq!(
            picture.units()[0].glyph(Instant::now()),
            Some(Glyph::History)
        );
    }

    #[test]
    fn age_only_grows_between_fixes() {
        // INV-06: age is the server's value plus monotonic elapsed time, so no clock jump can make a
        // fix younger. Simulated by asking twice with a later `now`.
        let mut picture = Picture::default();
        picture.replace_from_feed(&[http_entry(4, Some(fix(-6.1, 106.9, 30.0, false)))]);
        let start = Instant::now();
        let first = picture.units()[0].age_seconds(start).unwrap();
        let later = picture.units()[0]
            .age_seconds(start + Duration::from_secs(10))
            .unwrap();
        assert!(later > first, "INV-06: age went from {first} to {later}");
        assert!(
            (later - first - 10.0).abs() < 0.001,
            "ten seconds passed, so ten seconds aged: {first} → {later}"
        );
    }

    #[test]
    fn a_socket_fix_adds_a_hull_the_read_did_not_name() {
        // The two transports must not disagree about the fleet: the socket may know a hull first.
        let mut picture = Picture::default();
        picture.replace_from_feed(&[http_entry(1, None)]);
        picture.apply_fix(&live_fix(9, -6.0, 106.0, false));
        assert_eq!(picture.len(), 2);
        assert_eq!(picture.units()[1].unit.id_unit, 9);
        assert_eq!(picture.units()[1].glyph(Instant::now()), Some(Glyph::Live));
    }

    #[test]
    fn a_publication_moves_the_marker_and_keeps_the_previous_fix_for_the_draw() {
        let mut picture = Picture::default();
        picture.replace_from_feed(&[http_entry(9, Some(fix(-6.0, 106.0, 1.0, false)))]);
        let before = picture.units()[0].position.expect("a fix");
        picture.apply_fix(&live_fix(9, -6.001, 106.001, false));
        let after = picture.units()[0].position.expect("a fix");
        assert_ne!(before, after, "the new fix is the position");
        assert_eq!(
            picture.units()[0].previous,
            Some(before),
            "the old one is kept for the draw only"
        );
    }

    #[test]
    fn a_backfilled_publication_snaps_and_never_travels() {
        // INV-03 at the drawing layer: a days-old point must not be animated as though it just
        // arrived — that draws a hull gliding across the ocean.
        let mut picture = Picture::default();
        picture.replace_from_feed(&[http_entry(9, Some(fix(-6.0, 106.0, 1.0, false)))]);
        picture.apply_fix(&live_fix(9, -7.5, 110.0, true));
        assert!(
            picture.units()[0].travel.is_none(),
            "a backfilled fix snaps"
        );
        assert_eq!(
            picture.units()[0].glyph(Instant::now()),
            Some(Glyph::History)
        );
    }

    #[test]
    fn a_measured_zero_speed_survives_and_absence_does_not_become_one() {
        // INV-01: the wall must be able to tell "stopped" from "not measured".
        let mut measured = fix(-6.0, 106.0, 1.0, false);
        measured.speed_kn = Some(0.0);
        let absent = fix(-6.0, 106.0, 1.0, false);
        let mut picture = Picture::default();
        picture.replace_from_feed(&[http_entry(1, Some(measured))]);
        picture.apply_fix(&live_fix(2, -6.0, 106.0, false));
        assert_eq!(
            picture.units()[0].speed_kn,
            Some(0.0),
            "INV-01: a measured stop is a measurement"
        );
        assert_eq!(
            picture.units()[1].speed_kn,
            None,
            "INV-01: an absent speed is not a zero"
        );
        assert_eq!(
            picture.units()[1].course_deg,
            None,
            "INV-01: and an absent course draws no arrow"
        );
        let _ = absent;
    }
}
