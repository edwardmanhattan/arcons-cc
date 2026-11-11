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

use crate::data::types::{
    Envelope, FeedPosition, Installation, InstallationType, Pages, TacticalObjectFacets,
    TacticalObjectManifest, TacticalObjectPlacement, TniCommandPost, UnitOrganisationDetail,
};
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
    let focused = state.layout.focused();
    state.select_in(focused, CARD_UNIT);
}

/// The `panes-cards` scene (#99): **two panes, and a card in one of them**.
///
/// The capture the operator asked for: press a hull in this pane and the neighbour must not follow. The
/// selection belongs to the pane, so the second pane draws no card at all — and `verify` counts the card
/// titles in the picture: one long run of name-sized ink, inside the pane that was pressed.
fn panes_cards(state: &mut State) {
    panes(state);
    let rects = state.pane_rects();
    let Some((first, _)) = rects.first().copied() else {
        return;
    };
    // **The same hull in every frame**: the others are pulled back to a wide zoom, so the hull the card is
    // about is drawn in all three panes. Without that, a wall-wide selection would draw no second card
    // simply because the hull was off-screen — and the capture would pass while the bug was there.
    for (id, _) in rects.iter().skip(1) {
        state.set_camera_of(*id, Camera::new(CAMERA.center, PANES_WIDE));
    }
    state.select_in(first, CARD_UNIT);
}

/// The zoom the unpressed panes of `panes-cards` hold: wide enough that the card's hull is in every frame,
/// which is what makes "no second card" a claim about the selection rather than about the camera.
pub const PANES_WIDE: f64 = 8.0;

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
    state.focused_pane_mut().view.range = crate::units::mark::Range::Picture;
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
    wait_for_picture(state);
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

/// Wait, **bounded**, for the scene's decoded picture: a capture that raced the decoder would draw a
/// symbol on some runs and a ship on others, and `INV-11` promises two runs identical (#35). A picture
/// that cannot decode draws the symbol rather than hanging the capture.
fn wait_for_picture(state: &mut State) {
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
}

/// The chart zoom pane B of `panes-range` holds: below `PHOTO_EXIT`, so the same hull that is a picture
/// in pane A is a symbol here.
pub const PANES_RANGE_CHART: f64 = 13.0;

/// The `panes-range` scene (#115): **two panes at two zooms, looking at the same hull**.
///
/// Pane A is focused and centred on a hull at `PHOTO_ZOOM`, where a ship is its photograph; pane B is the
/// same hull at a chart zoom, where it is a symbol. Both panes can draw the picture — the decoded bytes and
/// the dimensions are the wall's, one cache for several views — so the capture is a claim about the
/// **latch**, not about the cache: a wall-wide range draws one representation in both panes, and
/// `scripts/assert.sh out/panes-range.png` requires the picture in A and the symbol in B in one capture.
fn panes_range(state: &mut State) {
    load(state);
    let Some(drawn) = state
        .picture
        .units()
        .iter()
        .find(|unit| unit.unit.id_unit == PHOTO_UNIT)
        .and_then(|unit| unit.drawn(state.now()))
    else {
        return;
    };
    let centre = (drawn.lat, drawn.lon);
    let first = state.layout.focused();
    state.set_camera(Camera::new(centre, crate::units::mark::PHOTO_ZOOM));
    if state
        .split_pane(first, Camera::new(centre, PANES_RANGE_CHART))
        .is_none()
    {
        return;
    }
    state.layout.focus(first);
    state.sizes.insert(
        PHOTO_UNIT,
        crate::units::mark::Size {
            length_m: 120.0,
            beam_m: 16.0,
        },
    );
    // The same route `photo` takes, for the reason it takes it: bytes → cache → marker, decoded before
    // the first frame.
    crate::ui::apply_reply(
        state,
        &crate::data::Reply::Image(PHOTO_UNIT, Ok(PHOTO_FIXTURE.to_vec())),
    );
    wait_for_picture(state);
    // **The latch is the pane's** (#115), and the scene says which pane holds which. `poll` re-latches
    // both from their own frames; `verify` builds this state without a layout pass, so the ranges are
    // also set here, the way `photo` sets the single pane's.
    for pane in &mut state.panes {
        pane.view.range = crate::units::mark::Range::Symbol;
    }
    if let Some(pane) = state.pane_mut(first) {
        pane.view.range = crate::units::mark::Range::Picture;
    }
}

/// The buoy, the cone and the unlabelled light the `objects` scene draws, by placement id — the three
/// rows of `fixtures/scenes/objects.json`, which is derived from the recorded operational map.
pub const OBJECTS_BUOY: i64 = 2;
pub const OBJECTS_CONE: i64 = 3;
pub const OBJECTS_UNLABELLED: i64 = 1;

/// The water both panes of `objects` look at: the middle of the three recorded marks.
pub const OBJECTS_CENTRE: (f64, f64) = (-5.9835, 106.6636);

/// **Two zooms on the same water** (near, far). A placement drawn against the wall's camera instead of
/// its pane's lands tens of pixels away in the far pane, which is what makes the capture a proof of
/// `INV-04` on this layer rather than a picture of three dots.
pub const OBJECTS_ZOOMS: (f64, f64) = (10.6, 9.5);

/// The stand-in artwork's ink, so an assertion can find the **decoded bytes** at a placement's point
/// rather than any ink. The three PNGs are repository fixtures, like `hull.png`: the recorded
/// `asset_url` is presigned and a scene may not open a socket (`INV-11`).
pub const OBJECT_BUOY_INK: [u8; 3] = [58, 208, 122];
pub const OBJECT_CONE_INK: [u8; 3] = [224, 85, 58];
pub const OBJECT_LIGHT_INK: [u8; 3] = [245, 200, 66];

