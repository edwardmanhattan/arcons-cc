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

/// Whether any pixel inside `rect` is `colour` within `tolerance`.
///
/// The plate probe is a rectangle rather than a point because a rounded plate's fill is antialiased at
/// its corner, and a point probe written against the token would fail on the rasteriser rather than on
/// the drawing. A rectangle that is **entirely** fill when a plate is drawn, and entirely ground when
/// none is, is the question `INV-01` asks.
fn rect_has(image: &Image, rect: crate::ui::Rect, colour: [u8; 3], tolerance: u8) -> bool {
    let x0 = rect.x.max(0.0) as u32;
    let y0 = rect.y.max(0.0) as u32;
    let x1 = (rect.x + rect.w).min(image.width as f32) as u32;
    let y1 = (rect.y + rect.h).min(image.height as f32) as u32;
    (y0..y1).any(|y| (x0..x1).any(|x| near(image.at(x, y), colour, tolerance)))
}

/// The bounding box of a **solid** block of `fill` inside the picture panel `within` (x, y, w, h).
///
/// A pixel counts only when it and its four neighbours are `fill`, so one antialiased pixel on the
/// picture's own border cannot stretch the box across the wall. The returned box is put back the pixel
/// the erosion took off each side, so it is the surface's own rectangle and can be compared with a token.
///
/// Three assertions ask this question — the settings sheet, and the filter panel in two scenes — and a
/// box measured three different ways is three chances to disagree with what the view drew.
fn solid_box(
    image: &Image,
    within: (f64, f64, f64, f64),
    fill: [u8; 3],
    tolerance: u8,
) -> Option<(f64, f64, f64, f64)> {
    let (x0, y0, w, h) = within;
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
                        fill,
                        tolerance,
                    )
                })
    };
    let (mut sx, mut sy, mut ex, mut ey) = (f64::MAX, f64::MAX, 0.0f64, 0.0f64);
    let mut found = false;
    for y in (y0 as u32 + 2)..((y0 + h) as u32 - 2).min(image.height) {
        for x in (x0 as u32 + 2)..((x0 + w) as u32 - 2).min(image.width) {
            if solid(x, y) {
                found = true;
                sx = sx.min(f64::from(x));
                sy = sy.min(f64::from(y));
                ex = ex.max(f64::from(x));
                ey = ey.max(f64::from(y));
            }
        }
    }
    found.then(|| {
        // Put back the pixel the erosion took off each side.
        let (sx, sy) = (sx - 1.0, sy - 1.0);
        (sx, sy, ex - sx + 2.0, ey - sy + 2.0)
    })
}

/// Count the pixels inside `rect` that are close to `colour`. A rectangle's answer where the rest of
/// this module asks a whole image's: a newsroom pane is a region, and the accent ink a marker would
/// leave is only a failure **inside** it.
fn count_colour_in(
    image: &Image,
    rect: (f64, f64, f64, f64),
    colour: [u8; 3],
    tolerance: u8,
) -> usize {
    let (x0, y0, w, h) = rect;
    let mut count = 0;
    for y in (y0.max(0.0) as u32)..((y0 + h) as u32).min(image.height) {
        for x in (x0.max(0.0) as u32)..((x0 + w) as u32).min(image.width) {
            if near(image.at(x, y), colour, tolerance) {
                count += 1;
            }
        }
    }
    count
}

/// The bands of rows inside `region` that carry ink close to one of `colours`, merged when they touch:
/// **one band per line of text**. A band shorter than three pixels is an edge or a rule, not a line.
///
/// This is how the newsroom's visible rows are counted without a text recogniser: one title is one band
/// of `ink()`, and the assertion's number is the number a reader can count in the picture.
fn ink_bands(
    image: &Image,
    region: (f64, f64, f64, f64),
    colours: &[[u8; 3]],
    tolerance: u8,
) -> Vec<(u32, u32)> {
    let (x0, y0, w, h) = region;
    let first_y = y0.max(0.0) as u32;
    let last_y = ((y0 + h) as u32).min(image.height);
    let first_x = x0.max(0.0) as u32;
    let last_x = ((x0 + w) as u32).min(image.width);
    let mut bands: Vec<(u32, u32)> = Vec::new();
    let mut run: Option<(u32, u32)> = None;
    for y in first_y..last_y {
        let inked = (first_x..last_x).any(|x| {
            colours
                .iter()
                .any(|colour| near(image.at(x, y), *colour, tolerance))
        });
        if inked {
            run = Some((run.map_or(y, |(start, _)| start), y));
        } else if let Some((start, last)) = run.take() {
            bands.push((start, last));
        }
    }
    if let Some((start, last)) = run.take() {
        bands.push((start, last));
    }
    bands
        .into_iter()
        .filter(|(start, last)| last - start >= 2)
        .collect()
}

/// How many **words** run across `x` within the band `y`: runs of inked columns merged across gaps
/// smaller than `gap`. Letters inside a word are a pixel or two apart and two controls are a padding
/// apart, so one number separates the three tab labels without recognising a glyph — and the band keeps
/// the count from depending on which row of a glyph the sample happens to cross.
fn word_runs(
    image: &Image,
    x: (u32, u32),
    band: (u32, u32),
    colours: &[[u8; 3]],
    tolerance: u8,
    gap: u32,
) -> usize {
    let first_y = band.0.min(image.height);
    let last_y = band.1.min(image.height);
    let mut runs = 0;
    let mut last_ink: Option<u32> = None;
    for column in x.0..x.1.min(image.width) {
        let inked = (first_y..last_y).any(|y| {
            colours
                .iter()
                .any(|colour| near(image.at(column, y), *colour, tolerance))
        });
        if inked {
            if last_ink.is_some_and(|last| column - last > gap) {
                runs += 1;
            }
            last_ink = Some(column);
        }
    }
    if last_ink.is_some() {
        runs += 1;
    }
    runs
}

/// The x runs of `colour` inside the y band, merged across gaps smaller than `merge` pixels: one run per
/// filled control. `image.find_panel_rect` answers for a picture panel; a header chip is a run.
fn colour_runs_in_band(
    image: &Image,
    band: (f64, f64),
    colour: [u8; 3],
    tolerance: u8,
    merge: u32,
) -> Vec<(f64, f64)> {
    let y0 = band.0.max(0.0) as u32;
    let y1 = (band.1.max(0.0) as u32).min(image.height);
    let mut runs: Vec<(f64, f64)> = Vec::new();
    let mut start: Option<u32> = None;
    let mut last: u32 = 0;
    for x in 0..image.width {
        let hit = (y0..y1).any(|y| near(image.at(x, y), colour, tolerance));
        if hit {
            match start {
                Some(_) if x - last <= merge => {}
                Some(from) => {
                    runs.push((f64::from(from), f64::from(last)));
                    start = Some(x);
                }
                None => start = Some(x),
            }
            last = x;
        }
    }
    if let Some(from) = start {
        runs.push((f64::from(from), f64::from(last)));
    }
    runs
}

