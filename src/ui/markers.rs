//! The marker layer: one element per hull that has a fix, placed where the map's own projection puts
//! it.
//!
//! **Projection parity is by construction** (`INV-04`): the layer takes the [`Rendered`] frame the map
//! thread published — its camera and its size — and calls `Rendered::project`. There is no second
//! lat/lon → pixel conversion in the crate, and the test that proves it lives next to the projection.
//!
//! The rules the drawing keeps:
//!
//! - **No fix, no marker.** A hull that has never reported is in the picture and not on the map. There
//!   is no marker at latitude 0 unless a fix really is there (`INV-02`).
//! - **`backfilled` is history**, drawn as a hollow mark — never the "here now" glyph (`INV-03`).
//! - **Absence renders nothing.** No `course_deg` means no heading tick: a hull with no course does
//!   not point north (`INV-01`). The ring's radius follows `accuracy_m` only when it was measured.
//! - **Every marker has a 24 pt target** even when the glyph is 12, because a touch target is a
//!   decision about fingers, not about the size of the symbol.
//!
//! MIL-STD-2525 symbology is a later ticket; [`glyph`] is the seam it will land behind.

use std::time::Instant;

use rune::{El, el};

use crate::map::geometry::Rendered;
use crate::ui::tokens;
use crate::units::{Glyph, Pos, UnitView};

/// The symbol's side, in points. In `tokens` with every other size, not here.
pub const GLYPH: f32 = tokens::SYMBOL;
/// The hit target's side, in points: at least 24, whatever the glyph is.
pub const TARGET: f32 = tokens::MARKER_TARGET;

/// One marker's placement: where its centre goes, in the frame's own pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub id_unit: i64,
    pub x: f64,
    pub y: f64,
    pub glyph: Glyph,
    pub course_deg: Option<f64>,
    pub accuracy_m: Option<f64>,
    /// The latitude the marker is drawn at, for the ground resolution the accuracy ring needs.
    pub lat: f64,
    /// The zoom of the frame it is drawn on.
    pub zoom: f64,
    /// Symbol or picture, and the picture's size and rotation when it is one (`units::mark`).
    pub mark: crate::units::mark::Mark,
}

/// Project every hull that has a fix, and nothing else.
///
/// Pure, so the placement can be asserted without a window: the test that no marker exists for a
/// silent hull asks this function, and the test that a marker lands on its projection asks it too.
pub fn place(
    picture: &crate::units::Picture,
    frame: &Rendered,
    now: Instant,
    stale_after: std::time::Duration,
    range: crate::units::mark::Range,
    sizes: &dyn Fn(i64) -> Option<crate::units::mark::Size>,
) -> Vec<Placed> {
    picture
        .units()
        .iter()
        .filter_map(|unit| place_one(unit, frame, now, stale_after, range, sizes))
        .collect()
}

fn place_one(
    unit: &UnitView,
    frame: &Rendered,
    now: Instant,
    stale_after: std::time::Duration,
    range: crate::units::mark::Range,
    sizes: &dyn Fn(i64) -> Option<crate::units::mark::Size>,
) -> Option<Placed> {
    let glyph = unit.glyph_with(now, stale_after)?;
    let drawn: Pos = unit.drawn(now)?;
    let (x, y) = frame.project(drawn.lat, drawn.lon);
    Some(Placed {
        id_unit: unit.unit.id_unit,
        x,
        y,
        glyph,
        course_deg: unit.course_deg,
        accuracy_m: unit.accuracy_m,
        lat: drawn.lat,
        zoom: frame.camera.zoom,
        mark: crate::units::mark::mark(range, unit, frame, sizes(unit.unit.id_unit))?,
    })
}

/// The marker layer, sized to the frame and drawn above it.
///
/// `scale` converts the frame's device pixels into the points Rune lays out in: the frame is rendered
/// at device resolution so its labels are sharp, and a marker placed in device pixels on a 2× panel
/// would sit at twice its coordinate.
pub fn layer(placed: &[Placed], frame: &Rendered, scale: f32, pictures: &Pictures) -> El {
    let scale = if scale > 0.0 { f64::from(scale) } else { 1.0 };
    let mut layer = el()
        .key("markers")
        .w_full()
        .h_full()
        .absolute(0.0, 0.0)
        .size(
            frame.width as f32 / scale as f32,
            frame.height as f32 / scale as f32,
        );
    for marker in placed {
        layer = layer.child(glyph(marker, scale, pictures));
    }
    layer
}

/// Where the pictures come from: one **decoded** handle per hull, owned by the state.
///
/// The state asks the cache once, when a hull's bytes arrive (`State::offer_picture`), and keeps the
/// handle. Asking here instead — per marker per frame — both re-ran the decode (the cache keeps a weak
/// entry per content hash, so a dropped handle expires it) and left `poll` unable to tell when a decode
/// finished, which is how a wall ended up drawing symbols over fetched photographs (`#51`).
pub struct Pictures<'a> {
    pub images: &'a std::collections::HashMap<i64, rune::Image>,
}

