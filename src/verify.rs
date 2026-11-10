//! Pixel assertions for a capture: numbers with an exit code, not a human squinting at a PNG.
//!
//! A golden compares bytes (`INV-11`: two runs, or a run and the golden, are identical or the build
//! is red). A golden alone, though, says only *"this changed"*. This module says *what* is wrong:
//! "no marker within 3 px of (498, 300)", "the stale marker is brighter than the live one", "the card
//! starts at 294, not 300".
//!
//! **Colours come from [`tokens`]**, never from literals: an assertion written against `#3fa8d8`
//! would outlive the palette it describes and quietly stop testing anything. Geometry comes from
//! `map::geometry::project`, the same function the markers are placed with, so a marker that moves is
//! a marker that fails.
//!
//! The assertions are deliberately about geometry, presence and the tokens' colours. Antialiasing
//! differs between rasterisers; "this pixel is exactly this shade" is a test of the rasteriser.

use std::path::Path;

use crate::map::geometry::{Camera, Rendered};
use crate::scene;
use crate::ui::tokens;

/// How far a marker's ink may be from where the raw projection says it is.
///
/// **Eight, and measured**: with the panel's hairline border and Rune's f32 layout, a marker that is
/// exactly where the camera says draws about four pixels from the unadjusted projection. Eight leaves
/// that room and is still smaller than a hull's grid spacing (≈40 px) and far smaller than a wrong
/// camera. The golden pins the exact pixels; this number is about *geography* — which is why a marker
/// moved ten is a failure and a marker moved one shade is the golden's job.
pub const TOLERANCE_PX: f64 = 8.0;

/// A decoded capture: RGBA8, top-left origin.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pixels: Vec<u8>,
}

impl Image {
    /// Read a PNG. Only what Rune's headless renderer writes: 8-bit RGB or RGBA, no interlacing.
    pub fn read(path: &Path) -> Result<Image, String> {
        let file = std::fs::File::open(path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let decoder = png::Decoder::new(std::io::BufReader::new(file));
        let mut reader = decoder
            .read_info()
            .map_err(|error| format!("{} is not a PNG: {error}", path.display()))?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| format!("{} is too large to decode", path.display()))?;
        let mut buffer = vec![0; size];
        let info = reader
            .next_frame(&mut buffer)
            .map_err(|error| format!("{} has no frame: {error}", path.display()))?;
        let pixels = match info.color_type {
            png::ColorType::Rgba => buffer,
            png::ColorType::Rgb => buffer
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
                .collect(),
            other => {
                return Err(format!(
                    "{}: unsupported colour type {other:?} (RGBA or RGB, 8-bit)",
                    path.display()
                ));
            }
        };
        Ok(Image {
            width: info.width,
            height: info.height,
            pixels,
        })
    }

    /// A synthetic capture, for testing the comparison itself. Not reachable from the binary: a real
    /// capture is always read from a file, and a constructor that could be called at run time would
    /// invite a comparison of something that never rendered.
    #[cfg(test)]
    pub fn synthetic(width: u32, height: u32, fill: [u8; 4]) -> Image {
        Image {
            width,
            height,
            pixels: fill
                .iter()
                .copied()
                .cycle()
                .take((width as usize) * (height as usize) * 4)
                .collect(),
        }
    }

    #[cfg(test)]
    pub fn set(&mut self, x: u32, y: u32, pixel: [u8; 4]) {
        let index = ((y as usize * self.width as usize) + x as usize) * 4;
        self.pixels[index..index + 4].copy_from_slice(&pixel);
    }

    /// Luminance of one pixel, `0..255`. A border is judged by how bright it is, not by its exact hue:
    /// a 1 pt line over a dark panel is antialiased wherever it meets the panel's own corner.
    pub fn luma(&self, x: u32, y: u32) -> u8 {
        let pixel = self.at(x, y);
        ((u32::from(pixel[0]) * 299 + u32::from(pixel[1]) * 587 + u32::from(pixel[2]) * 114) / 1000)
            as u8
    }

    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let index = ((y as usize * self.width as usize) + x as usize) * 4;
        let pixel = &self.pixels[index..index + 4];
        [pixel[0], pixel[1], pixel[2], pixel[3]]
    }

    /// The centre of the ink blob around `(x, y)`, and how many pixels it has.
    ///
    /// A centroid, not "some ink within N pixels": a marker glyph is twelve pixels across, so a
    /// check that looks for *any* matching ink near the expected point passes when the marker moves
    /// by five. This one moves with the blob, so ten pixels is ten.
    pub fn blob_centre(
        &self,
        x: f64,
        y: f64,
        colour: [u8; 3],
        radius: i32,
    ) -> Option<((f64, f64), usize)> {
        let mut sum = (0.0f64, 0.0f64);
        let mut count = 0usize;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let px = x.round() as i32 + dx;
                let py = y.round() as i32 + dy;
                if px < 0 || py < 0 || px as u32 >= self.width || py as u32 >= self.height {
                    continue;
                }
                if near(self.at(px as u32, py as u32), colour, 24) {
                    sum.0 += f64::from(px);
                    sum.1 += f64::from(py);
                    count += 1;
                }
            }
        }
        (count > 0).then(|| ((sum.0 / count as f64, sum.1 / count as f64), count))
    }

    /// The nearest pixel to `colour` within `tolerance` of `(x, y)`, as a distance in pixels.
    ///
    /// Exact equality is not the test: a filled circle at a sub-pixel position is antialiased at its
    /// edge, and the centre pixel is the colour only when the circle lands on the pixel grid.
    pub fn nearest_colour(&self, x: f64, y: f64, colour: [u8; 3], tolerance: f64) -> Option<f64> {
        let radius = 12i32;
        let mut best: Option<f64> = None;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let px = x.round() as i32 + dx;
                let py = y.round() as i32 + dy;
                if px < 0 || py < 0 || px as u32 >= self.width || py as u32 >= self.height {
                    continue;
                }
                let pixel = self.at(px as u32, py as u32);
                if near(pixel, colour, 24) {
                    let distance = ((px as f64 - x).powi(2) + (py as f64 - y).powi(2)).sqrt();
                    if distance <= tolerance && best.is_none_or(|found| distance < found) {
                        best = Some(distance);
                    }
                }
            }
        }
        best
    }

    /// The top-left corner of the picture panel, found by looking for the panel's fill colour.
    ///
    /// The markers are positioned **inside the panel**, so a projection lands in panel coordinates
    /// and a capture reports window ones. Reading the corner from the image rather than from a layout
    /// constant keeps the two from drifting apart: the panel's fill is a token, and it is either on
    /// screen or the capture is wrong.
    pub fn find_panel(&self, fill: [u8; 3]) -> Option<(f64, f64)> {
        let row_has = |y: u32| {
            (0..self.width)
                .filter(|x| near(self.at(*x, y), fill, 6))
                .count()
                > (self.width as usize / 2)
        };
        let y = (0..self.height).find(|y| row_has(*y))?;
        let x = (0..self.width).find(|x| near(self.at(*x, y), fill, 6))?;
        Some((f64::from(x), f64::from(y)))
    }

    /// The panel's whole rectangle, in the capture's pixels: the bounding box of its fill.
    ///
    /// `find_panel` answers "where does the picture start"; the panes need the size as well, because
    /// the rectangles they tile are computed from it — by `Layout::rects`, the same function the view
    /// lays the panes out with.
    pub fn find_panel_rect(&self, fill: [u8; 3]) -> Option<(f64, f64, f64, f64)> {
        // A row or a column belongs to the panel when **most of it** is the panel's fill. A bounding
        // box over every matching pixel would take its top from a stray antialiased glyph in the
        // header, which is exactly how a panel rect comes out 20 px too tall.
        let mut rows = Vec::new();
        for y in 0..self.height {
            let count = (0..self.width)
                .filter(|x| near(self.at(*x, y), fill, 6))
                .count();
            if count * 2 > self.width as usize {
                rows.push(y);
            }
        }
        let (top, bottom) = (*rows.first()?, *rows.last()?);
        let mut cols = Vec::new();
        for x in 0..self.width {
            let count = (top..=bottom)
                .filter(|y| near(self.at(x, *y), fill, 6))
                .count();
            if count * 2 > (bottom - top + 1) as usize {
                cols.push(x);
            }
        }
        let (left, right) = (*cols.first()?, *cols.last()?);
        Some((
            f64::from(left),
            f64::from(top),
            f64::from(right - left + 1),
            f64::from(bottom - top + 1),
        ))
    }

    /// The brightest pixel in a small window, as `0..255` luminance — how "dim" a marker is.
    pub fn brightest_luma(&self, x: f64, y: f64, radius: i32) -> u8 {
        let mut best = 0;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let px = x.round() as i32 + dx;
                let py = y.round() as i32 + dy;
                if px < 0 || py < 0 || px as u32 >= self.width || py as u32 >= self.height {
                    continue;
                }
                let [r, g, b, _] = self.at(px as u32, py as u32);
                let luma =
                    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8;
                best = best.max(luma);
            }
        }
        best
    }

    /// The same count, but only below `y_from` — for asking "are there markers" without counting the
    /// wordmark, which is painted in the same accent as a fresh fix.
    pub fn count_colour_below(&self, colour: [u8; 3], tolerance: u8, y_from: u32) -> usize {
        (y_from..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .filter(|(x, y)| near(self.at(*x, *y), colour, tolerance))
            .count()
    }

    /// How many pixels are within `tolerance` of `colour`. Presence, not placement: "the wall drew
    /// markers at all" is a different question from "this one is here".
    pub fn count_colour(&self, colour: [u8; 3], tolerance: u8) -> usize {
        self.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| {
                pixel[0].abs_diff(colour[0]) <= tolerance
                    && pixel[1].abs_diff(colour[1]) <= tolerance
                    && pixel[2].abs_diff(colour[2]) <= tolerance
            })
            .count()
    }
}

