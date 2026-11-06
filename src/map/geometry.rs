//! The map's geometry, with **no MapLibre in it**.
//!
//! The Web Mercator projection, the ground resolution it implies, and the camera arithmetic for a
//! drag and a wheel zoom. It lives apart from [`crate::map::render`] for one reason: it must be
//! testable — and true — with the map engine switched off, so `scripts/verify.sh` can prove the
//! projection without a GL context, a tile, or a network.
//!
//! **Both the picture and the markers go through this.** The picture is MapLibre's render of a
//! camera; every marker is a lat/lon projected with that same camera, through the same function,
//! against the frame's own size. `INV-04` is a property of that construction — one transform, called
//! from two places — not of an eye comparing a screenshot.
//!
//! The world is `WORLD_BASE_PX * 2^zoom` pixels across. It is 512 and not 256 because MapLibre's
//! world is 512-based: a 256 base puts every marker at half the offset the map draws, which looks
//! like a small bug at zoom 3 and an obvious one at zoom 11.

/// World width in pixels at zoom 0. Calibrated against the renderer, not assumed: a 256 base halves
/// every marker offset and the error grows with every zoom step.
pub const WORLD_BASE_PX: f64 = 512.0;

/// Earth's equatorial circumference, in metres: the numerator of the ground resolution.
pub const EARTH_EQUATOR_M: f64 = 40_075_016.686;

/// Mercator is defined to ±85.05112878°; past it the projection diverges, so every renderer clamps
/// and so does this one rather than producing infinities at a pole.
pub const MAX_MERCATOR_LAT: f64 = 85.051_128_78;

/// Where the wall opens: Jakarta, at a zoom that shows the bay and the roads. The offline seed covers
/// this ground; a hull outside it is off-screen until the camera moves, which is the honest behaviour
/// of a map with a fixed seed.
pub const START_CENTER: (f64, f64) = (-6.108, 106.910);
pub const START_ZOOM: f64 = 11.0;

/// How much one pixel of wheel travel moves the camera, in zoom levels. A wheel is a gesture, not a
/// button, so the zoom is proportional to it rather than one notch per event.
pub const WHEEL_ZOOM_PER_PX: f64 = 0.005;

/// The smallest and largest zoom worth rendering. Below the first the seed has one tile per province;
/// above the second it has no vector detail left to show.
pub const MIN_ZOOM: f64 = 2.0;
pub const MAX_ZOOM: f64 = 18.0;

/// A camera: where it looks, and how far in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Centre, as `(latitude, longitude)` in degrees.
    pub center: (f64, f64),
    /// Zoom level.
    pub zoom: f64,
}

impl Camera {
    pub const START: Camera = Camera {
        center: START_CENTER,
        zoom: START_ZOOM,
    };

    pub fn new(center: (f64, f64), zoom: f64) -> Self {
        Self {
            center: (clamp_lat(center.0), wrap_lon(center.1)),
            zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
        }
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::START
    }
}

/// The map's drawing area, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub w: f64,
    pub h: f64,
}

impl Viewport {
    pub fn new(w: f64, h: f64) -> Self {
        Self { w, h }
    }

    /// A viewport with area. Anything else is a window that has not been laid out yet, and projecting
    /// into it would put every marker at NaN.
    pub fn is_usable(&self) -> bool {
        self.w > 0.0 && self.h > 0.0
    }
}

fn clamp_lat(lat: f64) -> f64 {
    lat.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT)
}

fn wrap_lon(lon: f64) -> f64 {
    // Keep longitude in [-180, 180): a pan across the antimeridian must not wind up at 400°.
    let wrapped = (lon + 180.0).rem_euclid(360.0) - 180.0;
    if wrapped == -180.0 {
        180.0 - 360.0
    } else {
        wrapped
    }
}