pub const OBJECT_BUOY_PNG: &[u8] = include_bytes!("../fixtures/scenes/buoy.png");
pub const OBJECT_CONE_PNG: &[u8] = include_bytes!("../fixtures/scenes/cone.png");
pub const OBJECT_LIGHT_PNG: &[u8] = include_bytes!("../fixtures/scenes/light.png");

/// `fixtures/scenes/objects.json` — the operational placements, wire-shaped. Derived from
/// `rest/tactical-object-placements.json` by filling in two labels and a bearing (the recording wrote
/// `""` and `0`); the envelope and every other key are the recorded ones.
pub fn objects_fixture() -> Result<Vec<TacticalObjectPlacement>, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/scenes/objects.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let envelope: Envelope<Vec<TacticalObjectPlacement>> = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not the placements envelope: {error}", path.display()))?;
    Ok(envelope.data)
}

/// The recorded manifest, read straight from `fixtures/rest/`: the cache a wall would have.
fn manifest_fixture() -> Result<TacticalObjectManifest, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/rest/tactical-objects-manifest.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let envelope: Envelope<TacticalObjectManifest> = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not the manifest envelope: {error}", path.display()))?;
    Ok(envelope.data)
}

/// The `objects` scene (#110): **a buoy, a cone and one unlabelled placement**, on a two-pane wall.
///
/// The manifest and the placements go through the same replies the wall takes, and the artwork through
/// the same reply a fetched picture takes — so the capture covers wire data → cache → mark, not a
/// shortcut into the cache (`#54`'s rule, applied to #110). The unlabelled mark is the recorded
/// `"label": ""`, which must draw **no plate**; the other two carry labels somebody wrote.
fn objects(state: &mut State) {
    load(state);
    let manifest = match manifest_fixture() {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("arcons-cc: scene: {error}");
            return;
        }
    };
    crate::ui::apply_reply(
        state,
        &crate::data::Reply::TacticalObjectManifest(Ok(Box::new(manifest.clone()))),
    );
    let rows = match objects_fixture() {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("arcons-cc: scene: {error}");
            return;
        }
    };
    let total = rows.len() as u64;
    crate::ui::apply_reply(
        state,
        &crate::data::Reply::TacticalObjectPlacements(Ok(Pages {
            rows,
            pages: 1,
            total_records: Some(total),
            truncated: false,
        })),
    );
    // **The artwork, by the manifest's own keys** — not by a path written here, so a change to the
    // library's key is a missing asset rather than a scene that silently keeps drawing the old one.
    for (code, bytes) in [
        ("BOYCAR_N", OBJECT_BUOY_PNG),
        ("BOYLAT_STBD_CONE_GREEN", OBJECT_CONE_PNG),
        ("LIGHTS_FIX", OBJECT_LIGHT_PNG),
    ] {
        let key = manifest
            .symbols
            .iter()
            .find(|entry| entry.code == code)
            .map(|entry| entry.asset_key.clone());
        if let Some(key) = key {
            crate::ui::apply_reply(
                state,
                &crate::data::Reply::TacticalAsset(key, Ok(bytes.to_vec())),
            );
        }
    }
    wait_for_assets(state);
    // **Two panes, the same water, two zooms**: the layer must be projected against each pane's own
    // frame. The focused pane is the near one, so a wall-wide camera agrees with it and is wrong in
    // its neighbour — the falsifier the assertion names.
    let first = state.layout.focused();
    state.set_camera(Camera::new(OBJECTS_CENTRE, OBJECTS_ZOOMS.0));
    state.split_pane(first, Camera::new(OBJECTS_CENTRE, OBJECTS_ZOOMS.1));
    state.layout.focus(first);
}

/// `fixtures/rest/tactical-objects-facets.json` — the recorded filter vocabulary, wire-shaped. The
/// counts are the server's, so the panel's domain and category rows carry them unchanged.
pub fn facets_fixture() -> Result<TacticalObjectFacets, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/rest/tactical-objects-facets.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let envelope: Envelope<TacticalObjectFacets> = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not the facets envelope: {error}", path.display()))?;
    Ok(envelope.data)
}

/// `fixtures/rest/installation-types.json` — the facility vocabulary in picker order.
pub fn installation_types_fixture() -> Result<Vec<InstallationType>, String> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/rest/installation-types.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let envelope: Envelope<Vec<InstallationType>> =
        serde_json::from_str(&text).map_err(|error| {
            format!(
                "{} is not the installation-types envelope: {error}",
                path.display()
            )
        })?;
    Ok(envelope.data)
}

/// `fixtures/rest/installations.json` — the facilities the wall caches at init. The recording is empty,
/// which is a complete answer and not a missing read: every type's count is `0`, honestly.
pub fn installations_fixture() -> Result<Vec<Installation>, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/rest/installations.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let envelope: Envelope<Vec<Installation>> = serde_json::from_str(&text).map_err(|error| {
        format!(
            "{} is not the installations envelope: {error}",
            path.display()
        )
    })?;
    Ok(envelope.data)
}

/// The zoom both panes of `panes-filter` hold. One zoom, so the only difference between the panes is
/// the filter — not a camera.
pub const PANES_FILTER_ZOOM: f64 = 9.5;