/// Whether a pixel is the token's colour, allowing for antialiasing and the rasteriser's rounding.
fn near(pixel: [u8; 4], colour: [u8; 3], tolerance: u8) -> bool {
    pixel[0].abs_diff(colour[0]) <= tolerance
        && pixel[1].abs_diff(colour[1]) <= tolerance
        && pixel[2].abs_diff(colour[2]) <= tolerance
        && pixel[3] > 200
}

/// A token's colour as sRGB bytes, through Rune's own encoder — the same one that paints the pixel
/// and prints the hex in `--tree`. A literal here would outlive the palette it describes.
pub fn ink(colour: rune::Color) -> [u8; 3] {
    let hex = colour.hex();
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
    [byte(1), byte(3), byte(5)]
}

/// The viewport every scene is rendered at, and the one the assertions project against.
pub fn scene_frame() -> Rendered {
    Rendered {
        camera: scene::CAMERA,
        width: tokens::WINDOW_SIZE.0 as u32,
        height: tokens::WINDOW_SIZE.1 as u32,
    }
}

/// The camera a scene renders at, so the assertions project against the scene they check.
pub fn scene_camera(scene: &str) -> Camera {
    crate::scene::state_for(scene).camera()
}

/// Where a hull's marker should be, in the capture's pixels, for the scene's own camera.
pub fn marker_in(scene: &str, id_unit: i64) -> Result<(f64, f64), String> {
    let entries = scene::fixture()?;
    let entry = entries
        .iter()
        .find(|hull| hull.id_unit == id_unit)
        .ok_or_else(|| format!("the fixture has no hull {id_unit}"))?;
    let fix = entry
        .position
        .as_ref()
        .ok_or_else(|| format!("hull {id_unit} has never reported, so it has no marker"))?;
    let frame = Rendered {
        camera: scene_camera(scene),
        ..scene_frame()
    };
    Ok(frame.project(fix.latitude, fix.longitude))
}

/// The base scene's marker, for callers that do not need a specific camera.
pub fn marker_at(id_unit: i64) -> Result<(f64, f64), String> {
    marker_in("picture", id_unit)
}

/// The failure that names a distance, so a moved marker reports the number rather than "failed".
#[cfg_attr(not(test), allow(dead_code))]
fn expect_marker(
    image: &Image,
    label: &str,
    at: (f64, f64),
    colour: [u8; 3],
) -> Result<(), String> {
    let Some((centre, _count)) = image.blob_centre(at.0, at.1, colour, 14) else {
        return Err(format!(
            "{label}: no marker ink near ({:.0}, {:.0})",
            at.0, at.1
        ));
    };
    let distance = ((centre.0 - at.0).powi(2) + (centre.1 - at.1).powi(2)).sqrt();
    if distance > TOLERANCE_PX {
        return Err(format!(
            "{label}: the marker is {distance:.1} px from ({:.0}, {:.0}) — {TOLERANCE_PX:.0} px is \
             the limit, and the ink is centred at ({:.1}, {:.1})",
            at.0, at.1, centre.0, centre.1
        ));
    }
    Ok(())
}

/// A marker's drawn place on screen, and how far its ink was from there.
fn marker_in_panel(
    image: &Image,
    scene: &str,
    id_unit: i64,
    origin: (f64, f64),
) -> Result<((f64, f64), f64), String> {
    let (x, y) = marker_in(scene, id_unit)?;
    let at = (x + origin.0, y + origin.1);
    let Some((centre, count)) = image.blob_centre(at.0, at.1, ink(tokens::accent()), 8) else {
        return Err(format!(
            "no marker ink near ({:.0}, {:.0}) — hull {id_unit}'s projected place, with the panel's \
             corner at ({:.0}, {:.0})",
            at.0, at.1, origin.0, origin.1
        ));
    };
    if count < 8 {
        return Err(format!(
            "the ink near ({:.0}, {:.0}) is {count} px — too little to be a marker",
            at.0, at.1
        ));
    }
    let distance = ((centre.0 - at.0).powi(2) + (centre.1 - at.1).powi(2)).sqrt();
    if distance > TOLERANCE_PX {
        return Err(format!(
            "the marker for hull {id_unit} is {distance:.1} px from where the projection puts it \
             (expected ({:.1}, {:.1}), ink centred at ({:.1}, {:.1})): {TOLERANCE_PX:.0} px is the \
             limit",
            at.0, at.1, centre.0, centre.1
        ));
    }
    Ok((at, distance))
}

