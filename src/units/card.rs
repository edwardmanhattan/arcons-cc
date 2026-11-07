//! The unit card: what is known about one hull, and where to put it.
//!
//! The card is a **display**, not a control surface. It shows only what the feed and the three reads
//! actually carry, and a field that was not measured is shown as *not measured*, never as a number
//! (`INV-01`) — empty is information.
//!
//! Two things live here and nowhere else:
//!
//! - **The content model** ([`Card`]), built from a [`UnitView`] plus whatever the reads have said so
//!   far. It is built when the card opens or the fix changes, not per frame; only [`Card::age_text`]
//!   is a function of the clock, which is why age can tick once a second without re-running the map.
//! - **The placement** ([`place`]), which is pure arithmetic and therefore testable: the card points at
//!   its marker, flips to the other side when it would leave the window, and never clips a corner
//!   (`INV-10`).

use std::time::Instant;

use chrono::{DateTime, Utc};

use crate::data::types::{UnitCommander, UnitDetail, UnitSpecification};
use crate::units::{Glyph, Unit, UnitView};

/// The card's own size in points. Fixed: a card that resizes as fields arrive is a card that jumps
/// around the marker it belongs to.
pub const WIDTH: f64 = 260.0;
pub const HEIGHT: f64 = 216.0;

/// The space between a marker and the card beside it.
pub const GAP: f64 = 14.0;

/// The smallest margin a card keeps from the window's edge.
pub const MARGIN: f64 = 8.0;

/// One of the three reads a card needs, and how it is going.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Fetch<T> {
    /// Asked, and not answered yet. The card shows a skeleton row rather than a gap, so a reader can
    /// tell "still coming" from "there is nothing".
    #[default]
    Pending,
    Ready(T),
    /// The read failed. The card says so in one line and keeps the rest of its content.
    Failed(String),
}

impl<T> Fetch<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Fetch::Ready(value) => Some(value),
            _ => None,
        }
    }
}

/// Everything the card knows about one hull.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub unit: Unit,
    /// Age of the newest fix, in seconds, at the moment the model was built.
    pub age_at_build: Option<f64>,
    /// When the model was built, so [`Card::age_text`] can advance without rebuilding anything.
    pub built: Instant,
    pub recorded_at: Option<DateTime<Utc>>,
    pub glyph: Option<Glyph>,
    pub speed_kn: Option<f64>,
    pub course_deg: Option<f64>,
    pub accuracy_m: Option<f64>,
    /// `GET /units/{id}` — identity and class.
    pub identity: Fetch<UnitDetail>,
    /// `GET /units/{id}/commander` — newest first; the current entry has no `to_at`.
    pub commander: Fetch<Vec<UnitCommander>>,
    /// `GET /units/{id}/specifications`, cached per `(id_unit, version)` by the caller.
    pub specification: Fetch<Vec<UnitSpecification>>,
}

impl Card {
    /// The model for a hull, at `now`.
    pub fn new(view: &UnitView, now: Instant) -> Self {
        Self {
            unit: view.unit.clone(),
            age_at_build: view.age_seconds(now),
            built: now,
            recorded_at: view.position.map(|_| view.received_at),
            glyph: view.glyph(now),
            speed_kn: view.speed_kn,
            course_deg: view.course_deg,
            accuracy_m: view.accuracy_m,
            identity: Fetch::Pending,
            commander: Fetch::Pending,
            specification: Fetch::Pending,
        }
    }

