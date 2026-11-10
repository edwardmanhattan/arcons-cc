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
    state.set_camera(CAMERA);
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
        state.set_camera(Camera::new((drawn.lat, drawn.lon), CAMERA.zoom));
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
/// wall would have read. No network is touched: this is the same **route** the live wall takes — the bytes
/// arrive as a `Reply::Image` and go through `ui::apply_reply`, which is what offers them to the cache and
/// schedules the frame that draws them (`#54`). A capture of this scene therefore covers bytes → cache →
/// marker, not just the drawing half.
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
    // **The reply path, then decoded before the first frame.** The reply is the live wall's own door
    // (`apply_reply` → `offer_picture`), so a regression in *that* — the bug #51 was — breaks a golden
    // instead of a screenshot nobody takes. The wait is what makes it a capture: `INV-11` promises two
    // runs identical, and a frame that raced the decoder would be a symbol on some runs and a ship on
    // others (#35). Bounded, because a scene that hangs is worse than one that draws the symbol.
    crate::ui::apply_reply(
        state,
        &crate::data::Reply::Image(PHOTO_UNIT, Ok(PHOTO_FIXTURE.to_vec())),
    );
    for _ in 0..400 {
        if state
            .pictures
            .get(&PHOTO_UNIT)
            .is_some_and(|image| image.is_ready())
        {
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
        state.set_camera(Camera::new(
            (drawn.lat, drawn.lon),
            crate::units::mark::PHOTO_ZOOM,
        ));
    }
}

/// The hull each pane of the `panes` scene is centred on, and the zooms those panes use. Three
/// different cameras at three different zooms, because the claim the capture makes is that **each pane
/// projects against its own frame**: a marker drawn in another pane's camera would be visibly wrong at
/// these zooms, and a pane that reused one frame for all three would put every hull in the first pane.
pub const PANES: &[(i64, f64)] = &[(LIVE_UNIT, 12.0), (CARD_UNIT, 13.0), (STALE_UNIT, 11.0)];

/// The pane the `panes` scene has focused: the middle one, so the ring is not on the pane at the corner
/// where a reader might mistake it for the panel's own border.
pub const PANES_FOCUSED: usize = 1;

/// The `panes` scene (#64, #68): three panes, three cameras, one window.
///
/// This is the scene Multi View exists to make capturable. Each pane's frame comes from its own layout
/// (`ui::pane_laid_out`), so a marker is projected against the pane it is drawn in — and
/// `scripts/assert.sh out/panes.png` checks exactly that, by calling the same `Layout::rects` the view
/// lays the panes out with.
fn panes(state: &mut State) {
    panes_wall(state, true)
}

/// The same wall with the border drag **left to a pointer**: `panes-pan`'s script presses the border and
/// drags it to the share the scene above sets in code.
///
/// This is the only capture in the repo driven by an input event rather than by a function call, and it
/// exists because a whole class of bug is invisible to every other kind: the drag that reaches nothing
/// (#80), the point measured in the wrong coordinate space (#84), a press that lands on the wrong element.
/// `fixtures/scripts/panes-pan.json` makes the gesture; `scripts/capture.sh` hands the script to Rune;
/// `verify` projects the hulls against the 58% layout the drag must produce, so a dropped gesture moves the
/// border back to half and the assertion goes red rather than the picture going quietly wrong.
fn panes_pan(state: &mut State) {
    panes_wall(state, false)
}

