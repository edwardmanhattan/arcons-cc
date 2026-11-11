//! The installations layer (#109): the **places**, not the hulls — a posko, a base, a dock.
//!
//! A facility is not a `unit`: it has no hull number, class, course or movement domain, and it never
//! enters the fix chain. It is one row from the cache read at init, at one coordinate, drawn against
//! **this pane's** frame like everything else (`INV-04`) so a tiled wall shows the same water's bases
//! in every pane that looks at it.
//!
//! The rules the layer keeps:
//!
//! - **No position, no mark.** `latitude`/`longitude` are optional on the wire, and a facility
//!   registered before its location is known has none. It is in the cache and not on the map — there
//!   is no mark at (0, 0) (`INV-02`, the same rule the hulls keep).
//! - **A symbol is a code** (#107). `symbol_code` names a `tactical_object_symbols.code`; the
//!   manifest may not carry it yet, and a kind may have no code at all. Either way the mark is the
//!   **facility shape**, never a hull's symbol: a `Pangkalan` that looks like a ship is a wrong
//!   picture on a wall whose job is a right one.
//! - **Standings, not fixes.** There is no age and no stale mark here. The hull path's staleness
//!   judgement is the first thing someone will copy; it does not apply to a building.
//! - **The filter is the pane's** (#112): the layer and its types are hidden by `Filter`, asked once
//!   here, so neither the element builder nor a caller decides visibility a second time.

use std::collections::HashMap;

use rune::shape::Shape;
use rune::{El, col, el, row, spacer, text};

use crate::data::types::{Installation, InstallationType, TacticalObjectManifestEntry};
use crate::map::geometry::Rendered;
use crate::ui::filter::Filter;
use crate::ui::tokens;
use crate::units::card::{self, Placement, WIDTH};

/// An installation's mark, in points — its artwork, or the generic facility shape.
pub const MARK: f32 = tokens::INSTALLATION_MARK;

/// One facility, projected into **this** pane's frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: i64,
    pub id_type: i64,
    /// The row's own name (`Pangkalan Tanjung Priok`).
    pub name: String,
    /// The joined kind when the wire carried one, else the vocabulary's name for the type.
    pub type_name: Option<String>,
    /// The branch id the row carries. There is no branch vocabulary in this cache, so the card shows
    /// the id rather than a name nobody supplied.
    pub branch: i64,
    pub note: Option<String>,
    /// The point in the frame's own pixels.
    pub x: f64,
    pub y: f64,
    /// The kind's `symbol_code`, when it names one. A code, not an id, and not a guarantee.
    pub symbol_code: Option<String>,
    /// The manifest's key for the artwork, when the library carries the code.
    pub asset_key: Option<String>,
}

/// Project every facility that has a position and that **this pane's filter** leaves visible.
///
/// Pure, so the layer can be asserted without a window: `verify` asks this function where each mark
/// belongs, and the tests ask it what a hidden type or an unlocated row does.
pub fn place(
    installations: &[Installation],
    types: &[InstallationType],
    manifest: &HashMap<String, TacticalObjectManifestEntry>,
    frame: &Rendered,
    filter: &Filter,
) -> Vec<Placed> {
    installations
        .iter()
        .filter(|installation| !filter.hides_installation_type(installation.id_installation_type))
        .filter_map(|installation| {
            let (lat, lon) = installation.coordinates()?;
            let (x, y) = frame.project(lat, lon);
            let symbol_code = types
                .iter()
                .find(|kind| kind.id == installation.id_installation_type)
                .and_then(|kind| kind.symbol_code.clone());
            Some(Placed {
                id: installation.id,
                id_type: installation.id_installation_type,
                name: installation.name.clone(),
                type_name: type_name(installation, types),
                branch: installation.id_service_branch,
                note: installation.note.clone(),
                x,
                y,
                asset_key: symbol_code
                    .as_ref()
                    .and_then(|code| manifest.get(code))
                    .map(|entry| entry.asset_key.clone()),
                symbol_code,
            })
        })
        .collect()
}

/// What a facility is called: the joined `type_name` the wire sent, else the vocabulary's `name`.
///
/// The joined field is the server's convenience, so a client need not resolve it; when it is absent
/// the type cache answers. Neither defaults to a word invented here (`INV-01`).
pub fn type_name(installation: &Installation, types: &[InstallationType]) -> Option<String> {
    installation
        .type_name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            types
                .iter()
                .find(|kind| kind.id == installation.id_installation_type)
                .map(|kind| kind.name.clone())
        })
}

/// What one facility is drawn as this frame.
#[derive(Clone, Debug)]
pub enum Mark {
    /// The library's artwork, decoded and ready.
    Asset { image: rune::Image },
    /// The facility shape: "a place is here, and this wall does not have its picture".
    Generic,
}