/// A point in world pixels at a zoom: the projection before the camera is applied.
fn world_px(lat: f64, lon: f64, zoom: f64) -> (f64, f64) {
    let scale = WORLD_BASE_PX * 2f64.powf(zoom);
    let lat = clamp_lat(lat).to_radians();
    let x = (wrap_lon(lon) + 180.0) / 360.0 * scale;
    // asinh(tan φ) — the Mercator ordinate, written so it is obvious where it comes from.
    let s = ((lat.tan() + 1.0 / lat.cos()).ln()).min(std::f64::consts::PI);
    let y = (1.0 - s / std::f64::consts::PI) / 2.0 * scale;
    (x, y)
}

/// Project a lat/lon to window pixels for this camera and viewport.
///
/// This is the **only** lat/lon → pixel conversion in the client. A second one is how markers come to
/// disagree with the tiles (`INV-04`).
pub fn project(camera: Camera, viewport: Viewport, lat: f64, lon: f64) -> (f64, f64) {
    let (x, y) = world_px(lat, lon, camera.zoom);
    let (cx, cy) = world_px(camera.center.0, camera.center.1, camera.zoom);
    (x - cx + viewport.w / 2.0, y - cy + viewport.h / 2.0)
}

/// Inverse of [`project`]: window pixels back to lat/lon. Used by the pointer, not the markers.
pub fn unproject(camera: Camera, viewport: Viewport, px: f64, py: f64) -> (f64, f64) {
    let scale = WORLD_BASE_PX * 2f64.powf(camera.zoom);
    let (cx, cy) = world_px(camera.center.0, camera.center.1, camera.zoom);
    let x = px - viewport.w / 2.0 + cx;
    let y = py - viewport.h / 2.0 + cy;
    let lon = wrap_lon(x / scale * 360.0 - 180.0);
    let s = (1.0 - 2.0 * y / scale) * std::f64::consts::PI;
    (clamp_lat(s.sinh().atan().to_degrees()), lon)
}

/// The camera after dragging the picture by `(dx, dy)` points.
///
/// A drag points at the ground under the cursor and asks for that ground to stay there, so the
/// centre becomes that pixel. The sign lives here and not at the call site: dragging the picture
/// right moves the centre **left**, and writing that arithmetic twice is how the two directions come
/// to disagree.
pub fn pan(camera: Camera, viewport: Viewport, dx: f64, dy: f64) -> Camera {
    let center = unproject(
        camera,
        viewport,
        viewport.w / 2.0 - dx,
        viewport.h / 2.0 - dy,
    );
    Camera {
        center,
        zoom: camera.zoom,
    }
}

/// The camera after a zoom about a pointer, keeping the ground under that pointer fixed.
///
/// Zooming about the window's centre instead is the classic mistake: the operator's target slides out
/// from under the cursor every time the wheel moves.
pub fn zoom_about(
    camera: Camera,
    viewport: Viewport,
    pointer: (f64, f64),
    delta_zoom: f64,
) -> Camera {
    let new_zoom = (camera.zoom + delta_zoom).clamp(MIN_ZOOM, MAX_ZOOM);
    if new_zoom == camera.zoom {
        return camera;
    }
    let (lat, lon) = unproject(camera, viewport, pointer.0, pointer.1);
    let scale = WORLD_BASE_PX * 2f64.powf(new_zoom);
    let (gx, gy) = world_px(lat, lon, new_zoom);
    // Put that ground back under the pointer: the centre is the world point the pointer asks for.
    let cx = gx - (pointer.0 - viewport.w / 2.0);
    let cy = gy - (pointer.1 - viewport.h / 2.0);
    let lon2 = wrap_lon(cx / scale * 360.0 - 180.0);
    let s = (1.0 - 2.0 * cy / scale) * std::f64::consts::PI;
    Camera {
        center: (clamp_lat(s.sinh().atan().to_degrees()), lon2),
        zoom: new_zoom,
    }
}

/// Ground resolution at a latitude and zoom, in metres per pixel.
///
/// The only metres → pixels conversion in the client. Nothing else may scale a hull: an image's
/// aspect ratio is a property of a photograph, not a measurement of a ship.
pub fn meters_per_pixel(latitude: f64, zoom: f64) -> f64 {
    let lat = clamp_lat(latitude).to_radians();
    EARTH_EQUATOR_M * lat.cos() / (WORLD_BASE_PX * 2f64.powf(zoom))
}