fn panes_wall(state: &mut State, drag_the_border: bool) {
    load(state);
    let mut ids = vec![state.layout.focused()];
    for (index, (id_unit, zoom)) in PANES.iter().enumerate() {
        let camera = state
            .picture
            .units()
            .iter()
            .find(|unit| unit.unit.id_unit == *id_unit)
            .and_then(|unit| unit.drawn(state.now()))
            .map(|drawn| Camera::new((drawn.lat, drawn.lon), *zoom))
            .unwrap_or(Camera::new(CAMERA.center, *zoom));
        if index == 0 {
            state.set_camera(camera);
        } else {
            // Split the pane the operator asked for, along its longer edge, as the menu does.
            let from = ids[index - 1];
            if let Some(new) = state.split_pane(from, camera) {
                ids.push(new);
            }
        }
    }
    if let Some(id) = ids.get(PANES_FOCUSED) {
        state.layout.focus(*id);
    }
    // **One border dragged** (#69), through the two calls the gesture makes: find the border under the
    // pointer, ask it for the ratio the pointer means. The root split is dragged from half to 58%, so
    // the capture is of a layout someone moved rather than of a layout a constructor produced — and
    // `assert.sh` projects against the same `Layout::rects`, so a drag that moved a hull out of its
    // pane fails there. `panes-pan` skips this: its pointer does the same thing from the outside.
    if !drag_the_border {
        return;
    }
    let viewport = state.pane_viewport();
    let middle = (viewport.w as f32 / 2.0, viewport.h as f32 / 2.0);
    if let Some(border) = state.layout.border_at(viewport, middle, state.pane_grip()) {
        let pointer = border.start + border.span * PANES_DRAGGED;
        state.layout.set_ratio(&border, border.ratio_for(pointer));
    }
}

/// Where the `panes` scene drags its first border to: the share of the panel the first pane keeps.
pub const PANES_DRAGGED: f32 = 0.58;

/// Where the `pane-menu` scene right-clicks: the menu's own point, in the pane's points.
pub const MENU_AT: (f32, f32) = (300.0, 220.0);

/// The `panes-max` scene (#85): one pane **full size over the wall**, the tiling still behind it.
///
/// The pane is the middle one — not the first — so the capture says the picture is the pane that was asked
/// for rather than whichever pane happened to be first, and `verify` projects its hulls against a frame the
/// size of the *panel*, which is the claim: a maximised pane is a bigger rectangle, not a different pane.
fn panes_max(state: &mut State) {
    panes(state);
    let Some(id) = state.pane_rects().get(1).map(|(id, _)| *id) else {
        return;
    };
    // The gesture, not a shortcut: the same function the menu row and the close plate call.
    crate::ui::full_size_for_scene(state, id);
}

/// The `panes-named` scene (#83): three panes, two of them **named** — one short enough to read and one long
/// enough to be cut — and one unnamed, so the capture says both "a name is drawn" and "no name draws
/// nothing". The names are set the way the sheet sets them, through `shown_name`, so a name the wall would
/// trim is drawn trimmed here too.
fn panes_named(state: &mut State) {
    panes(state);
    let ids: Vec<crate::ui::ViewId> = state.pane_rects().into_iter().map(|(id, _)| id).collect();
    if let Some(id) = ids.first().copied()
        && let Some(pane) = state.pane_mut(id)
    {
        pane.name = crate::ui::shown_name("Surabaya approaches");
    }
    if let Some(id) = ids.get(2).copied()
        && let Some(pane) = state.pane_mut(id)
    {
        pane.name =
            crate::ui::shown_name("the strait at first light, and the traffic separation scheme");
    }
    // The middle pane is deliberately left unnamed.
}

/// The `panes-gaps` scene (#82): the same wall with **space between the panes** — the settings sheet's
/// first setting, and the only one so far.
///
/// It exists because the gate must cover both looks and because a capture cannot read the operator's
/// `settings.toml`: a scene sets the value in its own code, so two machines render the same picture
/// (`INV-11`). The gap is `tokens::GAP` — the same 8 pt the toggle turns on, not a number invented here:
/// a capture of a look the display cannot produce would be a lie about the wall.
fn panes_gaps(state: &mut State) {
    panes(state);
    state.settings.pane_gap = crate::ui::tokens::GAP;
}

/// The `settings` scene (#82): a wall with its **settings sheet open**, which is the only picture of the
/// one modal surface on a wall. It is a single pane so the sheet covers the picture rather than a tiling:
/// what a reader needs to see is the sheet, where it sits and what it says.
fn settings_sheet(state: &mut State) {
    load(state);
    state.settings_open = true;
}

/// The `panes-menu` scene (#81): **a tiled wall with its menu open**, which is the only wall that offers
/// `Close pane`. The one-pane `pane-menu` is its control: one row there, two here, and the surface has to be
/// tall enough for both.
fn panes_menu(state: &mut State) {
    panes(state);
    state.menu = Some(crate::ui::Menu {
        pane: state.layout.focused(),
        at: MENU_AT,
    });
}

