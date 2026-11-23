//! **The chart** — where the picture is on the water, and where a press lands.
//!
//! The projection is the wall's ([`arcons_cc::map::geometry`]): one implementation of Web Mercator for
//! both applications, so a hull the desk drops and a hull the wall draws at the same camera are the same
//! point on the sea. What is the desk's own is the **camera**: the UI owns it (ADR-0009, decision 4), so a
//! press resolves with `unproject` at the moment of the press instead of waiting for a render thread to
//! tell it where it was pointing. A marker that trails the pointer is a marker that lies about where the
//! operator clicked.
//!
//! This module is the arithmetic, the camera and the placement — the part a test can hold. The engine's
//! frame is [`engine`] behind `--features map`; without it the chart draws the graticule and the hulls and
//! no water, which is a chart that still means what it says.

use arcons_cc::map::geometry::{
    Camera, MAX_ZOOM, MIN_ZOOM, Viewport, pan, project, unproject, zoom_about,
};

#[cfg(feature = "map")]
pub mod engine;

/// The zoom a click on `+` or `−` moves by. A whole level is the map convention, and half of one is the
/// difference between "the operator adjusted the view" and "the view jumped somewhere else".
pub const ZOOM_STEP: f64 = 1.0;

/// The camera and the rectangle it is looking through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chart {
    camera: Camera,
    viewport: Viewport,
}

impl Chart {
    /// A chart looking at `center`, at `zoom`, through a rectangle of `size` points.
    pub fn new(center: (f64, f64), zoom: f64, size: (f64, f64)) -> Self {
        Self {
            camera: Camera {
                center,
                zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
            },
            viewport: Viewport::new(size.0.max(1.0), size.1.max(1.0)),
        }
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// The rectangle changed: the layout's own event. The camera does not move, so the picture grows or
    /// shrinks around what it was looking at rather than jumping to a new centre.
    pub fn resize(&mut self, w: f64, h: f64) {
        self.viewport = Viewport::new(w.max(1.0), h.max(1.0));
    }

    /// Where a hull is on the chart, in points from its top-left corner.
    pub fn project(&self, lat: f64, lon: f64) -> (f64, f64) {
        project(self.camera, self.viewport, lat, lon)
    }

    /// What a point on the chart means: the coordinate a press there claims.
    pub fn place_at(&self, x: f64, y: f64) -> (f64, f64) {
        unproject(self.camera, self.viewport, x, y)
    }

    /// Whether a projected point is on the chart at all. A marker drawn outside the rectangle would be a
    /// ship in the panel next door.
    pub fn holds(&self, point: (f64, f64)) -> bool {
        point.0 >= 0.0 && point.1 >= 0.0 && point.0 <= self.viewport.w && point.1 <= self.viewport.h
    }

    /// Put the camera where a gesture has taken it. The drag handler owns the *preview* (`pan` from the
    /// camera the gesture started on), and this is how one settled value replaces another.
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = Camera {
            center: camera.center,
            zoom: camera.zoom.clamp(MIN_ZOOM, MAX_ZOOM),
        };
    }

    /// The hand moved the chart: the camera moves the other way so the water follows the pointer.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.camera = pan(self.camera, self.viewport, dx, dy);
    }

    /// A wheel or a key: zoom about a point, keeping the water under it where it was.
    pub fn zoom_at(&mut self, point: (f64, f64), levels: f64) {
        self.camera = zoom_about(self.camera, self.viewport, point, levels);
    }
}
