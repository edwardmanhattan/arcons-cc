//! **The named states a capture renders** — the desk's side of Rune's `AppSpec::scenes`.
//!
//! A scene is `&'static [(&'static str, fn(&mut State))]`, exactly as on the wall, so
//! `--scene time-field --snap out/desk-time.png` works with nothing from us beyond the state it
//! builds. Rune starts a scene at logical time zero, and the desk reads no clock of its own, so
//! two runs are the same picture.
//!
//! **Two states of the time field**, because the calendar is the one thing that covers the panel it
//! belongs to: `time-field` is the panel at rest, with the regime line and the UTC readout visible, and
//! `time-calendar` is the same panel with the popover open. A single scene with the calendar open would
//! hide the banner the ticket exists to show, which is how the first capture of this screen was wrong.
//!
//! **Two states of the working surface** (#277): `batch` has a draft of two vessels — one of them a
//! hull that never reported — with the selection on a third hull, so the tray, the badge and the prefill
//! are all in one picture; `empty` has nothing selected and an empty draft, which is the state the
//! empty inspector and the disabled send exist for.

use arcons_cc::verify::Bounds;

use crate::batch::{Fix, Row};
use crate::picker::{LastKnown, Picker, Vessel};
use crate::ui::desk::Desk;
use crate::ui::{State, WIB_OFFSET_MS};

/// One scene: a name and the function that builds its state.
pub type Scene = (&'static str, fn(&mut State));

/// Every scene the binary can render, in the order `--scenes` prints them.
pub const SCENES: &[Scene] = &[
    ("time-field", time_field),
    ("time-calendar", time_calendar),
    ("batch", desk_batch),
    ("empty", desk_empty),
    ("no-picture", no_picture),
];

/// The sample ADR-0009's wireframe names: 20 November 2026, 14:03:22 WIB, the calendar closed, and the
/// server's clock three seconds behind the wall clock. Closed, so the regime line and the UTC readout
/// are in the picture: they are what this ticket's proof is about.
fn time_field(state: &mut State) {
    *state = State::sample();
    state.calendar_open = false;
}

/// The same panel with the calendar open, to witness the anchoring: it covers what is behind it and
/// moves nothing.
fn time_calendar(state: &mut State) {
    *state = State::sample();
}

/// **The draft in use.** Two hulls are in the batch — Diponegoro, which reported 23 seconds ago and is
/// live, and Bung Tomo, which has never reported and therefore has no position to prefill — and the
/// selection is on Halasan, a third hull, so the tray's `+ Add vessel` is on screen and the inspector is
/// showing a hull that is not in the draft.
fn desk_batch(state: &mut State) {
    *state = State::sample();
    state.calendar_open = false;
    let base_ms = state.epoch_ms;
    let mut desk = Desk::new(picture(base_ms));
    let rows: Vec<Row> = [1_u64, 2]
        .into_iter()
        .filter_map(|id_unit| {
            let vessel = desk.picker().get(id_unit)?;
            let recorded_at_ms = vessel
                .last
                .as_ref()
                .map_or(base_ms, |last| last.recorded_at_ms);
            Some(row_for(vessel, recorded_at_ms))
        })
        .collect();
    for row in rows {
        desk.add_row(row);
    }
    desk.set_source_note("Plot 4 · phone call 14:03");
    state.desk = desk;
    state.select(3);
}

/// **Nothing chosen yet.** The picture is there and the batch is empty: the empty inspector, the disabled
/// send, and a list with no selected row are the states this scene exists to show.
fn desk_empty(state: &mut State) {
    *state = State::sample();
    state.calendar_open = false;
    let base_ms = state.epoch_ms;
    state.desk = Desk::new(picture(base_ms));
}

/// **The cold start with no cache** (#284): the first run. The list is empty, the label says no picture has
/// been read, and the inspector has no subject; the refresh control is what asks for one. A corrupt cache
/// lands here too, with one line in the log rather than an empty state dressed up as a picture.
fn no_picture(state: &mut State) {
    *state = State::sample();
    state.calendar_open = false;
    state.desk = Desk::new(Picker::new(Vec::new()));
    state.picture_status = crate::picture::Status::None;
}

/// The standing picture, in the age order the ADR asks the list for: the live hull, the backfills oldest
/// last, and the silent ones at the bottom. Ages are carried both as seconds (what the row shows) and as
/// an instant (what the regime and the inspector use), and both come from one number so they cannot tell
/// two stories about the same hull.
fn picture(base_ms: i64) -> Picker {
    Picker::new(vec![
        vessel(
            1,
            "KRI DIPONEGORO",
            "368",
            Some(last(
                base_ms,
                23.0,
                false,
                -6.30330,
                106.88120,
                Some(12.4),
                Some(284.0),
            )),
        ),
        vessel(
            3,
            "KRI HALASAN",
            "372",
            Some(last(
                base_ms,
                5.0 * 3_600.0,
                true,
                -6.10230,
                106.87120,
                Some(8.1),
                Some(91.0),
            )),
        ),
        vessel(
            4,
            "KRI SULTAN HASANUDDIN",
            "366",
            Some(last(
                base_ms,
                8.0 * 3_600.0,
                true,
                -6.40120,
                106.90210,
                Some(14.7),
                Some(12.0),
            )),
        ),
        vessel(
            5,
            "KRI A. YANI",
            "352",
            Some(last(
                base_ms,
                3.0 * 86_400.0,
                true,
                -5.90110,
                106.75030,
                Some(9.3),
                Some(178.0),
            )),
        ),
        vessel(
            6,
            "KRI MAKASSAR",
            "590",
            Some(last(
                base_ms,
                40.0 * 86_400.0,
                true,
                -6.70140,
                107.10110,
                None,
                None,
            )),
        ),
        vessel(2, "KRI BUNG TOMO", "397", None),
        vessel(7, "KRI CUT NYAK DHEN", "375", None),
        vessel(8, "KRI SURYA", "383", None),
    ])
}

fn vessel(id_unit: u64, name: &str, hull_number: &str, last: Option<LastKnown>) -> Vessel {
    Vessel {
        id_unit,
        name: name.to_string(),
        hull_number: hull_number.to_string(),
        last,
    }
}

/// One last known position. The instant and the age are two views of one number: `age_seconds` is what
/// the list shows, `recorded_at_ms` is what the verdict and the inspector read.
fn last(
    base_ms: i64,
    age_seconds: f64,
    backfilled: bool,
    latitude: f64,
    longitude: f64,
    speed_kn: Option<f64>,
    course_deg: Option<f64>,
) -> LastKnown {
    LastKnown {
        recorded_at_ms: base_ms - (age_seconds * 1_000.0) as i64,
        age_seconds,
        backfilled,
        latitude,
        longitude,
        speed_kn,
        course_deg,
        accuracy_m: None,
    }
}

/// A row built from the picture, or from the field's time when the hull has no position. A never-reported
/// hull gets a row with no coordinates on purpose: the batch keeps it on the board with its reason, and the
/// inspector's empty state is what this scene is for.
fn row_for(vessel: &Vessel, recorded_at_ms: i64) -> Row {
    let last = vessel.last.as_ref();
    Row {
        id_unit: vessel.id_unit,
        name: vessel.name.clone(),
        fix: Fix {
            recorded_at_ms,
            offset_ms: WIB_OFFSET_MS,
            latitude: last.map(|last| last.latitude),
            longitude: last.map(|last| last.longitude),
            speed_kn: last.and_then(|last| last.speed_kn),
            course_deg: last.and_then(|last| last.course_deg),
            accuracy_m: last.and_then(|last| last.accuracy_m),
        },
    }
}

/// **The sample picture with a hull selected**, for `State::sample` and therefore for `--snap` with no
/// scene: the panel always has a subject, and the field's six segments are never drawn about nobody.
///
/// The selection is KRI SULTAN HASANUDDIN, the hull ADR-0009's wireframe panel is about. Scenes that want
/// a different state say so themselves; this is only the default the window opens with.
pub fn sample_desk(now_ms: i64) -> Desk {
    let mut desk = Desk::new(picture(now_ms));
    desk.select(4);
    desk
}

/// The scene function for `name`, or `None`.
pub fn find(name: &str) -> Option<fn(&mut State)> {
    SCENES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, scene)| *scene)
}