/// The `pane-menu` scene (#77): **a wall with one pane and its menu open**.
///
/// The one shape the menu had never been drawn on, and the only wall that needs it — it is the only route
/// to "Focus on new pane", so a menu that does not draw here is a wall that can never have a second pane.
/// The capture is the witness the pointer-driven bug could not have: no test right-clicks.
fn pane_menu(state: &mut State) {
    load(state);
    state.menu = Some(crate::ui::Menu {
        pane: state.layout.focused(),
        at: MENU_AT,
    });
}

/// The `panes-drag` scene (#70): a pane lifted, and the gap it will land in.
///
/// The lift is built the way the gesture builds it — `drop_for` on the point the handle has been dragged
/// to — so the capture is of the same state a hand would produce, not of a hand-written `Drop`. The point
/// is the middle of the first pane, which is a **swap** (the middle of a pane swaps; its edges re-tile),
/// and the gap is drawn from the preview layout, so what the picture shows is where the pane lands.
fn panes_drag(state: &mut State) {
    panes(state);
    let viewport = state.pane_viewport();
    let Some((dragged, _)) = state.pane_rects().into_iter().last() else {
        return;
    };
    let Some((target, rect)) = state.pane_rects().into_iter().next() else {
        return;
    };
    if dragged == target {
        return;
    }
    // **The top edge, not the middle.** The middle is a swap — which needs no geometry — and it was the
    // only drop a capture made, so a `Drop::Retile` that quietly split the wrong way stayed invisible
    // (#100). The top band of the first pane makes the picture show a stacked slot, and `verify` says
    // that slot is wider than it is tall.
    let at = (rect.x + rect.w / 2.0, rect.y + rect.h * 0.08);
    let drop = state.layout.drop_for(viewport, dragged, at);
    state.dragging = Some(crate::ui::PaneDrag {
        pane: dragged,
        drop,
    });
    // The pane being lifted is the one the operator picked up, so it is the focused one.
    if drop.is_some() {
        state.layout.focus(dragged);
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
    ("panes", panes),
    ("panes-drag", panes_drag),
    ("panes-pan", panes_pan),
    ("panes-menu", panes_menu),
    ("panes-gaps", panes_gaps),
    ("panes-named", panes_named),
    ("panes-max", panes_max),
    ("settings", settings_sheet),
    ("pane-menu", pane_menu),
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
            vec![
                "empty",
                "picture",
                "stale",
                "card-open",
                "photo",
                "panes",
                "panes-drag",
                // The only scene whose picture is made by an **input event** rather than by a call
                // (`fixtures/scripts/panes-pan.json`), so it is also the only witness a dropped
                // gesture has (#80).
                "panes-pan",
                "panes-menu",
                "panes-gaps",
                "panes-named",
                "panes-max",
                "settings",
                "pane-menu"
            ]
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

        // Multi View's witness: one pane per entry in `PANES`, at that entry's zoom, and the focus on
        // the one the constant names.
        let panes = state_for("panes");
        assert_eq!(panes.panes.len(), PANES.len(), "one pane per camera");
        for (index, (id_unit, zoom)) in PANES.iter().enumerate() {
            let id = panes.layout.ids()[index];
            let pane = panes.pane(id).expect("the scene's pane");
            assert_eq!(pane.camera.zoom, *zoom, "pane {index} is at its own zoom");
            let hull = panes
                .picture
                .units()
                .iter()
                .find(|unit| unit.unit.id_unit == *id_unit)
                .and_then(|unit| unit.drawn(panes.now()))
                .expect("the hull the pane is centred on");
            assert!(
                (pane.camera.center.0 - hull.lat).abs() < 1e-9
                    && (pane.camera.center.1 - hull.lon).abs() < 1e-9,
                "pane {index} is centred on its own hull"
            );
        }
        assert_eq!(
            panes.layout.focused(),
            panes.layout.ids()[PANES_FOCUSED],
            "the focus is on the pane the constant names"
        );
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
