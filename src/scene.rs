//! The named states a capture renders: a fixture feed, no network, no wall clock.
//!
//! A scene is `&'static [(&'static str, fn(&mut State))]` in `AppSpec` — Rune's own vocabulary — so
//! `--scene picture --snap out/picture.png` works with nothing from us beyond the state it builds.
//! Rune starts a scene at a logical time of zero and feeds `poll` a **virtual** clock, which is why a
//! capture is byte-identical between runs: no `Instant::now()` decides what the picture says.
//!
//! The fixture is **wire-shaped** (`fixtures/scenes/feed.json`, the `GET /live-feed/positions`
//! envelope) so a scene loads through the same `FeedPosition` the REST client parses. A fixture
//! written from our own structs would prove we agree with ourselves.
//!
//! What a scene deliberately does **not** show: the map. MapLibre draws on its own surface, and a
//! headless snap renders Rune's tree only. The map's own witness is `--probe-map` (#5); the scenes
//! here verify markers, cards and the header, which is what a reader of a screenshot actually judges.

use std::path::PathBuf;

use crate::data::types::{Envelope, FeedPosition};
use crate::map::geometry::Camera;
use crate::ui::State;

/// The camera every marker scene uses, and the one the assertions project against. Fixed here so a
/// capture and the test that checks its pixels cannot disagree about where a marker should be.
pub const CAMERA: Camera = Camera {
    center: (-6.145, 106.895),
    zoom: 12.0,
};

/// The hull the `card-open` scene selects.
pub const CARD_UNIT: i64 = 3;
/// A hull with a fresh fix: the one the pixel assertion looks for.
pub const LIVE_UNIT: i64 = 12;
/// A hull whose fix is older than `stale_after`: the dim marker.
pub const STALE_UNIT: i64 = 11;

/// Where the fixture lives. `env!("CARGO_MANIFEST_DIR")`, like the map seed: a scene is a repository
/// artefact, not something a deployment ships.
pub fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/scenes/feed.json")
}

/// Parse the fixture into the same type the REST client produces.
pub fn fixture() -> Result<Vec<FeedPosition>, String> {
    let path = fixture_path();
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let envelope: Envelope<Vec<FeedPosition>> = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not the positions envelope: {error}", path.display()))?;
    Ok(envelope.data)
}

/// The picture a marker scene shows, or an empty picture when the fixture cannot be read.
///
/// A scene that cannot read its fixture renders an empty wall rather than panicking: the capture then
/// differs from its golden, which is the failure a human sees, and the message names the file.
fn load(state: &mut State) {
    match fixture() {
        Ok(entries) => state.load_fixture(&entries),
        Err(error) => eprintln!("arcons-cc: scene: {error}"),
    }
    state.camera = CAMERA;
}

fn empty(_state: &mut State) {}

fn picture(state: &mut State) {
    load(state);
}

fn stale(state: &mut State) {
    load(state);
    // Centred on the stale hull, so the dim marker is the thing the picture is about.
    if let Some(view) = state
        .picture
        .units()
        .iter()
        .find(|unit| unit.unit.id_unit == STALE_UNIT)
        && let Some(drawn) = view.drawn(state.now())
    {
        state.camera = Camera::new((drawn.lat, drawn.lon), CAMERA.zoom);
    }
}

fn card_open(state: &mut State) {
    load(state);
    state.select(CARD_UNIT);
}

/// The hull whose picture the `photo` scene draws: it has a fix in the fixture.
pub const PHOTO_UNIT: i64 = CARD_UNIT;

/// The picture fixture: a hull silhouette, drawn from above. Its **content** is not the point — that it
/// is a real decoded image reaching the marker is, and a scene may read a file in this repository while
/// it may never open a socket (`INV-11`).
pub const PHOTO_FIXTURE: &[u8] = include_bytes!("../fixtures/scenes/hull.png");

