//! The tactical-object layer (#110): the operational map's fixed furniture — a buoy, a cone, a light.
//!
//! **Not vessels and not tracks.** A placement has no speed, no course and no commander, and it never
//! enters the fix chain. It is one library symbol at one coordinate, and it **embeds** that symbol, so
//! drawing never needs a second lookup.
//!
//! The rules the layer keeps:
//!
//! - **Projected against this pane's frame** (`INV-04`). The layer is built per pane from the same
//!   [`Rendered`] its hull markers use, so a tiled wall draws the same water's objects in every pane
//!   showing it — and a placement drawn against the wall's camera lands in the wrong place on any
//!   pane but the focused one.
//! - **A label is optional, and an empty label is not a label** (`INV-01`). `label` absent, `null` or
//!   `""` draws no plate; only a label somebody actually wrote draws one.
//! - **`rotation_deg` turns the artwork when it is present.** Absent means "as stored" — the identity
//!   turn, not a bearing anybody claimed.
//! - **Artwork or the generic mark.** The bytes come from the manifest's `asset_key` through the same
//!   decoded cache a hull's picture uses (`State::offer_asset`). An unsigned or expired URL is a log
//!   line and a hollow diamond, never an error state: a signing failure omits the URL, not the symbol.
//!
//! The module is pure where it matters: [`place`] is the projection and [`label_rect`] is the plate's
//! geometry, so `verify` asserts against the same functions the view draws from.

use std::collections::HashMap;

use rune::{El, el, row, text};

use crate::data::types::{TacticalObjectManifestEntry, TacticalObjectPlacement};
use crate::map::geometry::Rendered;
use crate::ui::tokens;

/// A tactical object's mark, in points: the box its artwork is drawn in and the generic mark's size.
/// The same 24 pt a hull's symbol uses, because it is read from the same distance across the room.
pub const MARK: f32 = tokens::OBJECT_MARK;

/// One placement, projected into **this** pane's frame.
///
/// The label and the asset key are resolved here rather than at draw time so the decision is a value:
/// `verify` can ask what a placement *is* without building an element tree, and a unit test can prove
/// that an empty label is no label and that an unsigned entry is still a known symbol.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: i64,
    /// The library symbol's stable code. Kept for the log and for a reader of the assertion.
    pub code: String,
    /// The point in the frame's own pixels.
    pub x: f64,
    pub y: f64,
    /// The plate's text, or `None` when nobody wrote one — absent and empty both mean no plate.
    pub label: Option<String>,
    /// The manifest's key for the artwork, when the library knows this symbol.
    pub asset_key: Option<String>,
    /// Clockwise degrees from the wire, or `None` when the placement does not say.
    pub rotation_deg: Option<f64>,
}

/// Project every placement against `frame` that **this pane's filter** leaves visible, and resolve what
/// each one draws.
///
/// **The filter decides here, once** (#112): a placement whose layer, domain or category the pane hides
/// is absent from the layer, rather than skipped at the element builder or by three callers agreeing to
/// ask the same question. Projecting against the frame passed in — and never a wall-wide camera — is the
/// whole of `INV-04` on this layer.
pub fn place(
    placements: &[TacticalObjectPlacement],
    manifest: &HashMap<String, TacticalObjectManifestEntry>,
    frame: &Rendered,
    filter: &crate::ui::filter::Filter,
) -> Vec<Placed> {
    placements
        .iter()
        .filter(|placement| {
            !filter.hides_object_in(
                placement.symbol.category.domain.id,
                placement.symbol.category.id,
            )
        })
        .map(|placement| {
            let (x, y) = frame.project(placement.latitude, placement.longitude);
            Placed {
                id: placement.id,
                code: placement.symbol.code.clone(),
                x,
                y,
                label: label_of(placement.label.as_deref()),
                // An unsigned entry is **kept**: the symbol exists and has a name, and only its
                // picture is missing. The key is the cache's identity either way.
                asset_key: manifest
                    .get(&placement.symbol.code)
                    .map(|entry| entry.asset_key.clone()),
                rotation_deg: placement.rotation_deg,
            }
        })
        .collect()
}