/// Check one scene's capture. Returns the lines it wants printed, or the failures.
pub fn assert_scene(image: &Image, scene: &str) -> Result<Vec<String>, Vec<String>> {
    let mut failures = Vec::new();
    let mut lines = Vec::new();
    let accent = ink(tokens::accent());
    let dim = ink(tokens::ink_dim());

    match scene {
        "empty" => {
            // No picture, no markers: if any accent ink is on the panel, a marker was drawn from a
            // state that has no units in it.
            // Below the panel's corner: the wordmark is accent-coloured too, and a wall with no
            // picture still has a header.
            let from = image
                .find_panel(ink(tokens::panel()))
                .map(|(_, y)| y as u32)
                .unwrap_or(0);
            let count = image.count_colour_below(accent, 24, from);
            if count > 0 {
                failures.push(format!(
                    "empty: {count} accent pixels in the picture — a wall with no picture has no \
                     markers"
                ));
            } else {
                lines.push("empty: no marker ink anywhere".to_string());
            }
        }
        "picture" | "card-open" => {
            // The markers live inside the panel: a projection is panel-relative, a capture is not.
            let Some(origin) = image.find_panel(ink(tokens::panel())) else {
                failures.push(format!(
                    "{scene}: no picture panel on screen — nothing to assert about markers"
                ));
                return Err(failures);
            };
            lines.push(format!(
                "{scene}: picture panel at ({:.0}, {:.0})",
                origin.0, origin.1
            ));
            match marker_in_panel(image, scene, scene::LIVE_UNIT, origin) {
                Ok((at, distance)) => lines.push(format!(
                    "{scene}: live marker within {distance:.1} px of ({:.0}, {:.0})",
                    at.0, at.1
                )),
                Err(error) => failures.push(error),
            }
            if scene == "card-open" {
                lines.extend(assert_card(image, origin, &mut failures));
            }
        }
        "photo" => {
            // The picture half of #32: at this zoom the hull is drawn as its **picture**, not its symbol.
            // The fixture's ink is a colour no token uses, so finding it at the hull's position is proof
            // that the decoded bytes reached the marker — and its absence would be a symbol drawn at a
            // zoom where the wall should be showing the ship.
            let Some(origin) = image.find_panel(ink(tokens::panel())) else {
                failures.push("photo: no picture panel on screen".to_string());
                return Err(failures);
            };
            let (px, py) = match marker_in(scene, scene::PHOTO_UNIT) {
                Ok(at) => at,
                Err(error) => {
                    failures.push(error);
                    return Err(failures);
                }
            };
            let (x, y) = (px + origin.0, py + origin.1);
            // The fixture's hull is white-ish; the deckhouse is the lighter blue.
            let hud = [232u8, 243, 255];
            let found = image.nearest_colour(x, y, hud, 30.0);
            match found {
                Some(distance) => lines.push(format!(
                    "photo: the hull's own picture within {distance:.1} px of ({x:.0}, {y:.0})"
                )),
                None => failures.push(format!(
                    "photo: no picture ink within 30 px of the hull at ({x:.0}, {y:.0}) — the capture                      drew the symbol, so the decoded picture never reached the marker"
                )),
            }
        }
        "settings" => {
            // **The sheet is drawn, it is the size the token says, and it is on the wall** (#82). The
            // scrim darkens everything behind it, so the sheet is the only `panel()`-coloured ink left in
            // the picture — which is what makes its box findable at all.
            // The sheet's box, measured the plain way: every pixel that is the sheet's fill, bounded.
            // `find_panel_rect` is not usable here — it wants a *row* that is mostly the panel's colour,
            // which is true of the wall and not of a 360 pt sheet in the middle of it.
            let surface = ink(tokens::panel());
            let (mut sx, mut sy, mut ex, mut ey) = (f64::MAX, f64::MAX, 0.0f64, 0.0f64);
            let mut found = false;
            // Eroded by one: a *solid* block of the sheet's fill, so an antialiased edge or a stray
            // handle pixel a shade off the panel cannot stretch the box across the wall.
            let solid = |x: u32, y: u32| {
                x > 0
                    && y > 0
                    && x + 1 < image.width
                    && y + 1 < image.height
                    && [(0i32, 0i32), (1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .all(|(dx, dy)| {
                            near(
                                image.at((x as i32 + dx) as u32, (y as i32 + dy) as u32),
                                surface,
                                6,
                            )
                        })
            };
            for y in 0..image.height {
                for x in 0..image.width {
                    if solid(x, y) {
                        found = true;
                        sx = sx.min(f64::from(x));
                        sy = sy.min(f64::from(y));
                        ex = ex.max(f64::from(x));
                        ey = ey.max(f64::from(y));
                    }
                }
            }
            if !found {
                failures.push(
                    "settings: no sheet on screen — the setting is invisible on the one scene that opens it"
                        .to_string(),
                );
                return Err(failures);
            }
            // The erosion took a pixel off each side; put it back.
            let (sx, sy) = (sx - 1.0, sy - 1.0);
            let (sw, sh) = (ex - sx + 2.0, ey - sy + 2.0);
            let (want_w, want_h) = (
                f64::from(tokens::SHEET_SIZE.0),
                f64::from(tokens::SHEET_SIZE.1),
            );
            if (sw - want_w).abs() > 2.0 || (sh - want_h).abs() > 2.0 {
                failures.push(format!(
                    "settings: the sheet is {sw:.0}×{sh:.0} for a {want_w:.0}×{want_h:.0} token"
                ));
            } else {
                lines.push(format!(
                    "settings: the sheet is {sw:.0}×{sh:.0} at ({sx:.0}, {sy:.0})"
                ));
            }
            // Its rows have ink: a title and the one setting, which is what a reader has to be able to
            // read to change anything.
            let mut brightest = 0u8;
            for y in (sy as u32 + 8)..((sy + sh) as u32 - 8) {
                for x in (sx as u32 + 8)..((sx + sw) as u32 - 8) {
                    if x < image.width && y < image.height {
                        brightest = brightest.max(image.luma(x, y));
                    }
                }
            }
            if brightest < 120 {
                failures.push(format!(
                    "settings: the sheet's rows have no ink (brightest {brightest} luma)"
                ));
            } else {
                lines.push(format!(
                    "settings: the sheet's rows are drawn ({brightest} luma)"
                ));
            }
        }
        "panes-named" => {
            // **A pane with a name draws its plate; a pane without one draws nothing** (#83). The probe is
            // `view::name_probe` — the same rectangle the plate is placed against — and the test is its
            // plate colour, so a marker that happens to sit in the corner cannot be mistaken for a name.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("panes-named: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("panes-named");
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(&state.layout, viewport, state.settings.pane_gap);
            let plate = ink(tokens::name_plate());
            for (index, (id, rect)) in rects.iter().enumerate() {
                let probe = crate::ui::view::name_probe(*rect);
                let (px, py) = (origin_x + f64::from(probe.x), origin_y + f64::from(probe.y));
                let mut hits = 0;
                let mut samples = 0;
                for dy in 0..probe.h as i32 {
                    for dx in 0..probe.w as i32 {
                        let (x, y) = ((px as i32 + dx) as u32, (py as i32 + dy) as u32);
                        if x >= image.width || y >= image.height {
                            continue;
                        }
                        samples += 1;
                        if near(image.at(x, y), plate, 6) {
                            hits += 1;
                        }
                    }
                }
                let named = state
                    .pane(*id)
                    .and_then(|pane| pane.name.clone())
                    .and_then(|name| crate::ui::shown_name(&name))
                    .is_some();
                let drawn = samples > 0 && hits * 2 >= samples;
                match (named, drawn) {
                    (true, true) => lines.push(format!(
                        "panes-named: pane {index} draws its name plate at ({px:.0}, {py:.0})"
                    )),
                    (false, false) => lines.push(format!(
                        "panes-named: pane {index} is unnamed and draws no plate"
                    )),
                    (true, false) => failures.push(format!(
                        "panes-named: pane {index} has a name and no plate at ({px:.0}, {py:.0}) — \
                         {hits}/{samples} samples are a plate"
                    )),
                    (false, true) => failures.push(format!(
                        "panes-named: pane {index} has no name and a plate at ({px:.0}, {py:.0}) — a \
                         name nobody typed"
                    )),
                }
            }
        }
        "panes-gaps" => {
            // **The gutter is real, and it is the gap the setting says** (#82). The panes are the only
            // `panel()`-coloured ink, so their union box is the picture inset by half a gap; inside it, a
            // column (or a row) that is mostly the window's own background is the space between two panes.
            // The width of that run is measured against `tokens::GAP`, so the assertion is about the look
            // rather than about a number copied out of it.
            let Some((ox, oy, pw, ph)) = image.find_panel_rect(ink(tokens::panel())) else {
                failures.push("panes-gaps: no pane surfaces on screen".to_string());
                return Err(failures);
            };
            let gap = f64::from(tokens::GAP);
            let background = ink(tokens::bg());
            let mostly_background = |points: &[(u32, u32)]| {
                let hits = points
                    .iter()
                    .filter(|(x, y)| {
                        *x < image.width
                            && *y < image.height
                            && near(image.at(*x, *y), background, 6)
                    })
                    .count();
                !points.is_empty() && hits * 10 >= points.len() * 8
            };
            // `(start, length)` of every run along one axis, in image pixels.
            let runs_along =
                |vertical: bool, from: u32, to: u32, other: (u32, u32)| -> Vec<(u32, u32)> {
                    let mut runs = Vec::new();
                    let mut run: Option<(u32, u32)> = None;
                    for at in from..to {
                        let points: Vec<(u32, u32)> = (other.0..other.1)
                            .step_by(2)
                            .map(|v| if vertical { (at, v) } else { (v, at) })
                            .collect();
                        if mostly_background(&points) {
                            run = Some((run.map_or(at, |(start, _)| start), at));
                        } else if let Some((start, last)) = run.take() {
                            runs.push((start, last - start + 1));
                        }
                    }
                    if let Some((start, last)) = run.take() {
                        runs.push((start, last - start + 1));
                    }
                    runs
                };
            let inset = 4u32;
            let right = (ox + pw) as u32;
            let bottom = (oy + ph) as u32;
            let top = oy as u32;
            // Columns across the whole picture: the split between the left pane and the right column.
            let across = runs_along(
                true,
                ox as u32 + 1,
                right - 1,
                (top + inset, bottom.saturating_sub(inset)),
            );
            let Some(&(vx, vlen)) = across
                .iter()
                .find(|(_, len)| (*len as f64 - gap).abs() <= 1.0)
            else {
                failures.push(format!(
                    "panes-gaps: no vertical gutter of {gap:.0} pt — measured {across:?}, so the panes \
                     are drawn without the space the setting says they have"
                ));
                return Err(failures);
            };
            // Rows **inside the right column**, which is where two panes are stacked: the horizontal
            // gutter spans only that column, so a row across the whole picture would be mostly panes.
            let down = runs_along(
                false,
                oy as u32 + 1,
                bottom - 1,
                (vx + vlen + inset, right.saturating_sub(inset)),
            );
            let Some(&(_hy, hlen)) = down
                .iter()
                .find(|(_, len)| (*len as f64 - gap).abs() <= 1.0)
            else {
                failures.push(format!(
                    "panes-gaps: no horizontal gutter of {gap:.0} pt in the right column — measured \
                     {down:?}"
                ));
                return Err(failures);
            };
            lines.push(format!(
                "panes-gaps: a {vlen} pt gutter down at x={vx} and a {hlen} pt gutter across — the \
                 {gap:.0} pt the setting asks for"
            ));
        }
        "panes" | "panes-pan" => {
            // **Each pane projects against its own frame** (`INV-04`, per pane). The rectangles come from
            // `Layout::rects` measured against the panel this capture actually has, so this check and the
            // view cannot disagree about where a pane is.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("panes: no picture panel on screen".to_string());
                return Err(failures);
            };
            // `panes-pan` is asserted against the layout its **pointer** must produce: the scene leaves the
            // split at half and `fixtures/scripts/panes-pan.json` drags it to 58%, so a gesture that
            // reaches nothing (#80) leaves the border at half and every hull lands outside its pane here.
            let state = scene::state_for("panes");
            // The **drawn** rectangles: the cells pulled in by the gap the scene chose (#82), which is the
            // same function the view lays the panes out with.
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(&state.layout, viewport, state.settings.pane_gap);
            if rects.len() != scene::PANES.len() {
                failures.push(format!(
                    "panes: {} rectangles for {} panes",
                    rects.len(),
                    scene::PANES.len()
                ));
                return Err(failures);
            }
            for (index, (id_unit, _zoom)) in scene::PANES.iter().enumerate() {
                let (id, rect) = rects[index];
                let Some(pane) = state.pane(id) else {
                    failures.push(format!("panes: pane {id} has no state"));
                    continue;
                };
                // The frame a headless pane draws with: its own rectangle, the rule `pane_laid_out`
                // applies on the wall's behalf too.
                let frame = crate::ui::view::synthetic_frame(pane.camera, rect, 1.0);
                let Some(fix) = scene::fixture().ok().and_then(|entries| {
                    entries
                        .into_iter()
                        .find(|entry| entry.id_unit == *id_unit)
                        .and_then(|entry| entry.position)
                }) else {
                    failures.push(format!("panes: hull {id_unit} has no fix"));
                    continue;
                };
                let (px, py) = frame.project(fix.latitude, fix.longitude);
                let (x, y) = (
                    origin_x + f64::from(rect.x) + px,
                    origin_y + f64::from(rect.y) + py,
                );
                let inside = x >= origin_x + f64::from(rect.x)
                    && x <= origin_x + f64::from(rect.x) + f64::from(rect.w)
                    && y >= origin_y + f64::from(rect.y)
                    && y <= origin_y + f64::from(rect.y) + f64::from(rect.h);
                if !inside {
                    failures.push(format!(
                        "panes: hull {id_unit} projects to ({x:.0}, {y:.0}), outside its pane at \
                         ({:.0}, {:.0}) {:.0}×{:.0}",
                        origin_x + f64::from(rect.x),
                        origin_y + f64::from(rect.y),
                        rect.w,
                        rect.h
                    ));
                    continue;
                }
                // A stale hull draws in the dim ink (`INV-03`/`stale`): the marker is there, it is
                // just not the "here now" colour, and this scene contains one on purpose.
                let ink_here = if image.blob_centre(x, y, accent, 8).is_some() {
                    accent
                } else {
                    dim
                };
                match image.blob_centre(x, y, ink_here, 8) {
                    Some((centre, count)) if count >= 8 => {
                        let distance =
                            ((centre.0 - x).powi(2) + (centre.1 - y).powi(2)).sqrt();
                        if distance > TOLERANCE_PX {
                            failures.push(format!(
                                "panes: pane {index}'s hull {id_unit} is {distance:.1} px from where \
                                 its own pane's projection puts it ({x:.0}, {y:.0}) — a marker drawn \
                                 against another pane's frame lands here (INV-04)"
                            ));
                        } else {
                            lines.push(format!(
                                "panes: pane {} ({id_unit}) marker {distance:.1} px from ({x:.0}, {y:.0})",
                                index
                            ));
                        }
                    }
                    _ => failures.push(format!(
                        "panes: no marker ink at ({x:.0}, {y:.0}) — pane {index}'s hull {id_unit} is \
                         not where that pane's projection puts it"
                    )),
                }
            }
            // **The focused pane is marked by a brighter edge than the others.** Counting "focus-coloured"
            // pixels does not work: the accent marks a live hull and a neutral ink draws a symbol's
            // frame, so a ring cannot be separated from the marks inside the panes by colour alone. What
            // *is* separable is the edge itself — the focused pane draws a bright 1 pt line, an
            // unfocused one a hairline that is barely above the panel. So: the median luma down each
            // pane's left edge, and the focused one must stand out by a margin a reader would see.
            let focused = state.layout.focused();
            let mut edges = Vec::new();
            for (id, rect) in &rects {
                let x = (origin_x + f64::from(rect.x)).max(0.0) as u32;
                let top = (origin_y + f64::from(rect.y) + 10.0).max(0.0) as u32;
                let bottom =
                    ((origin_y + f64::from(rect.y + rect.h) - 10.0) as u32).min(image.height);
                // Three columns, and the brightest of their medians: the panel this capture has is
                // found by its fill colour, so its rectangle can be a pixel off the one the view laid
                // out — a single column would then miss a one-pixel line.
                let mut best = 0;
                for column in x.saturating_sub(1)..=(x + 1).min(image.width - 1) {
                    let mut luma: Vec<u8> = (top..bottom).map(|y| image.luma(column, y)).collect();
                    if luma.is_empty() {
                        continue;
                    }
                    luma.sort_unstable();
                    best = best.max(luma[luma.len() / 2]);
                }
                edges.push((*id, best));
            }
            let Some((_, focused_luma)) = edges.iter().find(|(id, _)| *id == focused) else {
                failures.push(format!("panes: the focused pane {focused} has no edge"));
                return Err(failures);
            };
            let quieter: Vec<_> = edges.iter().filter(|(id, _)| *id != focused).collect();
            let loudest_other = quieter.iter().map(|(_, luma)| *luma).max().unwrap_or(0);
            if *focused_luma < loudest_other.saturating_add(40) {
                failures.push(format!(
                    "panes: pane {focused} is focused and its edge is {focused_luma} luma; another \
                     pane's is {loudest_other} — the ring has to be visible against the panes it is \
                     distinguished from"
                ));
            } else {
                lines.push(format!(
                    "panes: focus ring on pane {focused} ({focused_luma} luma against {loudest_other})"
                ));
            }
        }
        "pane-menu" | "panes-menu" => {
            // **The menu is drawn, at the click, inside its pane** (#77), and **as tall as its items**
            // (#81): a menu sized for one row clips the second, which no golden notices until someone opens
            // it on a wall. So every row is sampled for its own label ink.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push(format!("{scene}: no picture panel on screen"));
                return Err(failures);
            };
            let state = scene::state_for(scene);
            let rows = state
                .menu
                .map(|menu| crate::ui::menu_items(&state, menu.pane).len())
                .unwrap_or(0);
            // One pane: the menu grows the wall and names this pane, and cannot close it. A tiled wall adds
            // `Close pane` (#81, #83).
            let expected = if scene == "panes-menu" { 3 } else { 2 };
            if rows != expected {
                failures.push(format!(
                    "{scene}: the menu offers {rows} items, and this wall is the case for {expected}"
                ));
            }
            let (w, h) = tokens::menu_size(rows.max(1));
            // **The pane's rectangle, not the panel's**: a menu is clamped inside the pane it was asked
            // on. With one pane those are the same rectangle, which is why #77's assertion passed on the
            // wall it was written for and would have been wrong on any other.
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rect = state
                .menu
                .and_then(|menu| state.layout.rect_of(viewport, menu.pane))
                .unwrap_or(crate::ui::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: panel_w as f32,
                    h: panel_h as f32,
                });
            let (mx, my) = crate::ui::menu_origin(rect, scene::MENU_AT, (w, h));
            let (mx, my) = (origin_x + f64::from(mx), origin_y + f64::from(my));
            let (w, h) = (f64::from(w), f64::from(h));
            // The surface's own fill, where the click was: the panel is `panel()`, a menu is `bg()`.
            let surface = ink(tokens::bg());
            let mut hits = 0;
            let mut samples = 0;
            for dy in (4..(h as i32 - 4)).step_by(4) {
                for dx in (4..(w as i32 - 4)).step_by(4) {
                    let (x, y) = ((mx + f64::from(dx)) as u32, (my + f64::from(dy)) as u32);
                    if x >= image.width || y >= image.height {
                        continue;
                    }
                    samples += 1;
                    if near(image.at(x, y), surface, 6)
                        || near(image.at(x, y), ink(tokens::ink()), 40)
                    {
                        hits += 1;
                    }
                }
            }
            let inside = mx >= origin_x
                && my >= origin_y
                && mx + w <= origin_x + panel_w + 0.5
                && my + h <= origin_y + panel_h + 0.5;
            if !inside {
                failures.push(format!(
                    "{scene}: the menu at ({mx:.0}, {my:.0}) {w:.0}×{h:.0} is not inside the picture \
                     at ({origin_x:.0}, {origin_y:.0}) {panel_w:.0}×{panel_h:.0} — a menu half over the \
                     bezel is a menu whose item cannot be pressed"
                ));
            } else if hits * 10 < samples * 8 {
                failures.push(format!(
                    "{scene}: {hits}/{samples} samples inside the menu's box are its surface or its \
                     label — the menu at ({mx:.0}, {my:.0}) is not drawn"
                ));
            } else {
                lines.push(format!(
                    "{scene}: menu at ({mx:.0}, {my:.0}) {w:.0}×{h:.0}, {rows} row(s), inside the picture"
                ));
            }
            // **Each row is on the plate, and has its own label** (#81). Sampled from `MENU_ROW` — one
            // row's height, the constant — rather than from the surface's declared size, so a surface that
            // is shorter than its items is caught: the second label still draws (nothing clips it), it just
            // hangs off the plate, which is the failure this looks for.
            for row in 0..rows {
                let top = my + f64::from(tokens::MENU_ROW) * row as f64;
                let mut on_plate = 0;
                let mut plate_samples = 0;
                let mut brightest = 0u8;
                for dy in 6..(tokens::MENU_ROW as i32 - 6) {
                    for dx in 6..(w as i32 - 6) {
                        let (x, y) = ((mx + f64::from(dx)) as u32, (top + f64::from(dy)) as u32);
                        if x >= image.width || y >= image.height {
                            continue;
                        }
                        plate_samples += 1;
                        let pixel = image.at(x, y);
                        if near(pixel, surface, 6) {
                            on_plate += 1;
                        }
                        brightest = brightest.max(image.luma(x, y));
                    }
                }
                if plate_samples == 0 || on_plate * 2 < plate_samples {
                    failures.push(format!(
                        "{scene}: row {row} of the menu is off its plate — {on_plate}/{plate_samples} \
                         samples are the menu's own fill at y={top:.0}, so the surface is shorter than \
                         its items"
                    ));
                } else if brightest < 120 {
                    failures.push(format!(
                        "{scene}: row {row} of the menu has no label ink (brightest {brightest})"
                    ));
                } else {
                    lines.push(format!(
                        "{scene}: row {row} is on the plate, with its label ({brightest} luma)"
                    ));
                }
            }
        }
        "panes-drag" => {
            // **The gap is where the pane lands** (#70): it is drawn from `Layout::preview`, so this
            // checks the drawn rectangle against the preview — the same function the release calls.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("panes-drag: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("panes-drag");
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(&state.layout, viewport, state.settings.pane_gap);
            let Some(drag) = state.dragging else {
                failures.push("panes-drag: no pane is lifted in the scene".to_string());
                return Err(failures);
            };
            let Some(drop) = drag.drop else {
                failures.push("panes-drag: the lifted pane has no drop under it".to_string());
                return Err(failures);
            };
            if state.layout.preview(drop).is_none() {
                failures.push(format!("panes-drag: {drop:?} has no preview"));
                return Err(failures);
            }
            // The slot is inset by a gap on each side (the slot, not the whole pane) — one function, the
            // same one the picture draws and the ghost stands on.
            let Some(landing) =
                crate::ui::view::landing_rect(&state.layout, drop, viewport, drag.pane)
            else {
                failures
                    .push("panes-drag: the preview does not place the dragged pane".to_string());
                return Err(failures);
            };
            let gap = crate::ui::view::inset_rect(landing, tokens::GAP);
            let (gx, gy) = (origin_x + f64::from(gap.x), origin_y + f64::from(gap.y));
            let (gw, gh) = (f64::from(gap.w), f64::from(gap.h));
            let fill = ink(tokens::gap_fill());
            let mut inside = 0;
            let mut fill_hits = 0;
            for dy in (6..(gh as i32 - 6)).step_by(4) {
                for dx in (6..(gw as i32 - 6)).step_by(4) {
                    inside += 1;
                    if near(
                        image.at((gx + f64::from(dx)) as u32, (gy + f64::from(dy)) as u32),
                        fill,
                        6,
                    ) {
                        fill_hits += 1;
                    }
                }
            }
            if inside > 0 && fill_hits * 10 < inside * 9 {
                failures.push(format!(
                    "panes-drag: the gap at ({gx:.0}, {gy:.0}) {gw:.0}×{gh:.0} is filled on \
                     {fill_hits}/{inside} samples — the slot a pane lands in is drawn where the \
                     preview puts it"
                ));
            } else {
                lines.push(format!(
                    "panes-drag: gap at ({gx:.0}, {gy:.0}) {gw:.0}×{gh:.0} for the pane that lands \
                     there"
                ));
            }
            // **The pane in the air is drawn as itself, on the slot it will land in** (#84). The slot is
            // drawn in `gap_fill` and the ghost is the pane standing on it — its own frame, its own camera — so its hulls
            // are the slot's rectangle plus the pane's own projection. A ghost that is missing, or drawn on
            // any other rectangle, leaves no hull ink where this looks.
            let landed = crate::ui::view::landing_rect(&state.layout, drop, viewport, drag.pane);
            let carried = state
                .pane_rects()
                .iter()
                .position(|(id, _)| *id == drag.pane)
                .and_then(|index| scene::PANES.get(index).copied());
            if let (Some(landed), Some((id_unit, _)), Some(pane)) =
                (landed, carried, state.pane(drag.pane))
            {
                let Some(fix) = scene::fixture().ok().and_then(|entries| {
                    entries
                        .into_iter()
                        .find(|entry| entry.id_unit == id_unit)
                        .and_then(|entry| entry.position)
                }) else {
                    failures.push(format!("panes-drag: hull {id_unit} has no fix"));
                    return Err(failures);
                };
                // The ghost draws the pane as the wall drew it in its own slot, so the frame is the one
                // the pane had, not the slot it is landing in: same size, both from `Layout`.
                // The frame is the **landing** rectangle's: a dropped pane is drawn at the size it lands at,
                // and the ghost is exactly that projection. Both come from `landing_rect`, so the assertion
                // and the picture cannot disagree — and the viewport is the capture's, not a scene's
                // default one (that mistake is worth seven pixels).
                let frame = crate::ui::view::synthetic_frame(pane.camera, landed, 1.0);
                let (px, py) = frame.project(fix.latitude, fix.longitude);
                let (gx, gy) = (
                    origin_x + f64::from(landed.x) + px,
                    origin_y + f64::from(landed.y) + py,
                );
                let ink_here = if image.blob_centre(gx, gy, accent, 8).is_some() {
                    accent
                } else {
                    dim
                };
                match image.blob_centre(gx, gy, ink_here, 8) {
                    Some((centre, count)) if count >= 8 => {
                        let distance =
                            ((centre.0 - gx).powi(2) + (centre.1 - gy).powi(2)).sqrt();
                        if distance > TOLERANCE_PX {
                            failures.push(format!(
                                "panes-drag: the lifted pane's hull {id_unit} is {distance:.1} px from \
                                 the ghost's projection ({gx:.0}, {gy:.0}) — the ghost is the pane, drawn \
                                 on the slot it lands in"
                            ));
                        } else {
                            lines.push(format!(
                                "panes-drag: the lifted pane is drawn on its landing slot, hull \
                                 {id_unit} at ({gx:.0}, {gy:.0})"
                            ));
                        }
                    }
                    _ => failures.push(format!(
                        "panes-drag: no hull ink at ({gx:.0}, {gy:.0}) — the pane in the air is not \
                         drawn on the slot it lands in"
                    )),
                }
            }

            // Every pane whose hulls are not covered by the gap still projects its own hulls.
            for (index, (id_unit, _zoom)) in scene::PANES.iter().enumerate() {
                let Some((id, rect)) = rects.get(index).copied() else {
                    continue;
                };
                if id == drag.pane {
                    continue;
                }
                // A pane the gap covers is expected to be hidden: the slot is drawn over it.
                let overlap_w = (rect.right().min(gap.right()) - rect.x.max(gap.x)).max(0.0);
                let overlap_h = (rect.bottom().min(gap.bottom()) - rect.y.max(gap.y)).max(0.0);
                if overlap_w * overlap_h > 1.0 {
                    continue;
                }
                let Some(pane) = state.pane(id) else { continue };
                let frame = crate::ui::view::synthetic_frame(pane.camera, rect, 1.0);
                let Some(fix) = scene::fixture().ok().and_then(|entries| {
                    entries
                        .into_iter()
                        .find(|entry| entry.id_unit == *id_unit)
                        .and_then(|entry| entry.position)
                }) else {
                    continue;
                };
                let (px, py) = frame.project(fix.latitude, fix.longitude);
                let at = (
                    origin_x + f64::from(rect.x) + px,
                    origin_y + f64::from(rect.y) + py,
                );
                let ink_here = if image.blob_centre(at.0, at.1, accent, 8).is_some() {
                    accent
                } else {
                    dim
                };
                match image.blob_centre(at.0, at.1, ink_here, 8) {
                    Some((centre, count)) if count >= 8 => {
                        let distance = ((centre.0 - at.0).powi(2) + (centre.1 - at.1).powi(2)).sqrt();
                        if distance > TOLERANCE_PX {
                            failures.push(format!(
                                "panes-drag: pane {index}'s hull {id_unit} is {distance:.1} px from \
                                 its own pane's projection ({at:.0}) — INV-04 holds while a pane is \
                                 being dragged too",
                                at = format!("({:.0}, {:.0})", at.0, at.1)
                            ));
                        } else {
                            lines.push(format!(
                                "panes-drag: pane {index} ({id_unit}) still projects its hull"
                            ));
                        }
                    }
                    _ => failures.push(format!(
                        "panes-drag: no marker ink at ({:.0}, {:.0}) — pane {index}'s hull {id_unit} \
                         vanished while another pane was lifted",
                        at.0, at.1
                    )),
                }
            }
        }
        "stale" => {
            // The dim marker must be dimmer than a live one, by a margin a reader would notice. A
            // threshold of 20 luma steps is roughly "one shade" — bigger than antialiasing noise,
            // smaller than the real difference between the two tokens.
            let Some(origin) = image.find_panel(ink(tokens::panel())) else {
                failures.push("stale: no picture panel on screen".to_string());
                return Err(failures);
            };
            let stale = marker_in(scene, scene::STALE_UNIT)
                .ok()
                .map(|(x, y)| (x + origin.0, y + origin.1));
            let live = marker_in(scene, scene::LIVE_UNIT)
                .ok()
                .map(|(x, y)| (x + origin.0, y + origin.1));
            match (stale, live) {
                (Some(stale), Some(live)) => {
                    let stale_luma = image.brightest_luma(stale.0, stale.1, 6);
                    let live_luma = image.brightest_luma(live.0, live.1, 6);
                    if stale_luma + 20 >= live_luma {
                        failures.push(format!(
                            "stale: the stale marker is {stale_luma} luma and the live one is \
                             {live_luma} — the stale ink ({dim:?}) must be dimmer than the live \
                             ({accent:?})"
                        ));
                    } else {
                        lines.push(format!(
                            "stale: stale marker {stale_luma} luma vs live {live_luma}"
                        ));
                    }
                }
                _ => failures.push("stale: the fixture is missing a stale or a live hull".into()),
            }
        }
        other => failures.push(format!("no assertions are written for scene {other:?}")),
    }

    if failures.is_empty() {
        Ok(lines)
    } else {
        Err(failures)
    }
}