impl Pictures<'_> {
    /// A hull's picture, if one is named and the cache has finished decoding it.
    ///
    /// **Not ready is not a failure**: a picture that has not arrived draws the symbol, because a hull
    /// that vanishes while its photograph downloads is worse than a hull drawn as what it is.
    fn ready(&self, id_unit: i64) -> Option<rune::Image> {
        self.images
            .get(&id_unit)
            .filter(|image| image.is_ready())
            .cloned()
    }
}

/// The mark a hull earns: its picture when the range is close enough and one is ready, else its symbol.
fn glyph(marker: &Placed, scale: f64, pictures: &Pictures) -> El {
    let accuracy = accuracy_ring(marker);
    let mark = match marker.mark {
        crate::units::mark::Mark::Picture {
            length_pt,
            beam_pt,
            rotation,
            ..
        } => match pictures.ready(marker.id_unit) {
            Some(image) => {
                // The picture is sized from length and beam and turned about its centre: a hull drawn
                // at the wrong size is a wrong measurement, and one turned about its corner drifts off
                // its own position.
                let picture = el()
                    .key(format!("marker-{}", marker.id_unit))
                    .size(length_pt, beam_pt)
                    .pixels_fit(image.pixels(), rune::SurfaceFit::Fill)
                    // `rotation` is already `course − IMAGE_BOW_DEGREES`: the picture's stored bow is
                    // east, so the turn belongs to `units::mark` and nothing here has a default (#63).
                    .rotate(rotation.to_radians());
                let id_unit = marker.id_unit;
                let mut target = el()
                    .key(format!("hit-{}", id_unit))
                    .absolute(
                        (marker.x / scale) as f32 - TARGET / 2.0,
                        (marker.y / scale) as f32 - TARGET / 2.0,
                    )
                    .size(TARGET.max(length_pt), TARGET.max(beam_pt))
                    .center()
                    .on_press(move |state: &mut crate::ui::State| state.press_marker(id_unit))
                    .child(picture);
                if let Some(ring) = accuracy {
                    target = target.child(ring);
                }
                // No heading tick: the picture's own rotation is its heading.
                return target;
            }
            None => crate::ui::symbol::mark(marker.id_unit, marker.glyph, GLYPH),
        },
        crate::units::mark::Mark::Symbol => {
            crate::ui::symbol::mark(marker.id_unit, marker.glyph, GLYPH)
        }
    };
    let symbol = mark;

    let id_unit = marker.id_unit;
    let mut target = el()
        .key(format!("hit-{}", id_unit))
        .absolute(
            (marker.x / scale) as f32 - TARGET / 2.0,
            (marker.y / scale) as f32 - TARGET / 2.0,
        )
        .size(TARGET, TARGET)
        .center()
        // A press on a marker selects its hull. One card at a time, always (`INV-10`): selecting
        // replaces the selection, it does not add to it.
        .on_press(move |state: &mut crate::ui::State| state.press_marker(id_unit))
        .child(symbol);
    if let Some(ring) = accuracy {
        target = target.child(ring);
    }
    if let Some(course) = marker.course_deg {
        target = target.child(heading(marker, course, scale));
    }
    target
}

/// A ring whose radius is the reported accuracy, when it was measured.
///
/// `accuracy_m` is metres, the frame is pixels, and `meters_per_pixel` is the only conversion between
/// them. An absent accuracy draws no ring: a default radius would claim a precision nobody stated.
fn accuracy_ring(marker: &Placed) -> Option<El> {
    let metres = marker.accuracy_m?;
    if metres <= 0.0 {
        return None;
    }
    // The ground resolution at this frame's camera and the marker's own latitude.
    let mpp = crate::map::geometry::meters_per_pixel(marker.lat, marker.zoom);
    let radius_px = (metres / mpp) as f32;
    // Below the glyph it is invisible; above a quarter of the viewport it is noise rather than
    // information, and drawing it would swamp the map.
    if !(GLYPH..=TARGET * 8.0).contains(&radius_px) {
        return None;
    }
    Some(
        el().size(radius_px * 2.0, radius_px * 2.0)
            .radius(radius_px)
            .border(tokens::HAIRLINE, tokens::hairline()),
    )
}