/// What a placement's label says, or `None` when it has none.
///
/// `label` is optional on the wire even though the spec lists it as required, and the recording sends
/// `""` for "nobody wrote one". `shown_name` is the wall's one answer to "is this text a name": it
/// trims, cuts with a drawn ellipsis, and answers `None` for an empty string (`INV-01`).
pub fn label_of(label: Option<&str>) -> Option<String> {
    label.and_then(crate::ui::shown_name)
}

/// What one placement is drawn as this frame.
#[derive(Clone, Debug)]
pub enum Mark {
    /// The library's artwork, decoded and ready, turned by `rotation_deg` when the wire stated one.
    Asset {
        image: rune::Image,
        rotation_deg: f32,
    },
    /// A hollow diamond: "something is here, and this wall does not have its picture".
    Generic,
}

/// The artwork when the cache has decoded it, else the generic mark.
///
/// **Not ready is not a failure**, the same rule a hull's picture keeps: a symbol that vanished while
/// its artwork downloaded would be worse than a symbol drawn as what it is. A key with no bytes — an
/// unsigned URL, an expired one, or a manifest entry this cache never saw — draws the generic mark.
pub fn mark_for(placed: &Placed, assets: &HashMap<String, rune::Image>) -> Mark {
    let Some(key) = &placed.asset_key else {
        return Mark::Generic;
    };
    match assets.get(key) {
        Some(image) if image.is_ready() => Mark::Asset {
            image: image.clone(),
            // Absent rotation is the identity: the artwork as stored, not a bearing. `INV-01` keeps
            // "nobody said which way it points" out of the drawing, the way a hull's card keeps an
            // absent course out of its text.
            rotation_deg: placed.rotation_deg.unwrap_or(0.0) as f32,
        },
        _ => Mark::Generic,
    }
}

/// The layer, sized to the frame and drawn over it — `None` when there is nothing to draw, so a wall
/// with no placements adds no element at all (and every golden before #110 is untouched).
pub fn layer(
    placed: &[Placed],
    frame: &Rendered,
    scale: f32,
    assets: &HashMap<String, rune::Image>,
    pane: crate::ui::ViewId,
) -> Option<El> {
    if placed.is_empty() {
        return None;
    }
    // The frame is device pixels and Rune lays out in points; the same conversion `markers::layer`
    // makes, for the same reason.
    let scale = if scale > 0.0 { f64::from(scale) } else { 1.0 };
    let mut layer = el()
        .key(format!("objects-{pane}"))
        .w_full()
        .h_full()
        .absolute(0.0, 0.0)
        .size(
            frame.width as f32 / scale as f32,
            frame.height as f32 / scale as f32,
        );
    for object in placed {
        layer = layer.child(mark(object, scale, assets, pane));
        // The plate is a **sibling** of the mark, not its child: the artwork may be turned, and a
        // label turned with it would be unreadable.
        if let Some(label) = &object.label {
            layer = layer.child(plate(object, label, scale, pane));
        }
    }
    Some(layer)
}

/// One object's mark, at its projected point.
fn mark(
    object: &Placed,
    scale: f64,
    assets: &HashMap<String, rune::Image>,
    pane: crate::ui::ViewId,
) -> El {
    let x = (object.x / scale) as f32;
    let y = (object.y / scale) as f32;
    let half = MARK / 2.0;
    let element = el()
        .key(format!("object-{pane}-{}", object.id))
        .absolute(x - half, y - half)
        .size(MARK, MARK);
    match mark_for(object, assets) {
        Mark::Asset {
            image,
            rotation_deg,
        } => element
            .pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
            .rotate(rotation_deg.to_radians()),
        Mark::Generic => element
            // A square turned a quarter is a diamond, which is nobody's hull symbol and nobody's
            // accent: the wall knows something is there and does not know what it looks like.
            .rotate(std::f32::consts::FRAC_PI_4)
            .border(tokens::HAIRLINE, tokens::ink_dim()),
    }
}