/// The `panes-filter` scene (#112): **two panes on the same water, one of them filtered**.
///
/// Pane A hides the buoy's **domain**; pane B hides nothing. The two panes hold the same camera, so the
/// only difference between their pictures is the filter — a wall-wide filter draws the same objects in
/// both, which is the falsifier the capture exists for.
///
/// The filter is set through [`crate::ui::filter::toggle_in`], the same function a press on a panel row
/// calls, and the panel is opened with the same `Begin::Filter` a press on the pane's toggle uses. The
/// vocabularies go through the replies the wall takes, so the panel in the capture is the wire's rows.
fn panes_filter(state: &mut State) {
    objects(state);
    if let Ok(facets) = facets_fixture() {
        crate::ui::apply_reply(state, &crate::data::Reply::TacticalObjectFacets(Ok(facets)));
    }
    if let Ok(types) = installation_types_fixture() {
        crate::ui::apply_reply(state, &crate::data::Reply::InstallationTypes(Ok(types)));
    }
    if let Ok(places) = installations_fixture() {
        crate::ui::apply_reply(state, &crate::data::Reply::Installations(Ok(places)));
    }
    // **The same water at the same zoom in both panes**: a difference between their pictures is the
    // filter and nothing else.
    for pane in &mut state.panes {
        pane.camera = Camera::new(OBJECTS_CENTRE, PANES_FILTER_ZOOM);
    }
    let first = state.pane_rects()[0].0;
    // The domain comes from the placement itself, not from an id written here: the scene hides the
    // buoy's domain whatever the backend calls it and whatever id it gives it.
    let domain = state
        .placements
        .iter()
        .find(|placement| placement.id == OBJECTS_BUOY)
        .map(|placement| placement.symbol.category.domain.id);
    if let Some(domain) = domain {
        crate::ui::filter::toggle_in(state, first, crate::ui::filter::Target::Domain(domain));
    }
    state.begin(crate::ui::Begin::Filter { pane: first });
    state.layout.focus(first);
}