/// A frame as the map thread rendered it: its size and the camera it was drawn with.
///
/// Markers project against **this**, not against the live camera, because the picture on screen is
/// this frame. When the camera has moved on the markers move with it and the picture lags — which is
/// a picture one gesture behind, not two transforms of the same water.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rendered {
    pub camera: Camera,
    pub width: u32,
    pub height: u32,
}

impl Rendered {
    pub fn viewport(&self) -> Viewport {
        Viewport::new(f64::from(self.width), f64::from(self.height))
    }

    /// A marker's pixel in this frame (`INV-04`).
    pub fn project(&self, lat: f64, lon: f64) -> (f64, f64) {
        project(self.camera, self.viewport(), lat, lon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic generator: a property test needs many cameras, and a seeded LCG is enough for
    /// "many", keeps the test reproducible, and adds no dependency.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
        }

        fn between(&mut self, lo: f64, hi: f64) -> f64 {
            lo + self.next() * (hi - lo)
        }
    }

    fn viewport() -> Viewport {
        Viewport::new(1280.0, 720.0)
    }

    #[test]
    fn the_centre_projects_to_the_middle_of_the_frame() {
        // The first thing a camera must do. If this drifts, every marker is offset by half a window.
        let camera = Camera::new((-6.108, 106.910), 11.0);
        let vp = viewport();
        let (x, y) = project(camera, vp, camera.center.0, camera.center.1);
        assert!(
            (x - vp.w / 2.0).abs() < 1e-9 && (y - vp.h / 2.0).abs() < 1e-9,
            "centre landed at ({x}, {y})"
        );
    }

    #[test]
    fn mercator_agrees_with_its_own_definition_at_known_points() {
        // Checked against the projection's DEFINITION, not against this module's arithmetic: the world
        // is `WORLD_BASE_PX * 2^zoom` across, longitude is linear in it, the antimeridian is its left
        // edge, the null island its middle, and the Mercator cut-off its top edge.
        let zoom = 2.0;
        let world = WORLD_BASE_PX * 2f64.powf(zoom);
        let vp = Viewport::new(512.0, 512.0);
        let camera = Camera::new((0.0, 0.0), zoom);

        let at = |world_x: f64| world_x - world / 2.0 + vp.w / 2.0;

        let (x, y) = project(camera, vp, 0.0, 0.0);
        assert!(
            (x - at(world / 2.0)).abs() < 0.5,
            "the null island is the world's middle, got x={x}"
        );
        assert!(
            (y - at(world / 2.0)).abs() < 0.5,
            "the equator is halfway down, got y={y}"
        );

        let (left, _) = project(camera, vp, 0.0, -180.0);
        assert!(
            (left - at(0.0)).abs() < 0.5,
            "the antimeridian is the world's left edge, got x={left}"
        );

        let (_, top) = project(camera, vp, MAX_MERCATOR_LAT, 0.0);
        assert!(
            (top - at(0.0)).abs() < 1.0,
            "the Mercator cut-off is the world's top edge, got y={top}"
        );

        let (east, _) = project(camera, vp, 0.0, 90.0);
        assert!(
            (east - at(world * 0.75)).abs() < 0.5,
            "90° east is three quarters across, got x={east}"
        );
    }