/// One object's label plate: the same surface and type a pane's name uses, because a label is a name
/// somebody typed. Drawn only for a label that exists — an empty plate is worse than no plate.
fn plate(object: &Placed, label: &str, scale: f64, pane: crate::ui::ViewId) -> El {
    let rect = label_rect((object.x / scale, object.y / scale));
    row()
        .key(format!("object-label-{pane}-{}", object.id))
        .absolute(rect.x, rect.y)
        .h(tokens::NAME_H)
        .max_w(tokens::NAME_MAX_W)
        .items_center()
        .px(tokens::GAP)
        .radius(tokens::NAME_H / 2.0)
        .fill(tokens::name_plate())
        .border(tokens::HAIRLINE, tokens::hairline())
        .child(
            text(label.to_string())
                .font(tokens::LABEL)
                .color(tokens::ink()),
        )
}

/// Where a placement's label plate starts, in the frame's own points: to the right of the mark,
/// vertically centred. One function, so `verify` probes the rectangle the view fills.
pub fn label_rect(at: (f64, f64)) -> crate::ui::Rect {
    let x = at.0 + f64::from(MARK) / 2.0 + f64::from(tokens::GAP) / 2.0;
    let y = at.1 - f64::from(tokens::NAME_H) / 2.0;
    crate::ui::Rect {
        x: x as f32,
        y: y as f32,
        // The plate is as wide as its text; this is the cap the row is drawn with, not a claim about
        // where its right edge lands.
        w: tokens::NAME_MAX_W,
        h: tokens::NAME_H,
    }
}