/// The card's rectangle, where `units::card::place` says it goes — the same arithmetic the view uses.
fn assert_card(image: &Image, origin: (f64, f64), failures: &mut Vec<String>) -> Vec<String> {
    let mut lines = Vec::new();
    let Ok(anchor) = marker_at(scene::CARD_UNIT).map(|(x, y)| (x + origin.0, y + origin.1)) else {
        failures.push("card-open: the selected hull has no marker".to_string());
        return lines;
    };
    let viewport = (
        f64::from(scene_frame().width),
        f64::from(scene_frame().height),
    );
    let placement = crate::units::card::place(anchor, viewport);

    // The card's top-left corner is inside the panel's fill; the panel is a different colour from
    // the ground, so the corner pixel is the proof the card is where the arithmetic put it.
    let panel = ink(tokens::panel());
    let x = placement.x + 2.0;
    let y = placement.y + 40.0;
    if image.count_colour(panel, 8) == 0 {
        failures.push(format!(
            "card-open: no panel ink anywhere — the card did not draw (expected at {:.0}, {:.0})",
            placement.x, placement.y
        ));
    } else if image.nearest_colour(x, y, panel, 6.0).is_none() {
        failures.push(format!(
            "card-open: the card's body is not at ({x:.0}, {y:.0}) — place() said \
             ({:.0}, {:.0}), flipped={}",
            placement.x, placement.y, placement.flipped
        ));
    } else {
        lines.push(format!(
            "card-open: card at ({:.0}, {:.0}), flipped={}",
            placement.x, placement.y, placement.flipped
        ));
        if placement.x < 0.0 || placement.x + crate::units::card::WIDTH > viewport.0 {
            failures.push(format!(
                "card-open: the card leaves the window: x={:.0}, width={}",
                placement.x,
                crate::units::card::WIDTH
            ));
        }
    }
    lines
}