/// The artwork when the manifest names a key this cache has decoded, else the facility shape.
///
/// **Not ready is not a failure** — the rule a hull's picture and an object's artwork keep. A kind
/// with no code, a code the library does not carry, or a key with no bytes all draw the same honest
/// shape; none of them is an error state.
pub fn mark_for(placed: &Placed, assets: &HashMap<String, rune::Image>) -> Mark {
    let Some(key) = &placed.asset_key else {
        return Mark::Generic;
    };
    match assets.get(key) {
        Some(image) if image.is_ready() => Mark::Asset {
            image: image.clone(),
        },
        _ => Mark::Generic,
    }
}

/// The layer, sized to the frame and drawn over it — `None` when nothing is drawn.
pub fn layer(
    placed: &[Placed],
    frame: &Rendered,
    scale: f32,
    assets: &HashMap<String, rune::Image>,
    selected: Option<i64>,
    pane: crate::ui::ViewId,
) -> Option<El> {
    if placed.is_empty() {
        return None;
    }
    let scale = if scale > 0.0 { f64::from(scale) } else { 1.0 };
    let mut layer = el()
        .key(format!("installations-{pane}"))
        .w_full()
        .h_full()
        .absolute(0.0, 0.0)
        .size(
            frame.width as f32 / scale as f32,
            frame.height as f32 / scale as f32,
        );
    for installation in placed {
        layer = layer.child(mark(installation, scale, assets, selected, pane));
    }
    Some(layer)
}

/// One facility's mark, at its projected point.
fn mark(
    installation: &Placed,
    scale: f64,
    assets: &HashMap<String, rune::Image>,
    selected: Option<i64>,
    pane: crate::ui::ViewId,
) -> El {
    let x = (installation.x / scale) as f32;
    let y = (installation.y / scale) as f32;
    let half = MARK / 2.0;
    let id = installation.id;
    let mut element = el()
        .key(format!("installation-{pane}-{id}"))
        .absolute(x - half, y - half)
        .size(MARK, MARK)
        // A press selects **this pane's** facility, or closes its card when it is already open.
        .on_press(move |state: &mut crate::ui::State| press(state, pane, id));
    // **The selection is the pane's**, so it is marked the way a selected post is: the same edge the
    // focused pane uses, on the mark rather than a second floating ring (#99, #116).
    if selected == Some(id) {
        element = element.border(tokens::HAIRLINE, tokens::focus_edge());
    }
    match mark_for(installation, assets) {
        Mark::Asset { image } => element.pixels_fit(image.pixels(), rune::SurfaceFit::Fit),
        Mark::Generic => element.shape(facility_shape()),
    }
}

/// The generic facility shape: a filled house in a 200-unit box — pitched roof, walls, a door gap.
///
/// **Not a hull symbol and not an object's diamond.** A hull is a quatrefoil in the accent, an object
/// without its artwork is a hollow diamond; a place is a filled warm silhouette, so the three layers
/// can be told apart across a room and a `Pangkalan` never reads as a ship.
fn facility_shape() -> Shape {
    const HOUSE: [[f32; 2]; 8] = [
        [100.0, 10.0],
        [190.0, 84.0],
        [160.0, 84.0],
        [160.0, 190.0],
        [118.0, 190.0],
        [118.0, 150.0],
        [82.0, 150.0],
        [82.0, 190.0],
    ];
    let mut house = Shape::new()
        .view_box(200.0, 200.0)
        .fill(HOUSE.to_vec(), tokens::facility_ink());
    // The wall's own floor line: a base's shape with a gap under it is a house, and the door is the
    // gap. The floor is a stroke so the gap reads at 24 pt.
    house = house.stroke(
        vec![[40.0, 190.0], [160.0, 190.0]],
        tokens::SYMBOL_STROKE * 200.0,
        tokens::facility_ink(),
    );
    house
}

/// A press on a facility: select it in **this pane**, or close its card if it is already selected.
///
/// A hull card and a facility card are both "this pane's card", so selecting one closes the other:
/// one pane, one card (`INV-10` per pane). This is the only way a facility selection changes.
pub fn press(state: &mut crate::ui::State, pane: crate::ui::ViewId, id: i64) {
    state.layout.focus(pane);
    if state.installation_in(pane) == Some(id) {
        state.clear_selection(pane);
    } else {
        state.clear_selection(pane);
        if let Some(pane) = state.pane_mut(pane) {
            pane.view.installation = Some(id);
        }
    }
    state.layout_dirty = true;
}