/// The part of a label plate that is **always** fill when a plate is drawn: the left padding, before
/// the first glyph, at the plate's vertical middle.
///
/// `verify` asks this rectangle for the plate's own colour. Asking the whole plate would pass on an
/// empty one — a `""` label still draws a surface if the code defaults it — which is the falsifier
/// this ticket names.
pub fn label_probe(at: (f64, f64)) -> crate::ui::Rect {
    let rect = label_rect(at);
    let side = tokens::GAP / 2.0;
    crate::ui::Rect {
        x: rect.x + side / 2.0,
        y: rect.y + (rect.h - side) / 2.0,
        w: side,
        h: side,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{Envelope, TacticalObjectManifest, TacticalObjectPlacement};
    use crate::map::geometry::Camera;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/rest")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
    }

    fn placements() -> Vec<TacticalObjectPlacement> {
        let envelope: Envelope<Vec<TacticalObjectPlacement>> =
            serde_json::from_str(&fixture("tactical-object-placements.json"))
                .expect("the recorded placements decode");
        envelope.data
    }

    fn manifest() -> HashMap<String, TacticalObjectManifestEntry> {
        let envelope: Envelope<TacticalObjectManifest> =
            serde_json::from_str(&fixture("tactical-objects-manifest.json"))
                .expect("the recorded manifest decodes");
        envelope
            .data
            .symbols
            .into_iter()
            .map(|entry| (entry.code.clone(), entry))
            .collect()
    }

    fn frame(zoom: f64) -> Rendered {
        Rendered {
            camera: Camera::new((-5.9835, 106.6636), zoom),
            width: 640,
            height: 600,
        }
    }

    /// Eight pixels, the same geography budget `verify` uses: a wrong camera is tens of pixels out,
    /// a rasteriser's rounding is one or two.
    const TOLERANCE: f64 = 8.0;

    /// `INV-04` on this layer: the point is **this frame's** projection, and two panes at two zooms
    /// put the same placement in two different places. A layer drawn from one wall-wide camera would
    /// agree with the first pane and be tens of pixels out in the second.
    #[test]
    fn a_placement_lands_where_this_panes_projection_puts_it() {
        let marks = place(
            &placements(),
            &manifest(),
            &frame(10.6),
            &crate::ui::filter::Filter::default(),
        );
        assert_eq!(marks.len(), 3, "the recorded operational map");
        let first = &marks[0];
        let (x, y) = frame(10.6).project(placements()[0].latitude, placements()[0].longitude);
        assert!(
            (first.x - x).abs() < 1e-9 && (first.y - y).abs() < 1e-9,
            "the placement is the frame's own projection, not a second conversion"
        );

        let near = place(
            &placements(),
            &manifest(),
            &frame(10.6),
            &crate::ui::filter::Filter::default(),
        );
        let far = place(
            &placements(),
            &manifest(),
            &frame(9.5),
            &crate::ui::filter::Filter::default(),
        );
        assert!(
            (near[0].x - far[0].x).abs() > TOLERANCE,
            "INV-04: two panes at two zooms put the same buoy at two points, got {} and {}",
            near[0].x,
            far[0].x
        );
    }

    /// **A hidden domain (or category) is absent from the layer, and its siblings still draw** (#112).
    /// The decision is the placement's, so a caller cannot draw a placement the pane asked it to hide.
    #[test]
    fn a_hidden_domain_or_category_leaves_the_layer_but_not_its_siblings() {
        let frame = frame(10.6);
        let all = place(
            &placements(),
            &manifest(),
            &frame,
            &crate::ui::filter::Filter::default(),
        );
        assert_eq!(
            all.len(),
            3,
            "the recorded marks are all visible by default"
        );

        let domain = placements()[0].symbol.category.domain.id;
        let mut filter = crate::ui::filter::Filter::default();
        filter.hidden_domains.insert(domain);
        assert!(
            place(&placements(), &manifest(), &frame, &filter).is_empty(),
            "every recorded mark is in this domain, so hiding it hides the whole layer"
        );

        let category = placements()[0].symbol.category.id;
        let mut filter = crate::ui::filter::Filter::default();
        filter.hidden_categories.insert(category);
        assert!(
            place(&placements(), &manifest(), &frame, &filter).is_empty(),
            "and the same for the category the recording puts them all in"
        );

        let mut filter = crate::ui::filter::Filter::default();
        filter
            .hidden_categories
            .insert(category.saturating_add(1_000));
        assert_eq!(
            place(&placements(), &manifest(), &frame, &filter).len(),
            3,
            "a hidden category hides only itself, so a mark in another one still draws: {filter:?}"
        );
    }

    /// `INV-01`: an absent label is no label, and neither is an empty or whitespace-only one. Only
    /// text somebody wrote draws a plate.
    #[test]
    fn an_absent_or_empty_label_draws_no_plate() {
        assert_eq!(label_of(None), None, "INV-01: absent is not a plate");
        assert_eq!(
            label_of(Some("")),
            None,
            "INV-01: an empty label is not a label"
        );
        assert_eq!(
            label_of(Some("   ")),
            None,
            "INV-01: whitespace is not a label either"
        );
        assert_eq!(
            label_of(Some(" North Cardinal 3 ")),
            Some("North Cardinal 3".to_string()),
            "a label somebody wrote is drawn, trimmed like a pane's name"
        );
    }

    /// The recorded fixture sends `label: ""` and `rotation_deg: 0`; both are values, and neither may
    /// be defaulted into a plate or a bearing. The absence case is the same rows with the keys gone.
    #[test]
    fn the_recorded_absence_and_its_empty_value_both_mean_no_label() {
        let marks = place(
            &placements(),
            &manifest(),
            &frame(10.6),
            &crate::ui::filter::Filter::default(),
        );
        assert_eq!(
            marks.iter().filter(|mark| mark.label.is_some()).count(),
            0,
            "the recording wrote no labels"
        );

        let body = fixture("tactical-object-placements.json")
            .replace("\"label\": \"\",\n", "")
            .replace("\"rotation_deg\": 0,\n", "");
        let envelope: Envelope<Vec<TacticalObjectPlacement>> =
            serde_json::from_str(&body).expect("the absence case decodes");
        let marks = place(
            &envelope.data,
            &manifest(),
            &frame(10.6),
            &crate::ui::filter::Filter::default(),
        );
        assert!(marks.iter().all(|mark| mark.label.is_none()));
        assert!(
            marks.iter().all(|mark| mark.rotation_deg.is_none()),
            "INV-01: an absent rotation is not 0 degrees"
        );
    }

    /// `rotation_deg` turns the artwork when it is there, and its absence is the identity turn rather
    /// than a bearing the wall invented.
    #[test]
    fn a_rotation_is_carried_and_its_absence_is_the_identity() {
        let mut rows = placements();
        rows[0].rotation_deg = Some(45.0);
        rows[1].rotation_deg = None;
        let marks = place(
            &rows,
            &manifest(),
            &frame(10.6),
            &crate::ui::filter::Filter::default(),
        );
        assert_eq!(marks[0].rotation_deg, Some(45.0));
        assert_eq!(marks[1].rotation_deg, None);
        // With no decoded bytes every mark is generic; `mark_for` takes the placement's own turn when
        // the bytes arrive, and the `objects` capture is the witness that the turn reaches the pixels.
        let assets = HashMap::new();
        assert!(matches!(mark_for(&marks[0], &assets), Mark::Generic));
    }

    /// The manifest is the cache's identity: a placement's embedded symbol names a `code`, and the
    /// manifest turns that into an `asset_key`. A code the library does not carry has no key and
    /// draws the generic mark.
    #[test]
    fn the_asset_key_comes_from_the_manifest_and_never_from_the_placement() {
        let marks = place(
            &placements(),
            &manifest(),
            &frame(10.6),
            &crate::ui::filter::Filter::default(),
        );
        assert_eq!(
            marks[0].asset_key.as_deref(),
            Some("minos/tactical-objects/LIGHTS_FIX.svg")
        );
        let empty = HashMap::new();
        let marks = place(
            &placements(),
            &empty,
            &frame(10.6),
            &crate::ui::filter::Filter::default(),
        );
        assert!(
            marks.iter().all(|mark| mark.asset_key.is_none()),
            "a symbol the library does not carry has no artwork to ask for"
        );
        assert!(matches!(
            mark_for(&marks[0], &HashMap::new()),
            Mark::Generic
        ));
    }

    /// **A signing failure omits the URL, not the entry** (#110): the symbol is still known and its
    /// key is still cached, and with no bytes the layer draws the generic mark rather than failing.
    #[test]
    fn an_unsigned_entry_is_kept_and_draws_the_generic_mark() {
        let mut library = manifest();
        let entry = library
            .get_mut("LIGHTS_FIX")
            .expect("the recorded library carries this symbol");
        entry.asset_url = None;
        let marks = place(
            &placements(),
            &library,
            &frame(10.6),
            &crate::ui::filter::Filter::default(),
        );
        assert_eq!(
            marks[0].asset_key.as_deref(),
            Some("minos/tactical-objects/LIGHTS_FIX.svg"),
            "an unsigned URL leaves the entry and its key in place"
        );
        assert!(
            matches!(mark_for(&marks[0], &HashMap::new()), Mark::Generic),
            "no bytes means the generic mark, not an error state"
        );
    }

    /// The label plate's probe is inside the plate and clear of the glyphs, and it sits to the right
    /// of the mark rather than over it — which is what lets `verify` tell a plate from a buoy.
    #[test]
    fn the_label_probe_is_inside_the_plate_and_clear_of_the_mark() {
        let at = (100.0, 100.0);
        let rect = label_rect(at);
        let probe = label_probe(at);
        assert!(
            rect.x > at.0 as f32 + MARK / 2.0,
            "the plate starts to the right of the mark"
        );
        assert!(probe.x > rect.x && probe.x + probe.w < rect.x + rect.w);
        assert!(probe.y > rect.y && probe.y + probe.h < rect.y + rect.h);
        assert!(
            (rect.y + rect.h / 2.0 - at.1 as f32).abs() < 0.5,
            "the plate is centred on the mark"
        );
    }
}