/// Structure, from the state the capture renders: one marker per hull with a fix, one card at most,
/// and no marker at the origin.
///
/// Rune's own `--tree` dump does not come out of a headless render in this build, so the structure is
/// read from the same `State` the capture draws. That is the stronger check anyway: it says "39
/// markers for 39 hulls that have reported", which no dump of element rectangles can say.
pub fn assert_structure(scene: &str) -> Result<Vec<String>, Vec<String>> {
    let mut failures = Vec::new();
    let mut lines = Vec::new();
    let state = crate::scene::state_for(scene);
    let frame = scene_frame();
    let markers = crate::ui::markers::place(
        &state.picture,
        &frame,
        state.now(),
        state.stale_after,
        state.range,
        &|_| None,
    );

    match scene {
        "panes" | "panes-drag" | "panes-pan" | "panes-gaps" | "panes-named" => {
            // One marker per hull with a fix **in every pane**: a pane that drew nothing, or drew the
            // same set twice, is not a view.
            let expected = state
                .picture
                .units()
                .iter()
                .filter(|unit| unit.position.is_some())
                .count();
            let rects = crate::ui::view::panes_on(
                &state.layout,
                state.pane_viewport(),
                state.settings.pane_gap,
            );
            for (id, rect) in &rects {
                let Some(pane) = state.pane(*id) else {
                    continue;
                };
                let frame = crate::ui::view::synthetic_frame(pane.camera, *rect, 1.0);
                let placed = crate::ui::markers::place(
                    &state.picture,
                    &frame,
                    state.now(),
                    state.stale_after,
                    state.range,
                    &|_| None,
                );
                if placed.len() != expected {
                    failures.push(format!(
                        "panes: pane {id} has {} markers for {expected} hulls with a fix",
                        placed.len()
                    ));
                }
            }
            if failures.is_empty() {
                lines.push(format!(
                    "panes: {} panes, {expected} markers each",
                    rects.len()
                ));
            }
        }
        "empty" => {
            if !markers.is_empty() {
                failures.push(format!(
                    "empty: {} markers for a picture with no units",
                    markers.len()
                ));
            } else {
                lines.push("empty: no markers".to_string());
            }
        }
        _ => {
            let expected = state
                .picture
                .units()
                .iter()
                .filter(|unit| unit.position.is_some())
                .count();
            if markers.len() != expected {
                failures.push(format!(
                    "{scene}: {} markers for {expected} hulls with a fix — one marker each, and a \
                     hull that never reported has none",
                    markers.len()
                ));
            } else {
                lines.push(format!(
                    "{scene}: {} markers, one per hull with a fix",
                    markers.len()
                ));
            }
            // A marker at the origin is the classic "absent parsed as zero" bug wearing a fix.
            if let Some(marker) = markers
                .iter()
                .find(|marker| marker.x.abs() < 1.0 && marker.y.abs() < 1.0)
            {
                failures.push(format!(
                    "hull {} is drawn at (0, 0): a missing fix is not the null island (INV-01)",
                    marker.id_unit
                ));
            }
        }
    }

    if scene == "pane-menu" && state.menu.is_none() {
        failures.push(
            "pane-menu: the scene has no menu open, so the capture shows nothing of it".into(),
        );
    }

    let cards = usize::from(state.card.is_some());
    match (scene, cards) {
        ("card-open", 1) => lines.push("card-open: exactly one card".to_string()),
        ("card-open", other) => {
            failures.push(format!("card-open: {other} cards, and INV-10 says one"))
        }
        (_, 0) => {}
        (name, other) => failures.push(format!("{name}: {other} cards with nothing selected")),
    }
    if scene == "card-open"
        && let Some(card) = &state.card
        && card.unit.id_unit != scene::CARD_UNIT
    {
        failures.push(format!(
            "card-open: the card is for hull {}, not {}",
            card.unit.id_unit,
            scene::CARD_UNIT
        ));
    }

    if failures.is_empty() {
        Ok(lines)
    } else {
        Err(failures)
    }
}