/// A small tick pointing along the measured course.
///
/// Absent course, no tick: a hull whose heading nobody measured does not point north.
fn heading(marker: &Placed, course_deg: f64, scale: f64) -> El {
    let radians = course_deg.to_radians();
    // Screen y grows downward and a bearing is measured clockwise from north.
    let reach = (GLYPH / 2.0 + 4.0) as f64;
    let dx = radians.sin() * reach;
    let dy = -radians.cos() * reach;
    let half = tokens::HEADING_TICK / 2.0;
    el().absolute(
        (marker.x / scale) as f32 + dx as f32 - half,
        (marker.y / scale) as f32 + dy as f32 - half,
    )
    .size(tokens::HEADING_TICK, tokens::HEADING_TICK)
    .radius(half)
    .fill(tokens::accent())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{FeedFix, FeedPosition};
    use crate::map::geometry::Camera;
    use crate::units::Picture;
    use chrono::{TimeZone, Utc};

    fn frame() -> Rendered {
        Rendered {
            camera: Camera::new((-6.108, 106.910), 11.0),
            width: 1280,
            height: 720,
        }
    }

    fn fix(lat: f64, lon: f64, age: f64, backfilled: bool) -> FeedFix {
        FeedFix {
            latitude: lat,
            longitude: lon,
            speed_kn: None,
            speed_ms: None,
            course_deg: None,
            accuracy_m: None,
            recorded_at: Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap(),
            received_at: Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap(),
            age_seconds: age,
            backfilled,
        }
    }

    fn entry(id: i64, position: Option<FeedFix>) -> FeedPosition {
        FeedPosition {
            id_unit: id,
            name: format!("KRI {id}"),
            hull_number: None,
            position,
        }
    }

    #[test]
    fn a_hull_without_a_fix_is_not_placed_on_the_map() {
        // INV-02: the hull is in the picture, and there is no marker — not at (0,0), not anywhere.
        let mut picture = Picture::default();
        picture.replace_from_feed(&[entry(1, None), entry(8, Some(fix(-6.1, 106.9, 5.0, false)))]);
        let placed = place(
            &picture,
            &frame(),
            Instant::now(),
            crate::units::STALE_AFTER,
            crate::units::mark::Range::Symbol,
            &|_| None,
        );
        assert_eq!(
            placed.len(),
            1,
            "one hull has a fix, so one marker exists: {placed:?}"
        );
        assert_eq!(placed[0].id_unit, 8);
        assert!(
            placed.iter().all(|marker| marker.id_unit != 1),
            "INV-02: the silent hull must have no marker, and certainly none at the origin"
        );
    }

    #[test]
    fn a_marker_lands_exactly_where_the_map_projects_its_fix() {
        // INV-04 seen from the drawing side: the placement is the frame's own projection, within a
        // pixel, for a marker that is not at the frame's centre.
        let frame = frame();
        let mut picture = Picture::default();
        picture.replace_from_feed(&[entry(8, Some(fix(-6.05, 106.95, 5.0, false)))]);
        let placed = place(
            &picture,
            &frame,
            Instant::now(),
            crate::units::STALE_AFTER,
            crate::units::mark::Range::Symbol,
            &|_| None,
        );
        assert_eq!(placed.len(), 1);
        let (x, y) = frame.project(-6.05, 106.95);
        assert!(
            (placed[0].x - x).abs() <= 1.0 && (placed[0].y - y).abs() <= 1.0,
            "marker at ({}, {}) vs projection ({x}, {y})",
            placed[0].x,
            placed[0].y
        );
        assert!(
            x > 640.0 && y < 360.0,
            "the fixture is off-centre, so the test is not trivially true"
        );
    }

    #[test]
    fn the_three_states_are_three_different_glyphs() {
        let mut picture = Picture::default();
        picture.replace_from_feed(&[
            entry(1, Some(fix(-6.10, 106.90, 2.0, false))),
            entry(2, Some(fix(-6.11, 106.91, 6.0 * 3600.0, false))),
            entry(3, Some(fix(-6.12, 106.92, 1.0, true))),
        ]);
        let placed = place(
            &picture,
            &frame(),
            Instant::now(),
            crate::units::STALE_AFTER,
            crate::units::mark::Range::Symbol,
            &|_| None,
        );
        let glyph = |id: i64| {
            placed
                .iter()
                .find(|marker| marker.id_unit == id)
                .map(|marker| marker.glyph)
        };
        assert_eq!(glyph(1), Some(Glyph::Live));
        assert_eq!(
            glyph(2),
            Some(Glyph::Stale),
            "six hours old is stale, whatever else it is"
        );
        assert_eq!(
            glyph(3),
            Some(Glyph::History),
            "INV-03: backfilled is history even at one second old"
        );
    }

    #[test]
    fn an_absent_course_draws_no_heading_and_an_absent_accuracy_no_ring() {
        // INV-01 on the drawing side: nothing is invented for a measurement nobody took.
        let mut picture = Picture::default();
        picture.replace_from_feed(&[entry(8, Some(fix(-6.1, 106.9, 5.0, false)))]);
        let placed = place(
            &picture,
            &frame(),
            Instant::now(),
            crate::units::STALE_AFTER,
            crate::units::mark::Range::Symbol,
            &|_| None,
        );
        assert_eq!(
            placed[0].course_deg, None,
            "INV-01: no course, no arrow — and certainly not north"
        );
        assert_eq!(placed[0].accuracy_m, None, "INV-01: no accuracy, no ring");
    }
}