/// The `photo` scene: close enough that a hull is its picture, with the dimensions and the bytes a real
/// wall would have read. No network is touched: this is the same drawing path the live wall uses, fed from
/// files instead of from Minos.
fn photo(state: &mut State) {
    load(state);
    state.range = crate::units::mark::Range::Picture;
    state.sizes.insert(
        PHOTO_UNIT,
        crate::units::mark::Size {
            length_m: 120.0,
            beam_m: 16.0,
        },
    );
    // **Decoded before the first frame.** A capture that raced the decoder would be a capture of a
    // symbol on some runs and a picture on others — the determinism `INV-11` promises, and exactly the
    // class of bug #35 was. Bounded, because a scene that hangs is worse than one that draws the symbol.
    // The handle is kept by the state, exactly as the live wall keeps it when the bytes arrive.
    let bytes: std::sync::Arc<[u8]> = std::sync::Arc::from(PHOTO_FIXTURE);
    let image = state.image_cache().load(bytes);
    state.pictures.insert(PHOTO_UNIT, image.clone());
    for _ in 0..400 {
        if image.is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // Centred on the hull, at the zoom where a 120 m ship is a 25 pt picture.
    if let Some(view) = state
        .picture
        .units()
        .iter()
        .find(|unit| unit.unit.id_unit == PHOTO_UNIT)
        && let Some(drawn) = view.drawn(state.now())
    {
        state.camera = Camera::new((drawn.lat, drawn.lon), crate::units::mark::PHOTO_ZOOM);
    }
}

/// One scene: a name and the function that builds its state.
pub type Scene = (&'static str, fn(&mut State));

/// The scene registry, handed to `AppSpec::scenes`.
pub const SCENES: &[Scene] = &[
    ("empty", empty),
    ("picture", picture),
    ("stale", stale),
    ("card-open", card_open),
    ("photo", photo),
];

/// Look a scene up by the name `scripts/capture.sh` was given.
pub fn find(name: &str) -> Option<fn(&mut State)> {
    SCENES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, scene)| *scene)
}

/// Build a scene's state without a window, for a test that wants the same state a capture renders.
pub fn state_for(name: &str) -> State {
    let mut state = State::default();
    match find(name) {
        Some(scene) => scene(&mut state),
        None => eprintln!(
            "arcons-cc: no scene {name:?}; scenes: {}",
            names().join(", ")
        ),
    }
    state
}

pub fn names() -> Vec<&'static str> {
    SCENES.iter().map(|(name, _)| *name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fixture_is_the_wire_shape_and_says_what_the_scenes_need() {
        let entries = fixture().expect("the fixture parses as the positions envelope");
        assert_eq!(entries.len(), 40, "the ticket asks for a 40-vessel picture");

        let silent = entries
            .iter()
            .filter(|hull| hull.position.is_none())
            .count();
        assert_eq!(silent, 1, "one hull has never reported (INV-02)");
        assert!(
            entries.iter().any(|hull| hull
                .position
                .as_ref()
                .is_some_and(|fix| fix.age_seconds > crate::units::STALE_AFTER.as_secs_f64())),
            "one fix is older than the stale threshold"
        );
        assert!(
            entries
                .iter()
                .any(|hull| hull.position.as_ref().is_some_and(|fix| fix.backfilled)),
            "one fix is backfilled (INV-03)"
        );
        assert!(
            entries.iter().any(|hull| hull
                .position
                .as_ref()
                .is_some_and(|fix| fix.course_deg.is_none())),
            "one fix has no course"
        );
    }

    #[test]
    fn every_scene_is_named_and_can_be_built() {
        // `--list-scenes` prints these; a scene that panics on build is a capture that fails at
        // 2 a.m. with a backtrace instead of a picture.
        assert_eq!(
            names(),
            vec!["empty", "picture", "stale", "card-open", "photo"]
        );
        for name in names() {
            let state = state_for(name);
            if name == "empty" {
                assert!(state.picture.is_empty());
            } else {
                assert_eq!(state.picture.len(), 40, "{name} loads the fixture");
            }
        }
        let card = state_for("card-open");
        assert_eq!(card.selected, Some(CARD_UNIT), "the card scene has a card");
        assert!(card.card.is_some());
    }

    #[test]
    fn a_scene_reads_no_clock_and_no_network() {
        // Ages come from the fixture's own `age_seconds`, so two builds of the same state agree.
        let first = state_for("picture");
        let second = state_for("picture");
        let ages = |state: &State| -> Vec<String> {
            state
                .picture
                .units()
                .iter()
                .map(|unit| format!("{:?}", unit.age_at_receipt))
                .collect()
        };
        assert_eq!(
            ages(&first),
            ages(&second),
            "INV-11: a scene has no wall clock in it"
        );
    }
}