/// How two captures differ, in the only two numbers that decide whether a change is a change.
///
/// **Why a tolerance exists at all.** The goldens are bytes from one rasteriser; CI renders with
/// lavapipe, a CPU Vulkan driver. Measured on 2026-11-08 across all four scenes, the two agree to
/// within **1/255 on 3–23 pixels of 921,600** — rounding at a glyph edge, not a different picture. A
/// byte compare therefore has to be the reference machine's privilege, or it proves nothing on CI. A
/// bounded compare can run everywhere, which is the point: CI must hold the goldens too, or a
/// consistent change (a moved card, a shifted token colour) passes CI and is caught only by whoever
/// happens to run the gate locally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Difference {
    /// Pixels with any channel differing, alpha included.
    pub differing: usize,
    /// The largest single-channel difference, 0–255.
    pub max_delta: u8,
}

impl Difference {
    /// Identical, which is what two renders of one scene must be on one machine.
    pub const NONE: Difference = Difference {
        differing: 0,
        max_delta: 0,
    };

    pub fn is_identical(self) -> bool {
        self.differing == 0
    }
}

/// Compare two captures. Different sizes are not a difference to be counted: they are a different
/// picture, and a golden of the wrong shape must fail loudly rather than report "0 differing".
pub fn difference(a: &Image, b: &Image) -> Result<Difference, String> {
    if (a.width, a.height) != (b.width, b.height) {
        return Err(format!(
            "the images are different sizes: {}x{} against {}x{}",
            a.width, a.height, b.width, b.height
        ));
    }
    let mut differing = 0;
    let mut max_delta = 0u8;
    for (pa, pb) in a
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0)
    {
        let delta = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        if delta > 0 {
            differing += 1;
            max_delta = max_delta.max(delta);
        }
    }
    Ok(Difference {
        differing,
        max_delta,
    })
}

