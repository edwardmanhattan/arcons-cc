//! **The chart's picture** — a graticule, the standing picture's hulls, and the row being placed.
//!
//! The engine's frame is the next commit; this draws what a chart *means* without it: one point per hull
//! that has ever reported, the selected hull ringed, every row in the draft ringed differently with a
//! dotted tether back to where that hull was last known — which is the claim the operator is about to
//! make, drawn before it leaves.
//!
//! **The camera is the UI's** (ADR-0009, decision 4). A press resolves through
//! [`crate::chart::Chart::place_at`], which is `unproject` of the camera the panel is drawing with, so the
//! coordinate a press claims is the coordinate under the pointer. Nothing here waits for a render thread.
//!
//! A drag pans and a tap places: the two gestures share one handler because the difference between them is
//! distance, not a mode. The drop is a **claim about now** — it fills the selected hull's one row and
//! stops there; sending is #278's.

use arcons_cc::map::geometry::Camera;
use arcons_cc::ui::tokens;
use rune::{El, el, row, spacer, text};

use crate::chart::{Chart, ZOOM_STEP};
use crate::ui::State;

/// How far a press may travel and still be a press rather than a pan, in points.
pub const TAP_SLOP: f64 = 4.0;

/// How far one arrow key moves the crosshair, and how far with Shift.
const NUDGE: f64 = 10.0;
const NUDGE_COARSE: f64 = 50.0;

/// The whole chart: the water, the hulls, and the controls that are not the water.
pub(super) fn view(state: &State, scale: f32) -> El {
    // The scale is the engine's business only; without the feature the chart draws in points.
    let _ = scale;
    let chart = state.chart;
    let mut layer = el()
        .key("chart")
        .w_full()
        .grow(1.0)
        .fill(tokens::bg())
        .border(tokens::HAIRLINE, tokens::card_edge())
        .radius(tokens::PANEL_RADIUS)
        // The rectangle the camera projects through is the rectangle the layout gave us, so a resized
        // window does not stretch a picture whose arithmetic still believes in the old size.
        .on_layout(|state: &mut State, [_, _, width, height]| {
            state.chart.resize(f64::from(width), f64::from(height));
        })
        .on_drag(|state: &mut State, drag, _tx| {
            match drag.phase {
                rune::Phase::Down => state.chart_origin = Some(state.chart.camera()),
                rune::Phase::Move => {
                    if let Some(origin) = state.chart_origin {
                        // Absolute, from the camera the gesture started on: summing per-event deltas
                        // drifts, and the wall learned that before this screen existed.
                        let viewport = state.chart.viewport();
                        state.chart.set_camera(arcons_cc::map::geometry::pan(
                            origin,
                            viewport,
                            f64::from(drag.dx),
                            f64::from(drag.dy),
                        ));
                    }
                }
                rune::Phase::Up => {
                    let travel = f64::from(drag.dx.abs() + drag.dy.abs());
                    state.chart_origin = None;
                    if travel < TAP_SLOP {
                        state.place_at(f64::from(drag.x), f64::from(drag.y));
                    }
                }
            }
        });

    // **The water, when there is water to draw** (#277, `--features map`).
    //
    // Absent until the thread has rendered, because a black rectangle looks like the sea at night rather
    // than like a map that is still opening; while it is absent the chart draws the graticule instead,
    // which is the structure a picture-less chart has and the one thing a basemap turns into noise.
    #[allow(unused_mut)]
    let mut water: Option<El> = None;
    #[cfg(feature = "map")]
    {
        let viewport = chart.viewport();
        let device = (
            (viewport.w * f64::from(scale)).round().max(1.0) as u32,
            (viewport.h * f64::from(scale)).round().max(1.0) as u32,
        );
        if let Some(pixels) = crate::chart::engine::pixels(chart.camera(), device)
            && crate::chart::engine::has_frame()
        {
            water = Some(el().key("chart-frame").w_full().h_full().pixels(&pixels));
        }
        // A chart with no water says why, in the corner, rather than looking like the sea at night.
        if !crate::chart::engine::has_frame() && crate::chart::engine::error().is_none() {
            let height = chart.viewport().h as f32;
            layer = layer.child(
                text("the chart is opening")
                    .key("chart-opening")
                    .font(tokens::LABEL)
                    .color(tokens::ink_dim())
                    .absolute(tokens::PAD, height - tokens::NAME_H * 2.0 - tokens::GAP),
            );
        }
        if let Some(error) = crate::chart::engine::error() {
            let height = chart.viewport().h as f32;
            layer = layer.child(
                text(format!("chart: {error}"))
                    .key("chart-error")
                    .font(tokens::LABEL)
                    .color(tokens::ink_dim())
                    .absolute(tokens::PAD, height - tokens::NAME_H * 2.0 - tokens::GAP),
            );
        }
    }

    if let Some(frame) = water {
        layer = layer.child(frame);
    } else {
        // No picture: a degree grid is the smallest honest structure, and the step follows the zoom, so a
        // one-degree line at z14 is a wall and a one-degree line at z5 is a dot. Over real water it would
        // be noise on top of a map that already says where it is.
        for line in graticule(chart) {
            layer = layer.child(line);
        }
    }
    for mark in markers(state) {
        layer = layer.child(mark);
    }
    layer
        .child(controls(state))
        .child(readout(state))
        .child(crosshair(state))
}