    /// The badges the card wears: stale and/or backfilled, in the marker's own vocabulary.
    pub fn badges(&self) -> Vec<&'static str> {
        match self.glyph {
            Some(Glyph::Stale) => vec!["stale"],
            Some(Glyph::History) => vec!["backfilled"],
            _ => Vec::new(),
        }
    }

    /// How old the fix is now, as text — the only field that changes without the model changing.
    ///
    /// A read is not a per-second server call: the age at build plus monotonic elapsed time is exact
    /// enough to the second and cannot run backwards (`INV-06`).
    pub fn age_text(&self, now: Instant) -> String {
        let Some(age_at_build) = self.age_at_build else {
            return "no fix".to_string();
        };
        let age = age_at_build + now.saturating_duration_since(self.built).as_secs_f64();
        if age < 60.0 {
            format!("{:.0} s old", age.max(0.0))
        } else if age < 3600.0 {
            format!("{:.0} min old", age / 60.0)
        } else if age < 86_400.0 {
            format!("{:.1} h old", age / 3600.0)
        } else {
            format!("{:.1} days old", age / 86_400.0)
        }
    }

    /// The measured-motion rows, in the order an operator reads them.
    ///
    /// **A row exists only when the value does.** `speed_kn: Some(0.0)` is a measured stop and prints
    /// `0.0`; an absent speed prints `—` with the reason, which is not the same thing and must not look
    /// like it (`INV-01`).
    pub fn motion_rows(&self) -> Vec<(&'static str, String, bool)> {
        let has_fix = self.recorded_at.is_some();
        let mut rows = Vec::with_capacity(3);
        for (label, value, unit) in [
            ("SOG", self.speed_kn, "kn"),
            ("COG", self.course_deg, "°"),
            ("ACC", self.accuracy_m, "m"),
        ] {
            let (text, measured) = value_or_dash(value, unit, has_fix);
            rows.push((label, text, measured));
        }
        rows
    }

    /// The commander's name, or the one line that says why there is none.
    pub fn commander_line(&self) -> (&'static str, String) {
        match &self.commander {
            Fetch::Pending => ("COMMANDER", "…".to_string()),
            Fetch::Failed(error) => ("COMMANDER", format!("unavailable ({error})")),
            Fetch::Ready(entries) => match entries.iter().find(|entry| entry.is_current()) {
                Some(current) => ("COMMANDER", current.user_name.clone()),
                // An empty list is a valid answer, not a 404 and not a failure.
                None => ("COMMANDER", "no commander recorded".to_string()),
            },
        }
    }

    /// The hull's class, type and dimensions — only the ones it actually has.
    pub fn specification_rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = Vec::new();
        if let Some(identity) = self.identity.ready() {
            rows.push(("CLASS", identity.unit_class.name.clone()));
            if let Some(unit_type) = &identity.unit_class.unit_type {
                rows.push(("TYPE", unit_type.name.clone()));
            }
        }
        if let Some(versions) = self.specification.ready()
            && let Some(current) = versions
                .iter()
                .find(|version| version.is_current)
                .or(versions.first())
        {
            if let Some(loa) = current.loa_m {
                rows.push(("LOA", format!("{loa:.1} m")));
            }
            if let Some(beam) = current.beam_m {
                rows.push(("BEAM", format!("{beam:.1} m")));
            }
            if let Some(draft) = current.draft_m {
                rows.push(("DRAFT", format!("{draft:.1} m")));
            }
            if let Some(displacement) = current.displacement_full_t {
                rows.push(("DISPL", format!("{displacement:.0} t")));
            }
        }

        rows
    }
}

/// A measured value, or a dash with the reason there is none.
///
/// The reason matters: "—" alone looks like a rendering bug, and `0.0` looks like a measurement. The
/// third element says whether the value is real, so the view can dim a dash without colouring a
/// measurement.
fn value_or_dash(value: Option<f64>, unit: &str, has_fix: bool) -> (String, bool) {
    match value {
        Some(value) if unit == "°" => (format!("{value:.0}{unit}"), true),
        Some(value) => (format!("{value:.1} {unit}"), true),
        // No measurement. Which reason depends on whether there is a fix at all: a hull that has never
        // reported has no measurements *because* it has no fix.
        None => {
            let reason = if has_fix { "not measured" } else { "no fix" };
            (format!("— {reason}"), false)
        }
    }
}

/// Where the card goes, and which side of the marker it is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The card's top-left, in points.
    pub x: f64,
    pub y: f64,
    /// True when the card had to go to the left of its marker.
    pub flipped: bool,
}

impl Placement {
    pub fn rect(&self) -> (f64, f64, f64, f64) {
        (self.x, self.y, self.x + WIDTH, self.y + HEIGHT)
    }
}

/// Place the card beside `anchor`, on the side the window has room for.
///
/// Right of the marker by default, left when the right edge would overflow; centred vertically on the
/// marker, pushed down or up when that would overflow. Font sizes, windows and wall layouts all change;
/// the arithmetic does not, so it is a pure function with a property test rather than a screenshot
/// someone checks by eye (`INV-10`).
pub fn place(anchor: (f64, f64), viewport: (f64, f64)) -> Placement {
    let (ax, ay) = anchor;
    let (vw, vh) = viewport;
    let prefer_right = ax + GAP + WIDTH + MARGIN <= vw;
    let flipped = !prefer_right;
    let x = if prefer_right {
        ax + GAP
    } else {
        ax - GAP - WIDTH
    };
    let y = ay - HEIGHT / 2.0;
    Placement {
        x: clamp(x, MARGIN, vw - WIDTH - MARGIN),
        y: clamp(y, MARGIN, vh - HEIGHT - MARGIN),
        flipped,
    }
}