/// Whether a difference is within the stated bounds: no channel further than `tolerance`, and no more
/// than `budget` pixels differing at all.
///
/// **Both bounds are needed**, and this is the whole argument for the pair. `budget` alone lets a change
/// to a couple of hundred pixels of *text* through; `tolerance` alone lets a whole panel shift by a
/// rounding step. Together they say "the picture is the same picture, rendered by a slightly different
/// machine" — and nothing more.
pub fn within(diff: Difference, tolerance: u8, budget: usize) -> Result<(), String> {
    if diff.max_delta > tolerance {
        return Err(format!(
            "a channel differs by {} (tolerance {tolerance}): {} pixels differ in total",
            diff.max_delta, diff.differing
        ));
    }
    if diff.differing > budget {
        return Err(format!(
            "{} pixels differ (budget {budget}), the largest difference {} /255",
            diff.differing, diff.max_delta
        ));
    }
    Ok(())
}

/// The whole check for one scene: its pixels, then its structure.
pub fn check(scene: &str, png: &Path) -> Result<Vec<String>, Vec<String>> {
    let image = match Image::read(png) {
        Ok(image) => image,
        Err(error) => return Err(vec![error]),
    };
    let mut lines = assert_scene(&image, scene)?;
    lines.extend(assert_structure(scene)?);
    Ok(lines)
}