    #[test]
    fn a_marker_and_the_picture_are_one_transform() {
        // INV-04: a marker's pixel agrees with the map's projection of the same lat/lon, under the
        // camera the frame was rendered with, within a pixel. The construction is what guarantees it
        // — Rendered::project delegates to the same function the picture was drawn with — and this
        // test fails the moment a second transform appears.
        let mut rng = Lcg(0x5EED_1234_ABCD_4321);
        for _ in 0..2000 {
            let camera = Camera::new(
                (rng.between(-70.0, 70.0), rng.between(-179.0, 179.0)),
                rng.between(MIN_ZOOM, MAX_ZOOM),
            );
            let frame = Rendered {
                camera,
                width: 1280,
                height: 720,
            };
            let lat = rng.between(-70.0, 70.0);
            let lon = rng.between(-179.0, 179.0);

            let marker = frame.project(lat, lon);
            let picture = project(camera, frame.viewport(), lat, lon);
            assert_eq!(
                marker, picture,
                "INV-04: the marker layer re-derived the projection instead of using the frame's camera"
            );

            // And the round trip: a pixel reads back as the ground it was projected from.
            let (back_lat, back_lon) = unproject(camera, frame.viewport(), marker.0, marker.1);
            assert!(
                (back_lat - lat).abs() < 1e-6,
                "INV-04: latitude round trip {lat} → {back_lat}"
            );
            assert!(
                (back_lon - lon).abs() < 1e-6,
                "INV-04: longitude round trip {lon} → {back_lon}"
            );
        }
    }

    #[test]
    fn a_drag_moves_the_picture_with_the_hand() {
        let camera = Camera::new((-6.108, 106.910), 11.0);
        let vp = viewport();
        let ground = unproject(camera, vp, 0.0, 0.0);
        let dragged = pan(camera, vp, 40.0, -10.0);
        // The ground that was at the top-left is now 40 right and 10 up of where it was.
        let (x, y) = project(dragged, vp, ground.0, ground.1);
        assert!(
            (x - 40.0).abs() < 0.01 && (y - (-10.0)).abs() < 0.01,
            "the picture slid to ({x}, {y}), wanted (40, -10)"
        );
    }

    #[test]
    fn a_wheel_zoom_keeps_the_ground_under_the_pointer() {
        let camera = Camera::new((-6.108, 106.910), 11.0);
        let vp = viewport();
        let pointer = (200.0, 420.0);
        let ground = unproject(camera, vp, pointer.0, pointer.1);
        let zoomed = zoom_about(camera, vp, pointer, 1.3);
        let (x, y) = project(zoomed, vp, ground.0, ground.1);
        assert!(
            (x - pointer.0).abs() < 0.5 && (y - pointer.1).abs() < 0.5,
            "the ground slid to ({x}, {y}) from the pointer {pointer:?}"
        );
        assert!((zoomed.zoom - 12.3).abs() < 1e-9);
    }

    #[test]
    fn zoom_stops_at_the_limits_instead_of_wrapping() {
        let camera = Camera::new((0.0, 0.0), MAX_ZOOM);
        assert_eq!(
            zoom_about(camera, viewport(), (10.0, 10.0), 3.0).zoom,
            MAX_ZOOM
        );
        let camera = Camera::new((0.0, 0.0), MIN_ZOOM);
        assert_eq!(
            zoom_about(camera, viewport(), (10.0, 10.0), -3.0).zoom,
            MIN_ZOOM
        );
    }

    #[test]
    fn a_longitude_past_the_antimeridian_wraps_instead_of_running_away() {
        let wrapped = Camera::new((0.0, 200.0), 5.0);
        assert!(
            (wrapped.center.1 - -160.0).abs() < 1e-9,
            "200° east is 160° west, got {}",
            wrapped.center.1
        );
        let polar = Camera::new((89.0, 0.0), 5.0);
        assert!(polar.center.0 <= MAX_MERCATOR_LAT + 1e-9);
    }

    #[test]
    fn ground_resolution_halves_per_zoom_level() {
        let a = meters_per_pixel(0.0, 10.0);
        let b = meters_per_pixel(0.0, 11.0);
        assert!(
            (a / b - 2.0).abs() < 1e-9,
            "one zoom level is half the ground per pixel: {a} vs {b}"
        );
        // And it is the same world the projection uses.
        let expected = EARTH_EQUATOR_M / (WORLD_BASE_PX * 2f64.powf(11.0));
        assert!((b - expected).abs() < 1e-9);
    }
}
