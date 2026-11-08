//! What a hull is drawn as, as a function of how big it is on screen (#32, #40).
//!
//! Far out a hull is a **symbol** — the standard's frame and icon, which is the larger and more honest
//! mark when a 120 m ship is twelve pixels long (`docs/research/nato-symbology.md`). Close in it is the
//! hull's **own picture**, sized from its length and beam and rotated to its course, because at that
//! scale the operator is looking at *this ship*, not at "a frigate".
//!
//! Three rules are not negotiable, and each is a trap this display exists to avoid:
//!
//! - **The threshold is read from the frame, never the camera.** Mid-gesture the two disagree, and a
//!   hull that switches representation while it is projected against the old frame is a hull that
//!   flickers (`INV-04`'s reason, applied to drawing).
//! - **No course is not north.** A fix without `course_deg` draws upright, and "upright" is `None`
//!   rather than `0.0` so nothing downstream can mistake it for a bearing (`INV-01`).
//! - **A picture that cannot be sized is drawn clamped, and says so.** `clamped` is what stops a
//!   photograph from implying a scale it was not given.
//!
//! This module is pure: no rune, no network, no clock of its own — so the switch can be swept across the
//! boundary in a test rather than nudged by a mouse.

use crate::map::geometry::Rendered;
use crate::units::UnitView;

/// The zoom a hull stops being a symbol and becomes its picture.
///
/// **Fourteen, and the convention matters**: MapLibre's zoom counts a 512 px world (`WORLD_BASE_PX`), so
/// z14 is the zoom classic 256-px slippy maps call z15. At z14 over Jakarta the ground is **4.75 m per
/// pixel**, so a 120 m hull is a 25 pt ship — the resolution at which a shape reads, and exactly the
/// number the sister project states for its own `PHOTO_ZOOM = 14`. Below it the symbol box is the larger
/// mark and the honest one.
pub const PHOTO_ZOOM: f64 = 14.0;
/// The zoom a picture drops back to a symbol. Below [`PHOTO_ZOOM`] so a zoom wobbling on the boundary
/// cannot strobe between two representations — the same 8 % of slack the sister project uses.
pub const PHOTO_EXIT: f64 = PHOTO_ZOOM * 0.97;
/// The smallest a picture is drawn, in points: the touch target. A hull shorter than this on screen is
/// drawn clamped, and the mark says so.
pub const MIN_PICTURE_PT: f32 = 24.0;

/// Whether the wall is in symbol range or picture range.
///
/// One latch for the whole wall, because the decision is about the *zoom*, not about a hull: two hulls at
/// the same zoom must not disagree about which representation they are in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Range {
    #[default]
    Symbol,
    Picture,
}

impl Range {
    /// Advance the latch with the frame's zoom.
    pub fn update(self, zoom: f64) -> Range {
        match self {
            Range::Symbol if zoom >= PHOTO_ZOOM => Range::Picture,
            Range::Picture if zoom < PHOTO_EXIT => Range::Symbol,
            other => other,
        }
    }
}

/// A hull's dimensions, from `GET /units/{id}/specifications`. Metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub length_m: f64,
    pub beam_m: f64,
}

/// How one hull is drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    /// The composed symbol. Size comes from the token, not from here.
    Symbol,
    /// The hull's own picture.
    Picture {
        /// The long edge, in points: `length_m` through the ground resolution at the hull's latitude.
        length_pt: f32,
        /// The short edge, in points.
        beam_pt: f32,
        /// Clockwise degrees from upright, or `None` when no course was measured — which draws upright
        /// and implies nothing.
        rotation: Option<f32>,
        /// `true` when the picture is drawn at [`MIN_PICTURE_PT`] rather than at its true scale, because
        /// the hull is smaller than that on screen or its length is not known.
        clamped: bool,
    },
}