/// Clamp into `[low, high]`, keeping `low` when the window is smaller than the card — a card pinned to
/// the top-left is visible, one pushed off the left is not.
fn clamp(value: f64, low: f64, high: f64) -> f64 {
    if high < low {
        low
    } else {
        value.max(low).min(high)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Pos;
    use chrono::TimeZone;

    fn view(speed: Option<f64>, fresh: bool) -> UnitView {
        UnitView {
            unit: Unit {
                id_unit: 8,
                name: "KRI Abdul Halim Perdanakusuma".into(),
                hull_number: Some("KRI-355".into()),
            },
            position: Some(Pos::new(-6.1, 106.9)),
            previous: None,
            travel: None,
            age_at_receipt: if fresh { 2.0 } else { 3.0 * 3600.0 },
            receipt: Instant::now(),
            backfilled: false,
            course_deg: None,
            accuracy_m: None,
            speed_kn: speed,
            received_at: Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap(),
        }
    }

    #[test]
    fn an_absent_measurement_is_a_dash_with_a_reason_and_a_measured_zero_is_a_zero() {
        // INV-01 where a reader can see it: `0.0` is a measurement, `— not measured` is not.
        let measured = Card::new(&view(Some(0.0), true), Instant::now());
        let rows = measured.motion_rows();
        assert_eq!(
            rows[0].1, "0.0 kn",
            "a measured stop prints as a measurement"
        );
        assert!(rows[0].2, "and is marked as real");

        let absent = Card::new(&view(None, true), Instant::now());
        let rows = absent.motion_rows();
        assert_eq!(rows[0].1, "— not measured", "an absent speed is not a zero");
        assert!(!rows[0].2, "and is marked as not real");
        assert!(
            !rows[0].1.contains("0.0"),
            "no digit that could be mistaken for a measurement"
        );
    }

    #[test]
    fn a_hull_with_no_fix_says_so_rather_than_showing_zeros() {
        let mut silent = view(None, true);
        silent.position = None;
        let card = Card::new(&silent, Instant::now());
        assert_eq!(card.age_text(Instant::now()), "no fix");
        assert_eq!(card.motion_rows()[0].1, "— no fix");
        assert!(
            card.badges().is_empty(),
            "a hull with no fix is not stale; it is unknown"
        );
    }

    #[test]
    fn the_age_text_advances_without_the_model_changing() {
        // INV-06 and the ticket's "once a second" without a re-render: the model is built once, and
        // the text is a function of the clock.
        let card = Card::new(&view(None, true), Instant::now());
        let built = card.built;
        let first = card.age_text(built);
        let later = card.age_text(built + std::time::Duration::from_secs(10));
        assert_ne!(first, later, "the age must advance: {first} → {later}");
        assert_eq!(
            card.speed_kn, None,
            "and nothing else about the card changed"
        );
    }

    #[test]
    fn stale_and_backfilled_badges_use_the_markers_vocabulary() {
        let stale = Card::new(&view(None, false), Instant::now());
        assert_eq!(stale.badges(), vec!["stale"]);

        let mut history = view(None, true);
        history.backfilled = true;
        let history = Card::new(&history, Instant::now());
        assert_eq!(history.badges(), vec!["backfilled"]);

        let mut both = view(None, false);
        both.backfilled = true;
        // INV-03 wins over the age at the glyph, so the badge says what the marker draws.
        assert_eq!(
            Card::new(&both, Instant::now()).badges(),
            vec!["backfilled"]
        );
    }

    #[test]
    fn an_empty_commander_list_reads_as_nobody_not_as_failure() {
        let mut card = Card::new(&view(None, true), Instant::now());
        assert_eq!(
            card.commander_line(),
            ("COMMANDER", "…".to_string()),
            "pending is a skeleton, not a gap"
        );
        card.commander = Fetch::Ready(Vec::new());
        assert_eq!(card.commander_line().1, "no commander recorded");
        card.commander = Fetch::Ready(vec![UnitCommander {
            id_unit: 8,
            id_user: 3,
            user_name: "KRI Example".into(),
            from_at: Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap(),
            to_at: None,
            assigned_by: 1,
            note: String::new(),
        }]);
        assert_eq!(
            card.commander_line().1,
            "KRI Example",
            "the current entry is the one with no to_at"
        );
    }

    #[test]
    fn the_card_always_stays_inside_the_window_and_points_at_its_marker() {
        // INV-10 as a property: for every marker position and window shape, the card is fully on
        // screen, and it is beside the marker rather than on top of it.
        let viewports = [
            (1280.0, 720.0),
            (640.0, 360.0),
            (3840.0, 2160.0),
            (400.0, 300.0),
        ];
        for (vw, vh) in viewports {
            for xi in 0..=20 {
                for yi in 0..=20 {
                    let anchor = (vw * f64::from(xi) / 20.0, vh * f64::from(yi) / 20.0);
                    let placement = place(anchor, (vw, vh));
                    let (left, top, right, bottom) = placement.rect();
                    assert!(
                        left >= 0.0
                            && top >= 0.0
                            && right <= vw.max(WIDTH)
                            && bottom <= vh.max(HEIGHT),
                        "INV-10: card {placement:?} leaves the {vw}x{vh} window for anchor {anchor:?}"
                    );
                    // It sits beside the marker: the gap on its own side, unless the window forced a
                    // clamp at an edge.
                    if placement.flipped {
                        assert!(
                            right <= anchor.0 || left <= MARGIN,
                            "flipped card overlaps its marker: {placement:?} vs {anchor:?}"
                        );
                    } else {
                        assert!(
                            left >= anchor.0 || left <= MARGIN,
                            "card overlaps its marker: {placement:?} vs {anchor:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_card_flips_to_the_left_instead_of_clipping_the_right_edge() {
        let placement = place((1200.0, 360.0), (1280.0, 720.0));
        assert!(
            placement.flipped,
            "a marker near the right edge puts the card on its left"
        );
        let (_, _, right, _) = placement.rect();
        assert!(right <= 1280.0 - MARGIN, "and the card still fits: {right}");
        let placement = place((100.0, 360.0), (1280.0, 720.0));
        assert!(
            !placement.flipped,
            "plenty of room on the right beats flipping"
        );
    }
}
