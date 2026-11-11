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
//! - **No course is not north, and a picture still has a bow.** A fix without `course_deg` draws its
//!   hull at the picture's own orientation — bow north, because every stored picture is bow-east and
//!   the drawn angle is `course − IMAGE_BOW_DEGREES` (#63) — which is how a ship icon is drawn, not a
//!   claim that the hull is steaming north. The card still shows no course (`INV-01`).
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
/// The lowest a picture is drawn, in points: the touch target. A hull shorter than this on screen is
/// drawn clamped, and the mark says so.
pub const MIN_PICTURE_PT: f32 = 24.0;

/// The bow points **east** in every picture the backend stores, and a picture that is not turned says
/// so every time it is drawn.
///
/// Measured, not assumed: `GET /units/8` is a presigned PNG **2167 × 280** with the flight deck and the
/// bow ramp at the right edge, and `/units/5` matches. So the drawn angle is `course −` this constant:
/// course 90° (east) is the image as stored, course 0° (north) is turned a quarter to port, and a hull
/// with **no measured course** lands at −90°, bow north — the orientation a ship icon is drawn in. A
/// photograph of a ship is the one mark whose rotation *is* a measurement, so being 90° off is the same
/// class of error as being 90° off the hull's position (`#63`).
///
/// One constant, because the convention belongs to the picture and not to the course. If the backend
/// ever re-orients its photographs, this is the line that changes, and the test below says so.
pub const IMAGE_BOW_DEGREES: f32 = 90.0;

/// Whether a pane is in symbol range or picture range.
///
/// **One latch per pane** (#115), because the decision is about the *zoom* and each pane holds its own:
/// zooming one pane into a hull's picture must not switch its neighbour's representation. Two hulls at
/// the same zoom in the same pane still cannot disagree about which representation they are in.
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
        /// Clockwise degrees to turn the picture: `course − IMAGE_BOW_DEGREES`, so a hull steaming
        /// north draws bow up and one steaming east draws the image as stored. **Always a value**, even
        /// with no course measured: the picture has an orientation the way it has a hull, and −90° is
        /// that orientation rather than a bearing. It is not a course, so it is not an `Option` — a
        /// `None` here would become someone's `unwrap_or(0.0)`, which is bow-east (#63).
        rotation: f32,
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
        // The picture's own orientation plus the course when one was measured. No course is not north
        // (`INV-01`): 0° here is "no course", and subtracting the picture's stored bow leaves −90°, which
        // draws the hull the way the icon is drawn. The card still shows `course —`: nothing was
        // measured, and a rotation is not a bearing.
        rotation: view.course_deg.unwrap_or(0.0) as f32 - IMAGE_BOW_DEGREES,
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

    /// #63: the backend stores every picture **bow-east**, so the drawn angle is `course − 90`.
    ///
    /// The expected angles are **written out**, not computed from [`IMAGE_BOW_DEGREES`]: a sweep that
    /// reads the constant it is checking passes when the constant is wrong (measured — setting it to
    /// `0.0` left this test green), and the convention is the claim, so the test states it. A hull
    /// steaming north is the case the bug was reported on: turn the image a quarter to port, never draw
    /// it as stored.
    #[test]
    fn a_picture_is_turned_from_its_stored_bow_east() {
        for (course, drawn) in [
            // No course: the icon's own orientation, bow north — not a bearing, and not broadside.
            (None, -90.0),
            // Steaming north: bow up, which is the whole of #63.
            (Some(0.0), -90.0),
            // Steaming east: exactly the image as stored, which is why course 0 used to look wrong.
            (Some(90.0), 0.0),
            (Some(180.0), 90.0),
            (Some(270.0), 180.0),
            (Some(45.0), -45.0),
        ] {
            let view = hull(JAKARTA.0, JAKARTA.1, course);
            let Some(Mark::Picture { rotation, .. }) =
                mark(Range::Picture, &view, &frame_at(15.5), DESTROYER())
            else {
                panic!("a picture for course {course:?}")
            };
            assert!(
                (rotation - drawn).abs() < 1e-4,
                "#63: course {course:?} must draw {drawn}°, got {rotation}° — the stored bow is east, \
                 so the drawn angle is course − 90 (IMAGE_BOW_DEGREES = {IMAGE_BOW_DEGREES})"
            );
        }
    }

    /// The constant is the convention, and the convention is one number: if the backend re-orients its
    /// photographs, this test and the sweep above are the two lines that have to be changed on purpose.
    #[test]
    fn the_stored_bow_is_east_which_is_ninety_degrees() {
        assert_eq!(
            IMAGE_BOW_DEGREES, 90.0,
            "#63: every stored picture is bow-east; a different constant means a different convention"
        );
    }

    /// The picture has an orientation even when the hull's course is absent — and that must not become a
    /// course anywhere else. `INV-01` is about the measurement, and it is still absent.
    #[test]
    fn no_course_draws_the_icon_without_making_one_up() {
        let view = hull(JAKARTA.0, JAKARTA.1, None);
        assert_eq!(view.course_deg, None, "INV-01: no course was measured");
        let Some(Mark::Picture { rotation, .. }) =
            mark(Range::Picture, &view, &frame_at(15.5), DESTROYER())
        else {
            panic!("a picture")
        };
        assert_eq!(
            rotation, -90.0,
            "INV-01: the picture is drawn bow north as an icon is, and the mark claims no course"
        );
    }

    #[test]
    fn a_hull_still_rotates_when_a_course_arrives() {
        let view = hull(JAKARTA.0, JAKARTA.1, Some(270.0));
        let Some(Mark::Picture { rotation, .. }) =
            mark(Range::Picture, &view, &frame_at(15.5), DESTROYER())
        else {
            panic!("a picture")
        };
        assert_eq!(
            rotation,
            270.0 - IMAGE_BOW_DEGREES,
            "course 270 is west: bow left"
        );
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