/// Wait, bounded, for the scene's decoded artwork — the same reason `wait_for_picture` waits: a
/// capture that raced the decoder would draw generic marks on some runs and artwork on others
/// (`INV-11`). A picture that cannot decode draws the generic mark rather than hanging.
fn wait_for_assets(state: &mut State) {
    for _ in 0..400 {
        if state.assets.len() >= 3 && state.assets.values().all(|asset| asset.is_ready()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
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

/// Which pane `panes-max` maximises: the middle one — not the first — so the capture says the picture is
/// the pane that was asked for rather than whichever pane happened to be first.
pub const PANES_MAXIMISED: usize = 1;

/// The `panes-max` scene (#85): one pane **full size over the wall**, the tiling still behind it.
///
/// The maximise is **a press on the pane's own toggle** (`fixtures/scripts/panes-max.json`), not a call
/// to the function behind it (#111): the capture is the witness that the icon is drawn where a hand can
/// reach it and that its press reaches `full_size_from_menu`. If the button is missing or in the wrong
/// place, the pane stays in its slot and the assertion below has a frame the size of a slot and a
/// maximised pane it cannot find. `verify` projects the hulls against a frame the size of the *panel*,
/// which is the claim: a maximised pane is a bigger rectangle, not a different pane.
fn panes_max(state: &mut State) {
    panes(state);
}

/// The `panes-chrome` scene (#111): the tiled wall with **both corner controls in every pane**, and the
/// filter panel open on the focused one. The open panel is the proof that the toggle is a control and
/// not decoration; the two buttons are sampled where `view::chrome_buttons` puts them.
fn panes_chrome(state: &mut State) {
    panes(state);
}

/// The `panes-named` scene (#83, #113): three panes, two of them **named** — one short enough to read and
/// one long enough to be cut — and the third left unnamed, so the capture says both "a typed name is
/// drawn" and "a pane nobody named still has a title". The names are set the way the sheet sets them,
/// through `shown_name`, so a name the wall would trim is drawn trimmed here too.
///
/// **The unnamed pane is the third**, whose id is 3, so the picture proves the default comes from the
/// pane's id rather than its position: it draws `pane-03` after the two named panes before it.
fn panes_named(state: &mut State) {
    panes(state);
    let ids: Vec<crate::ui::ViewId> = state.pane_rects().into_iter().map(|(id, _)| id).collect();
    if let Some(id) = ids.first().copied()
        && let Some(pane) = state.pane_mut(id)
    {
        pane.name = crate::ui::shown_name("Surabaya approaches");
    }
    if let Some(id) = ids.get(1).copied()
        && let Some(pane) = state.pane_mut(id)
    {
        pane.name =
            crate::ui::shown_name("the strait at first light, and the traffic separation scheme");
    }
    // The third pane is deliberately left unnamed: it draws `pane-03`.
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

/// The `panes-gap-resize` scene (#114): the gapped wall left at **half**, and the border dragged to
/// `PANES_DRAGGED` by a pointer that lands in the gutter.
///
/// This is the proof `panes-gaps` cannot make. There the split is set in code, so the picture is right
/// whether or not anything is grabbable; here `fixtures/scripts/panes-gap-resize.json` presses the middle
/// of the gap and drags it to 58%, and the capture must show the split `panes-gaps` draws — same gap, same
/// wall. If the gutter is drawn under the panes, or not at all, the press reaches nothing and the capture
/// stays at 50/50.
fn panes_gap_resize(state: &mut State) {
    // **No border drag in code**: the pointer is the only thing that may move the split here.
    panes_wall(state, false);
    state.settings.pane_gap = crate::ui::tokens::GAP;
}

/// The `settings` scene (#82): a wall with its **settings sheet open**, which is the only picture of the
/// one modal surface on a wall. It is a single pane so the sheet covers the picture rather than a tiling:
/// what a reader needs to see is the sheet, where it sits and what it says.
fn settings_sheet(state: &mut State) {
    load(state);
    state.begin(crate::ui::Begin::Settings);
}

/// The `settings-dismiss` scene (#98): the same sheet, and **a press outside it** — from
/// `fixtures/scripts/settings-dismiss.json`, so the dismissal is a pointer's and not a function call.
///
/// Its picture is the claim: with the sheet open the scrim darkens the whole wall, and with it closed the
/// wall is lit again. `verify` measures that, so a scrim whose press does nothing leaves the sheet up and
/// the gate red.
fn settings_dismiss(state: &mut State) {
    settings_sheet(state);
}

/// The `panes-menu` scene (#81): **a tiled wall with its menu open**, which is the only wall that offers
/// `Close pane`. The one-pane `pane-menu` is its control: one row there, two here, and the surface has to be
/// tall enough for both.
fn panes_menu(state: &mut State) {
    panes(state);
    let pane = state.layout.focused();
    state.begin(crate::ui::Begin::Menu { pane, at: MENU_AT });
}

/// The `pane-menu` scene (#77): **a wall with one pane and its menu open**.
///
/// The one shape the menu had never been drawn on, and the only wall that needs it — it is the only route
/// to "Focus on new pane", so a menu that does not draw here is a wall that can never have a second pane.
/// The capture is the witness the pointer-driven bug could not have: no test right-clicks.
fn pane_menu(state: &mut State) {
    load(state);
    let pane = state.layout.focused();
    state.begin(crate::ui::Begin::Menu { pane, at: MENU_AT });
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
    state.begin(crate::ui::Begin::Lift(crate::ui::PaneDrag {
        pane: dragged,
        drop,
    }));
    // The pane being lifted is the one the operator picked up, so it is the focused one.
    if drop.is_some() {
        state.layout.focus(dragged);
    }
}

/// One scene: a name and the function that builds its state.
pub type Scene = (&'static str, fn(&mut State));

/// The organisation the `pane-tree` scene draws: the Navy's structure — the one the recording carries a
/// forest for. `GET /unit-organisations` names four; the scene is about one of them (#116).
pub const TREE_ORGANISATION: i64 = 1;

/// The share of the panel the `pane-tree` scene leaves to the **map**. A forest is wide, so the tree
/// takes the larger share: the capture must show every post rather than the first two.
pub const TREE_MAP_SHARE: f32 = 0.34;

/// `fixtures/scenes/chain-of-command.json` — the Navy's structure, wire-shaped. **Derived** from
/// `fixtures/rest/unit-organisation-1.json` by moving the recorded node 3 out of Wakasal's subtree and
/// making it a root (the key a root does not have, `parent_id`, is gone). That is the one thing the
/// recording cannot show and the forest claim needs: `nodes` is a **forest**, and more than one root is
/// normal. Every field value is the wire's; nothing was invented.
pub fn chain_of_command_fixture() -> Result<UnitOrganisationDetail, String> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/scenes/chain-of-command.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let envelope: Envelope<UnitOrganisationDetail> = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not the structure envelope: {error}", path.display()))?;
    Ok(envelope.data)
}

/// The recorded supreme command: one post shared by every branch, **vacant** on this deployment.
fn supreme_fixture() -> Result<TniCommandPost, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/rest/tni-commander.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let envelope: Envelope<TniCommandPost> = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not the post envelope: {error}", path.display()))?;
    Ok(envelope.data)
}

/// The `pane-tree` scene (#116): **a tree pane beside a map pane** — one structure's forest under the
/// supreme command, and the wall's own map beside it.
///
/// The tree pane comes from the same call the header's control makes (`State::split_pane_as`), and its
/// reads arrive as the replies a wall takes (`apply_reply`), not as a shortcut into the state. So the
/// capture covers wire data → pane → plate, and the assertion can ask the pane what it drew.
fn pane_tree(state: &mut State) {
    load(state);
    let map = state.layout.focused();
    let camera = state.camera_of(map);
    let Some(tree) = state.split_pane_as(
        map,
        camera,
        crate::ui::PaneContent::Tree {
            organisation: Some(TREE_ORGANISATION),
        },
    ) else {
        return;
    };
    // The split is the wall's own: the border between the two panes is found the way a press finds it,
    // and the ratio is the share the map keeps. A scene that positioned panes itself would be a second
    // layout function.
    let viewport = state.pane_viewport();
    let middle = (viewport.w as f32 / 2.0, viewport.h as f32 / 2.0);
    if let Some(border) = state.layout.border_at(viewport, middle, state.pane_grip()) {
        state.layout.set_ratio(&border, TREE_MAP_SHARE);
    }
    state.layout.focus(tree);
    // The two reads the pane needs, through the replies the reads thread would send.
    match chain_of_command_fixture() {
        Ok(detail) => {
            crate::ui::apply_reply(
                state,
                &crate::data::Reply::UnitOrganisation(TREE_ORGANISATION, Ok(Box::new(detail))),
            );
        }
        Err(error) => eprintln!("arcons-cc: scene: {error}"),
    }
    match supreme_fixture() {
        Ok(post) => {
            crate::ui::apply_reply(state, &crate::data::Reply::TniCommander(Ok(Box::new(post))));
        }
        Err(error) => eprintln!("arcons-cc: scene: {error}"),
    }
}

/// The share of the panel the `pane-tree-narrow` scene leaves to the **map**: 320 pt of 1280, the
/// width the ticket's proof names.
pub const TREE_NARROW_MAP_SHARE: f32 = 0.75;

/// The post the `pane-tree-zoom` scene puts under a known point: the Navy's fleet command, a node with
/// children in the recorded structure. Its id is the wire's; the scene looks the node up, so a fixture
/// that renumbers it fails loudly rather than drawing a different tree.
pub const TREE_ZOOM_NODE: i64 = 4;

/// The zoom `pane-tree-zoom` holds, and the pane-local point the node's centre is put at.
pub const TREE_ZOOM_IN: f32 = 2.0;
pub const TREE_ZOOM_AT: (f32, f32) = (300.0, 280.0);

/// The `pane-tree-narrow` scene (#127): **the chain of command in a narrow pane**.
///
/// 320 pt of the window is a quarter of the wall, and at 1:1 the recorded forest is 784 pt wide — which is
/// the operator's report: narrowing the pane cut the tree. This scene's camera is untouched, so the fit
/// runs and the whole forest is drawn inside the pane. Nothing here sets a zoom: the fit is the claim.
fn pane_tree_narrow(state: &mut State) {
    pane_tree(state);
    let Some(tree) = state
        .layout
        .ids()
        .into_iter()
        .find(|id| state.pane(*id).is_some_and(|pane| !pane.content.is_map()))
    else {
        return;
    };
    // **The border this pane is on**, found from the pane rather than from a point: after `pane_tree`
    // the split is at 34 %, so a press at the middle finds nothing. A scene that guessed the line's
    // position would silently draw the wide wall it is here to avoid.
    let viewport = state.pane_viewport();
    if let Some(border) = state
        .layout
        .borders(viewport)
        .into_iter()
        .find(|border| border.a == tree || border.b == tree)
    {
        state.layout.set_ratio(&border, TREE_NARROW_MAP_SHARE);
    }
}

/// The point the `pane-tree-pan` script takes hold of, and the travel it drags: **inside the recorded
/// fleet command's plate** at `pane-tree`'s fit, so the drag starts on a post — which is the routing a
/// canvas has to get right, because Rune hands a gesture to the nearest element under the pointer.
pub const TREE_PAN_AT: (f32, f32) = (748.0, 470.0);
pub const TREE_PAN_BY: (f32, f32) = (-120.0, -40.0);

/// The `pane-tree-pan` scene (#127): the fitted tree, and a **pointer** drags it.
///
/// The camera is untouched in the state the scene builds; `fixtures/scripts/pane-tree-pan.json` presses
/// the known post and drags, so the capture is the witness that a drag reaches the canvas, that a drag
/// starting on a post pans rather than falling through, and that the pan is absolute (the travel, not a
/// sum of per-event deltas).
fn pane_tree_pan(state: &mut State) {
    pane_tree(state);
}

/// The `pane-tree-zoom` scene (#127): **the same forest, zoomed in, with a known post under a known
/// point** — so a capture can ask where the transform puts a node and get the same answer the drawing
/// used. The camera is `touched`, which is what a hand's zoom leaves behind and what a resize must not
/// undo.
fn pane_tree_zoom(state: &mut State) {
    pane_tree(state);
    let Some(tree_id) = state
        .pane_rects()
        .into_iter()
        .map(|(id, _)| id)
        .find(|id| state.pane(*id).is_some_and(|pane| !pane.content.is_map()))
    else {
        return;
    };
    let Some(tree) = crate::ui::tree::tree_for(state, tree_id) else {
        return;
    };
    let Some(node) = tree.nodes.iter().find(|node| node.id == TREE_ZOOM_NODE) else {
        return;
    };
    let centre = (
        node.rect.x + node.rect.w / 2.0,
        node.rect.y + node.rect.h / 2.0,
    );
    let pan = (
        TREE_ZOOM_AT.0 - centre.0 * TREE_ZOOM_IN,
        TREE_ZOOM_AT.1 - centre.1 * TREE_ZOOM_IN,
    );
    if let Some(pane) = state.pane_mut(tree_id) {
        pane.tree = Some(crate::ui::tree::TreeCamera::moved_to(pan, TREE_ZOOM_IN));
    }
}

/// The water the `installations` scene looks at: the same recorded operational map the objects scene
/// uses, so the two layers can be read against the same coast.
pub const INSTALLATIONS_CENTRE: (f64, f64) = (-5.9835, 106.6636);
/// Two zooms on that water, so a mark drawn from the wall's camera instead of its pane's lands tens of
/// pixels away in the neighbour — the falsifier #109 names.
pub const INSTALLATIONS_ZOOMS: (f64, f64) = (9.6, 8.5);
/// The facility the scene selects in its focused pane: the naval base.
pub const INSTALLATIONS_BASE: i64 = 2;

/// `fixtures/scenes/installations.json` — the scene's facilities, wire-shaped. Derived from the
/// schema and the recorded operational map's water: a pier, a base, a radar station, and **one row
/// with no position**, which the layer must leave off the map. The recorded `/installations` list is
/// empty, so there is no recording to reuse.
pub fn installation_scene_fixture() -> Result<Vec<Installation>, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/scenes/installations.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let envelope: Envelope<Vec<Installation>> = serde_json::from_str(&text).map_err(|error| {
        format!(
            "{} is not the installations envelope: {error}",
            path.display()
        )
    })?;
    Ok(envelope.data)
}

/// The `installations` scene (#109): **a pier, a base and a radar station on the map**, on two panes
/// at two zooms, with the base selected in the focused pane.
///
/// The types and the rows arrive through the replies a wall takes (`ui::apply_reply`), so the capture
/// covers wire data → cache → mark. The selection is made with `installations::press`, the same
/// function a mark's press calls, so the card is the one a hand would open.
fn installations_layer(state: &mut State) {
    load(state);
    if let Ok(types) = installation_types_fixture() {
        crate::ui::apply_reply(state, &crate::data::Reply::InstallationTypes(Ok(types)));
    }
    match installation_scene_fixture() {
        Ok(rows) => {
            crate::ui::apply_reply(state, &crate::data::Reply::Installations(Ok(rows)));
        }
        Err(error) => {
            eprintln!("arcons-cc: scene: {error}");
            return;
        }
    }
    let first = state.layout.focused();
    state.set_camera(Camera::new(INSTALLATIONS_CENTRE, INSTALLATIONS_ZOOMS.0));
    state.split_pane(
        first,
        Camera::new(INSTALLATIONS_CENTRE, INSTALLATIONS_ZOOMS.1),
    );
    state.layout.focus(first);
    crate::ui::installations::press(state, first, INSTALLATIONS_BASE);
}

/// The scene registry, handed to `AppSpec::scenes`.
pub const SCENES: &[Scene] = &[
    ("empty", empty),
    ("picture", picture),
    ("stale", stale),
    ("card-open", card_open),
    ("photo", photo),
    ("panes", panes),
    ("panes-chrome", panes_chrome),
    ("panes-drag", panes_drag),
    ("panes-pan", panes_pan),
    ("panes-menu", panes_menu),
    ("panes-gaps", panes_gaps),
    ("panes-gap-resize", panes_gap_resize),
    ("panes-named", panes_named),
    ("panes-max", panes_max),
    ("panes-cards", panes_cards),
    ("panes-range", panes_range),
    // The operational layer (#110): a buoy, a cone and one unlabelled mark, on two panes at two
    // zooms, so a wall-wide camera lands in the wrong place.
    ("objects", objects),
    // The per-pane filter (#112): two panes on the same water, one hiding a domain — and the panel
    // that lists the backend's own vocabulary.
    ("panes-filter", panes_filter),
    // The places, not the hulls (#109): a pier, a base and a radar station on two panes at two zooms,
    // with a facility card open in the focused pane.
    ("installations", installations_layer),
    ("settings", settings_sheet),
    ("settings-dismiss", settings_dismiss),
    ("pane-menu", pane_menu),
    // The chain of command (#116): a tree pane beside a map pane. A forest is a forest, the supreme
    // command is the root above it, and the tree draws nothing that projects.
    ("pane-tree", pane_tree),
    // The tree pane is a canvas (#127): a narrow pane fits the whole forest, and a zoomed one puts a
    // known post under a known point.
    ("pane-tree-narrow", pane_tree_narrow),
    ("pane-tree-zoom", pane_tree_zoom),
    // The canvas by **pointer**: a drag on a post pans the tree (a script makes the gesture).
    ("pane-tree-pan", pane_tree_pan),
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
                "panes-chrome",
                "panes-drag",
                // The only scene whose picture is made by an **input event** rather than by a call
                // (`fixtures/scripts/panes-pan.json`), so it is also the only witness a dropped
                // gesture has (#80).
                "panes-pan",
                "panes-menu",
                "panes-gaps",
                // The gapped wall moved by a **pointer in the gutter**, so the gap is the handle and
                // not just the look (#114).
                "panes-gap-resize",
                "panes-named",
                "panes-max",
                "panes-cards",
                // Two panes at two zooms on the same hull: the symbol-to-picture latch is the pane's,
                // not the wall's (#115).
                "panes-range",
                // The operational layer (#110): two panes at two zooms on the same water, so a
                // placement drawn from the wall's camera is in the wrong place in one of them.
                "objects",
                // The per-pane filter (#112): the same water in two panes, one hiding a domain.
                "panes-filter",
                // The places, not the hulls (#109): one mark per located facility, with a card.
                "installations",
                "settings",
                // The one scene whose point is that something is **not** there: the sheet, after a press
                // outside it (#98).
                "settings-dismiss",
                "pane-menu",
                // The chain of command (#116): a tree pane beside a map pane.
                "pane-tree",
                // The tree is a canvas (#127): the whole forest fits a 320 pt pane, and a zoomed camera
                // puts a known post under a known point.
                "pane-tree-narrow",
                "pane-tree-zoom",
                // The canvas by a pointer: a drag on a post pans it.
                "pane-tree-pan"
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
        let focused = card.layout.focused();
        assert_eq!(
            card.selected_in(focused),
            Some(CARD_UNIT),
            "the card scene has a card"
        );
        assert!(card.card_in(focused).is_some());

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

        // The range witness (#115): one hull, two panes, one of each representation — and the decoded
        // bytes are the wall's, so a pane that switches representation is the latch and nothing else.
        let range = state_for("panes-range");
        assert_eq!(range.panes.len(), 2, "a pane for each representation");
        let focused = range.layout.focused();
        let focused_pane = range.pane(focused).expect("the focused pane");
        assert_eq!(
            focused_pane.view.range,
            crate::units::mark::Range::Picture,
            "the focused pane is zoomed into the hull's picture"
        );
        assert!(
            range
                .panes
                .iter()
                .filter(|pane| pane.id != focused)
                .all(|pane| pane.view.range == crate::units::mark::Range::Symbol),
            "the neighbour at chart zoom is in symbol range"
        );
        assert!(
            range
                .pictures
                .get(&PHOTO_UNIT)
                .is_some_and(|image| image.is_ready()),
            "the picture is decoded for the wall, so the neighbour could draw it too"
        );

        // The operational layer (#110): the manifest is cached by code, the three recorded marks are
        // loaded through the reply path, and the artwork is decoded before the first frame. One mark
        // is unlabelled, which the capture proves draws no plate.
        let objects = state_for("objects");
        assert_eq!(objects.manifest.len(), 56, "the whole recorded manifest");
        assert_eq!(objects.placements.len(), 3, "the recorded operational map");
        let labelled = objects
            .placements
            .iter()
            .filter(|mark| mark.label.as_deref().is_some_and(|label| !label.is_empty()))
            .count();
        assert_eq!(labelled, 2, "a buoy and a cone carry labels");
        assert!(
            objects
                .placements
                .iter()
                .any(|mark| mark.id == OBJECTS_UNLABELLED && mark.label.as_deref() == Some("")),
            "the recorded empty label is the unlabelled mark, not a plate"
        );
        assert_eq!(
            objects.assets.len(),
            3,
            "one decoded handle per placed symbol"
        );
        assert!(
            objects.assets.values().all(|asset| asset.is_ready()),
            "the artwork is decoded before the first frame (INV-11)"
        );
        assert_eq!(objects.panes.len(), 2, "two panes at two zooms");
        for (pane, zoom) in objects.panes.iter().zip([OBJECTS_ZOOMS.0, OBJECTS_ZOOMS.1]) {
            assert_eq!(pane.camera.zoom, zoom, "each pane holds its own zoom");
            assert!(
                (pane.camera.center.0 - OBJECTS_CENTRE.0).abs() < 1e-9
                    && (pane.camera.center.1 - OBJECTS_CENTRE.1).abs() < 1e-9,
                "the same water, got {:?}",
                pane.camera.center
            );
        }

        // The per-pane filter (#112): two panes on the same water at the same zoom, the first hiding
        // the buoy's domain. The vocabularies came through the reply path, so the panel is the wire's
        // rows; the panel is open on the pane that was filtered.
        let filtered = state_for("panes-filter");
        assert_eq!(filtered.panes.len(), 2, "two panes on one water");
        let first = filtered.pane_rects()[0].0;
        let second = filtered.pane_rects()[1].0;
        let buoy = filtered
            .placements
            .iter()
            .find(|placement| placement.id == OBJECTS_BUOY)
            .expect("the recorded buoy");
        let domain = buoy.symbol.category.domain.id;
        let category = buoy.symbol.category.id;
        assert!(
            filtered
                .pane(first)
                .expect("the first pane")
                .view
                .filter
                .hides_object_in(domain, category),
            "the first pane hides the buoy's domain"
        );
        assert_eq!(
            filtered.pane(second).expect("the second pane").view.filter,
            crate::ui::Filter::default(),
            "the second pane hides nothing: the filter is the pane's"
        );
        assert_eq!(
            filtered.filtering(),
            Some(first),
            "the panel is open on the filtered pane"
        );
        assert!(
            filtered.facets.is_some(),
            "the facets came through the reply path"
        );
        assert_eq!(
            filtered.installation_types.len(),
            5,
            "the recorded facility vocabulary"
        );
        assert!(
            filtered.installations.is_empty(),
            "the recorded installation list is empty, which is a complete answer"
        );
        let rows = crate::ui::filter::rows(
            &filtered.pane(first).expect("pane").view.filter,
            filtered.facets.as_ref(),
            &filtered.installation_types,
            &filtered.installations,
        );
        assert!(
            rows.len() > 12,
            "the panel lists the layers and both vocabularies, got {} rows",
            rows.len()
        );
        assert_eq!(
            crate::ui::tokens::filter_size(rows.len()).1,
            crate::ui::tokens::FILTER_MAX_H,
            "this vocabulary is taller than the panel, so it scrolls inside its maximum"
        );
        for pane in &filtered.panes {
            assert_eq!(
                pane.camera.zoom, PANES_FILTER_ZOOM,
                "one zoom in both panes"
            );
        }

        // **The places, not the hulls** (#109): four facilities in the cache, one of them unlocated,
        // three marks per pane, and the base's card in the focused pane only. Two panes at two zooms,
        // so a mark drawn from the wall's camera is out in the neighbour.
        let facilities = state_for("installations");
        assert_eq!(facilities.installations.len(), 4, "the scene's facilities");
        assert_eq!(
            facilities.installation_types.len(),
            5,
            "the recorded facility vocabulary"
        );
        assert!(
            facilities
                .installations
                .iter()
                .any(|row| row.coordinates().is_none()),
            "one row has no position, which is a real state and not a mark"
        );
        assert_eq!(facilities.panes.len(), 2, "two zooms on one water");
        let located = facilities
            .installations
            .iter()
            .filter(|row| row.coordinates().is_some())
            .count();
        for (id, rect) in facilities.pane_rects() {
            let pane = facilities.pane(id).expect("the pane");
            let frame = crate::ui::view::synthetic_frame(pane.camera, rect, 1.0);
            let placed = crate::ui::installations::place(
                &facilities.installations,
                &facilities.installation_types,
                &facilities.manifest,
                &frame,
                &pane.view.filter,
            );
            assert_eq!(
                placed.len(),
                located,
                "one mark per located facility in pane {id}"
            );
            assert!(
                placed.iter().all(|mark| mark.x != 0.0 || mark.y != 0.0),
                "no facility is drawn at the origin"
            );
        }
        let first = facilities.layout.focused();
        let second = facilities
            .pane_rects()
            .into_iter()
            .map(|(id, _)| id)
            .find(|id| *id != first)
            .expect("a neighbour");
        assert_eq!(
            facilities.installation_in(first),
            Some(INSTALLATIONS_BASE),
            "the scene selects the base in the focused pane"
        );
        assert_eq!(
            facilities.installation_in(second),
            None,
            "and no facility in its neighbour"
        );

        // The chain of command (#116): a tree pane beside a map pane, one structure's forest under the
        // supreme command. The forest is a forest — two roots — and the supreme command is vacant on
        // this deployment, which draws its post and no invented name.
        let command = state_for("pane-tree");
        assert_eq!(command.panes.len(), 2, "a tree pane beside a map pane");
        let ids = command.layout.ids();
        let tree = ids[1];
        assert!(
            matches!(
                command.pane(tree).map(|pane| &pane.content),
                Some(crate::ui::PaneContent::Tree {
                    organisation: Some(TREE_ORGANISATION)
                })
            ),
            "the second pane is the chain of command"
        );
        assert!(
            command
                .pane(ids[0])
                .is_some_and(|pane| pane.content.is_map()),
            "the first pane is still the map"
        );
        assert_eq!(command.layout.focused(), tree, "the new pane has the keys");
        let detail = command
            .organisation_trees
            .get(&TREE_ORGANISATION)
            .expect("the structure was read through the reply path");
        assert_eq!(detail.nodes.len(), 2, "the derived fixture is a forest");
        let post = command.tni_command.as_ref().expect("the supreme command");
        assert!(
            post.commander.is_none(),
            "INV-01: the recorded post is vacant, and the scene invents nobody"
        );
        let drawn = crate::ui::tree::tree_for(&command, tree).expect("the pane draws a tree");
        assert_eq!(
            drawn.nodes.len(),
            8,
            "seven recorded posts and the supreme command above them"
        );
        assert_eq!(
            drawn.links.len(),
            7,
            "one connector per parent-to-child run"
        );

        // **The tree pane is a canvas** (#127). Narrow: the camera is untouched, so the pane fits the
        // whole forest — every post inside the pane's own rectangle, which is the operator's ask. The
        // recorded forest is 784 pt wide, so the fit has to shrink it into a 320 pt pane.
        let narrow = state_for("pane-tree-narrow");
        let narrow_tree = narrow
            .pane_rects()
            .into_iter()
            .find(|(id, _)| narrow.pane(*id).is_some_and(|pane| !pane.content.is_map()))
            .expect("the narrow scene has a tree pane");
        let narrow_rect = narrow_tree.1;
        assert!(
            (narrow_rect.w - 320.0).abs() < 0.5,
            "the narrow pane is a quarter of the window, got {}",
            narrow_rect.w
        );
        let narrow_drawn = crate::ui::tree::tree_for(&narrow, narrow_tree.0).expect("the tree");
        let narrow_camera = crate::ui::tree::camera_for(
            narrow.pane(narrow_tree.0).expect("the pane"),
            &narrow_drawn,
            narrow_rect,
        );
        assert!(
            !narrow_camera.touched,
            "no hand has taken the narrow camera"
        );
        assert!(
            narrow_camera.zoom < 1.0,
            "the fit shrinks a 784 pt forest into a 320 pt pane, got {}",
            narrow_camera.zoom
        );
        assert!(
            (narrow_camera.zoom - crate::ui::tokens::TREE_ZOOM_MIN).abs() > 1e-6,
            "the fit is inside the token range, not clamped at its floor: {narrow_camera:?}"
        );
        for node in &narrow_drawn.nodes {
            let (x, y) = crate::ui::view::tree_point(&narrow_camera, (node.rect.x, node.rect.y));
            let (w, h) = (
                node.rect.w * narrow_camera.zoom,
                node.rect.h * narrow_camera.zoom,
            );
            assert!(
                x >= -0.5
                    && y >= -0.5
                    && x + w <= narrow_rect.w + 0.5
                    && y + h <= narrow_rect.h + 0.5,
                "post {} is outside the narrow pane at ({x}, {y}, {w}, {h}) of {}×{} — narrowing the pane \
                 cut the tree, which is the behaviour this scene refuses",
                node.id,
                narrow_rect.w,
                narrow_rect.h
            );
        }

        // Zoomed: a hand has taken the camera, and the known post's centre is at the known point. The
        // zoom is large enough that not every post is on screen, which is what makes it a zoom and not
        // another fit.
        let zoomed = state_for("pane-tree-zoom");
        let zoomed_tree = zoomed
            .pane_rects()
            .into_iter()
            .find(|(id, _)| zoomed.pane(*id).is_some_and(|pane| !pane.content.is_map()))
            .expect("the zoomed scene has a tree pane");
        let zoomed_camera = zoomed
            .pane(zoomed_tree.0)
            .and_then(|pane| pane.tree)
            .expect("the zoomed pane has a camera");
        assert!(zoomed_camera.touched, "a hand has taken the zoomed camera");
        assert_eq!(zoomed_camera.zoom, TREE_ZOOM_IN);
        let zoomed_drawn = crate::ui::tree::tree_for(&zoomed, zoomed_tree.0).expect("the tree");
        let known = zoomed_drawn
            .nodes
            .iter()
            .find(|node| node.id == TREE_ZOOM_NODE)
            .expect("the known post");
        let (x, y) = crate::ui::view::tree_point(
            &zoomed_camera,
            (
                known.rect.x + known.rect.w / 2.0,
                known.rect.y + known.rect.h / 2.0,
            ),
        );
        assert!(
            (x - TREE_ZOOM_AT.0).abs() < 0.01 && (y - TREE_ZOOM_AT.1).abs() < 0.01,
            "the known post is at the known point, got ({x}, {y})"
        );
        let on_screen = zoomed_drawn
            .nodes
            .iter()
            .filter(|node| {
                let (x, y) =
                    crate::ui::view::tree_point(&zoomed_camera, (node.rect.x, node.rect.y));
                x + node.rect.w * zoomed_camera.zoom >= 0.0
                    && y + node.rect.h * zoomed_camera.zoom >= 0.0
                    && x <= zoomed_tree.1.w
                    && y <= zoomed_tree.1.h
            })
            .count();
        assert!(
            on_screen < zoomed_drawn.nodes.len(),
            "a zoom of {TREE_ZOOM_IN} leaves posts outside the pane: {on_screen} of {} on screen",
            zoomed_drawn.nodes.len()
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