/// The camera is shared with the scene it asserts on; exposed so a test can render at the same place.
pub fn camera() -> Camera {
    scene::CAMERA
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture with a dot of `colour` at `(x, y)`, `radius` pixels across.
    fn dot(width: u32, height: u32, x: f64, y: f64, colour: [u8; 3]) -> Image {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for dy in -3i32..=3 {
            for dx in -3i32..=3 {
                let px = x.round() as i32 + dx;
                let py = y.round() as i32 + dy;
                if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height {
                    continue;
                }
                let index = ((py as u32 * width + px as u32) * 4) as usize;
                pixels[index] = colour[0];
                pixels[index + 1] = colour[1];
                pixels[index + 2] = colour[2];
                pixels[index + 3] = 255;
            }
        }
        Image {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn a_marker_that_moves_ten_pixels_fails_and_the_message_has_the_number() {
        // The ticket's criterion, at the level the assertion actually works at: the expected place
        // is fixed, the ink moves, and the failure names the distance it was looking for.
        let accent = ink(tokens::accent());
        let here = dot(200, 200, 100.0, 100.0, accent);
        assert!(expect_marker(&here, "test", (100.0, 100.0), accent).is_ok());
        assert!(
            expect_marker(&here, "test", (105.0, 100.0), accent).is_ok(),
            "5 px is inside the tolerance, which is what the panel's chrome costs"
        );

        let moved = dot(200, 200, 110.0, 100.0, accent);
        let failure = expect_marker(&moved, "test", (100.0, 100.0), accent)
            .expect_err("a marker 10 px away must fail");
        assert!(
            failure.contains("(100, 100)"),
            "the message names the expected place: {failure}"
        );
        assert!(
            failure.contains("8 px is the limit"),
            "the message names the tolerance: {failure}"
        );
    }

    #[test]
    fn an_empty_scene_with_a_marker_in_it_is_a_failure_not_a_pass() {
        let accent = ink(tokens::accent());
        let image = dot(320, 200, 100.0, 100.0, accent);
        let failures = assert_scene(&image, "empty").expect_err("a marker in an empty scene");
        assert!(failures[0].contains("accent pixels"), "{failures:?}");
    }

    #[test]
    fn the_stale_marker_is_judged_by_its_ink_and_not_by_its_presence() {
        // A dim dot beside a bright one: 40 luma apart, which is what the two tokens are.
        let dim = ink(tokens::ink_dim());
        let bright = ink(tokens::accent());
        let mut image = dot(400, 400, 100.0, 100.0, dim);
        image = blend(image, dot(400, 400, 300.0, 100.0, bright));
        assert_ne!(
            image.brightest_luma(100.0, 100.0, 6),
            image.brightest_luma(300.0, 100.0, 6)
        );
        assert!(image.brightest_luma(100.0, 100.0, 6) < image.brightest_luma(300.0, 100.0, 6));
    }

    /// One image's non-black pixels over another's.
    fn blend(base: Image, over: Image) -> Image {
        let pixels = base
            .pixels
            .iter()
            .zip(over.pixels.iter())
            .map(|(a, b)| if *b > 0 { *b } else { *a })
            .collect();
        Image { pixels, ..base }
    }

    #[test]
    fn the_structure_check_counts_markers_and_cards_from_the_state() {
        // INV-11 as a structural claim: 39 hulls have a fix, so 39 markers, and one card at most.
        let lines = assert_structure("picture").expect("the picture scene is structurally sound");
        assert!(
            lines.iter().any(|line| line.contains("39 markers")),
            "INV-11: 39 hulls have a fix, so 39 markers: {lines:?}"
        );
        let lines = assert_structure("card-open").expect("the card scene is structurally sound");
        assert!(
            lines.iter().any(|line| line.contains("exactly one card")),
            "{lines:?}"
        );
        let lines = assert_structure("empty").expect("an empty scene has no markers");
        assert!(
            lines.iter().any(|line| line.contains("no markers")),
            "{lines:?}"
        );
    }

    #[cfg(test)]
    mod difference_tests {
        use super::*;

        fn image(width: u32, height: u32, fill: [u8; 4]) -> Image {
            Image::synthetic(width, height, fill)
        }

        #[test]
        fn two_renders_of_one_scene_have_no_difference_at_all() {
            let a = image(4, 4, [10, 20, 30, 255]);
            let b = image(4, 4, [10, 20, 30, 255]);
            let diff = difference(&a, &b).expect("same size");
            assert_eq!(diff, Difference::NONE);
            assert!(diff.is_identical());
            assert!(
                within(diff, 0, 0).is_ok(),
                "identical passes even with no room"
            );
        }

        /// The case that used to make CI skip the goldens: a rasteriser's rounding.
        #[test]
        fn a_rounding_difference_passes_the_bounds_and_fails_a_byte_compare() {
            let mut a = image(100, 100, [10, 20, 30, 255]);
            // 23 pixels off by one step, the worst measured between lavapipe and the integrated GPU.
            for i in 0..23 {
                a.set(i % 100, i / 100, [11, 20, 30, 255]);
            }
            let b = image(100, 100, [10, 20, 30, 255]);
            let diff = difference(&a, &b).expect("same size");
            assert_eq!(
                diff,
                Difference {
                    differing: 23,
                    max_delta: 1
                }
            );
            assert!(within(diff, 2, 200).is_ok(), "within the stated bounds");
            assert!(
                within(diff, 0, 0).is_err(),
                "and not byte-identical, which is what `ARCONS_GOLDENS=exact` asks for"
            );
        }

        /// The change a tolerance must still catch: one pixel, far enough off to be a different colour.
        #[test]
        fn one_pixel_of_a_different_colour_fails_however_few_pixels_are_involved() {
            let mut a = image(100, 100, [10, 20, 30, 255]);
            a.set(50, 50, [10, 20, 200, 255]);
            let b = image(100, 100, [10, 20, 30, 255]);
            let diff = difference(&a, &b).expect("same size");
            assert_eq!(diff.differing, 1);
            assert_eq!(diff.max_delta, 170);
            let reason = within(diff, 2, 200).expect_err("a channel moved 170 steps");
            assert!(reason.contains("170"), "{reason}");
        }

        /// The other bound: a large area one step off. A `tolerance` alone would wave it through, which is
        /// why both numbers exist.
        #[test]
        fn a_whole_panel_one_step_off_fails_the_budget() {
            let mut a = image(100, 100, [10, 20, 30, 255]);
            for y in 0..40 {
                for x in 0..40 {
                    a.set(x, y, [11, 20, 30, 255]);
                }
            }
            let b = image(100, 100, [10, 20, 30, 255]);
            let diff = difference(&a, &b).expect("same size");
            assert_eq!(diff.max_delta, 1);
            assert_eq!(diff.differing, 1600);
            let reason = within(diff, 2, 200).expect_err("1600 pixels is more than the budget");
            assert!(reason.contains("1600"), "{reason}");
            assert!(
                within(diff, 2, 2000).is_ok(),
                "and a budget of 2000 would allow it"
            );
        }

        /// Different sizes are not a big difference: they are not the same picture, and a golden of the
        /// wrong shape must say so rather than report a number.
        #[test]
        fn a_different_size_is_an_error_not_a_difference() {
            let a = image(4, 4, [0, 0, 0, 255]);
            let b = image(5, 4, [0, 0, 0, 255]);
            let error = difference(&a, &b).expect_err("sizes differ");
            assert!(error.contains("4x4") && error.contains("5x4"), "{error}");
        }

        /// Alpha counts: a pixel that is transparent where the golden is opaque is a different pixel, even
        /// if every colour channel matches.
        #[test]
        fn alpha_is_compared_too() {
            let mut a = image(2, 2, [0, 0, 0, 255]);
            a.set(0, 0, [0, 0, 0, 0]);
            let b = image(2, 2, [0, 0, 0, 255]);
            assert_eq!(difference(&a, &b).expect("same size").differing, 1);
        }
    }
}