/// The rightmost column **up to `limit`** in the band that carries any of `colours`: the last control in a
/// row that starts at the left edge. The limit keeps a right-aligned run of words — the status and the
/// header's controls — out of a question about the chips.
fn rightmost_ink(
    image: &Image,
    band: (f64, f64),
    limit: u32,
    colours: &[[u8; 3]],
    tolerance: u8,
) -> Option<f64> {
    let y0 = band.0.max(0.0) as u32;
    let y1 = (band.1.max(0.0) as u32).min(image.height);
    (0..limit.min(image.width))
        .rev()
        .find(|x| {
            (y0..y1).any(|y| {
                colours
                    .iter()
                    .any(|colour| near(image.at(*x, y), *colour, tolerance))
            })
        })
        .map(f64::from)
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
        // **The menu's keyboard model** (#164): the same wall as `panes-menu`, with the ring on the row Enter would
        // act on. The ring is `focus_edge` — the colour the wall uses for "this one has the keys".
        //
        // The strip is the ringed row's own: a menu box also holds the focused pane's focus edge, which crosses it,
        // so the question is asked of one row and the answer is a *count* — a border's worth of ink, or a line's.
        // **A reset from a remote** (#166): one chip in the header, and one pane in the picture. The chip is a
        // *fill* at least as tall as a chip (`name_plate`), which is what makes it the raised one.
        "close-all" => {
            let header = (0.0, f64::from(tokens::CHIP_H) + f64::from(tokens::PAD));
            let raised: Vec<(f64, f64)> =
                colour_runs_in_band(image, header, ink(tokens::name_plate()), 10, 2)
                    .into_iter()
                    .filter(|(start, end)| end - start >= f64::from(tokens::CHIP_H))
                    .collect();
            if raised.len() != 1 {
                failures.push(format!(
                    "close-all: {} raised chips in the header, and `workspace.close_others` leaves one",
                    raised.len()
                ));
            } else {
                lines.push(
                    "close-all: one raised chip, the workspace the wall stayed on".to_string(),
                );
            }
            // One pane: the panel has no gutter, so its whole width is ink-free water between the name plates.
            let Some((px, py, pw, _ph)) = image.find_panel_rect(ink(tokens::panel())) else {
                failures.push("close-all: no picture panel on screen".to_string());
                return Err(failures);
            };
            let plates = count_colour_in(
                image,
                (px, py, pw, f64::from(tokens::NAME_H) * 2.0),
                ink(tokens::name_plate()),
                10,
            );
            if plates == 0 {
                failures.push("close-all: no pane's name plate in the picture".to_string());
            } else {
                lines.push(format!("close-all: one pane's plate, {plates} px"));
            }
            if failures.is_empty() {
                lines.push("close-all: one chip, one pane".to_string());
            }
        }
        "menu-ring" => {
            let x = 1044.0;
            let y = 188.0 + f64::from(tokens::MENU_ROW) * crate::scene::MENU_RING_ROW as f64;
            let strip = (
                x,
                y,
                f64::from(tokens::MENU_WIDTH) - 8.0,
                f64::from(tokens::MENU_ROW),
            );
            let ring = count_colour_in(image, strip, ink(tokens::focus_edge()), 24);
            // A row's ring is its whole border: two widths plus two heights, about 460 px at this size. A pane's
            // focus edge crossing the strip is one line, about 22. Two hundred is between them and nearer the ring.
            const RING: usize = 200;
            if scene == "menu-ring" {
                if ring < RING {
                    failures.push(format!(
                        "menu-ring: {ring} px of ring ink on row {} — the keyboard's row is not drawn, and \
                         Enter would act on a row nobody can see",
                        crate::scene::MENU_RING_ROW
                    ));
                } else {
                    lines.push(format!(
                        "menu-ring: {ring} px of ring on the keyboard's row"
                    ));
                }
            } else if ring >= RING {
                failures.push(format!(
                    "panes-menu: {ring} px of ring ink on the row — a menu opened by a press has no row with \
                     the keyboard's keys, and a ring nobody put there is a second answer to \"which row\""
                ));
            } else {
                lines.push("panes-menu: no ring, as a pointer-opened menu should be".to_string());
            }
        }
        // **The client's cursor** (#167): the same wall twice, one line apart in the state and far apart in the
        // picture — the card its press opened in both, and the pointer in exactly one.
        //
        // The claim is a colour rather than a position: the cursor's ring is `focus_edge`, and nothing else draws
        // that colour **inside the picture panel** on a one-pane wall. So the arm asks the question a reader asks —
        // is the pointer there? — without re-deriving the renderer's coordinate space.
        "cursor-hull" | "cursor-idle" => {
            let Some(origin) = image.find_panel(ink(tokens::panel())) else {
                failures.push(format!(
                    "{scene}: no picture panel on screen — the cursor is drawn in it"
                ));
                return Err(failures);
            };
            let below = image
                .find_panel(ink(tokens::panel()))
                .map(|(_, y)| y + 2.0)
                .unwrap_or(0.0) as u32;
            let ring = image.count_colour_below(ink(tokens::cursor_ink()), 12, below);
            let state = scene::state_for(scene);
            let Some(cursor) = state.cursor else {
                failures.push(format!("{scene}: the scene placed no cursor"));
                return Err(failures);
            };
            let visible = cursor.visible_at(2.0);
            if scene == "cursor-hull" {
                if ring == 0 {
                    failures.push(
                        "cursor-hull: no cursor ring inside the picture — the scene's press landed \
                         a moment before the capture, so a cursor that is not drawn is a glyph that is \
                         not in the tree"
                            .to_string(),
                    );
                } else if !visible {
                    failures.push(
                        "cursor-hull: the state says the cursor is faded at the capture, and the picture \
                         draws it — the fade is not reading the frame clock"
                            .to_string(),
                    );
                } else {
                    lines.push(format!(
                        "cursor-hull: cursor ring in the picture, {ring} px of its own ink"
                    ));
                }
            } else if ring > 0 {
                failures.push(format!(
                    "cursor-idle: {ring} px of cursor ink in the picture — the cursor moved {:.2} s \
                     before the capture and Cursor::FADE is {:.1} s",
                    2.0 - cursor.moved_at,
                    crate::ui::Cursor::FADE
                ));
            } else {
                lines.push("cursor-idle: no cursor ink in the picture".to_string());
            }
            // The card is there in both: the point of the pair is that only the pointer goes.
            lines.extend(assert_card(image, origin, &mut failures));
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
        "objects" => {
            // **The operational layer, projected against each pane's own frame** (#110, `INV-04`).
            // Two panes at two zooms on the same water: the rectangles come from `Layout::rects`
            // measured against the panel this capture actually has, exactly as `panes` does, so the
            // view and the assertion cannot disagree about where a pane is. A layer drawn from the
            // wall's camera agrees with the focused pane and is tens of pixels out in its neighbour,
            // which is the falsifier the ticket names.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("objects: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("objects");
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            if rects.len() != 2 {
                failures.push(format!(
                    "objects: {} panes, and the capture is the case for two — a wall-wide camera \
                     cannot be told from a pane's with one",
                    rects.len()
                ));
                return Err(failures);
            }
            // The three marks the scene places, and the ink each one's decoded artwork carries.
            let expected = [
                (scene::OBJECTS_BUOY, scene::OBJECT_BUOY_INK, "buoy"),
                (scene::OBJECTS_CONE, scene::OBJECT_CONE_INK, "cone"),
                (
                    scene::OBJECTS_UNLABELLED,
                    scene::OBJECT_LIGHT_INK,
                    "unlabelled light",
                ),
            ];
            let plate = ink(tokens::name_plate());
            for (index, (id, rect)) in rects.iter().enumerate() {
                let Some(pane) = state.pane(*id) else {
                    failures.push(format!("objects: pane {id} has no state"));
                    continue;
                };
                // The frame a headless pane draws with: its own rectangle and its own camera, the
                // rule `pane_laid_out` applies on the wall's behalf.
                let frame = crate::ui::view::synthetic_frame(pane.camera, *rect, 1.0);
                let placed = crate::ui::objects::place(
                    &state.placements,
                    &state.manifest,
                    &frame,
                    &pane.view.filter,
                );
                if placed.len() != expected.len() {
                    failures.push(format!(
                        "objects: pane {index} draws {} placements, and the scene has {}",
                        placed.len(),
                        expected.len()
                    ));
                    continue;
                }
                for (placement, colour, what) in expected {
                    let Some(mark) = placed.iter().find(|mark| mark.id == placement) else {
                        failures.push(format!(
                            "objects: pane {index} has no placement {placement} ({what})"
                        ));
                        continue;
                    };
                    let x = origin_x + f64::from(rect.x) + mark.x;
                    let y = origin_y + f64::from(rect.y) + mark.y;
                    match image.nearest_colour(x, y, colour, 6.0) {
                        Some(distance) => lines.push(format!(
                            "objects: pane {index}'s {what} within {distance:.1} px of \
                             ({x:.0}, {y:.0})"
                        )),
                        None => failures.push(format!(
                            "objects: no artwork ink within 6 px of ({x:.0}, {y:.0}) — pane \
                             {index}'s projection of the {what}. A layer drawn against the wall's \
                             camera lands elsewhere (INV-04), and a missing asset draws the generic \
                             mark instead"
                        )),
                    }
                    // **A label is optional, and an empty label is not a label** (`INV-01`): the
                    // unlabelled placement must have no plate where the others have one. The probe
                    // is the plate's own left padding, so an empty plate is caught rather than
                    // passing because a surface was drawn around nothing.
                    let probe = crate::ui::objects::label_probe((x, y));
                    let has_plate = rect_has(image, probe, plate, 4);
                    if mark.id == scene::OBJECTS_UNLABELLED {
                        if has_plate {
                            failures.push(format!(
                                "objects: the unlabelled placement draws a label plate at \
                                 ({:.0}, {:.0}) — an empty label is not a label (INV-01)",
                                probe.x, probe.y
                            ));
                        }
                    } else if !has_plate {
                        failures.push(format!(
                            "objects: the {what}'s label has no plate at ({:.0}, {:.0}) — a label \
                             somebody wrote is drawn beside its mark",
                            probe.x, probe.y
                        ));
                    }
                }
            }
            lines.push(format!(
                "objects: {} placements in two panes, the unlabelled one with no plate",
                expected.len()
            ));
        }
        "installations" => {
            // **The places, not the hulls** (#109). Two panes on the same water at two zooms, so a mark
            // drawn from the wall's camera instead of its pane's is tens of pixels out in the
            // neighbour; one mark per **located** facility, in the facility's own ink (not a hull's
            // accent, not an object's diamond); the selected facility ringed in the focused pane only.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("installations: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("installations");
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            if rects.len() != 2 {
                failures.push(format!(
                    "installations: {} panes for a two-pane scene",
                    rects.len()
                ));
                return Err(failures);
            }
            let located: Vec<crate::data::types::Installation> = state
                .installations
                .iter()
                .filter(|row| row.coordinates().is_some())
                .cloned()
                .collect();
            let unlocated = state.installations.len() - located.len();
            let facility = ink(tokens::facility_ink());
            let selected = state.installation_in(state.focused());
            for (index, (id, rect)) in rects.iter().enumerate() {
                let Some(pane) = state.pane(*id) else {
                    failures.push(format!("installations: pane {id} has no state"));
                    continue;
                };
                let frame = crate::ui::view::synthetic_frame(pane.camera, *rect, 1.0);
                let placed = crate::ui::installations::place(
                    &state.installations,
                    &state.installation_types,
                    &state.manifest,
                    &frame,
                    &pane.view.filter,
                );
                if placed.len() != located.len() {
                    failures.push(format!(
                        "installations: pane {index} draws {} marks for {} located facilities (and \
                         {unlocated} with no position, which is no mark)",
                        placed.len(),
                        located.len()
                    ));
                    continue;
                }
                for mark in &placed {
                    let x = origin_x + f64::from(rect.x) + mark.x;
                    let y = origin_y + f64::from(rect.y) + mark.y;
                    match image.nearest_colour(x, y, facility, 6.0) {
                        Some(distance) => lines.push(format!(
                            "installations: pane {index}'s {} within {distance:.1} px of its \
                             projection",
                            mark.name
                        )),
                        None => failures.push(format!(
                            "installations: no facility ink within 6 px of ({x:.0}, {y:.0}) for {} — \
                             a facility drawn as a hull symbol, from the wall's camera (INV-04), or not \
                             at all",
                            mark.name
                        )),
                    }
                }
                // **The selection is the pane's** (#109): the focused pane rings its facility, and the
                // neighbour rings nothing. A wall-wide ring would be #99's bug on a third card.
                let ring = ink(tokens::focus_edge());
                let focused = state.focused() == *id;
                for mark in &placed {
                    let x = origin_x + f64::from(rect.x) + mark.x;
                    let y = origin_y + f64::from(rect.y) + mark.y;
                    let edge = x - f64::from(crate::ui::installations::MARK) / 2.0 + 1.0;
                    let ringed = image.nearest_colour(edge, y, ring, 8.0).is_some();
                    if focused && selected == Some(mark.id) {
                        if !ringed {
                            failures.push(format!(
                                "installations: the selected facility {} is not ringed at \
                                 ({edge:.0}, {y:.0}) in pane {index}",
                                mark.name
                            ));
                        }
                    } else if !focused && ringed {
                        failures.push(format!(
                            "installations: pane {index} rings {} and nothing there is selected — \
                             the selection is the pane's",
                            mark.name
                        ));
                    }
                }
            }
            // **The card** (#109): the selected facility's name, in the card's own top-left, where
            // `card_placement` puts it. In this scene that region is water when no card is drawn.
            if let Some(selected) = selected
                && let Some((id, rect)) = rects.first()
                && let Some(mark) = crate::ui::installations::place(
                    &state.installations,
                    &state.installation_types,
                    &state.manifest,
                    &crate::ui::view::synthetic_frame(
                        state.pane(*id).expect("the focused pane").camera,
                        *rect,
                        1.0,
                    ),
                    &state.pane(*id).expect("the focused pane").view.filter,
                )
                .into_iter()
                .find(|mark| mark.id == selected)
            {
                let placement = crate::ui::installations::card_placement(
                    (mark.x, mark.y),
                    (f64::from(rect.w), f64::from(rect.h)),
                );
                let probe = crate::ui::Rect {
                    x: origin_x as f32 + rect.x + placement.x as f32 + tokens::PAD,
                    y: origin_y as f32 + rect.y + placement.y as f32 + tokens::PAD,
                    w: crate::units::card::WIDTH as f32 - tokens::PAD * 2.0,
                    h: tokens::TITLE.size + tokens::PAD,
                };
                if !rect_has(image, probe, ink(tokens::ink()), 40) {
                    failures.push(format!(
                        "installations: no name ink in the facility card at ({:.0}, {:.0}) — a \
                         selected facility draws what the row carries",
                        probe.x, probe.y
                    ));
                } else {
                    lines.push(
                        "installations: the selected facility's card draws its name".to_string(),
                    );
                }
            }
            lines.push(format!(
                "installations: {} located facilities marked in two panes, {unlocated} unlocated and \
                 unmarked, {} selected in one",
                located.len(),
                usize::from(selected.is_some())
            ));
        }
        "panes-filter" => {
            // **The filter is the pane's** (#112). Two panes hold the same camera, so the *only*
            // difference their pictures may show is the filter: pane A hides the buoy's domain and pane
            // B hides nothing. A wall-wide filter draws the three marks in both panes — which is the
            // falsifier this capture exists for, and it is the same bug #99 fixed for the selection.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("panes-filter: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("panes-filter");
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            if rects.len() != 2 {
                failures.push(format!(
                    "panes-filter: {} panes for a two-pane scene",
                    rects.len()
                ));
                return Err(failures);
            }
            let filtered = state.pane_rects()[0].0;
            if state.filtering() != Some(filtered) {
                failures.push(format!(
                    "panes-filter: the panel is open on {:?}, not on the pane that was filtered \
                     ({filtered}) — the panel and the filter must be the same pane's",
                    state.filtering()
                ));
            }
            // The three marks the scene places, and the ink each one's decoded artwork carries.
            let expected = [
                (scene::OBJECTS_BUOY, scene::OBJECT_BUOY_INK, "buoy"),
                (scene::OBJECTS_CONE, scene::OBJECT_CONE_INK, "cone"),
                (
                    scene::OBJECTS_UNLABELLED,
                    scene::OBJECT_LIGHT_INK,
                    "unlabelled light",
                ),
            ];
            let accent = ink(tokens::accent());
            for (index, (id, rect)) in rects.iter().enumerate() {
                let Some(pane) = state.pane(*id) else {
                    failures.push(format!("panes-filter: pane {id} has no state"));
                    continue;
                };
                let hid = *id == filtered;
                let frame = crate::ui::view::synthetic_frame(pane.camera, *rect, 1.0);
                let placed = crate::ui::objects::place(
                    &state.placements,
                    &state.manifest,
                    &frame,
                    &pane.view.filter,
                );
                if hid {
                    if !placed.is_empty() {
                        failures.push(format!(
                            "panes-filter: pane {index} hides the buoy's domain and still draws {} \
                             placements — the filter is the wall's, not this pane's (#99's bug in a \
                             new place)",
                            placed.len()
                        ));
                    }
                } else if placed.len() != expected.len() {
                    failures.push(format!(
                        "panes-filter: pane {index} draws {} placements for {} that are not filtered",
                        placed.len(),
                        expected.len()
                    ));
                }
                for (placement_id, colour, what) in expected {
                    // The point is the pane's projection of the placement, whether or not the filter
                    // left it in the layer: the absence is checked at the place the mark *would* be.
                    let point = match placed.iter().find(|mark| mark.id == placement_id) {
                        Some(mark) => (mark.x, mark.y),
                        None => state
                            .placements
                            .iter()
                            .find(|placement| placement.id == placement_id)
                            .map(|placement| frame.project(placement.latitude, placement.longitude))
                            .unwrap_or((f64::NAN, f64::NAN)),
                    };
                    let x = origin_x + f64::from(rect.x) + point.0;
                    let y = origin_y + f64::from(rect.y) + point.1;
                    let found = image.nearest_colour(x, y, colour, 6.0);
                    if hid {
                        if let Some(distance) = found {
                            failures.push(format!(
                                "panes-filter: the {what}'s ink is within {distance:.1} px of \
                                 ({x:.0}, {y:.0}) in the pane that hides its domain — a filtered \
                                 layer that still draws is a toggle that lies"
                            ));
                        }
                    } else if found.is_none() {
                        failures.push(format!(
                            "panes-filter: no {what} ink within 6 px of ({x:.0}, {y:.0}) in the pane \
                             that hides nothing — a filter must hide one pane's marks and not the \
                             other's"
                        ));
                    }
                }
                // **Hulls in both.** The filter hid one domain, not the live feed: a wall that emptied
                // because one vocabulary row was toggled is the same bug in the other direction.
                let sizes = |id_unit: i64| state.sizes.get(&id_unit).copied();
                let markers = crate::ui::markers::place(
                    &state.picture,
                    &frame,
                    &pane.view.filter,
                    state.now(),
                    state.stale_after,
                    pane.view.range,
                    &sizes,
                );
                let expected_hulls = state
                    .picture
                    .units()
                    .iter()
                    .filter(|unit| unit.position.is_some())
                    .count();
                if markers.len() != expected_hulls {
                    failures.push(format!(
                        "panes-filter: pane {index} draws {} hulls for {expected_hulls} that have a fix",
                        markers.len()
                    ));
                }
                // One live marker, sampled for the accent its glyph is drawn in, clear of the pane's
                // edge and as far from the panel's corner as a hull happens to be.
                let visible = |marker: &&crate::ui::markers::Placed| {
                    marker.glyph == crate::units::Glyph::Live
                        && marker.x > 20.0
                        && marker.y > 40.0
                        && marker.x < f64::from(frame.width) - 20.0
                        && marker.y < f64::from(frame.height) - 20.0
                };
                match markers.iter().filter(visible).min_by(|a, b| {
                    a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    Some(marker) => {
                        let x = origin_x + f64::from(rect.x) + marker.x;
                        let y = origin_y + f64::from(rect.y) + marker.y;
                        match image.nearest_colour(x, y, accent, 12.0) {
                            Some(distance) => lines.push(format!(
                                "panes-filter: pane {index} draws the live feed ({distance:.1} px from \
                                 hull {}'s projection)",
                                marker.id_unit
                            )),
                            None => failures.push(format!(
                                "panes-filter: no live marker ink within 12 px of ({x:.0}, {y:.0}) in \
                                 pane {index} — the filter hid a domain, not the feed"
                            )),
                        }
                    }
                    None => failures.push(format!(
                        "panes-filter: pane {index} has no live hull on screen to prove the feed is drawn"
                    )),
                }
            }
            // The panel is the rows the vocabulary says, in the box the token set gives them. This scene
            // read both vocabularies, so the box is at its maximum and the panel scrolls inside itself.
            // The panel belongs to the pane the filter toggle was pressed on, which this scene focuses.
            let listed = {
                let pane = state.focused_pane();
                crate::ui::filter::rows(
                    &pane.view.filter,
                    state.facets.as_ref(),
                    &state.installation_types,
                    &state.installations,
                )
            };
            let (want_w, want_h) = tokens::filter_size(listed.len());
            match solid_box(
                image,
                (origin_x, origin_y, panel_w, panel_h),
                ink(tokens::bg()),
                6,
            ) {
                None => failures.push(
                    "panes-filter: no filter panel on screen — the scene's panel is not drawn"
                        .to_string(),
                ),
                Some((sx, sy, sw, sh)) => {
                    if (sw - f64::from(want_w)).abs() > 4.0 || (sh - f64::from(want_h)).abs() > 4.0
                    {
                        failures.push(format!(
                            "panes-filter: the panel is {sw:.0}×{sh:.0} for {} rows, and the token \
                             says {want_w:.0}×{want_h:.0} — a panel that grows past FILTER_MAX_H \
                             covers the water it filters",
                            listed.len()
                        ));
                    } else {
                        lines.push(format!(
                            "panes-filter: one pane hides a domain, the other draws it; the panel \
                             lists {} rows in {sw:.0}×{sh:.0} at ({sx:.0}, {sy:.0})",
                            listed.len()
                        ));
                    }
                }
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
            // **The way out is on the sheet, in accent ink** (#98): the operator asked for a close control,
            // and a control nobody can see is the same defect as no control. Scanned in the sheet's own
            // bottom strip, so the claim is about this sheet's foot and not "there is accent somewhere".
            let accent = ink(tokens::accent());
            let mut close_ink = 0;
            for y in (sy as u32 + sh as u32 - 45)..(sy as u32 + sh as u32 - 4) {
                for x in (sx as u32 + 4)..(sx as u32 + sw as u32 - 4) {
                    if x < image.width && y < image.height && near(image.at(x, y), accent, 60) {
                        close_ink += 1;
                    }
                }
            }
            if close_ink < 40 {
                failures.push(format!(
                    "settings: {close_ink} pixels of accent ink at the foot of the sheet — the Close row \
                     is what the operator asked for and it is not drawn (#98)"
                ));
            } else {
                lines.push(format!(
                    "settings: the Close row is drawn at the sheet's foot ({close_ink} px)"
                ));
            }
        }
        "settings-dismiss" => {
            // **The press outside the sheet closed it**, and the picture is how you can tell: with the
            // scrim up the whole wall is darkened (measured: 6.6 % of the panel's pixels above luma 12),
            // and with it gone the wall is lit again (98 %). A scrim whose press does nothing leaves the
            // sheet and its scrim on screen and this goes red (#98).
            let mut lit = 0usize;
            let mut total = 0usize;
            for y in (60..image.height.saturating_sub(10)).step_by(3) {
                for x in (10..image.width.saturating_sub(10)).step_by(3) {
                    total += 1;
                    if image.luma(x, y) >= 12 {
                        lit += 1;
                    }
                }
            }
            let share = if total == 0 {
                0.0
            } else {
                lit as f64 / total as f64
            };
            if share < 0.5 {
                failures.push(format!(
                    "settings-dismiss: {:.1} % of the wall is lit — a press outside the sheet must close \
                     it, and a wall still under the scrim means the press reached nothing (#98)",
                    share * 100.0
                ));
            } else {
                lines.push(format!(
                    "settings-dismiss: the sheet is gone — {:.1} % of the wall is lit again",
                    share * 100.0
                ));
            }
        }
        "panes-cards" => {
            // **One card, in one pane** (#99) — the operator's screenshot read as a measurement. A card's
            // text is `tokens::ink()`, which nothing else inside a pane draws: markers are accent-cyan
            // glyphs, the pane's own chrome is hairlines and — since #111 — two icon buttons, whose
            // rectangles are excluded by the same `chrome_buttons` that draws them. So each pane is asked
            // how much name-and-value ink it holds, and the answer must be "one pane, not the others".
            // Two is the shared-selection bug; none means the pane that was pressed has no card.
            let Some((px, py, pw, ph)) = image.find_panel_rect(ink(tokens::panel())) else {
                failures.push("panes-cards: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("panes-cards");
            let viewport = crate::map::geometry::Viewport::new(pw, ph);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            let ink_colour = ink(tokens::ink());
            let mut counts: Vec<(crate::ui::ViewId, usize)> = Vec::new();
            for (id, rect) in rects {
                let mut count = 0;
                let (filter, maximise) = crate::ui::view::chrome_buttons(rect);
                let x0 = (px + f64::from(rect.x)).max(0.0) as u32 + 4;
                let y0 = (py + f64::from(rect.y)).max(0.0) as u32 + 4;
                let x1 = ((px + f64::from(rect.x + rect.w)) as u32).min(image.width) - 4;
                let y1 = ((py + f64::from(rect.y + rect.h)) as u32).min(image.height) - 4;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let local = (f64::from(x) - px, f64::from(y) - py);
                        let on_button = |button: crate::ui::Rect| {
                            local.0 >= f64::from(button.x)
                                && local.0 < f64::from(button.x + button.w)
                                && local.1 >= f64::from(button.y)
                                && local.1 < f64::from(button.y + button.h)
                        };
                        if on_button(filter) || on_button(maximise) {
                            continue;
                        }
                        if near(image.at(x, y), ink_colour, 10) {
                            count += 1;
                        }
                    }
                }
                counts.push((id, count));
            }
            let pressed = counts.first().map(|(id, _)| *id);
            let with_card: Vec<_> = counts.iter().filter(|(_, n)| *n > 40).collect();
            match with_card.len() {
                1 if Some(with_card[0].0) == pressed => lines.push(format!(
                    "panes-cards: one card, in pane {} ({} px of card ink; the others {} px)",
                    with_card[0].0,
                    with_card[0].1,
                    counts
                        .iter()
                        .filter(|(id, _)| Some(*id) != pressed)
                        .map(|(_, n)| n.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                1 => failures.push(format!(
                    "panes-cards: the card is in pane {} but the press was on pane {}",
                    with_card[0].0,
                    pressed.unwrap_or_default()
                )),
                0 => failures.push(
                    "panes-cards: no card ink in any pane — the pane that was pressed has no card"
                        .to_string(),
                ),
                other => failures.push(format!(
                    "panes-cards: {other} panes hold a card — a card in every pane is the selection \
                     being the wall's instead of the pane's (#99)"
                )),
            }
        }
        "panes-chrome" => {
            // **Two iconed controls in every pane** (#111), sampled where `view::chrome_buttons` puts
            // them — the same function the picture places the row with. The filter panel is open on the
            // focused pane because `fixtures/scripts/panes-chrome.json` pressed its toggle: a control
            // that draws but reaches no handler leaves the panel absent and this goes red.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("panes-chrome: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("panes-chrome");
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            let icon_ink = ink(tokens::ink());
            for (index, (id, rect)) in rects.iter().enumerate() {
                let (filter, maximise) = crate::ui::view::chrome_buttons(*rect);
                for (name, button) in [("filter", filter), ("maximise", maximise)] {
                    let mut hits = 0;
                    let mut samples = 0;
                    for dy in 0..button.h as i32 {
                        for dx in 0..button.w as i32 {
                            let x = (origin_x + f64::from(button.x) + f64::from(dx)) as u32;
                            let y = (origin_y + f64::from(button.y) + f64::from(dy)) as u32;
                            if x >= image.width || y >= image.height {
                                continue;
                            }
                            samples += 1;
                            if near(image.at(x, y), icon_ink, 60) {
                                hits += 1;
                            }
                        }
                    }
                    if hits < 8 {
                        failures.push(format!(
                            "panes-chrome: pane {index} ({id}) has {hits}/{samples} ink pixels in its \
                             {name} button at ({:.0}, {:.0}) — the {name} control is not drawn (#111)",
                            origin_x + f64::from(button.x),
                            origin_y + f64::from(button.y)
                        ));
                    } else {
                        lines.push(format!(
                            "panes-chrome: pane {index} draws its {name} control ({hits} ink px)"
                        ));
                    }
                }
            }
            // **The filter panel is a popover over the picture, not a modal over it.** Its surface is the
            // window's own `bg()`, inside a picture that is `panel()`; the bounding box of that fill is
            // the panel's box, measured against the token set for the rows it lists. No scrim: the water
            // outside the box is still the picture.
            let boxed = solid_box(
                image,
                (origin_x, origin_y, panel_w, panel_h),
                ink(tokens::bg()),
                6,
            );
            // **The panel's box is the token set's, for the rows it actually lists** (#112): three
            // layer rows on this scene, which reads no vocabulary, and `FILTER_MAX_H` past the limit.
            // The panel belongs to the pane the toggle was pressed on, and every scene that opens one
            // focuses that pane first — including `panes-chrome`, whose press happens in a script and so
            // is not in the state `state_for` builds.
            let listed = {
                let pane = state.focused_pane();
                crate::ui::filter::rows(
                    &pane.view.filter,
                    state.facets.as_ref(),
                    &state.installation_types,
                    &state.installations,
                )
            };
            let (want_w, want_h) = tokens::filter_size(listed.len());
            let (want_w, want_h) = (f64::from(want_w), f64::from(want_h));
            match boxed {
                None => failures.push(
                    "panes-chrome: no filter panel on screen — the toggle's press reached nothing"
                        .to_string(),
                ),
                Some((sx, sy, sw, sh)) => {
                    if (sw - want_w).abs() > 4.0 || (sh - want_h).abs() > 4.0 {
                        failures.push(format!(
                            "panes-chrome: the filter panel is {sw:.0}×{sh:.0} for a \
                             {want_w:.0}×{want_h:.0} token, at ({sx:.0}, {sy:.0})"
                        ));
                    } else {
                        lines.push(format!(
                            "panes-chrome: filter panel {sw:.0}×{sh:.0} at ({sx:.0}, {sy:.0})"
                        ));
                    }
                    // The shell has words in it: a title and its layer rows. A panel of empty fill would
                    // pass the box check and tell the operator nothing.
                    let mut ink_px = 0;
                    for y in (sy as u32 + 6)..(sy + sh) as u32 - 6 {
                        for x in (sx as u32 + 6)..(sx + sw) as u32 - 6 {
                            if x < image.width
                                && y < image.height
                                && near(image.at(x, y), icon_ink, 60)
                            {
                                ink_px += 1;
                            }
                        }
                    }
                    if ink_px < 20 {
                        failures.push(format!(
                            "panes-chrome: the filter panel has {ink_px} px of label ink — its rows are \
                             not drawn"
                        ));
                    } else {
                        lines.push(format!(
                            "panes-chrome: the filter panel's rows are drawn ({ink_px} px of ink)"
                        ));
                    }
                }
            }
        }
        "panes-range" => {
            // **The symbol-to-picture range belongs to the pane** (#115). Two panes look at the same
            // hull: the focused one at `PHOTO_ZOOM`, where a ship is its photograph, and its neighbour at
            // a chart zoom, where it is a symbol. Both panes hold the decoded picture — the cache is the
            // wall's — so this capture is a claim about the *latch*: a wall-wide range draws one
            // representation in both, and one of these two checks goes red.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("panes-range: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("panes-range");
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            if rects.len() != 2 {
                failures.push(format!(
                    "panes-range: {} panes for a two-pane scene",
                    rects.len()
                ));
                return Err(failures);
            }
            let Some(fix) = scene::fixture().ok().and_then(|entries| {
                entries
                    .into_iter()
                    .find(|entry| entry.id_unit == scene::PHOTO_UNIT)
                    .and_then(|entry| entry.position)
            }) else {
                failures.push(format!(
                    "panes-range: hull {} has no fix",
                    scene::PHOTO_UNIT
                ));
                return Err(failures);
            };
            // The photograph's own ink: a colour no token uses, so finding it at the projected hull is
            // proof that this pane drew the decoded bytes rather than a symbol (#54).
            let hud = [232u8, 243, 255];
            let (mut pictures, mut symbols) = (0usize, 0usize);
            for (index, (id, rect)) in rects.iter().enumerate() {
                let Some(pane) = state.pane(*id) else {
                    failures.push(format!("panes-range: pane {id} has no state"));
                    continue;
                };
                // **The latch each pane's own zoom calls for** (#115): picture at or past `PHOTO_ZOOM`,
                // symbol below it — and this scene's chart zoom is below `PHOTO_EXIT`, so hysteresis has
                // no say in either answer. A pane holding the neighbour's range fails here before any
                // pixel is read, so the failure names the latch rather than a missing marker.
                let want = if pane.camera.zoom >= crate::units::mark::PHOTO_ZOOM {
                    crate::units::mark::Range::Picture
                } else {
                    crate::units::mark::Range::Symbol
                };
                if pane.view.range != want {
                    failures.push(format!(
                        "panes-range: pane {index} holds {:?} at zoom {} — its own zoom asks for {want:?}, \
                         and a wall-wide latch is why a neighbour's zoom would switch it (#115)",
                        pane.view.range, pane.camera.zoom
                    ));
                    continue;
                }
                let frame = crate::ui::view::synthetic_frame(pane.camera, *rect, 1.0);
                let (px, py) = frame.project(fix.latitude, fix.longitude);
                let (x, y) = (
                    origin_x + f64::from(rect.x) + px,
                    origin_y + f64::from(rect.y) + py,
                );
                match pane.view.range {
                    crate::units::mark::Range::Picture => {
                        pictures += 1;
                        match image.nearest_colour(x, y, hud, 30.0) {
                            Some(distance) => lines.push(format!(
                                "panes-range: pane {index} draws the hull's own picture within \
                                 {distance:.1} px of ({x:.0}, {y:.0})"
                            )),
                            None => failures.push(format!(
                                "panes-range: pane {index} is in picture range but has no picture ink \
                                 near ({x:.0}, {y:.0}) — the zoomed pane drew the symbol (#115)"
                            )),
                        }
                    }
                    crate::units::mark::Range::Symbol => {
                        symbols += 1;
                        match image.blob_centre(x, y, accent, 8) {
                            Some((centre, count)) if count >= 8 => {
                                let distance =
                                    ((centre.0 - x).powi(2) + (centre.1 - y).powi(2)).sqrt();
                                if distance > TOLERANCE_PX {
                                    failures.push(format!(
                                        "panes-range: pane {index}'s symbol is {distance:.1} px from \
                                         ({x:.0}, {y:.0}), its own projection"
                                    ));
                                } else {
                                    lines.push(format!(
                                        "panes-range: pane {index} draws the hull's symbol \
                                         {distance:.1} px from ({x:.0}, {y:.0})"
                                    ));
                                }
                            }
                            _ => failures.push(format!(
                                "panes-range: pane {index} is in symbol range but has no symbol ink \
                                 near ({x:.0}, {y:.0}) — the neighbour's zoom switched it to a picture \
                                 (#115)"
                            )),
                        }
                    }
                }
            }
            if pictures != 1 || symbols != 1 {
                failures.push(format!(
                    "panes-range: {pictures} panes in picture range and {symbols} in symbol range — one \
                     capture must hold both representations (#115)"
                ));
            }
        }
        "panes-max" => {
            // **A maximised pane is a bigger rectangle, not a different pane** (#85). Its hulls project
            // against a frame the size of the panel — its own camera, the window's frame — and the tiling
            // behind it is untouched (checked structurally below). The frame here is `synthetic_frame` of
            // the panel, the rule a headless pane lays its own frame out by.
            let Some((ox, oy, pw, ph)) = image.find_panel_rect(ink(tokens::panel())) else {
                failures.push("panes-max: no picture panel on screen".to_string());
                return Err(failures);
            };
            // The scene builds the tiled wall; the **maximise came from the toggle's own press** in
            // `fixtures/scripts/panes-max.json` (#111). `state_for` does not replay a script, so the
            // pane this wall must be looking at is the one the scene focuses, named once in `scene`.
            let state = scene::state_for("panes-max");
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                crate::map::geometry::Viewport::new(pw, ph),
                state.settings.pane_gap,
            );
            assert!(
                rects.len() > 1,
                "panes-max: the wall behind a full-size pane is still a wall"
            );
            let index = scene::PANES_MAXIMISED;
            let Some((id, _)) = rects.get(index).copied() else {
                failures.push(format!("panes-max: pane {index} is not on the wall"));
                return Err(failures);
            };
            let Some(pane) = state.pane(id) else {
                failures.push(format!("panes-max: pane {id} has no state"));
                return Err(failures);
            };
            let Some((id_unit, _zoom)) = scene::PANES.get(index).copied() else {
                failures.push(format!("panes-max: pane {index} has no hull"));
                return Err(failures);
            };
            let frame = crate::ui::view::synthetic_frame(
                pane.camera,
                crate::ui::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: pw as f32,
                    h: ph as f32,
                },
                1.0,
            );
            let Some(fix) = scene::fixture().ok().and_then(|entries| {
                entries
                    .into_iter()
                    .find(|entry| entry.id_unit == id_unit)
                    .and_then(|entry| entry.position)
            }) else {
                failures.push(format!("panes-max: hull {id_unit} has no fix"));
                return Err(failures);
            };
            let (px, py) = frame.project(fix.latitude, fix.longitude);
            let (x, y) = (ox + px, oy + py);
            let ink_here = if image.blob_centre(x, y, accent, 8).is_some() {
                accent
            } else {
                dim
            };
            match image.blob_centre(x, y, ink_here, 8) {
                Some((centre, count)) if count >= 8 => {
                    let distance = ((centre.0 - x).powi(2) + (centre.1 - y).powi(2)).sqrt();
                    if distance > TOLERANCE_PX {
                        failures.push(format!(
                            "panes-max: hull {id_unit} is {distance:.1} px from where the panel-sized \
                             frame puts it ({x:.0}, {y:.0}) — the full-size pane projects against the \
                             frame it is drawn in"
                        ));
                    } else {
                        lines.push(format!(
                            "panes-max: pane {id} full size draws hull {id_unit} at ({x:.0}, {y:.0}) \
                             of a {pw:.0}×{ph:.0} frame"
                        ));
                    }
                }
                _ => failures.push(format!(
                    "panes-max: no marker ink at ({x:.0}, {y:.0}) — the full-size pane is not drawn \
                     against the window's frame"
                )),
            }
        }
        "panes-named" => {
            // **Every pane has a title** (#113). The probe is `view::name_probe` — the same rectangle the
            // plate is placed against — and the test is its plate colour, so a marker that happens to sit
            // in the corner cannot be mistaken for a title. #83's claim survives in what the plate says: a
            // pane nobody named shows its own number (`pane-NN`, from its id), and a named pane shows the
            // name somebody typed. A default title is not a name.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("panes-named: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("panes-named");
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
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
                let Some(pane) = state.pane(*id) else {
                    failures.push(format!("panes-named: pane {index} has no state"));
                    continue;
                };
                let drawn = samples > 0 && hits * 2 >= samples;
                if !drawn {
                    failures.push(format!(
                        "panes-named: pane {index} has no title plate at ({px:.0}, {py:.0}) — every \
                         pane has a title, and the plate is what types a new one (#113)"
                    ));
                    continue;
                }
                let title = pane.title();
                let shown =
                    crate::ui::shown_name(&title).unwrap_or_else(|| crate::ui::default_title(*id));
                match pane.name.as_deref().and_then(crate::ui::shown_name) {
                    Some(name) if shown == name => lines.push(format!(
                        "panes-named: pane {index} shows its name {shown:?} at ({px:.0}, {py:.0})"
                    )),
                    Some(name) => failures.push(format!(
                        "panes-named: pane {index} has the name {name:?} and its plate says \
                         {shown:?} — a named pane shows the name somebody typed"
                    )),
                    None if shown == crate::ui::default_title(*id) => lines.push(format!(
                        "panes-named: pane {index} is unnamed and shows {shown} at ({px:.0}, {py:.0})"
                    )),
                    None => failures.push(format!(
                        "panes-named: pane {index} is unnamed and its plate says {shown:?}, not its \
                         own number {:?} — the default comes from the pane's id (#113)",
                        crate::ui::default_title(*id)
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
        "panes" | "panes-pan" | "panes-gap-resize" => {
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
            //
            // `panes-gap-resize` is the same claim for the **gutter** (#114): the scene leaves a gapped wall
            // at half and its script drags the gap to 58%, so the expected layout is exactly what
            // `panes-gaps` draws — the same split and the same space, reached from the other direction.
            let state = if scene == "panes-gap-resize" {
                scene::state_for("panes-gaps")
            } else {
                scene::state_for("panes")
            };
            // The **drawn** rectangles: the cells pulled in by the gap the scene chose (#82), which is the
            // same function the view lays the panes out with.
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
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
            let focused = state.focused();
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

            // **The drag in the gap moved the split, and the number says so** (#114). The hulls above
            // project against the expected 58% layout, but a capture can agree with a projection for the
            // wrong reason; this reads the gutter the operator sees and compares its centre with
            // `PANES_DRAGGED` of the panel. A gesture that reached nothing leaves the space at half and
            // is a hundred pixels away here.
            if scene == "panes-gap-resize" {
                let gap = f64::from(tokens::GAP);
                let expected = origin_x + panel_w * f64::from(scene::PANES_DRAGGED);
                let y = ((origin_y + panel_h * 0.25).max(0.0) as u32).min(image.height - 1);
                let background = ink(tokens::bg());
                let mut runs: Vec<(u32, u32)> = Vec::new();
                let mut run: Option<(u32, u32)> = None;
                for x in (origin_x as u32 + 1)..((origin_x + panel_w) as u32).saturating_sub(1) {
                    if near(image.at(x, y), background, 6) {
                        run = Some((run.map_or(x, |(start, _)| start), x));
                    } else if let Some((start, last)) = run.take() {
                        runs.push((start, last - start + 1));
                    }
                }
                if let Some((start, last)) = run.take() {
                    runs.push((start, last - start + 1));
                }
                let measured = runs
                    .iter()
                    .filter(|(_, len)| (*len as f64 - gap).abs() <= 1.0)
                    .map(|(start, len)| {
                        (f64::from(*start) + f64::from(*len) / 2.0, f64::from(*len))
                    })
                    .min_by(|a, b| (a.0 - expected).abs().total_cmp(&(b.0 - expected).abs()));
                match measured {
                    Some((centre, len)) if (centre - expected).abs() <= 2.0 => lines.push(format!(
                        "panes-gap-resize: the {len:.0} pt gutter is at x={centre:.0}, and \
                         {:.0}% of the panel is {expected:.0}",
                        scene::PANES_DRAGGED * 100.0
                    )),
                    Some((centre, len)) => failures.push(format!(
                        "panes-gap-resize: the {len:.0} pt gutter is at x={centre:.0}, not at \
                         {:.0}% of the panel ({expected:.0}) — the drag in the gap reached nothing",
                        scene::PANES_DRAGGED * 100.0
                    )),
                    None => failures.push(format!(
                        "panes-gap-resize: no {gap:.0} pt gutter in the row at y={y} — measured \
                         {runs:?}"
                    )),
                }
            }
        }
        "pane-menu" | "panes-menu" => {
            // **A menu opened by a press has no ring** (#164): the pointer is where the operator's attention is, and
            // a ring nobody put there would be a second answer to "which row". The strip is the first row's.
            let strip = (
                1044.0,
                188.0,
                f64::from(tokens::MENU_WIDTH) - 8.0,
                f64::from(tokens::MENU_ROW),
            );
            let ring = count_colour_in(image, strip, ink(tokens::focus_edge()), 24);
            if ring >= 200 {
                failures.push(format!(
                    "panes-menu: {ring} px of ring ink on the first row — a menu a press opened has no row with \
                     the keyboard's keys"
                ));
            } else {
                lines.push("panes-menu: no ring, as a pointer-opened menu should be".to_string());
            }
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
                .menu()
                .map(|menu| crate::ui::menu_items(&state, menu.pane).len())
                .unwrap_or(0);
            // One pane: grow the wall, name this pane — and no `Close pane`, because the last pane cannot
            // be closed. A tiled wall adds it (#81, #83). `Full size` is the pane chrome's toggle since
            // #111, so it is not a row on either wall.
            // #140 added `Focus on new workspace` and the waterfall: growth at the top, the
            // destructive row still last.
            let expected = if scene == "panes-menu" { 5 } else { 4 };
            if rows != expected {
                failures.push(format!(
                    "{scene}: the menu offers {rows} items, and this wall is the case for {expected}"
                ));
            }
            // **The lone pane of the wall is refused, and the row is refused for the same reason**
            // (#141): the wall has three workspaces and two of them empty, so "the last pane" is not
            // "the last pane on this workspace". `menu_items` and `State::close_pane` read one answer, so
            // this asserts both at once.
            if scene == "pane-menu"
                && let Some(menu) = state.menu()
            {
                if state.can_close(menu.pane) {
                    failures.push(
                        "pane-menu: the wall would let its only pane close, and a wall of empty \
                         workspaces cannot be fixed with a menu row (#141)"
                            .into(),
                    );
                } else if rows == 4
                    && crate::ui::menu_items(&state, menu.pane)
                        .iter()
                        .all(|item| item.label != crate::ui::MENU_CLOSE)
                {
                    lines.push(
                        "pane-menu: no `Close pane` row, and the wall's only pane refuses to close"
                            .to_string(),
                    );
                }
            }
            let (w, h) = tokens::menu_size(rows.max(1));
            // **The pane's rectangle, not the panel's**: a menu is clamped inside the pane it was asked
            // on. With one pane those are the same rectangle, which is why #77's assertion passed on the
            // wall it was written for and would have been wrong on any other.
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rect = state
                .menu()
                .and_then(|menu| {
                    state
                        .layout()
                        .and_then(|layout| layout.rect_of(viewport, menu.pane))
                })
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
        "pane-tree" => {
            // **The chain of command** (#116): a tree pane beside a map pane, one structure's forest
            // under the supreme command. Four claims, each with a number:
            //
            // 1. **one node per post** — the plates are sampled where `tree::layout` puts them, and the
            //    count comes from the wire data, not from the layout: a dropped node is a missing plate;
            // 2. **connectors between them** — each run's elbow is sampled for the connector's own ink,
            //    so a diagonal (or a run that never turned) is not a connector;
            // 3. **no marker anywhere in the tree pane** — `INV-04` holds literally: a tree has no frame,
            //    so nothing is projected into it, and the accent the map paints is absent there;
            // 4. **the map pane beside it still draws its markers**, so the tree did not take the wall.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push("pane-tree: no picture panel on screen".to_string());
                return Err(failures);
            };
            let state = scene::state_for("pane-tree");
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            let tree = rects
                .iter()
                .find(|(id, _)| state.pane(*id).is_some_and(|pane| !pane.content.is_map()));
            let map = rects
                .iter()
                .find(|(id, _)| state.pane(*id).is_some_and(|pane| pane.content.is_map()));
            let (Some((tree_id, tree_rect)), Some((_, map_rect))) = (tree.copied(), map.copied())
            else {
                failures.push(format!(
                    "pane-tree: {} panes and no tree beside a map — the scene is one of each",
                    rects.len()
                ));
                return Err(failures);
            };
            let Some(drawn) = crate::ui::tree::tree_for(&state, tree_id) else {
                failures.push(format!("pane-tree: pane {tree_id} draws no tree"));
                return Err(failures);
            };
            // **The tree pane's camera** (#127): the fit while no hand has taken it, the hand's after.
            // The same function the view draws with, so a plate is sampled where it was drawn.
            let camera = state
                .pane(tree_id)
                .map(|pane| crate::ui::tree::camera_for(pane, &drawn, tree_rect))
                .unwrap_or_else(|| crate::ui::tree::fit(&drawn, (tree_rect.w, tree_rect.h)));

            // **One node per post**, counted from the wire data the pane names. A layout that dropped a
            // root, or a flattened forest, draws fewer plates than the server sent posts.
            let expected = {
                let ids: Vec<i64> = match state.pane(tree_id).map(|pane| &pane.content) {
                    Some(crate::ui::PaneContent::Tree {
                        organisation: Some(id),
                    }) => vec![*id],
                    Some(crate::ui::PaneContent::Tree { organisation: None }) => {
                        state.organisations.iter().map(|row| row.id).collect()
                    }
                    _ => Vec::new(),
                };
                let mut count = usize::from(state.tni_command.is_some());
                fn walk(nodes: &[crate::data::types::UnitOrganisationNode]) -> usize {
                    nodes
                        .iter()
                        .map(|node| 1 + walk(&node.children))
                        .sum::<usize>()
                }
                for id in ids {
                    if let Some(detail) = state.organisation_trees.get(&id) {
                        count += walk(&detail.nodes);
                    }
                }
                count
            };
            if drawn.nodes.len() != expected {
                failures.push(format!(
                    "pane-tree: {} plates for {expected} posts — one node per post, and a forest's \
                     roots are posts too (#116)",
                    drawn.nodes.len()
                ));
            }
            let plate = ink(tokens::name_plate());
            let mut missing = Vec::new();
            for node in &drawn.nodes {
                let (nx, ny) = crate::ui::view::tree_point(&camera, (node.rect.x, node.rect.y));
                let rect = crate::ui::Rect {
                    x: tree_rect.x + nx,
                    y: tree_rect.y + ny,
                    w: node.rect.w * camera.zoom,
                    h: node.rect.h * camera.zoom,
                };
                let probe = crate::ui::tree::node_probe(rect, camera.zoom);
                let at = crate::ui::Rect {
                    x: origin_x as f32 + probe.x,
                    y: origin_y as f32 + probe.y,
                    w: probe.w,
                    h: probe.h,
                };
                if !rect_has(image, at, plate, 4) {
                    missing.push(node.label.clone());
                }
            }
            if !missing.is_empty() {
                failures.push(format!(
                    "pane-tree: {} posts have no plate: {} — a node is a post drawn, not a name in a \
                     list",
                    missing.len(),
                    missing.join(", ")
                ));
            } else {
                lines.push(format!(
                    "pane-tree: {expected} posts drawn, one plate each, in pane {tree_id}"
                ));
            }

            // **Connectors between them**, sampled at each elbow's own point.
            let link_ink = ink(tokens::tree_link());
            let mut unlinked = Vec::new();
            for link in &drawn.links {
                let (x, y) = crate::ui::tree::link_probe(link);
                let (px, py) = crate::ui::view::tree_point(&camera, (x, y));
                let at = crate::ui::Rect {
                    x: origin_x as f32 + tree_rect.x + px - 1.0,
                    y: origin_y as f32 + tree_rect.y + py - 1.0,
                    w: 3.0,
                    h: 3.0,
                };
                if !rect_has(image, at, link_ink, 8) {
                    unlinked.push(format!("{}→{}", link.from, link.to));
                }
            }
            if !unlinked.is_empty() {
                failures.push(format!(
                    "pane-tree: {} of {} connectors are missing at their elbows: {} — a tree with no \
                     lines is a list of names",
                    unlinked.len(),
                    drawn.links.len(),
                    unlinked.join(", ")
                ));
            } else {
                lines.push(format!(
                    "pane-tree: {} connectors, each with its elbow drawn",
                    drawn.links.len()
                ));
            }

            // **No marker anywhere in the tree pane** (`INV-04`), and the map still has its own.
            let accent = ink(tokens::accent());
            let count_in = |rect: crate::ui::Rect, colour: [u8; 3]| -> usize {
                let x0 = (origin_x + f64::from(rect.x)).max(0.0) as u32;
                let y0 = (origin_y + f64::from(rect.y)).max(0.0) as u32;
                let x1 = ((origin_x + f64::from(rect.x + rect.w)) as u32).min(image.width);
                let y1 = ((origin_y + f64::from(rect.y + rect.h)) as u32).min(image.height);
                (y0..y1)
                    .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                    .filter(|(x, y)| near(image.at(*x, *y), colour, 8))
                    .count()
            };
            let in_tree = count_in(tree_rect, accent);
            if in_tree > 0 {
                failures.push(format!(
                    "pane-tree: {in_tree} accent pixels inside the tree pane — a tree has no frame, so \
                     it draws no marker and nothing that projects (INV-04)"
                ));
            } else {
                lines.push("pane-tree: no marker ink in the tree pane".to_string());
            }
            let in_map = count_in(map_rect, accent);
            if in_map == 0 {
                failures.push(
                    "pane-tree: no marker ink in the map pane — the tree must not take the wall's map \
                     with it"
                        .to_string(),
                );
            } else {
                lines.push(format!(
                    "pane-tree: the map pane still draws its markers ({in_map} accent px)"
                ));
            }
        }
        "pane-tree-narrow" | "pane-tree-zoom" | "pane-tree-pan" => {
            // **The tree pane is a canvas** (#127). Two claims, one per scene:
            //
            // - `narrow`: no hand has taken the camera, so the fit runs — **every post and every
            //   connector is inside the pane's own rectangle** in a 320 pt pane, where 1:1 would cut the
            //   784 pt forest.
            // - `zoom`: a hand has, and the known post's centre is at the known pane point, at the known
            //   zoom — the one transform, not a per-node re-layout.
            let Some((origin_x, origin_y, panel_w, panel_h)) =
                image.find_panel_rect(ink(tokens::panel()))
            else {
                failures.push(format!("{scene}: no picture panel on screen"));
                return Err(failures);
            };
            let state = scene::state_for(scene);
            let viewport = crate::map::geometry::Viewport::new(panel_w, panel_h);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            let Some((tree_id, tree_rect)) = rects
                .iter()
                .find(|(id, _)| state.pane(*id).is_some_and(|pane| !pane.content.is_map()))
                .copied()
            else {
                failures.push(format!(
                    "{scene}: no tree pane — the scene is a tree beside a map"
                ));
                return Err(failures);
            };
            let Some(drawn) = crate::ui::tree::tree_for(&state, tree_id) else {
                failures.push(format!("{scene}: pane {tree_id} draws no tree"));
                return Err(failures);
            };
            let fitted = crate::ui::tree::fit(&drawn, (tree_rect.w, tree_rect.h));
            let camera = if scene == "pane-tree-pan" {
                // The script drags from the fitted camera; state_for builds the scene *before* the
                // pointer moves, so the expectation is the fit plus the script's travel.
                fitted.panned(scene::TREE_PAN_BY)
            } else {
                state
                    .pane(tree_id)
                    .map(|pane| crate::ui::tree::camera_for(pane, &drawn, tree_rect))
                    .unwrap_or(fitted)
            };
            let project = |point: (f32, f32)| crate::ui::view::tree_point(&camera, point);

            if scene == "pane-tree-narrow" {
                if camera.touched {
                    failures.push(
                        "pane-tree-narrow: the camera is the hand's, and nothing has touched this \
                         pane — a narrow pane must re-fit instead of keeping a camera it never had"
                            .to_string(),
                    );
                }
                let mut outside = Vec::new();
                for node in &drawn.nodes {
                    let (x, y) = project((node.rect.x, node.rect.y));
                    let (w, h) = (node.rect.w * camera.zoom, node.rect.h * camera.zoom);
                    if x < -0.5
                        || y < -0.5
                        || x + w > tree_rect.w + 0.5
                        || y + h > tree_rect.h + 0.5
                    {
                        outside.push(node.label.clone());
                    }
                }
                if !outside.is_empty() {
                    failures.push(format!(
                        "pane-tree-narrow: {} of {} posts are outside the {} pt pane (zoom {:.2}): {} — \
                         narrowing the pane cut the tree, which is the behaviour this scene exists to \
                         refuse",
                        outside.len(),
                        drawn.nodes.len(),
                        tree_rect.w,
                        camera.zoom,
                        outside.join(", ")
                    ));
                }
                let mut off = 0;
                for link in &drawn.links {
                    for point in &link.points {
                        let (x, y) = project((point[0], point[1]));
                        if x < -0.5 || y < -0.5 || x > tree_rect.w + 0.5 || y > tree_rect.h + 0.5 {
                            off += 1;
                        }
                    }
                }
                if off > 0 {
                    failures.push(format!(
                        "pane-tree-narrow: {off} connector points are outside the {} pt pane — a \
                         connector that leaves the canvas is the forest being cut",
                        tree_rect.w
                    ));
                }
                if failures.is_empty() {
                    lines.push(format!(
                        "pane-tree-narrow: {} posts and {} connectors inside a {} pt pane at zoom \
                         {:.2}, by the fit",
                        drawn.nodes.len(),
                        drawn.links.len(),
                        tree_rect.w,
                        camera.zoom
                    ));
                }
            } else if scene == "pane-tree-pan" {
                // **A drag on a post pans the canvas** (#127). The known post's plate is where the fit
                // plus the script's travel puts it: a drag that reached nothing leaves the tree at the
                // fit and the plate is a travel away from where this looks.
                if !camera.touched {
                    failures.push(
                        "pane-tree-pan: the camera is the fit's, and the script dragged it"
                            .to_string(),
                    );
                }
                let Some(known) = drawn
                    .nodes
                    .iter()
                    .find(|node| node.id == scene::TREE_ZOOM_NODE)
                else {
                    failures.push(format!(
                        "pane-tree-pan: no post {} — the scene's known node",
                        scene::TREE_ZOOM_NODE
                    ));
                    return Err(failures);
                };
                // **The script takes hold of a post, not of empty canvas.** If the point were empty,
                // the pan would still pass on the canvas root and prove nothing about routing a gesture
                // that starts on a plate.
                let (kx, ky) = crate::ui::view::tree_point(&fitted, (known.rect.x, known.rect.y));
                let plate = crate::ui::Rect {
                    x: tree_rect.x + kx,
                    y: tree_rect.y + ky,
                    w: known.rect.w * fitted.zoom,
                    h: known.rect.h * fitted.zoom,
                };
                let press = (
                    scene::TREE_PAN_AT.0 - origin_x as f32,
                    scene::TREE_PAN_AT.1 - origin_y as f32,
                );
                if !plate.contains(press) {
                    failures.push(format!(
                        "pane-tree-pan: the script presses ({:.0}, {:.0}) and the known post's plate \
                         is ({:.0}, {:.0}) {}×{} — a drag from empty canvas does not prove a post is \
                         a handle",
                        press.0, press.1, plate.x, plate.y, plate.w, plate.h
                    ));
                }
                let centre = (
                    known.rect.x + known.rect.w / 2.0,
                    known.rect.y + known.rect.h / 2.0,
                );
                let (was_x, was_y) = crate::ui::view::tree_point(&fitted, centre);
                let (x, y) = project(centre);
                if ((x - was_x) - scene::TREE_PAN_BY.0).abs() > 0.5
                    || ((y - was_y) - scene::TREE_PAN_BY.1).abs() > 0.5
                {
                    failures.push(format!(
                        "pane-tree-pan: the known post is at ({x:.1}, {y:.1}) and the fit put it at \
                         ({was_x:.1}, {was_y:.1}) — a travel of ({:.0}, {:.0}) was not paned, so the \
                         drag reached no canvas",
                        scene::TREE_PAN_BY.0,
                        scene::TREE_PAN_BY.1
                    ));
                }
                let (nx, ny) = project((known.rect.x, known.rect.y));
                let rect = crate::ui::Rect {
                    x: tree_rect.x + nx,
                    y: tree_rect.y + ny,
                    w: known.rect.w * camera.zoom,
                    h: known.rect.h * camera.zoom,
                };
                let probe = crate::ui::tree::node_probe(rect, camera.zoom);
                let at = crate::ui::Rect {
                    x: origin_x as f32 + probe.x,
                    y: origin_y as f32 + probe.y,
                    w: probe.w,
                    h: probe.h,
                };
                if !rect_has(image, at, ink(tokens::name_plate()), 4) {
                    failures.push(format!(
                        "pane-tree-pan: the known post's plate is not at ({:.0}, {:.0}) — the panned \
                         tree was not drawn",
                        at.x, at.y
                    ));
                }
                if failures.is_empty() {
                    lines.push(format!(
                        "pane-tree-pan: the known post moved by ({:.0}, {:.0}) to ({x:.0}, {y:.0}) — \
                         a drag on a post pans the canvas",
                        scene::TREE_PAN_BY.0,
                        scene::TREE_PAN_BY.1
                    ));
                }
            } else {
                if !camera.touched {
                    failures.push(
                        "pane-tree-zoom: the camera is the fit's, and this scene is a hand's zoom"
                            .to_string(),
                    );
                }
                if (camera.zoom - scene::TREE_ZOOM_IN).abs() > 1e-3 {
                    failures.push(format!(
                        "pane-tree-zoom: zoom {:.3}, and the scene set {}",
                        camera.zoom,
                        scene::TREE_ZOOM_IN
                    ));
                }
                let Some(known) = drawn
                    .nodes
                    .iter()
                    .find(|node| node.id == scene::TREE_ZOOM_NODE)
                else {
                    failures.push(format!(
                        "pane-tree-zoom: no post {} — the scene's known node",
                        scene::TREE_ZOOM_NODE
                    ));
                    return Err(failures);
                };
                let (x, y) = project((
                    known.rect.x + known.rect.w / 2.0,
                    known.rect.y + known.rect.h / 2.0,
                ));
                if (x - scene::TREE_ZOOM_AT.0).abs() > 0.5
                    || (y - scene::TREE_ZOOM_AT.1).abs() > 0.5
                {
                    failures.push(format!(
                        "pane-tree-zoom: the known post is at ({x:.1}, {y:.1}) in its pane, and the \
                         scene put it at ({:.0}, {:.0}) — the camera the drawing used is not the camera \
                         the scene set",
                        scene::TREE_ZOOM_AT.0,
                        scene::TREE_ZOOM_AT.1
                    ));
                }
                // The plate is where the transform puts it, and it is drawn: a node that is placed but
                // not filled is a rectangle in a test and a hole on the wall.
                let (nx, ny) = project((known.rect.x, known.rect.y));
                let rect = crate::ui::Rect {
                    x: tree_rect.x + nx,
                    y: tree_rect.y + ny,
                    w: known.rect.w * camera.zoom,
                    h: known.rect.h * camera.zoom,
                };
                let probe = crate::ui::tree::node_probe(rect, camera.zoom);
                let at = crate::ui::Rect {
                    x: origin_x as f32 + probe.x,
                    y: origin_y as f32 + probe.y,
                    w: probe.w,
                    h: probe.h,
                };
                if !rect_has(image, at, ink(tokens::name_plate()), 4) {
                    failures.push(format!(
                        "pane-tree-zoom: the known post's plate is not at ({:.0}, {:.0}) — the one \
                         transform puts a node where the hit test can find it",
                        at.x, at.y
                    ));
                }
                // A zoom, not a fit: some posts are off the canvas, which is what "fewer nodes" means.
                let on_screen = drawn
                    .nodes
                    .iter()
                    .filter(|node| {
                        let (x, y) = project((node.rect.x, node.rect.y));
                        x + node.rect.w * camera.zoom >= 0.0
                            && y + node.rect.h * camera.zoom >= 0.0
                            && x <= tree_rect.w
                            && y <= tree_rect.h
                    })
                    .count();
                if on_screen >= drawn.nodes.len() {
                    failures.push(format!(
                        "pane-tree-zoom: all {} posts are on screen at zoom {:.1} — this is a fit, \
                         not the zoomed canvas",
                        drawn.nodes.len(),
                        camera.zoom
                    ));
                }
                if failures.is_empty() {
                    lines.push(format!(
                        "pane-tree-zoom: {} of {} posts on screen at zoom {:.1}, the known post at \
                         ({:.0}, {:.0}) and its plate drawn",
                        on_screen,
                        drawn.nodes.len(),
                        camera.zoom,
                        x,
                        y
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
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            let Some(drag) = state.dragging() else {
                failures.push("panes-drag: no pane is lifted in the scene".to_string());
                return Err(failures);
            };
            let Some(drop) = drag.drop else {
                failures.push("panes-drag: the lifted pane has no drop under it".to_string());
                return Err(failures);
            };
            if state.preview(drop).is_none() {
                failures.push(format!("panes-drag: {drop:?} has no preview"));
                return Err(failures);
            }
            // The slot is inset by a gap on each side (the slot, not the whole pane) — one function, the
            // same one the picture draws and the ghost stands on.
            let Some(landing) = crate::ui::view::landing_rect(
                state.layout().expect("the scene has a tiling"),
                drop,
                viewport,
                drag.pane,
            ) else {
                failures
                    .push("panes-drag: the preview does not place the dragged pane".to_string());
                return Err(failures);
            };
            // **A drop on the top edge makes a wide slot, not a tall one** (#100): `Drop::Retile`'s
            // direction is the *side's*, and a tree that handed its own direction down made every
            // top/bottom drop a left/right split. `landing_rect` comes from the preview, so this is the
            // same question the release asks — and the answer is a shape, not an intention.
            let expected = crate::ui::view::Drop::Retile {
                dragged: drag.pane,
                target: state.pane_rects()[0].0,
                side: crate::ui::view::Side::Top,
            };
            if drop != expected {
                failures.push(format!(
                    "panes-drag: the scene drops on the first pane's top edge, but the drop is {drop:?}"
                ));
            } else if landing.w <= landing.h {
                failures.push(format!(
                    "panes-drag: a pane dropped on the top edge lands in a {:.0}×{:.0} slot — wider \
                     than tall is the whole point of a top drop (a left/right split leaking into a \
                     top/bottom one, #100)",
                    landing.w, landing.h
                ));
            }
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
            let landed = crate::ui::view::landing_rect(
                state.layout().expect("the scene has a tiling"),
                drop,
                viewport,
                drag.pane,
            );
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
        // **The newsroom** (#136): three tabs, rows of stories, a footer — and **no marker ink**: a
        // newsroom pane has no frame, so nothing is projected into it (`INV-04` is vacuous there, and
        // this is what says so). The pane rectangles come from `Layout::rects` against the panel this
        // capture actually has, exactly as `objects` does, so the view and this check cannot disagree
        // about where a pane is.
        "newsroom"
        | "newsroom-scopes"
        | "newsroom-page-2"
        | "newsroom-story"
        | "newsroom-story-back" => {
            let Some((ox, oy, pw, ph)) = image.find_panel_rect(ink(tokens::panel())) else {
                failures.push(format!("{scene}: no picture panel on screen"));
                return Err(failures);
            };
            let state = scene::state_for(scene);
            let viewport = crate::map::geometry::Viewport::new(pw, ph);
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                viewport,
                state.settings.pane_gap,
            );
            let ink_colours = [ink(tokens::ink()), ink(tokens::ink_dim())];
            let mut feeds = 0;
            for (id, rect) in &rects {
                let Some(pane) = state.pane(*id) else {
                    continue;
                };
                if pane.news.is_none() {
                    continue;
                }
                feeds += 1;
                let px = ox + f64::from(rect.x);
                let py = oy + f64::from(rect.y);
                // The tab strip's own band — the header row, and nothing below it: the first story's
                // time line runs the pane's whole width and would bridge the three labels into one.
                let strip = py + f64::from(tokens::NEWS_TOP);
                // The strip's own band: the three labels sit on it, and the pill behind the active one
                // is a fill rather than text, so a word count across it is three.
                let words = word_runs(
                    image,
                    // The tabs are at the left of the strip; the feed's count is right-aligned and is
                    // two words of its own, so the scan stops at the pane's middle.
                    (
                        (px + f64::from(tokens::NEWS_PAD)) as u32,
                        (px + f64::from(rect.w) / 2.0) as u32,
                    ),
                    (strip as u32, strip as u32 + tokens::NEWS_TAB_H as u32),
                    &ink_colours,
                    24,
                    tokens::GAP as u32 + 2,
                );
                // **A story open is a surface over the list** (#137), and the strip's band is the
                // detail's own top row when it is up — `back` and the title, not three tabs — so the word
                // count only means something on the list. The footer and the article lines below are
                // what tell the two apart: the list has a footer, the surface covers it.
                // `newsroom-story-back` is the other half: its script clicks `back`, so its capture must
                // have the list's three tabs again even though the scene left the story open.
                let detail_open = scene == "newsroom-story";
                if !detail_open && words != 3 {
                    failures.push(format!(
                        "{scene}: pane {id} has {words} words on its tab strip, not three — the scope \
                         tabs are International, Regional and National, and a wall-wide or missing tab \
                         is what this scene exists to catch"
                    ));
                }
                // The rows: one band of primary ink per visible title, between the tabs and the footer.
                let list_top = py + f64::from(tokens::NEWS_TOP + tokens::NEWS_TAB_H + tokens::GAP);
                let list_bottom = py + f64::from(rect.h)
                    - f64::from(tokens::NEWS_PAD + tokens::NEWS_FOOTER_H + tokens::GAP);
                let rows = ink_bands(
                    image,
                    (
                        px + f64::from(tokens::NEWS_PAD),
                        list_top,
                        f64::from(rect.w) - 2.0 * f64::from(tokens::NEWS_PAD),
                        (list_bottom - list_top).max(0.0),
                    ),
                    &[ink(tokens::ink())],
                    24,
                );
                if detail_open && rows.len() < 6 {
                    // The story's title, then a domain line and a headline per article.
                    failures.push(format!(
                        "{scene}: pane {id} draws {} lines of primary ink below the strip — three \
                         articles are a domain line and a headline each, plus the story's title",
                        rows.len()
                    ));
                } else if !detail_open && rows.len() < 3 {
                    failures.push(format!(
                        "{scene}: pane {id} draws {} story titles, and three fit — the rows come from \
                         the wire, not from a list written in the view",
                        rows.len()
                    ));
                } else if detail_open {
                    lines.push(format!(
                        "{scene}: pane {id} draws {} lines of the story and its articles",
                        rows.len()
                    ));
                } else {
                    lines.push(format!(
                        "{scene}: pane {id} draws {} story titles below three tabs",
                        rows.len()
                    ));
                }
                // The footer, when the server said how many pages there are.
                let footer = ink_bands(
                    image,
                    (
                        px,
                        py + f64::from(rect.h)
                            - f64::from(tokens::NEWS_PAD + tokens::NEWS_FOOTER_H),
                        f64::from(rect.w),
                        f64::from(tokens::NEWS_FOOTER_H),
                    ),
                    &ink_colours,
                    24,
                );
                if detail_open && !footer.is_empty() {
                    failures.push(format!(
                        "{scene}: pane {id} draws its page footer under the story — the detail is a \
                         surface over the whole pane, footer included"
                    ));
                } else if !detail_open && footer.is_empty() {
                    failures.push(format!(
                        "{scene}: pane {id} has no footer — the page number is how a reader knows \
                         there is more, and it is drawn whenever the server sent metadata"
                    ));
                } else if !detail_open {
                    lines.push(format!("{scene}: pane {id} draws its page footer"));
                }
                // **No marker ink in a feed**: `INV-04` is vacuous in a pane with no frame, and this is
                // the pixel fact behind it.
                let accent = count_colour_in(
                    image,
                    (px, py, f64::from(rect.w), f64::from(rect.h)),
                    ink(tokens::accent()),
                    24,
                );
                if accent > 0 {
                    failures.push(format!(
                        "{scene}: {accent} accent pixels inside the newsroom pane {id} — a feed has no \
                         frame, so nothing may be projected into it"
                    ));
                } else {
                    lines.push(format!("{scene}: no marker ink in the newsroom pane {id}"));
                }
            }
            if feeds == 0 {
                failures.push(format!("{scene}: no newsroom pane on screen"));
            }
            if scene != "newsroom-scopes" {
                // The map beside the feed still projects: the scene is the wall the operator has, not a
                // feed alone in a frame.
                let accent = count_colour_in(image, (ox, oy, pw, ph), ink(tokens::accent()), 24);
                if accent == 0 {
                    failures.push(format!(
                        "{scene}: no marker ink anywhere — the map beside the feed has stopped drawing \
                         its picture"
                    ));
                } else {
                    lines.push(format!(
                        "{scene}: the map beside the feed still draws its markers ({accent} accent px)"
                    ));
                }
            }
        }
        // **A workspace that is not on screen can be named** (#154). The capture is the **gesture**:
        // `fixtures/scripts/workspaces-rename-hidden.json` right-clicks chip 2, selects the field with
        // Shift+Home and types "Straits watch". So the structure here is the scene's wall (the state
        // `state_for` builds, before the script runs) and the sheet is the picture's — the split the
        // script-driven scenes have always had (`workspaces-switch` is the same shape).
        //
        // Its own arm rather than the header arm's, because a modal sheet draws its scrim over the chips
        // and the title: a pixel claim about the raised chip would be a claim about the scrim.
        "workspaces-rename-hidden" => {
            let state = scene::state_for(scene);
            if state.active != 0 || state.workspaces.len() != 3 {
                failures.push(format!(
                    "workspaces-rename-hidden: the scene leaves {} workspaces with {} active, and the operator's wall is three with the first on screen",
                    state.workspaces.len(),
                    state.active + 1
                ));
            }
            if state.rename().is_some() {
                failures.push(
                    "workspaces-rename-hidden: the scene opens the sheet itself, and the sheet must be the script's right-click"
                        .into(),
                );
            }
            if state.workspaces.iter().any(|workspace| {
                workspace.name != "Workspace 1"
                    && workspace.name != "Workspace 2"
                    && workspace.name != "Workspace 3"
            }) {
                failures.push(
                    "workspaces-rename-hidden: a workspace starts named, and the scene is the wall before anybody typed"
                        .into(),
                );
            } else {
                lines.push(
                    "workspaces-rename-hidden: the scene is workspace 1 with three default names, so the sheet in the picture is the script's secondary press"
                        .to_string(),
                );
            }
            // **The sheet is drawn, in the middle of the wall**: the widest run of the sheet's own fill
            // across the middle of its height. The picture's panel is *not* looked for, and that is the
            // point of this scene — the scrim covers the wall, so the only `panel()` ink there is the
            // sheet's. A band rather than one row, because the sheet's own children break a single line.
            let middle = image.height as f64 / 2.0;
            let widest = colour_runs_in_band(
                image,
                (middle - 20.0, middle + 20.0),
                ink(tokens::panel()),
                4,
                2,
            )
            .into_iter()
            .map(|(start, end)| end - start)
            .fold(0.0_f64, f64::max);
            if widest < f64::from(tokens::SHEET_SIZE.0) * 0.8 {
                failures.push(format!(
                    "workspaces-rename-hidden: the widest panel run across the wall's middle is {widest:.0} px, and a sheet is {:.0} — the sheet is not drawn",
                    tokens::SHEET_SIZE.0
                ));
                return Err(failures);
            }
            lines.push(format!(
                "workspaces-rename-hidden: the sheet fills {widest:.0} px across the wall's middle, over the scrim"
            ));
            // **And it carries words in two rows**: the sheet's own title, and the name in its field. The
            // hint below is `ink_dim()`, so it is not counted — two bands of `ink()` is "a sheet with a
            // name in it", and a blank field is one.
            let sheet = (
                (image.width as f64 - f64::from(tokens::SHEET_SIZE.0)) / 2.0,
                (image.height as f64 - f64::from(tokens::SHEET_SIZE.1)) / 2.0,
                f64::from(tokens::SHEET_SIZE.0),
                f64::from(tokens::SHEET_SIZE.1) * 0.7,
            );
            let bands = ink_bands(image, sheet, &[ink(tokens::ink())], 24);
            if bands.len() < 2 {
                failures.push(format!(
                    "workspaces-rename-hidden: {} row(s) of the sheet's own ink, and the sheet says its name and shows the one being typed",
                    bands.len()
                ));
            } else {
                lines.push(format!(
                    "workspaces-rename-hidden: the sheet draws {} rows of ink — its title, and the name in its field",
                    bands.len()
                ));
            }
        }
        // **Workspaces** (#138): three chips and a `+`, exactly one chip raised, the title naming the
        // active one, and an empty workspace that says so with no pane chrome and no marker ink.
        //
        // The pixel half of a switch cannot read *which* chip was pressed; it can read **where** the
        // raised chip is, so the claim is geometric: on `workspaces` the raised chip is the one furthest
        // from the `+`, on `workspaces-switch` it is the one nearest it. That scene leaves the wall on
        // workspace 1, so a capture showing workspace 3 is the script's click.
        "workspaces" | "workspaces-empty" | "workspaces-switch" | "workspaces-named"
        | "workspaces-close" => {
            let state = scene::state_for(scene);
            let header = (0.0, f64::from(tokens::CHIP_H) + f64::from(tokens::PAD));
            // A chip's fill is a **block** at least as wide as a chip is tall; an inactive chip's
            // hairline border reads as a similar colour at a loose tolerance and is one pixel wide, and
            // an active chip's fill is a chip gap away from the next chip's border — so the runs are
            // filtered by width and not merged across a gap.
            let raised: Vec<(f64, f64)> =
                colour_runs_in_band(image, header, ink(tokens::name_plate()), 10, 2)
                    .into_iter()
                    .filter(|(start, end)| end - start >= f64::from(tokens::CHIP_H))
                    .collect();
            if raised.len() != 1 {
                failures.push(format!(
                    "{scene}: {} raised chips in the header, and a row of chips has exactly one — \
                     the active workspace is the one the operator is looking at",
                    raised.len()
                ));
            }
            let Some(&(raised_start, raised_end)) = raised.first() else {
                return Err(failures);
            };
            let Some(plus) = rightmost_ink(
                image,
                header,
                image.width / 2,
                &[ink(tokens::ink()), ink(tokens::ink_dim())],
                24,
            ) else {
                failures.push(format!("{scene}: no chip ink in the header at all"));
                return Err(failures);
            };
            let distance = plus - raised_end;
            let (want, why): (fn(f64) -> bool, &str) = match scene {
                "workspaces" | "workspaces-close" => {
                    (|d| d > 60.0, "the first chip, furthest from the `+`")
                }
                "workspaces-empty" | "workspaces-named" => (
                    |d| (20.0..=80.0).contains(&d),
                    "the second chip, between the first and the `+`",
                ),
                _ => (|d| d < 25.0, "the third chip, next to the `+`"),
            };
            if want(distance) {
                lines.push(format!(
                    "{scene}: the raised chip ends {distance:.0} px left of the `+` — {why}"
                ));
            } else {
                failures.push(format!(
                    "{scene}: the raised chip ends {distance:.0} px left of the `+`, starting at \
                     {raised_start:.0} — the capture is not {why}, so the chip that is raised is not \
                     the workspace on screen"
                ));
            }

            // The empty state: the picture panel draws the words, and nothing projects into it.
            if state.layout().is_none() {
                let Some((px, py, pw, ph)) = image.find_panel_rect(ink(tokens::panel())) else {
                    failures.push(format!(
                        "{scene}: an empty workspace draws no picture panel"
                    ));
                    return Err(failures);
                };
                let centre = (px, py + ph / 3.0, pw, ph / 3.0);
                let words = ink_bands(image, centre, &[ink(tokens::ink_dim())], 24);
                if words.is_empty() {
                    failures.push(format!(
                        "{scene}: no words in the middle of an empty workspace — it says \
                         \"No panes yet.\", not nothing"
                    ));
                } else {
                    lines.push(format!(
                        "{scene}: the empty workspace draws its message, centred"
                    ));
                }
                let accent = count_colour_in(image, (px, py, pw, ph), ink(tokens::accent()), 24);
                if accent > 0 {
                    failures.push(format!(
                        "{scene}: {accent} accent pixels in an empty workspace — no pane is drawn, so \
                         nothing projects into it (INV-04)"
                    ));
                } else {
                    lines.push(format!("{scene}: an empty workspace draws no marker ink"));
                }
            }
            // The structure: the chips, the active index and the tiling.
            if state.workspaces.len() != 3 {
                failures.push(format!(
                    "{scene}: {} workspaces, and the default is three",
                    state.workspaces.len()
                ));
            }
            let expected = match scene {
                // #141: an emptied workspace sends the wall back to the nearest previous one that has
                // a pane, which is the first here.
                "workspaces" | "workspaces-switch" | "workspaces-close" => 0,
                _ => 1,
            };
            if state.active != expected {
                failures.push(format!(
                    "{scene}: workspace {} is active and the scene means workspace {}",
                    state.active + 1,
                    expected + 1
                ));
            } else {
                lines.push(format!(
                    "{scene}: {} workspaces, workspace {} active ({:?})",
                    state.workspaces.len(),
                    state.active + 1,
                    state.active().name
                ));
            }
            // **The title is the workspace's name** (#138) and the **thing you click to rename it**
            // (#139). Its ink is one run at the header's left padding, and it is **not** the app's name:
            // `Arcons Command Center` measured ~205 px at this style, and a workspace's name is cut to
            // `tokens::NAME_MAX_CHARS` — so a run wider than the app's would be the wordmark back again.
            let title = colour_runs_in_band(
                image,
                (
                    f64::from(tokens::PAD),
                    f64::from(tokens::PAD) + f64::from(tokens::CHIP_H) * 2.0,
                ),
                ink(tokens::accent()),
                24,
                12,
            );
            match title.as_slice() {
                [(start, end)]
                    if (*start - f64::from(tokens::PAD)).abs() <= 2.0 && *end < 160.0 =>
                {
                    lines.push(format!(
                        "{scene}: the title is one run of {:.0} px at x={start:.0}, and the app's name \
                         is not in the header",
                        end - start
                    ));
                }
                [(start, end)] => failures.push(format!(
                    "{scene}: the title runs from {start:.0} to {end:.0} — it must start at the \
                     header's padding and be narrower than the app's name (`TITLE` belongs to the \
                     window, not the topbar)"
                )),
                other => failures.push(format!(
                    "{scene}: {} runs of title ink in the header, and the title is one name",
                    other.len()
                )),
            }

            match scene {
                "workspaces" => {
                    if state.pane_count() != scene::WORKSPACE_PANES {
                        failures.push(format!(
                            "workspaces: {} panes, and the scene tiles {}",
                            state.pane_count(),
                            scene::WORKSPACE_PANES
                        ));
                    }
                    if state
                        .workspaces
                        .iter()
                        .skip(1)
                        .any(|workspace| workspace.layout.is_some())
                    {
                        failures.push(
                            "workspaces: a workspace beside the first holds a tiling — the scene's \
                             point is that those two are empty"
                                .into(),
                        );
                    }
                }
                "workspaces-empty" => {
                    if state.layout().is_some() || state.pane_count() != 0 {
                        failures.push(format!(
                            "workspaces-empty: the active workspace holds {} panes",
                            state.pane_count()
                        ));
                    }
                    if state
                        .workspaces
                        .first()
                        .is_some_and(|workspace| !workspace.panes.is_empty())
                    {
                        lines.push(
                            "workspaces-empty: workspace 1 keeps its panes, and they are not drawn"
                                .to_string(),
                        );
                    }
                }
                "workspaces-named" => {
                    // **The operator's name, through the sheet's own commit** (#139): the state holds it,
                    // the header shows it, and the chips keep their numbers.
                    let shown = crate::ui::workspace_title(state.active(), state.active);
                    if shown == "Straits watch" && state.workspaces[1].name == "Straits watch" {
                        lines.push(
                            "workspaces-named: the title reads \"Straits watch\", the name the sheet \
                             committed"
                                .to_string(),
                        );
                    } else {
                        failures.push(format!(
                            "workspaces-named: the title reads {shown:?} and the workspace's name is \
                             {:?} — the sheet's commit is the only way a workspace is named",
                            state.workspaces[1].name
                        ));
                    }
                    let numbers: Vec<&str> = state
                        .workspaces
                        .iter()
                        .map(|workspace| workspace.name.as_str())
                        .collect();
                    if numbers != ["Workspace 1", "Straits watch", "Workspace 3"] {
                        failures.push(format!(
                            "workspaces-named: the workspaces are {numbers:?} — naming one must not \
                             rename another"
                        ));
                    }
                }
                // **The sheet is open on a workspace that is not on screen** (#154): the wall stays on
                // workspace 1, its title stays its name, and the sheet names workspace 2. A right-click on
                // a chip is the gesture; `begin_workspace_rename` is the function it calls, and the scene
                // calls the same one.
                "workspaces-close" => {
                    // **Closing a workspace's last pane** (#141): the workspace is still there, still
                    // named, and empty; the wall landed on the nearest previous workspace; and the
                    // focus is on **the pane the operator was last in there**, not the first leaf.
                    if state.workspaces.len() != 3 {
                        failures.push(format!(
                            "workspaces-close: {} workspaces, and emptying one does not remove it from \
                             the chips",
                            state.workspaces.len()
                        ));
                    }
                    let emptied = &state.workspaces[1];
                    if !emptied.panes.is_empty()
                        || emptied.layout.is_some()
                        || emptied.name.is_empty()
                    {
                        failures.push(format!(
                            "workspaces-close: workspace 2 holds {} panes and {:?} — a workspace whose \
                             last pane closed is empty and keeps its name",
                            emptied.panes.len(),
                            emptied.layout.as_ref().map(|layout| layout.len())
                        ));
                    } else {
                        lines.push(
                            "workspaces-close: workspace 2 is empty, still in the chips, and still named"
                                .to_string(),
                        );
                    }
                    let ids = state.pane_ids();
                    if ids.len() != 2 || state.panes().len() != 2 {
                        failures.push(format!(
                            "workspaces-close: {} panes and {} in the workspace, and the close took one \
                             of three",
                            ids.len(),
                            state.panes().len()
                        ));
                    }
                    if ids.len() == 2 && state.focused() == ids[1] {
                        lines.push(format!(
                            "workspaces-close: focus is on pane {}, the one this workspace was last \
                             focused on — not the first leaf {}",
                            ids[1], ids[0]
                        ));
                    } else {
                        failures.push(format!(
                            "workspaces-close: focus is on {:?}, and workspace 1 was last focused on \
                             its second pane",
                            state.focused()
                        ));
                    }
                    // **The ring says it too.** The focused pane's edge is a hairline of its own
                    // colour, and the two panes **share** their top and bottom edges with the picture
                    // panel — so the honest pixel is a **vertical** one: the left pane's outer edge
                    // carries no ring ink, and the right pane's outer edge does.
                    let Some((ox, oy, pw, ph)) = image.find_panel_rect(ink(tokens::panel())) else {
                        failures.push("workspaces-close: no picture panel".into());
                        return Err(failures);
                    };
                    let ring = ink(tokens::focus_edge());
                    let edge = f64::from(tokens::HAIRLINE) * 3.0;
                    // The region's right edge is exclusive, and the pane's edge **is** the panel's last
                    // column: one pixel of width on top of the band is not slack, it is that column.
                    let unfocused = count_colour_in(image, (ox, oy, edge + 1.0, ph), ring, 10);
                    let focused =
                        count_colour_in(image, (ox + pw - edge, oy, edge + 1.0, ph), ring, 10);
                    if focused > 0 && unfocused == 0 {
                        lines.push(format!(
                            "workspaces-close: {focused} px of focus-edge ink on the right pane's \
                             outer edge and none on the left's"
                        ));
                    } else {
                        failures.push(format!(
                            "workspaces-close: {unfocused} px of focus-edge ink on the left pane's \
                             outer edge and {focused} on the right's — the ring is on the remembered \
                             pane alone"
                        ));
                    }
                }
                _ => {
                    // The scene leaves workspace 1 with its panes; the **capture** is workspace 3's
                    // empty state, which the pixel half above says by the chip and the words.
                    if state.pane_count() != scene::WORKSPACE_PANES {
                        failures.push(
                            "workspaces-switch: the scene must start on workspace 1 with its panes, so \
                             the switch in the capture is the click's"
                                .into(),
                        );
                    }
                }
            }
        }
        // **Workspace actions** (#140): the waterfall, a pane moved between workspaces, and a new
        // workspace from a pane. The chip checks are the same as the workspaces arm's — where the raised
        // chip sits relative to the `+` says **which** workspace is active in the capture.
        // **The remote twin is in this arm on purpose** (#172): a scene driven only by intents must satisfy every
        // structural claim the pointer-driven scene makes, not a weaker set of its own.
        "workspaces-menu" | "workspaces-moved" | "workspaces-new" | "workspaces-new-remote" => {
            let state = scene::state_for(scene);
            let header = (0.0, f64::from(tokens::CHIP_H) + f64::from(tokens::PAD));
            let raised: Vec<(f64, f64)> =
                colour_runs_in_band(image, header, ink(tokens::name_plate()), 10, 2)
                    .into_iter()
                    .filter(|(start, end)| end - start >= f64::from(tokens::CHIP_H))
                    .collect();
            if raised.len() != 1 {
                failures.push(format!(
                    "{scene}: {} raised chips in the header, and there is exactly one",
                    raised.len()
                ));
            }
            let Some(&(_start, raised_end)) = raised.first() else {
                return Err(failures);
            };
            let Some(plus) = rightmost_ink(
                image,
                header,
                image.width / 2,
                &[ink(tokens::ink()), ink(tokens::ink_dim())],
                24,
            ) else {
                failures.push(format!("{scene}: no chip ink in the header at all"));
                return Err(failures);
            };
            let chip_index = state.active + 1;
            let count = state.workspaces.len();
            // The chips are the same width, so the distance from the raised chip's end to the `+`
            // shortens by about one chip per workspace to its right. The claim is the **last** chip for
            // a scene that ends on the newest workspace, and chip 2 for the one on workspace 2.
            let expected_chip = match scene {
                // `Focus on new workspace` makes one and switches to it: the last chip.
                "workspaces-new" | "workspaces-new-remote" => count,
                // The move switches to the workspace it moved the pane into.
                "workspaces-moved" => 2,
                // The waterfall's scene stays on the wall it opened the menu on.
                _ => 1,
            };
            let distance = plus - raised_end;
            if chip_index != expected_chip {
                failures.push(format!(
                    "{scene}: workspace {chip_index} of {count} is active, and this scene means \
                     workspace {expected_chip}"
                ));
            }
            if distance > 0.0 {
                lines.push(format!(
                    "{scene}: workspace {chip_index} of {count} active, its chip {distance:.0} px \
                     left of the `+`"
                ));
            }

            match scene {
                "workspaces-menu" => {
                    let Some(menu) = state.menu() else {
                        failures.push("workspaces-menu: no menu is open".into());
                        return Err(failures);
                    };
                    if menu.open != Some("menu-move") {
                        failures.push(format!(
                            "workspaces-menu: the open submenu is {:?}, and the scene opens the \
                             waterfall",
                            menu.open
                        ));
                    }
                    let items = crate::ui::menu_items(&state, menu.pane);
                    let Some(move_row) =
                        items.iter().find(|item| item.label == crate::ui::MENU_MOVE)
                    else {
                        failures.push("workspaces-menu: the menu has no waterfall row".into());
                        return Err(failures);
                    };
                    let names: Vec<&str> = move_row
                        .children
                        .iter()
                        .map(|child| child.label.as_str())
                        .collect();
                    if names != ["Straits watch", "Workspace 3", "Workspace 4"] {
                        failures.push(format!(
                            "workspaces-menu: the waterfall offers {names:?}, and it lists every \
                             workspace but this pane's own, by name"
                        ));
                    } else {
                        lines.push(
                            "workspaces-menu: the waterfall offers the three workspaces this pane is \
                             not in, by name"
                                .to_string(),
                        );
                    }
                    // **Two surfaces**, and the second is to the right of the first: along a row that
                    // crosses both, the menu's own fill (`bg()`) appears as two runs.
                    let Some(rect) = state.pane_rect(menu.pane) else {
                        failures.push("workspaces-menu: the menu's pane is gone".into());
                        return Err(failures);
                    };
                    let Some((ox, oy, pw, panel_h)) = image.find_panel_rect(ink(tokens::panel()))
                    else {
                        failures.push("workspaces-menu: no picture panel".into());
                        return Err(failures);
                    };
                    // **Clamped to the panel the capture has**: the state's pane rect is in the window's
                    // points (a scene never runs a layout pass), and the rows below the panel are the
                    // window's own background — the same `bg()` the menu is, which would read as a
                    // surface spanning the whole picture.
                    let top = (oy + f64::from(rect.y) + 4.0).min(oy + panel_h - 4.0);
                    let bottom =
                        (oy + f64::from(rect.y) + f64::from(rect.h) - 4.0).min(oy + panel_h - 4.0);
                    let band = (top, bottom.max(top + 1.0));
                    // A hairline apart: the two surfaces must not be merged into one run, and the gap
                    // between them is `GAP / 2`.
                    let surfaces: Vec<(f64, f64)> =
                        colour_runs_in_band(image, band, ink(tokens::bg()), 4, 2)
                            .into_iter()
                            // The picture's rounded corners show the window's own background, which is
                            // the same colour the menu is; a surface that starts inside the panel is a
                            // surface, and one at its edge is the window.
                            .filter(|(start, end)| *start > ox && *end < ox + pw)
                            .collect();
                    if surfaces.len() < 2 {
                        failures.push(format!(
                            "workspaces-menu: {} menu surface(s) on screen — the waterfall is a second \
                             one beside the first (band {band:?}, runs {surfaces:?})",
                            surfaces.len()
                        ));
                    } else {
                        lines.push(format!(
                            "workspaces-menu: {} menu surfaces, the second at x={:.0}",
                            surfaces.len(),
                            surfaces[1].0
                        ));
                    }
                }
                "workspaces-moved" => {
                    let source = state.workspaces.first().expect("workspace 1");
                    let target = state.workspaces.get(1).expect("workspace 2");
                    let maps = source
                        .panes
                        .iter()
                        .filter(|pane| pane.content.is_map())
                        .count();
                    if maps != 1 {
                        failures.push(format!(
                            "workspaces-moved: workspace 1 has {maps} map panes, and the move must \
                             not take its other pane"
                        ));
                    }
                    match target.panes.first() {
                        Some(pane) if pane.news.is_some() => lines.push(format!(
                            "workspaces-moved: workspace 2 holds the newsroom pane {} and workspace 1 \
                             its map",
                            pane.id
                        )),
                        other => failures.push(format!(
                            "workspaces-moved: workspace 2 holds {:?}, and the moved pane was the \
                             newsroom",
                            other.map(|pane| &pane.content)
                        )),
                    }
                    if let Some(pane) = target.panes.first().and_then(|pane| pane.news.as_ref())
                        && pane.stories.len() != 25
                    {
                        failures.push(format!(
                            "workspaces-moved: the moved pane draws {} rows, and it carried 25 from \
                             the page it had read",
                            pane.stories.len()
                        ));
                    }
                    // The picture is the **feed**, not the map: three tabs, and no marker ink. A moved
                    // pane that lost its content, or a map that came along, is red here.
                    let Some((ox, oy, pw, ph)) = image.find_panel_rect(ink(tokens::panel())) else {
                        failures.push("workspaces-moved: no picture panel".into());
                        return Err(failures);
                    };
                    let _ = pw;
                    let accent =
                        count_colour_in(image, (ox, oy, pw, ph), ink(tokens::accent()), 24);
                    if accent > 0 {
                        failures.push(format!(
                            "workspaces-moved: {accent} accent pixels in the pane — the moved pane is \
                             a feed, and a feed projects nothing (the workspace's map is not on screen)"
                        ));
                    } else {
                        lines.push(
                            "workspaces-moved: the moved feed draws no marker ink".to_string(),
                        );
                    }
                }
                _ => {
                    let source = state.workspaces.first().expect("workspace 1");
                    if source.panes.len() != 1 {
                        failures.push(format!(
                            "workspaces-new: workspace 1 has {} panes, and the action keeps its \
                             source pane",
                            source.panes.len()
                        ));
                    }
                    let last = state.workspaces.last().expect("the new workspace");
                    match last.panes.first() {
                        Some(pane) if pane.content.is_map() => lines.push(
                            "workspaces-new: the new workspace holds one new map pane, and the source \
                             workspace keeps its own"
                                .to_string(),
                        ),
                        other => failures.push(format!(
                            "workspaces-new: the new workspace holds {:?}, and the source pane was a \
                             map",
                            other.map(|pane| &pane.content)
                        )),
                    }
                    if state.workspaces.len() != 4 {
                        failures.push(format!(
                            "workspaces-new: {} workspaces, and the wall opens with three plus the \
                             one this action made",
                            state.workspaces.len()
                        ));
                    }
                }
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
    // **Everything but the focused pane, and every workspace but this one** (#166): the reset's end state, in the
    // machine. The picture half is the golden — one chip and one pane are what it is a picture of.
    if scene == "close-all" {
        let panes = state.panes().len();
        if state.workspaces.len() != 1 {
            failures.push(format!(
                "close-all: {} workspaces, and `workspace.close_others` keeps exactly one — the one on screen",
                state.workspaces.len()
            ));
        }
        if panes != 1 {
            failures.push(format!(
                "close-all: {panes} panes in the workspace the wall is on, and `pane.close_others` keeps exactly \
                 one — the one with the keys"
            ));
        }
        if state.active != 0 {
            failures.push(format!(
                "close-all: the wall is on workspace {}, and the workspace it was on is the one that stays",
                state.active
            ));
        }
        if state.intents_applied != state.pending_intents.len() {
            failures.push(format!(
                "close-all: {} of {} steps ran, so the wall is not the state the script meant",
                state.intents_applied,
                state.pending_intents.len()
            ));
        }
        if failures.is_empty() {
            lines.push(format!(
                "close-all: one workspace, one pane ({}), focused, after {} steps",
                state.focused(),
                state.intents_applied
            ));
        }
    }

    // **The menu's ring is a value in the machine** (#164): the capture is of the row the keyboard is on, so the
    // structure half asks which row that is.
    if scene == "menu-ring" {
        let ring = state.menu().and_then(|menu| menu.ring);
        if ring != Some(crate::scene::MENU_RING_ROW) {
            failures.push(format!(
                "menu-ring: the ring is {ring:?}, and the scene put it on row {}",
                crate::scene::MENU_RING_ROW
            ));
        } else {
            lines.push(format!(
                "menu-ring: the ring is on row {}, the row Enter acts on",
                crate::scene::MENU_RING_ROW
            ));
        }
    }

    // **The cursor scenes' structure** (#167): the press reached the hull, its card is up, and the cursor's
    // visibility is the one thing the two scenes disagree about.
    if matches!(scene, "cursor-hull" | "cursor-idle") {
        let focused = state.focused();
        if state.selected_in(focused) != Some(crate::scene::CURSOR_UNIT) {
            failures.push(format!(
                "{scene}: the pane's selection is {:?}, and a remote's press on hull {} must select it",
                state.selected_in(focused),
                crate::scene::CURSOR_UNIT
            ));
        } else {
            lines.push(format!(
                "{scene}: hull {} selected by the cursor's press",
                crate::scene::CURSOR_UNIT
            ));
        }
        if state.card_in(focused).is_none() {
            failures.push(format!("{scene}: a selected hull has no card"));
        }
        let Some(cursor) = state.cursor else {
            failures.push(format!("{scene}: no cursor in the state"));
            return Err(failures);
        };
        let visible = cursor.visible_at(2.0);
        let want = scene == "cursor-hull";
        if visible != want {
            failures.push(format!(
                "{scene}: the cursor is {} at 2 s, and this scene's steps are at {:.2} s — \
                 Cursor::FADE is {:.1} s",
                if visible { "visible" } else { "faded" },
                cursor.moved_at,
                crate::ui::Cursor::FADE
            ));
        }
    }

    let frame = scene_frame();
    let markers = crate::ui::markers::place(
        &state.picture,
        &frame,
        &state.focused_pane().view.filter,
        state.now(),
        state.stale_after,
        state.focused_pane().view.range,
        &|_| None,
    );

    match scene {
        "panes" | "panes-chrome" | "panes-drag" | "panes-pan" | "panes-gaps"
        | "panes-gap-resize" | "panes-named" | "panes-max" | "panes-range"
        // The workspaces scene is this wall with two more (empty) workspaces behind it, and the switch
        // scene is the same wall before its click: both are two panes with a marker per reported hull.
        | "workspaces" | "workspaces-switch" => {
            // One marker per hull with a fix **in every pane**: a pane that drew nothing, or drew the
            // same set twice, is not a view.
            let expected = state
                .picture
                .units()
                .iter()
                .filter(|unit| unit.position.is_some())
                .count();
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
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
                    &pane.view.filter,
                    state.now(),
                    state.stale_after,
                    pane.view.range,
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
        "pane-tree" | "pane-tree-narrow" | "pane-tree-zoom" | "pane-tree-pan" => {
            // **A tree pane is a pane, and it is not a map** (#116). It has no frame — so nothing is
            // projected into it (`INV-04`) — and it draws one node per post of the structure it names,
            // under the supreme command. The pane beside it is still a map with its own frame.
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                state.pane_viewport(),
                state.settings.pane_gap,
            );
            let trees: Vec<crate::ui::ViewId> = rects
                .iter()
                .filter(|(id, _)| state.pane(*id).is_some_and(|pane| !pane.content.is_map()))
                .map(|(id, _)| *id)
                .collect();
            let maps: Vec<crate::ui::ViewId> = rects
                .iter()
                .filter(|(id, _)| state.pane(*id).is_some_and(|pane| pane.content.is_map()))
                .map(|(id, _)| *id)
                .collect();
            if trees.len() != 1 || maps.len() != 1 {
                failures.push(format!(
                    "{scene}: {} tree panes and {} map panes — the scene is one of each",
                    trees.len(),
                    maps.len()
                ));
            }
            for id in &trees {
                let pane = state.pane(*id).expect("the tree pane");
                if pane.frame.is_some() {
                    failures.push(format!(
                        "{scene}: tree pane {id} has a frame — INV-04 holds literally, and a tree \
                         draws nothing that projects"
                    ));
                }
                match crate::ui::tree::tree_for(&state, *id) {
                    Some(tree) => lines.push(format!(
                        "{scene}: tree pane {id} draws {} posts and {} connectors, and has no frame",
                        tree.nodes.len(),
                        tree.links.len()
                    )),
                    None => failures.push(format!(
                        "{scene}: tree pane {id} draws no tree at all"
                    )),
                }
            }
        }
        // **The newsroom** (#136): a pane holds its own tab, page and rows, the rows are the wire's,
        // and absence draws a dash rather than a code or a zero. Read from the same `State` the capture
        // draws, because a text recogniser is not what this gate is.
        "newsroom"
        | "newsroom-scopes"
        | "newsroom-page-2"
        | "newsroom-story"
        | "newsroom-story-back" => {
            let rects = crate::ui::view::panes_on(
                state.layout().expect("the scene has a tiling"),
                state.pane_viewport(),
                state.settings.pane_gap,
            );
            let feeds: Vec<(crate::ui::ViewId, &crate::ui::newsroom::View)> = rects
                .iter()
                .filter_map(|(id, _)| {
                    state
                        .pane(*id)
                        .and_then(|pane| pane.news.as_ref())
                        .map(|view| (*id, view))
                })
                .collect();
            let maps = rects
                .iter()
                .filter(|(id, _)| state.pane(*id).is_some_and(|pane| pane.content.is_map()))
                .count();
            let wire = |name: &str| crate::scene::newsroom_page_fixture(name);
            let international_1 = wire("newsroom-international-1.json");
            let international_2 = wire("newsroom-international-2.json");
            let regional_1 = wire("newsroom-regional-1.json");
            for page in [&international_1, &international_2, &regional_1] {
                if let Err(error) = page {
                    failures.push(format!("{scene}: {error}"));
                }
            }

            // No tab for the API's fourth scope: the pane's tab is always one of the three drawn.
            for (id, view) in &feeds {
                if !crate::ui::newsroom::SCOPES
                    .iter()
                    .any(|(scope, _)| *scope == view.scope)
                {
                    failures.push(format!(
                        "{scene}: pane {id} is on {:?}, which has no tab — `unknown` is a real scope and \
                         not one the panel offers, because `min_confidence=0.5` empties it",
                        view.scope
                    ));
                }
            }

            match scene {
                "newsroom" => {
                    if feeds.len() != 1 || maps != 1 {
                        failures.push(format!(
                            "newsroom: {} feed pane(s) and {maps} map pane(s) — the scene is the feed \
                             beside the map, which is the wall the operator has",
                            feeds.len()
                        ));
                    }
                    if let Some((id, view)) = feeds.first() {
                        if view.scope != crate::data::types::NewsScope::International {
                            failures.push(format!(
                                "newsroom: pane {id} opens on {:?}, and the panel opens on International",
                                view.scope
                            ));
                        }
                        match &view.pagination {
                            Some(pagination)
                                if pagination.current_page == 1
                                    && pagination.total_pages == 6
                                    && pagination.total_records == 128 => lines.push(format!(
                                    "newsroom: pane {id} on page 1 of 6, 128 stories"
                                )),
                            Some(pagination) => failures.push(format!(
                                "newsroom: the footer says page {} of {} and the wire's metadata says \
                                 1 of 6 — a page is what the server said, not what the press assumed",
                                pagination.current_page, pagination.total_pages
                            )),
                            None => failures
                                .push("newsroom: the pane has no pages: the reply never landed".into()),
                        }
                        if view.page != 1 {
                            failures.push(format!(
                                "newsroom: the pane shows page {}, and the scene asks page 1",
                                view.page
                            ));
                        }
                        match (&international_1, view.first_title()) {
                            (Ok(page), Some(first)) if page.rows[0].title == first => lines
                                .push(format!(
                                    "newsroom: the first row is the wire's {:?}",
                                    page.rows[0].title
                                )),
                            (Ok(page), other) => failures.push(format!(
                                "newsroom: the first row is {other:?}, and the wire's first story is \
                                 {:?} — the rows are the reply's, in its order",
                                page.rows[0].title
                            )),
                            _ => {}
                        }
                        if view.stories.len() != 25 {
                            failures.push(format!(
                                "newsroom: {} rows drawn from a 25-row page",
                                view.stories.len()
                            ));
                        }
                        // Absence is not zero, in the words the row draws.
                        if let Some(no_country) = view.stories.iter().find(|s| s.country.is_none())
                        {
                            let line = crate::ui::newsroom::when_and_where(no_country);
                            if line.ends_with('—') {
                                lines.push(format!(
                                    "newsroom: a story with no country draws {line:?}"
                                ));
                            } else {
                                failures.push(format!(
                                    "newsroom: a story with no country draws {line:?} — an absent \
                                     country is a dash, never an empty code or a place (INV-01)"
                                ));
                            }
                        } else {
                            failures.push(
                                "newsroom: the fixture has no story without a country, so the absence \
                                 rule is not covered by this capture"
                                    .into(),
                            );
                        }
                        if let Some(no_place) = view.stories.iter().find(|story| {
                            story
                                .country
                                .as_ref()
                                .is_some_and(|country| country.place.is_none())
                        }) {
                            let line = crate::ui::newsroom::when_and_where(no_place);
                            if line.ends_with("· —") {
                                lines.push(format!(
                                    "newsroom: a country without a city draws {line:?}"
                                ));
                            } else {
                                failures.push(format!(
                                    "newsroom: a country without a city draws {line:?} — the place is \
                                     a dash and the country is still shown"
                                ));
                            }
                        }
                    }
                }
                "newsroom-page-2" => {
                    let Some((id, view)) = feeds.first() else {
                        failures.push("newsroom-page-2: no feed pane".into());
                        return Err(failures);
                    };
                    if view.page != 2 {
                        failures.push(format!(
                            "newsroom-page-2: the pane shows page {}, and the scene pressed Next once",
                            view.page
                        ));
                    }
                    if view
                        .pagination
                        .is_none_or(|pagination| pagination.current_page != 2)
                    {
                        failures.push(
                            "newsroom-page-2: the footer does not say page 2 — it moves when the reply \
                             lands, and the reply did"
                                .into(),
                        );
                    }
                    match (&international_2, view.first_title()) {
                        (Ok(page), Some(first)) if page.rows[0].title == first => lines.push(format!(
                            "newsroom-page-2: pane {id} draws page 2's first story {:?}",
                            page.rows[0].title
                        )),
                        (Ok(page), other) => failures.push(format!(
                            "newsroom-page-2: the first row is {other:?}, and page 2 of the wire starts \
                             with {:?} — page 2 is not page 2 at the read size",
                            page.rows[0].title
                        )),
                        _ => {}
                    }
                    if let (Ok(one), Some(first)) = (&international_1, view.first_title())
                        && one.rows[0].title == first
                    {
                        failures.push(format!(
                            "newsroom-page-2: the pane draws page 1's first story {first:?} — the Next \
                             press reached nothing, or the reply was applied as page 1"
                        ));
                    }
                }
                "newsroom-scopes" => {
                    if feeds.len() != 2 {
                        failures.push(format!(
                            "newsroom-scopes: {} feed panes — the scene is two, on two tabs",
                            feeds.len()
                        ));
                    }
                    let international = feeds.iter().find(|(_, view)| {
                        view.scope == crate::data::types::NewsScope::International
                    });
                    let regional = feeds
                        .iter()
                        .find(|(_, view)| view.scope == crate::data::types::NewsScope::Regional);
                    let (
                        Some((international_id, international_view)),
                        Some((regional_id, regional_view)),
                    ) = (international, regional)
                    else {
                        failures.push(
                            "newsroom-scopes: the two panes are not one International and one Regional \
                             — a wall-wide scope would leave both on one tab"
                                .into(),
                        );
                        return Err(failures);
                    };
                    if let (Ok(page), Some(first)) =
                        (&international_1, international_view.first_title())
                        && page.rows[0].title != first
                    {
                        failures.push(format!(
                            "newsroom-scopes: the International pane draws {first:?}, and its own wire \
                             page starts with {:?}",
                            page.rows[0].title
                        ));
                    }
                    if let (Ok(page), Some(first)) = (&regional_1, regional_view.first_title())
                        && page.rows[0].title != first
                    {
                        failures.push(format!(
                            "newsroom-scopes: the Regional pane draws {first:?}, and the regional wire \
                             page starts with {:?}",
                            page.rows[0].title
                        ));
                    }
                    if international_view.first_title() == regional_view.first_title() {
                        failures.push(
                            "newsroom-scopes: both panes draw the same first row — the tab press in one \
                             pane moved the wall (or reached neither)"
                                .into(),
                        );
                    } else {
                        lines.push(format!(
                            "newsroom-scopes: pane {international_id} draws the International page and \
                             pane {regional_id} the Regional one, and their first rows differ"
                        ));
                    }
                    if regional_view
                        .pagination
                        .is_none_or(|pagination| pagination.total_pages != 3)
                        || international_view
                            .pagination
                            .is_none_or(|pagination| pagination.total_pages != 6)
                    {
                        failures.push(
                            "newsroom-scopes: the two panes do not hold their own pages — a shared read \
                             would give both the same footer"
                                .into(),
                        );
                    }
                }
                // **A story's provenance** (#137): three outlets, oldest first, and the absences the
                // wire can carry drawn as they are — a flagged source in words, a country that is not
                // there as a dash, and a measured tone of zero still drawn.
                "newsroom-story" => {
                    let Some((id, view)) = feeds.first() else {
                        failures.push("newsroom-story: no feed pane".into());
                        return Err(failures);
                    };
                    if view.selected != Some(crate::scene::NEWSROOM_STORY) {
                        failures.push(format!(
                            "newsroom-story: the pane has {:?} selected, and the scene presses story {}",
                            view.selected,
                            crate::scene::NEWSROOM_STORY
                        ));
                    }
                    match view.detail.as_ref() {
                        Some(crate::ui::newsroom::Detail::Ready(detail)) => {
                            let story = &detail.story;
                            if detail.articles.len() != 3 || story.source_count != 3 {
                                failures.push(format!(
                                    "newsroom-story: {} articles and a `source_count` of {} — the \
                                     recorded story carried three documents, and the header is the \
                                     wire's number",
                                    detail.articles.len(),
                                    story.source_count
                                ));
                            } else {
                                lines.push(format!(
                                    "newsroom-story: pane {id} draws 3 documents and `3 sources`"
                                ));
                            }
                            let seen: Vec<chrono::DateTime<chrono::Utc>> =
                                detail.articles.iter().map(|article| article.seen_at).collect();
                            if !seen.windows(2).all(|pair| pair[0] <= pair[1]) {
                                failures.push(
                                    "newsroom-story: the articles are not oldest first — the wire's \
                                     order is provenance, who had it first"
                                        .into(),
                                );
                            } else {
                                lines.push(
                                    "newsroom-story: the articles are oldest first".to_string(),
                                );
                            }
                            let domains: Vec<&str> = detail
                                .articles
                                .iter()
                                .map(|article| article.domain.as_str())
                                .collect();
                            if domains != ["rappler.com", "en.antaranews.com", "malaysiasun.com"] {
                                failures.push(format!(
                                    "newsroom-story: the domains are {domains:?}, and the fixture's \
                                     provenance order is rappler, antaranews, malaysiasun"
                                ));
                            }
                            match detail.articles.iter().find(|article| article.flagged) {
                                Some(flagged)
                                    if crate::ui::newsroom::article_facts(flagged)
                                        .contains("unclassified source") =>
                                {
                                    lines.push(format!(
                                        "newsroom-story: the flagged source draws the words {:?}",
                                        crate::ui::newsroom::article_facts(flagged)
                                    ));
                                }
                                Some(flagged) => failures.push(format!(
                                    "newsroom-story: a flagged source draws {:?} — an unclassified \
                                     domain is a question for an operator, and it is a word",
                                    crate::ui::newsroom::article_facts(flagged)
                                )),
                                None => failures.push(
                                    "newsroom-story: the fixture has no flagged article, so the words \
                                     are not covered by this capture"
                                        .into(),
                                ),
                            }
                            if story.country.is_some() {
                                failures.push(
                                    "newsroom-story: the fixture's story has a country, so the dash \
                                     the ticket asks for is not covered"
                                        .into(),
                                );
                            } else if crate::ui::newsroom::country_line(story) == "—" {
                                lines.push(
                                    "newsroom-story: a story with no country draws \"—\""
                                        .to_string(),
                                );
                            } else {
                                failures.push(format!(
                                    "newsroom-story: a story with no country draws {:?}",
                                    crate::ui::newsroom::country_line(story)
                                ));
                            }
                            if detail.articles.iter().any(|article| article.tone == Some(0.0)) {
                                lines.push(
                                    "newsroom-story: a measured tone of 0.0 is still drawn".to_string(),
                                );
                            } else {
                                failures.push(
                                    "newsroom-story: no article has a measured tone of 0.0, so the \
                                     absence-is-not-zero case is not covered here"
                                        .into(),
                                );
                            }
                        }
                        other => failures.push(format!(
                            "newsroom-story: the pane's detail is {other:?} — the reply never landed"
                        )),
                    }
                }
                "newsroom-story-back" => {
                    // The scene leaves **page 2 open on a story**; the script clicks `back`. This half of
                    // the claim is the state the capture started from, and the pixel arm is the list the
                    // capture ends with — together they say the word reached the handler and that `back`
                    // returned to the page the operator left.
                    let Some((id, view)) = feeds.first() else {
                        failures.push("newsroom-story-back: no feed pane".into());
                        return Err(failures);
                    };
                    if view.page != 2
                        || view
                            .pagination
                            .is_none_or(|pagination| pagination.current_page != 2)
                    {
                        failures.push(format!(
                            "newsroom-story-back: the pane is on page {} and the scene pressed Next \
                             once — `back` must not return to page 1",
                            view.page
                        ));
                    }
                    if view.selected != Some(crate::scene::NEWSROOM_STORY_PAGE_2) {
                        failures.push(format!(
                            "newsroom-story-back: the pane has {:?} selected, and the scene presses \
                             story {} on page 2",
                            view.selected,
                            crate::scene::NEWSROOM_STORY_PAGE_2
                        ));
                    }
                    if matches!(view.detail, Some(crate::ui::newsroom::Detail::Ready(_))) {
                        lines.push(format!(
                            "newsroom-story-back: pane {id} opens page 2 on a story, which the \
                             script's `back` must close"
                        ));
                    } else {
                        failures.push(
                            "newsroom-story-back: the story's detail never landed, so the click has \
                             nothing to close"
                                .into(),
                        );
                    }
                }
                _ => {}
            }
        }
        // **An empty workspace projects nothing** (#138): no tiling, so no pane, so no frame for a mark
        // to be placed against — `INV-04` holds literally. The wall beside it keeps its panes and they
        // are not drawn.
        "workspaces-empty" | "workspaces-named" => {
            if state.layout().is_some() || state.pane_count() != 0 {
                failures.push(format!(
                    "{scene}: the active workspace holds {} panes and a tiling",
                    state.pane_count()
                ));
            }
            if state
                .workspaces
                .first()
                .is_some_and(|workspace| workspace.panes.is_empty())
            {
                failures.push(format!(
                    "{scene}: workspace 1 has no panes to hide, so the capture proves nothing about a \
                     hidden workspace"
                ));
            }
            if failures.is_empty() {
                lines.push(format!(
                    "{scene}: no tiling, nothing projected, workspace 1's panes not drawn"
                ));
            }
        }
        // **A moved feed is a workspace's only pane, and the map is not on screen** (#140): nothing in
        // the active workspace projects, because a feed has no frame (`INV-04` is vacuous there).
        "workspaces-moved" => {
            if state.pane_count() != 1 {
                failures.push(format!(
                    "workspaces-moved: the active workspace holds {} panes, and the move put one there",
                    state.pane_count()
                ));
            }
            if !state.panes().iter().all(|pane| pane.news.is_some()) {
                failures.push(
                    "workspaces-moved: a pane in the active workspace is not the moved feed".into(),
                );
            }
            let hidden_maps = state
                .workspaces
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != state.active)
                .flat_map(|(_, workspace)| workspace.panes.iter())
                .filter(|pane| pane.content.is_map())
                .count();
            if hidden_maps != 1 {
                failures.push(format!(
                    "workspaces-moved: {hidden_maps} map panes outside the active workspace, and the \
                     source workspace keeps exactly one"
                ));
            }
            if failures.is_empty() {
                lines.push(
                    "workspaces-moved: the moved feed is the workspace's only pane, and workspace 1's \
                     map is not drawn"
                        .to_string(),
                );
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

    if scene == "pane-menu" && state.menu().is_none() {
        failures.push(
            "pane-menu: the scene has no menu open, so the capture shows nothing of it".into(),
        );
    }

    // **The operational layer is drawn in every pane** (#110): the data is the wall's, the frame is
    // the pane's. This scene's panes both hide nothing, so every mark is placed in both; the case where
    // one pane hides a domain is `panes-filter`, below. A pane that drew no placement, or drew another
    // pane's projection, is the bug the capture's pixel half names.
    if scene == "objects" {
        let expected = state.placements.len();
        let rects = crate::ui::view::panes_on(
            state.layout().expect("the scene has a tiling"),
            state.pane_viewport(),
            state.settings.pane_gap,
        );
        if rects.len() != 2 {
            failures.push(format!(
                "objects: {} panes for a scene that is the case for two",
                rects.len()
            ));
        }
        for (id, rect) in &rects {
            let Some(pane) = state.pane(*id) else {
                continue;
            };
            let frame = crate::ui::view::synthetic_frame(pane.camera, *rect, 1.0);
            let placed = crate::ui::objects::place(
                &state.placements,
                &state.manifest,
                &frame,
                &pane.view.filter,
            );
            if placed.len() != expected {
                failures.push(format!(
                    "objects: pane {id} draws {} placements for {expected} operational marks",
                    placed.len()
                ));
            }
        }
        if failures.is_empty() {
            lines.push(format!(
                "objects: {expected} placements in {} panes",
                rects.len()
            ));
        }
    }

    // **The places, not the hulls** (#109), in the structure as well as in the pixels: one mark per
    // located facility in each pane, the unlocated row absent, and the facility selection in the
    // focused pane only.
    if scene == "installations" {
        let located = state
            .installations
            .iter()
            .filter(|row| row.coordinates().is_some())
            .count();
        let rects = crate::ui::view::panes_on(
            state.layout().expect("the scene has a tiling"),
            state.pane_viewport(),
            state.settings.pane_gap,
        );
        if rects.len() != 2 {
            failures.push(format!(
                "installations: {} panes for a scene that is the case for two",
                rects.len()
            ));
        }
        for (id, rect) in &rects {
            let Some(pane) = state.pane(*id) else {
                continue;
            };
            let frame = crate::ui::view::synthetic_frame(pane.camera, *rect, 1.0);
            let placed = crate::ui::installations::place(
                &state.installations,
                &state.installation_types,
                &state.manifest,
                &frame,
                &pane.view.filter,
            );
            if placed.len() != located {
                failures.push(format!(
                    "installations: pane {id} draws {} marks for {located} located facilities",
                    placed.len()
                ));
            }
        }
        let focused = state.focused();
        let neighbour = state
            .pane_rects()
            .into_iter()
            .map(|(id, _)| id)
            .find(|id| *id != focused);
        if state.installation_in(focused).is_none() {
            failures.push(
                "installations: the focused pane has no facility selected, and the scene selects one"
                    .to_string(),
            );
        }
        if neighbour.is_some_and(|id| state.installation_in(id).is_some()) {
            failures.push(
                "installations: a facility is selected in the neighbour too — the selection is the \
                 pane's (#99), and a card in every pane is the wall-wide bug in a new costume"
                    .to_string(),
            );
        }
        if failures.is_empty() {
            lines.push(format!(
                "installations: {located} located facilities marked in {} panes, one selected",
                rects.len()
            ));
        }
    }

    // **The filter is the pane's** (#112), in the structure as well as in the pixels: the pane that
    // hides the buoy's domain places none of the recorded marks, and the pane beside it places all
    // three. The two panes hold the same camera, so the only difference is the filter.
    if scene == "panes-filter" {
        let rects = crate::ui::view::panes_on(
            state.layout().expect("the scene has a tiling"),
            state.pane_viewport(),
            state.settings.pane_gap,
        );
        if rects.len() != 2 {
            failures.push(format!(
                "panes-filter: {} panes for a scene that is the case for two",
                rects.len()
            ));
        }
        let filtered = state.pane_rects()[0].0;
        let expected = state.placements.len();
        for (id, rect) in &rects {
            let Some(pane) = state.pane(*id) else {
                continue;
            };
            let frame = crate::ui::view::synthetic_frame(pane.camera, *rect, 1.0);
            let placed = crate::ui::objects::place(
                &state.placements,
                &state.manifest,
                &frame,
                &pane.view.filter,
            );
            let want = if *id == filtered { 0 } else { expected };
            if placed.len() != want {
                failures.push(format!(
                    "panes-filter: pane {id} places {} of {expected} marks, and its filter asks for \
                     {want} — the filter is the pane's, and the objects layer is the layer it filters",
                    placed.len()
                ));
            }
        }
        if state.filtering() != Some(filtered) {
            failures.push(format!(
                "panes-filter: the scene's panel is open on {:?}, not on the filtered pane",
                state.filtering()
            ));
        }
        if failures.is_empty() {
            lines.push(format!(
                "panes-filter: {expected} marks in the unfiltered pane, 0 in the filtered one, one \
                 camera each"
            ));
        }
    }

    // **One card per pane, and its own** (#99): the selection belongs to a window, so the question is
    // asked per pane. A wall-wide count would pass with the same card in every pane — which is the bug
    // the operator photographed.
    let cards: Vec<(crate::ui::ViewId, i64)> = state
        .panes()
        .iter()
        .filter_map(|pane| {
            pane.view
                .card
                .as_ref()
                .map(|card| (pane.id, card.unit.id_unit))
        })
        .collect();
    match (scene, cards.len()) {
        ("card-open", 1) => lines.push("card-open: exactly one card, in one pane".to_string()),
        ("card-open", other) => {
            failures.push(format!("card-open: {other} cards, and INV-10 says one per pane"))
        }
        // **A cursor's press opens one card too** (#167): the hull its point was on, and the capture is the
        // proof that a remote's press is the same press a hand makes.
        ("cursor-hull" | "cursor-idle", 1) => {
            lines.push(format!("{scene}: one card, opened by the cursor's press"))
        }
        // Two panes, one card: the pane that was pressed on, and not its neighbour.
        ("panes-cards", 1) => lines.push("panes-cards: one card, in one pane".to_string()),
        ("panes-cards", other) => failures.push(format!(
            "panes-cards: {other} cards — a card in every pane means the selection is still the wall's \
             and not the pane's (#99)"
        )),
        (_, 0) => {}
        (name, other) => failures.push(format!("{name}: {other} cards with nothing selected")),
    }
    if scene == "card-open"
        && let Some((_, hull)) = cards.first()
        && *hull != scene::CARD_UNIT
    {
        failures.push(format!(
            "card-open: the card is for hull {hull}, not {}",
            scene::CARD_UNIT
        ));
    }
    // The panes-cards scene draws its card in the first pane, and the second pane must have nothing: the
    // *absence* is the claim, and it is checked here so the picture and the structure say the same thing.
    if scene == "panes-cards"
        && let Some((pane, hull)) = cards.first()
    {
        if *pane != state.pane_rects()[0].0 {
            failures.push(format!(
                "panes-cards: the card is in pane {pane}, not the pane that was pressed ({})",
                state.pane_rects()[0].0
            ));
        }
        if *hull != scene::CARD_UNIT {
            failures.push(format!(
                "panes-cards: the card is for hull {hull}, not {}",
                scene::CARD_UNIT
            ));
        }
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
    /// Pixels whose largest channel difference is **exactly one step**: a rasteriser's rounding.
    pub rounding: usize,
    /// Pixels whose largest channel difference is **more than one step**: a different colour, or ink
    /// where the golden has water — the pixels a moved label or a shifted edge makes.
    pub changed: usize,
    /// The largest single-channel difference, 0–255.
    pub max_delta: u8,
}

impl Difference {
    /// Identical, which is what two renders of one scene must be on one machine.
    pub const NONE: Difference = Difference {
        differing: 0,
        rounding: 0,
        changed: 0,
        max_delta: 0,
    };

    pub fn is_identical(self) -> bool {
        self.differing == 0
    }
}

/// The bounds one golden is compared within (#130).
///
/// **Two counts, because one count cannot tell noise from content.** A rasteriser that rounds a glyph
/// edge differently changes hundreds of pixels by **one step** — that is the same picture. A label that
/// moved, an edge that shifted, or a token colour that changed moves pixels by **more than one step** —
/// that is a different picture. One budget for both made CI red for a year of text-heavy scenes (373
/// rounding pixels against a bound of 200) while a moved label could hide behind the number that had to
/// grow to accept it.
///
/// `rounding` is still **bounded**, not ignored: a whole panel one step off is tens of thousands of
/// rounding pixels and fails, which is the falsifier the original budget existed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    /// The farthest one channel may differ, in 255ths. Two is a rounding step; scaled text on lavapipe
    /// was measured at six, so the two scenes with scaled text allow eight.
    pub tolerance: u8,
    /// Pixels that differ by **more than one step**: a moved glyph, a shifted edge, a new colour.
    pub changed: usize,
    /// Pixels that differ by exactly one step. Generous enough for a rasteriser's text, still small
    /// enough that a panel-sized shift fails.
    pub rounding: usize,
}

impl Bounds {
    pub const fn new(tolerance: u8, changed: usize, rounding: usize) -> Bounds {
        Bounds {
            tolerance,
            changed,
            rounding,
        }
    }
}

/// The bounds every golden is compared with unless its scene measured differently.
pub const DEFAULT_BOUNDS: Bounds = Bounds::new(2, 200, 600);

/// The bounds for one scene: the defaults, except where a measurement on CI said otherwise.
///
/// The numbers are **measured, then given a margin**, and printed on every run. A scene added later
/// gets the defaults; a scene whose text is scaled by a transform — where two rasterisers put glyph
/// coverage on different subpixel phases — is the case that needs a stated exception.
pub fn golden_bounds(scene: &str) -> Bounds {
    match scene {
        // **Hundreds of one-step pixels** (#110, #112, #127): labels, the filter panel's rows, and a
        // zoomed tree. Measured 229–416 on lavapipe; the change budget is the ordinary 200, because
        // these are rounding, not moved glyphs. The rounding allowance is the measurement plus about
        // half, so a driver that moves a panel still fails.
        "objects" => Bounds::new(2, 200, 450),
        "panes-filter" => Bounds::new(2, 200, 550),
        "pane-tree-zoom" => Bounds::new(2, 200, 600),
        // The tree canvas draws 12 pt text at a transform. Until Rune `205b8d4` its run cells were
        // rasterized at the GPU's scale and lavapipe differed by 6/255; #599 rasterizes them at the
        // text's own factor, and both CI runs now measure **1/255**, so the 8/255 exception this
        // scene used to need is gone.
        "pane-tree" | "pane-tree-pan" => Bounds::new(2, 200, 450),
        // **The newsroom's rows** (#136, #137): 12 pt text at 1:1, nine visible rows, three tabs. CI's
        // lavapipe measured 656 px (`newsroom-scopes`), 785 (`newsroom-page-2`) and 828 (`newsroom`) as
        // one-step rounding; the allowance is the measurement plus about half, and the change budget
        // stays the ordinary 200, so a row that moved is still red. The two story scenes set the same
        // 12 pt rows, so they share the allowance until CI measures their own number.
        "newsroom"
        | "newsroom-page-2"
        | "newsroom-scopes"
        | "newsroom-story"
        | "newsroom-story-back"
        // **A moved feed is the same nine rows of 12 pt text** (#140): CI's lavapipe measured 791
        // one-step pixels there, the newsroom scenes' class of rounding.
        | "workspaces-moved" => Bounds::new(2, 200, 1_200),
        _ => DEFAULT_BOUNDS,
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
    let mut rounding = 0;
    let mut changed = 0;
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
            // One step is a rasteriser's rounding; anything further is ink or colour that moved.
            if delta == 1 {
                rounding += 1;
            } else {
                changed += 1;
            }
            max_delta = max_delta.max(delta);
        }
    }
    Ok(Difference {
        differing,
        rounding,
        changed,
        max_delta,
    })
}

/// Whether a difference is within the stated bounds: no channel further than `tolerance`, no more than
/// `changed` pixels off by more than a step, and no more than `rounding` pixels off by exactly one.
///
/// **All three are needed**, and this is the whole argument. `tolerance` alone lets a whole panel shift
/// by a rounding step; a single budget lets a moved label hide behind the number that had to grow to
/// accept a rasteriser's text; and an unbounded rounding allowance would never notice a panel one step
/// off. Together they say "the picture is the same picture, rendered by a slightly different machine" —
/// and nothing more.
pub fn within(diff: Difference, bounds: Bounds) -> Result<(), String> {
    if diff.max_delta > bounds.tolerance {
        return Err(format!(
            "a channel differs by {} (tolerance {}): {} pixels differ in total",
            diff.max_delta, bounds.tolerance, diff.differing
        ));
    }
    if diff.changed > bounds.changed {
        return Err(format!(
            "{} pixels changed by more than one step (budget {}): {} pixels differ in total, the              largest by {} /255",
            diff.changed, bounds.changed, diff.differing, diff.max_delta
        ));
    }
    if diff.rounding > bounds.rounding {
        return Err(format!(
            "{} pixels differ by one step (budget {}): {} pixels differ in total",
            diff.rounding, bounds.rounding, diff.differing
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
                within(diff, Bounds::new(0, 0, 0)).is_ok(),
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
                    rounding: 23,
                    changed: 0,
                    max_delta: 1
                }
            );
            assert!(
                within(diff, DEFAULT_BOUNDS).is_ok(),
                "within the stated bounds"
            );
            assert!(
                within(diff, Bounds::new(0, 0, 0)).is_err(),
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
            let reason = within(diff, DEFAULT_BOUNDS).expect_err("a channel moved 170 steps");
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
            let reason = within(diff, DEFAULT_BOUNDS)
                .expect_err("1600 rounding pixels is more than the rounding budget");
            assert!(reason.contains("1600"), "{reason}");
            assert!(
                within(diff, Bounds::new(2, 200, 2000)).is_ok(),
                "and a rounding budget of 2000 would allow it"
            );
        }

        /// **The falsifier the budget exists for** (#130): a label that moved. Its pixels are ink
        /// where the golden has water and water where it had ink, so they differ by far more than a
        /// rounding step — the change budget catches a text-sized block moved by a visible amount, even
        /// though the rounding allowance is three times the old budget.
        #[test]
        fn a_moved_label_fails_the_change_budget_though_rounding_is_generous() {
            let mut a = image(320, 80, [10, 20, 30, 255]);
            let mut b = image(320, 80, [10, 20, 30, 255]);
            // A label-sized block of ink at x=20, and the same block eight pixels to the right.
            for y in 20..36 {
                for x in 20..220 {
                    a.set(x, y, [240, 240, 235, 255]);
                }
                for x in 28..228 {
                    b.set(x, y, [240, 240, 235, 255]);
                }
            }
            // A rasteriser's own noise on top: 200 pixels one step off, which is not a change.
            for i in 0..200 {
                b.set(i % 320, 60 + i / 320, [11, 20, 30, 255]);
            }
            let diff = difference(&a, &b).expect("same size");
            assert!(
                diff.changed > DEFAULT_BOUNDS.changed,
                "a moved label is {} changed pixels, over the {} budget",
                diff.changed,
                DEFAULT_BOUNDS.changed
            );
            assert!(
                diff.rounding <= DEFAULT_BOUNDS.rounding,
                "the rounding is inside its own allowance: {}",
                diff.rounding
            );
            // A real golden would also fail the tolerance (the moved ink is a different colour); the
            // point here is that the **count** catches it even when the tolerance is generous.
            let reason = within(
                diff,
                Bounds::new(255, DEFAULT_BOUNDS.changed, DEFAULT_BOUNDS.rounding),
            )
            .expect_err("a moved label is a change");
            assert!(reason.contains("more than one step"), "{reason}");
        }

        /// **No scene relaxes the colour contract** (#127, #130): scaled text used to need 8/255,
        /// and Rune `205b8d4` rasterizes run cells at the text's own factor instead, so the measured
        /// difference on lavapipe fell to 1/255. The tolerance is two steps everywhere; only the
        /// rounding *count* is per scene, because a rasteriser's text has more of those.
        #[test]
        fn the_scaled_text_scenes_need_no_tolerance_exception() {
            for scene in [
                "pane-tree",
                "pane-tree-pan",
                "pane-tree-zoom",
                "objects",
                "panes-filter",
                "installations",
            ] {
                assert_eq!(
                    golden_bounds(scene).tolerance,
                    DEFAULT_BOUNDS.tolerance,
                    "{scene} keeps the two-step contract"
                );
            }
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