/// The mark a hull earns: `None` when it has no fix (a hull with no position is not on the map, `INV-02`).
pub fn mark(range: Range, view: &UnitView, frame: &Rendered, size: Option<Size>) -> Option<Mark> {
    let position = view.position?;
    if range == Range::Symbol {
        return Some(Mark::Symbol);
    }
    let metres_per_point = crate::map::geometry::meters_per_pixel(position.lat, frame.camera.zoom);
    let (length_pt, beam_pt, clamped) = match size {
        Some(size) => {
            let length = (size.length_m / metres_per_point) as f32;
            let beam = (size.beam_m / metres_per_point) as f32;
            let clamped = length < MIN_PICTURE_PT;
            let scale = if clamped {
                MIN_PICTURE_PT / length
            } else {
                1.0
            };
            (length * scale, beam * scale, clamped)
        }
        // No dimensions: the picture is drawn at the touch target and **says** it is not at scale. A
        // photograph with no length would otherwise claim a size nobody measured.
        None => (MIN_PICTURE_PT, MIN_PICTURE_PT * DEFAULT_BEAM_RATIO, true),
    };
    Some(Mark::Picture {
        length_pt,
        beam_pt,
        // Upright is not north: no course draws upright with no rotation at all (`INV-01`).
        rotation: view.course_deg.map(|degrees| degrees as f32),
        clamped,
    })
}