/// A degree grid, coarse enough to read and fine enough to place by: the step follows the zoom, because a
/// one-degree line at z14 is a wall and a one-degree line at z5 is a dot.
fn graticule(chart: Chart) -> Vec<El> {
    let viewport = chart.viewport();
    let step = if chart.camera().zoom >= 11.0 {
        0.1
    } else if chart.camera().zoom >= 8.0 {
        1.0
    } else {
        5.0
    };
    let (top_left, bottom_right) = (
        chart.place_at(0.0, 0.0),
        chart.place_at(viewport.w, viewport.h),
    );
    let mut lines = Vec::new();

    let lon_start = (top_left.1 / step).floor() * step;
    let mut lon = lon_start;
    while lon <= bottom_right.1 {
        let (x, _) = chart.project(top_left.0, lon);
        if x >= 0.0 && x <= viewport.w {
            lines.push(
                el().absolute(x as f32, 0.0)
                    .h_full()
                    .w(tokens::HAIRLINE)
                    .fill(tokens::hairline()),
            );
        }
        lon += step;
    }

    let lat_start = (bottom_right.0 / step).floor() * step;
    let mut lat = lat_start;
    while lat <= top_left.0 {
        let (_, y) = chart.project(lat, top_left.1);
        if y >= 0.0 && y <= viewport.h {
            lines.push(
                el().absolute(0.0, y as f32)
                    .w_full()
                    .h(tokens::HAIRLINE)
                    .fill(tokens::hairline()),
            );
        }
        lat += step;
    }
    lines
}

/// One dot per hull in the picture, and one ring per row in the draft.
fn markers(state: &State) -> Vec<El> {
    let chart = state.chart;
    let mut marks = Vec::new();

    for vessel in state.desk.picker().vessels() {
        let Some(last) = vessel.last.as_ref() else {
            // A hull that never reported has no place to draw. It stays in the list, where its silence
            // is a fact; on the chart there is nothing to point at, and a marker at 0,0 would be a ship
            // in the Gulf of Guinea.
            continue;
        };
        let point = chart.project(last.latitude, last.longitude);
        if !chart.holds(point) {
            continue;
        }
        let selected = state.desk.selected() == Some(vessel.id_unit);
        let colour = if selected {
            tokens::accent()
        } else {
            tokens::ink()
        };
        // **A dot has to read over water and over the panel.** The colour that carries meaning is the
        // operator's own ink; the halo is the panel's background, which is the one colour that stands
        // against a light basemap and against the empty chart alike.
        marks.push(halo(point, tokens::DESK_MARK + 3.0, tokens::bg()));
        let half = f64::from(tokens::DESK_MARK) / 2.0;
        marks.push(
            el().key(format!("hull-{}", vessel.id_unit))
                .absolute(point.0 as f32 - half as f32, point.1 as f32 - half as f32)
                .size(tokens::DESK_MARK, tokens::DESK_MARK)
                .radius(tokens::DESK_MARK / 2.0)
                .fill(colour),
        );
        if selected {
            marks.push(ring(point, tokens::DESK_RING, tokens::accent()));
        }
    }

    // The draft: where each row claims the hull is, and a dotted tether back to where it was known.
    for row in state.desk.batch().rows() {
        let (Some(lat), Some(lon)) = (row.fix.latitude, row.fix.longitude) else {
            continue;
        };
        let point = chart.project(lat, lon);
        if !chart.holds(point) {
            continue;
        }
        marks.push(ring(point, tokens::DESK_RING, tokens::accent()));
        if let Some(last) = state
            .desk
            .vessel(row.id_unit)
            .and_then(|vessel| vessel.last.as_ref())
        {
            let from = chart.project(last.latitude, last.longitude);
            for step in 1..6 {
                let t = f64::from(step) / 6.0;
                let x = from.0 + (point.0 - from.0) * t;
                let y = from.1 + (point.1 - from.1) * t;
                if chart.holds((x, y)) {
                    marks.push(
                        el().absolute(x as f32, y as f32)
                            .size(tokens::CHIP_PAD, tokens::CHIP_PAD)
                            .radius(tokens::CHIP_PAD / 2.0)
                            .fill(tokens::ink_dim()),
                    );
                }
            }
        }
    }
    marks
}