/// Build a scene's state without a window, for a test that wants what a capture renders.
pub fn state_for(name: &str) -> State {
    let mut state = State::sample();
    match find(name) {
        Some(scene) => scene(&mut state),
        None => eprintln!("desk: no scene {name:?}; scenes: {}", names().join(", ")),
    }
    state
}

/// The scene names, one per line, as `--scenes` prints them.
pub fn names() -> Vec<&'static str> {
    SCENES.iter().map(|(name, _)| *name).collect()
}

/// The bounds a desk golden is compared within.
///
/// **Unmeasured, and honestly so.** `arcons_cc::verify::golden_bounds` is keyed by the wall's scenes
/// and does not name one of ours — the wall does not know what the desk draws (#276). Every desk scene
/// therefore starts from its `DEFAULT_BOUNDS`: two steps of channel tolerance, 200 pixels changed by
/// more than one step, and 600 pixels changed by exactly one — the numbers the wall's ordinary scenes
/// use, and the numbers `--compare` prints on every run.
///
/// **Where a measurement goes:** here, beside the scene list it describes. When CI's lavapipe has
/// rendered these scenes and the first real diff is recorded, this function returns that measured
/// `Bounds` for the scene that measured it, exactly as `golden_bounds` does for the wall's exceptions.
/// It does not go in the wall's table, and it is never a number picked to make a diff pass.
pub fn bounds(name: &str) -> Bounds {
    arcons_cc::verify::golden_bounds(name)
}