/// The aspect a picture is drawn at when its beam is unknown: a hull is roughly a fifth as wide as it is
/// long. Only ever used with `clamped`, so it cannot pass for a measurement.
const DEFAULT_BEAM_RATIO: f32 = 0.2;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::geometry::Camera;
    use crate::units::{Pos, Unit, UnitView};

    /// Jakarta, where a 120 m hull at z14 (MapLibre's 512-px zoom) is a 25 pt ship: the number the
    /// threshold was chosen on.
    const JAKARTA: (f64, f64) = (-6.108, 106.910);

    fn frame_at(zoom: f64) -> Rendered {
        Rendered {
            camera: Camera::new(JAKARTA, zoom),
            width: 1280,
            height: 720,
        }
    }

    fn hull(lat: f64, lon: f64, course: Option<f64>) -> UnitView {
        UnitView {
            position: Some(Pos::new(lat, lon)),
            course_deg: course,
            ..UnitView::silent(Unit {
                id_unit: 3,
                name: "KRI Ardadedali".into(),
                hull_number: Some("404".into()),
            })
        }
    }

    fn sized(length_m: f64, beam_m: f64) -> Option<Size> {
        Some(Size { length_m, beam_m })
    }

    const DESTROYER: fn() -> Option<Size> = || sized(120.0, 16.0);

    #[test]
    fn the_threshold_latches_with_hysteresis_and_never_strobes() {
        // Entering needs z15; leaving needs z14.55, so a zoom wobbling on the boundary stays put.
        let mut range = Range::Symbol;
        for zoom in [10.0, 13.0, 13.9, 13.9999] {
            range = range.update(zoom);
            assert_eq!(range, Range::Symbol, "below the threshold at {zoom}");
        }
        range = range.update(PHOTO_ZOOM);
        assert_eq!(range, Range::Picture, "z15 enters picture range");

        // A wobble either side of the threshold must not change the answer in either direction.
        for zoom in [13.99, PHOTO_ZOOM, 13.98, 14.01, PHOTO_EXIT + 0.01] {
            let next = range.update(zoom);
            assert_eq!(next, range, "no strobe at {zoom}");
        }
        // Only below the exit threshold does it fall back.
        assert_eq!(range.update(PHOTO_EXIT - 0.001), Range::Symbol);
        assert_eq!(range.update(13.0), Range::Symbol);
        // And it can come back.
        assert_eq!(Range::Symbol.update(15.0), Range::Picture);
    }

    #[test]
    fn a_hull_is_a_symbol_until_the_range_says_otherwise() {
        let view = hull(JAKARTA.0, JAKARTA.1, Some(90.0));
        assert_eq!(
            mark(Range::Symbol, &view, &frame_at(15.5), DESTROYER()),
            Some(Mark::Symbol),
            "the range decides, not the zoom on its own"
        );
        assert!(matches!(
            mark(Range::Picture, &view, &frame_at(15.5), DESTROYER()),
            Some(Mark::Picture { .. })
        ));
    }

    /// The acceptance criterion from #32: a 120 m hull at the threshold over Jakarta is 25 ± 1 pt long.
    #[test]
    fn a_hundred_and_twenty_metres_at_the_threshold_is_a_twenty_five_point_ship() {
        let view = hull(JAKARTA.0, JAKARTA.1, Some(0.0));
        let Some(Mark::Picture {
            length_pt,
            beam_pt,
            clamped,
            ..
        }) = mark(Range::Picture, &view, &frame_at(PHOTO_ZOOM), DESTROYER())
        else {
            panic!("a picture at the threshold")
        };
        assert!(
            (24.0..=26.0).contains(&length_pt),
            "120 m at z{PHOTO_ZOOM} is 25 ± 1 pt, got {length_pt}"
        );
        assert!(!clamped, "a 25 pt ship is drawn at its own scale");
        assert!(
            beam_pt > 0.0 && beam_pt < length_pt,
            "beam is short: {beam_pt}"
        );
    }

    #[test]
    fn a_short_hull_is_drawn_clamped_and_says_so() {
        // 12 m at z15 is 2.5 pt: unreadable, and below the touch target.
        let view = hull(JAKARTA.0, JAKARTA.1, None);
        let Some(Mark::Picture {
            length_pt, clamped, ..
        }) = mark(
            Range::Picture,
            &view,
            &frame_at(PHOTO_ZOOM),
            sized(12.0, 4.0),
        )
        else {
            panic!("a picture")
        };
        assert!(
            (length_pt - MIN_PICTURE_PT).abs() < 1e-3,
            "drawn at the target: {length_pt}"
        );
        assert!(
            clamped,
            "the clamp is recorded, because the picture is no longer at scale"
        );
    }

    #[test]
    fn a_hull_with_no_dimensions_is_drawn_at_the_target_and_flagged() {
        let view = hull(JAKARTA.0, JAKARTA.1, None);
        let Some(Mark::Picture {
            length_pt, clamped, ..
        }) = mark(Range::Picture, &view, &frame_at(16.0), None)
        else {
            panic!("a picture")
        };
        assert_eq!(length_pt, MIN_PICTURE_PT);
        assert!(clamped, "no length means no scale, and the mark says so");
    }

    #[test]
    fn no_course_draws_upright_and_is_not_north() {
        let no_course = hull(JAKARTA.0, JAKARTA.1, None);
        let Some(Mark::Picture { rotation, .. }) =
            mark(Range::Picture, &no_course, &frame_at(15.5), DESTROYER())
        else {
            panic!("a picture")
        };
        assert_eq!(
            rotation, None,
            "upright, and nothing that could be read as a bearing"
        );

        let north = hull(JAKARTA.0, JAKARTA.1, Some(0.0));
        let Some(Mark::Picture { rotation, .. }) =
            mark(Range::Picture, &north, &frame_at(15.5), DESTROYER())
        else {
            panic!("a picture")
        };
        assert_eq!(
            rotation,
            Some(0.0),
            "a measured north IS a bearing, and says so"
        );
    }

    #[test]
    fn a_hull_with_no_course_still_rotates_when_one_arrives() {
        let view = hull(JAKARTA.0, JAKARTA.1, Some(270.0));
        let Some(Mark::Picture { rotation, .. }) =
            mark(Range::Picture, &view, &frame_at(15.5), DESTROYER())
        else {
            panic!("a picture")
        };
        assert_eq!(rotation, Some(270.0));
    }

    #[test]
    fn a_hull_with_no_fix_is_not_drawn_at_all() {
        // `INV-02`: absence is not zero, and a hull with no position has no mark.
        let silent = UnitView::silent(Unit {
            id_unit: 7,
            name: "KRI Cakra".into(),
            hull_number: None,
        });
        for range in [Range::Symbol, Range::Picture] {
            assert_eq!(mark(range, &silent, &frame_at(16.0), DESTROYER()), None);
        }
    }
}