/// The facility card: **what the row carries**, not a hull's card.
///
/// Name, kind, branch id and note, and nothing invented: an installation has no commander, no speed
/// and no specification, so there is no row for them. The surface and the placement are the hull
/// card's — one card on this wall — but the content is the facility's (`#109`).
pub fn card(placed: &Placed, placement: Placement) -> El {
    let mut body = col()
        .key("installation-card")
        .absolute(placement.x as f32, placement.y as f32)
        .w(WIDTH as f32)
        .gap(tokens::CARD_GAP)
        .padding(tokens::PAD)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::CARD_BORDER, tokens::card_edge())
        .fill(tokens::panel())
        // A press on the card must not clear the selection behind it.
        .on_press(|_state: &mut crate::ui::State| {})
        .child(
            text(placed.name.clone())
                .key("installation-card-name")
                .font(tokens::TITLE)
                .color(tokens::ink()),
        );
    if let Some(kind) = &placed.type_name {
        body = body.child(
            text(kind.clone())
                .key("installation-card-type")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }
    body = body.child(
        row()
            .w_full()
            .gap(tokens::GAP)
            .child(text("BRANCH").font(tokens::LABEL).color(tokens::ink_dim()))
            .child(spacer())
            .child(
                text(placed.branch.to_string())
                    .key("installation-card-branch")
                    .font(tokens::LABEL)
                    .color(tokens::ink()),
            ),
    );
    if let Some(note) = &placed.note {
        body = body.child(
            text(note.clone())
                .key("installation-card-note")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }
    body
}

/// The card's placement for a facility at `anchor`, in the pane's own points. One function, so the
/// view and any assertion agree where the card starts.
pub fn card_placement(anchor: (f64, f64), viewport: (f64, f64)) -> Placement {
    card::place(anchor, viewport)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{Envelope, TacticalObjectManifest};
    use crate::map::geometry::Camera;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
    }

    fn installations() -> Vec<Installation> {
        let envelope: Envelope<Vec<Installation>> =
            serde_json::from_str(&fixture("scenes/installations.json"))
                .expect("the scene's installations decode");
        envelope.data
    }

    fn types() -> Vec<InstallationType> {
        let envelope: Envelope<Vec<InstallationType>> =
            serde_json::from_str(&fixture("rest/installation-types.json"))
                .expect("the recorded vocabulary decodes");
        envelope.data
    }

    fn manifest() -> HashMap<String, TacticalObjectManifestEntry> {
        let envelope: Envelope<TacticalObjectManifest> =
            serde_json::from_str(&fixture("rest/tactical-objects-manifest.json"))
                .expect("the recorded manifest decodes");
        envelope
            .data
            .symbols
            .iter()
            .map(|entry| (entry.code.clone(), entry.clone()))
            .collect()
    }

    fn frame(zoom: f64) -> Rendered {
        Rendered {
            camera: Camera::new((-5.9835, 106.6636), zoom),
            width: 640,
            height: 600,
        }
    }

    /// **No position, no mark** (`INV-02`): a facility registered before its location is known is in
    /// the cache and not on the map, and certainly not at (0, 0).
    #[test]
    fn a_facility_without_a_position_is_not_placed() {
        let rows = installations();
        let unlocated = rows
            .iter()
            .find(|row| row.coordinates().is_none())
            .expect("the scene carries an unlocated facility");
        let placed = place(
            &rows,
            &types(),
            &manifest(),
            &frame(10.6),
            &Filter::default(),
        );
        assert!(
            placed.iter().all(|mark| mark.id != unlocated.id),
            "the unlocated facility has no mark"
        );
        assert!(
            placed.iter().all(|mark| mark.x != 0.0 || mark.y != 0.0),
            "and no facility is drawn at the origin"
        );
        assert_eq!(
            placed.len(),
            rows.iter()
                .filter(|row| row.coordinates().is_some())
                .count(),
            "one mark per located facility"
        );
    }

    /// **The filter is the pane's** (#112): the layer hides every facility, and one hidden type hides
    /// only its own — the same precedence the objects layer keeps.
    #[test]
    fn a_hidden_layer_or_type_removes_the_mark() {
        let rows = installations();
        let types = types();
        let manifest = manifest();
        let frame = frame(10.6);
        let all = place(&rows, &types, &manifest, &frame, &Filter::default());
        let hidden_type = all.first().expect("a located facility").id_type;

        let filter = Filter {
            installations: true,
            ..Filter::default()
        };
        assert!(
            place(&rows, &types, &manifest, &frame, &filter).is_empty(),
            "the layer switch hides every facility"
        );

        let mut filter = Filter::default();
        filter.hidden_types.insert(hidden_type);
        let filtered = place(&rows, &types, &manifest, &frame, &filter);
        assert!(
            filtered.iter().all(|mark| mark.id_type != hidden_type),
            "a hidden type hides only its own"
        );
        assert!(
            filtered.len() < all.len(),
            "and leaves the other types on the map"
        );
    }

    /// **`symbol_code` is a code** (`#107`): it resolves through the manifest to an asset key, and a
    /// code the library does not carry (or a kind with no code) draws the facility shape. The firing
    /// does not fall back to a hull's symbol.
    #[test]
    fn a_symbol_code_resolves_through_the_library_and_unknown_codes_are_generic() {
        let mut kinds = types();
        let kind_id = installations()[0].id_installation_type;
        kinds
            .iter_mut()
            .find(|kind| kind.id == kind_id)
            .expect("the scene's kind")
            .symbol_code = Some("LIGHTS_FIX".to_string());
        let placed = place(
            &installations(),
            &kinds,
            &manifest(),
            &frame(10.6),
            &Filter::default(),
        );
        let marked = placed
            .iter()
            .find(|mark| mark.id_type == kind_id)
            .expect("a facility of that type");
        assert_eq!(
            marked.asset_key.as_deref(),
            Some("minos/tactical-objects/LIGHTS_FIX.svg"),
            "the code resolves to the manifest's key, not to an id"
        );
        // No decoded bytes is the honest generic mark, not an error and not a hull symbol.
        assert!(matches!(mark_for(marked, &HashMap::new()), Mark::Generic));

        let mut unknown = kinds.clone();
        unknown
            .iter_mut()
            .find(|kind| kind.id == placed[0].id_type)
            .expect("kind")
            .symbol_code = Some("A_CODE_THE_LIBRARY_DOES_NOT_CARRY".to_string());
        let placed = place(
            &installations(),
            &unknown,
            &manifest(),
            &frame(10.6),
            &Filter::default(),
        );
        assert!(
            placed
                .iter()
                .filter(|mark| mark.id_type == unknown[0].id)
                .all(|mark| mark.asset_key.is_none()),
            "an unknown code has no key, so the facility shape is drawn"
        );

        let mut none = kinds;
        none.iter_mut().for_each(|kind| kind.symbol_code = None);
        assert!(
            place(
                &installations(),
                &none,
                &manifest(),
                &frame(10.6),
                &Filter::default()
            )
            .iter()
            .all(|mark| mark.asset_key.is_none()),
            "a kind with no code has no key"
        );
    }

    /// **`INV-04` on this layer**: a mark is its own frame's projection, and two panes at two zooms put
    /// the same facility at two points. A layer drawn from the wall's camera lands tens of pixels away
    /// in the neighbour — the falsifier the capture names.
    #[test]
    fn a_facility_lands_where_this_panes_projection_puts_it() {
        let rows = installations();
        let types = types();
        let manifest = manifest();
        let near = place(&rows, &types, &manifest, &frame(10.6), &Filter::default());
        let far = place(&rows, &types, &manifest, &frame(9.5), &Filter::default());
        let row = rows
            .iter()
            .find(|row| row.coordinates().is_some())
            .expect("a located facility");
        let (lat, lon) = row.coordinates().expect("its position");
        let (x, y) = frame(10.6).project(lat, lon);
        let mark = near
            .iter()
            .find(|mark| mark.id == row.id)
            .expect("its mark");
        assert!(
            (mark.x - x).abs() < 1e-9 && (mark.y - y).abs() < 1e-9,
            "the mark is the frame's own projection"
        );
        let other = far.iter().find(|mark| mark.id == row.id).expect("its mark");
        assert!(
            (mark.x - other.x).abs() > 8.0,
            "INV-04: two zooms put the same facility at two points, got {} and {}",
            mark.x,
            other.x
        );
    }

    /// **One pane, one card**: pressing a facility selects it there and closes a hull's card, and
    /// selecting a hull closes the facility's. Two panes keep two selections.
    #[test]
    fn a_facility_is_selected_in_its_pane_and_never_beside_a_hull_card() {
        let mut state = crate::scene::state_for("installations");
        let first = state.layout.focused();
        let Some(second) = state.split_pane(first, Camera::START) else {
            panic!("the wall holds two panes");
        };
        let facility = state.installations[0].id;
        press(&mut state, first, facility);
        assert_eq!(state.installation_in(first), Some(facility));
        assert_eq!(
            state.installation_in(second),
            None,
            "the neighbour's card is its own"
        );

        // A hull selection replaces the facility card in that pane, and only that pane.
        state.select_in(first, crate::scene::LIVE_UNIT);
        assert_eq!(
            state.installation_in(first),
            None,
            "selecting a hull closes the facility's card"
        );
        assert_eq!(
            state.installation_in(second),
            None,
            "and touches no other pane"
        );

        press(&mut state, second, facility);
        assert_eq!(state.installation_in(second), Some(facility));
        assert_eq!(
            state.installation_in(first),
            None,
            "the panes are independent"
        );
    }
}