/// A disc behind a marker: the reason a light dot survives a light basemap.
fn halo(point: (f64, f64), size: f32, colour: rune::Color) -> El {
    let half = size / 2.0;
    el().absolute(point.0 as f32 - half, point.1 as f32 - half)
        .size(size, size)
        .radius(half)
        .fill(colour)
}

fn ring(point: (f64, f64), size: f32, colour: rune::Color) -> El {
    let half = size / 2.0;
    el().absolute(point.0 as f32 - half, point.1 as f32 - half)
        .size(size, size)
        .radius(half)
        .border(tokens::HAIRLINE, colour)
}

/// The corner controls: zoom, and the keyboard's way into a placement.
fn controls(state: &State) -> El {
    let mut row = row()
        .absolute(tokens::GAP, tokens::GAP)
        .gap(tokens::CARD_GAP)
        .child(chart_button("+", "zoom-in", |state: &mut State| {
            let centre = (
                state.chart.viewport().w / 2.0,
                state.chart.viewport().h / 2.0,
            );
            state.chart.zoom_at(centre, ZOOM_STEP);
        }))
        .child(chart_button("−", "zoom-out", |state: &mut State| {
            let centre = (
                state.chart.viewport().w / 2.0,
                state.chart.viewport().h / 2.0,
            );
            state.chart.zoom_at(centre, -ZOOM_STEP);
        }))
        .child(spacer());
    if state.arming {
        row = row.child(chart_button(
            "Cancel",
            "cancel-placement",
            |state: &mut State| state.disarm(),
        ));
    } else {
        row = row.child(chart_button(
            "Place",
            "arm-placement",
            |state: &mut State| state.arm(),
        ));
    }
    row.w_full().px(tokens::PAD)
}

fn chart_button(label: &'static str, key: &'static str, action: fn(&mut State)) -> El {
    el().key(key)
        .role("button")
        .label(label)
        .px(tokens::GAP)
        .py(tokens::CHIP_PAD)
        .radius(tokens::GAP)
        .fill(tokens::panel())
        .border(tokens::HAIRLINE, tokens::card_edge())
        .on_press(move |state: &mut State| action(state))
        .child(text(label).font(tokens::LABEL).color(tokens::ink()))
}

/// Zoom and the centre, in words: the chart is a measurement, so it says what it is measuring.
fn readout(state: &State) -> El {
    let camera: Camera = state.chart.camera();
    let height = state.chart.viewport().h as f32;
    text(format!(
        "z {:.1} · {:.2} S {:.2} E",
        camera.zoom, -camera.center.0, camera.center.1
    ))
    .font(tokens::LABEL)
    .color(tokens::ink_dim())
    .absolute(tokens::PAD, height - tokens::NAME_H - tokens::GAP)
}

/// The crosshair the arrows move: a placement has to be reachable without a pointer, or it is a gesture
/// half the operators cannot make.
fn crosshair(state: &State) -> El {
    if !state.arming {
        return el().key("crosshair").size(0.0, 0.0);
    }
    let (x, y) = state.crosshair;
    let size = tokens::DESK_CROSSHAIR;
    el().key("crosshair")
        .absolute(x as f32 - size / 2.0, y as f32 - size / 2.0)
        .size(size, size)
        .radius(size / 2.0)
        .border(tokens::HAIRLINE, tokens::accent())
}

/// The nudge an arrow key makes, coarse with Shift.
pub fn nudge_step(coarse: bool) -> f64 {
    if coarse { NUDGE_COARSE } else { NUDGE }
}
