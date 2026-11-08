//! The hull's mark: a MIL-STD-2525 / APP-6 symbol, composed from a frame and an icon.
//!
//! This is the *drawing* half. What a symbol may honestly claim is decided in
//! `docs/research/nato-symbology.md` (#33), and the answer is short: **one frame, one hull icon, and
//! the label does the rest.** Minos carries no affiliation, no echelon and no modifiers, so the frame is
//! the standard's *unknown* quatrefoil (blue is "your side", and nobody has said), the dimension is
//! surface (every hull in the live feed reports a beacon from the water), and the icon is a generic
//! surface vessel until the operator's categories are mapped to icons one by one.
//!
//! Two numbers come from #33's measurement, not from taste:
//!
//! - **the symbol is drawn at [`tokens::SYMBOL`] (24 pt)**, because at the old 12 pt marker the icon
//!   inside the frame was 6.9 pt across with a 0.86 pt stroke — a dot with a halo;
//! - **the stroke is a ratio of the box** (`0.072 × 24 = 1.73 pt`), because a literal is chunky at one
//!   size and invisible at another, and the frame must stay a frame at every size.
//!
//! The geometry is a `rune::Shape` in a unit view box: the quatrefoil is the standard's canonical
//! outline (four cubic lobes on an inner square of side 74/200), and the icon is the filled arrow the
//! standard uses for a surface combatant — the one sea icon that TFG's contact sheet showed reads at
//! this size (`docs/research/sheets/wall-symbol-icons.svg`).

use rune::{El, el, shape::Shape};

use crate::ui::tokens;
use crate::units::Glyph;

/// The icon's ink, as a polygon in the unit box: the standard's surface-combatant arrow.
///
/// Points are (x, y) with y DOWN, as Rune's own geometry is. Taken from the generated table in the
/// sister project (`SurfaceCombatant`), which is milsymbol's geometry — MIT, © 2017 Mans Beckman, and
/// named in `THIRD-PARTY-NOTICES.md`.
const SURFACE: &[[f32; 2]] = &[
    [0.500, 0.900],
    [0.000, 0.560],
    [0.300, 0.600],
    [0.300, 0.400],
    [0.400, 0.400],
    [0.400, 0.300],
    [0.100, 0.300],
    [0.100, 0.200],
    [0.400, 0.200],
    [0.400, 0.100],
    [0.600, 0.100],
    [0.600, 0.200],
    [0.900, 0.200],
    [0.900, 0.300],
    [0.600, 0.300],
    [0.600, 0.400],
    [0.700, 0.400],
    [0.700, 0.600],
    [1.000, 0.560],
];

/// How much of the box the icon may occupy, for the quatrefoil frame.
///
/// Derived, not tuned: the frame takes the outside of the box, and the icon's reach is
/// `2 · (50 / 138.5 − 0.072)` — half the canonical icon box scaled to this frame's height, less one
/// stroke so the icon never touches the frame (`docs/research/nato-symbology.md` §2). At 24 pt that is
/// 13.9 pt of icon, which is why the symbol is drawn this large in the first place.
const ICON_FIT: f32 = 0.578;

/// The standard's unknown-affiliation frame, flattened once for the life of the process.
///
/// **Flattened here, and measured**: handing Rune the cubic path for every marker costs 7.7 ms of frame
/// CPU for 200 hulls against an 8 ms budget, because each marker re-tessellates four cubics. The curve
/// is a constant, so its polyline is too — six segments per lobe, smooth at 24 pt and cheap.
static QUATREFOIL: std::sync::LazyLock<Vec<[f32; 2]>> = std::sync::LazyLock::new(|| {
    // The canonical path around (100, 100) in the standard's 200-unit box, as (control, control, end).
    const START: [f32; 2] = [63.0, 63.0];
    const LOBES: [([f32; 2], [f32; 2], [f32; 2]); 4] = [
        ([63.0, 20.0], [137.0, 20.0], [137.0, 63.0]),
        ([180.0, 63.0], [180.0, 137.0], [137.0, 137.0]),
        ([137.0, 180.0], [63.0, 180.0], [63.0, 137.0]),
        ([20.0, 137.0], [20.0, 63.0], [63.0, 63.0]),
    ];
    const PER_LOBE: usize = 6;
    let mut points = Vec::with_capacity(LOBES.len() * PER_LOBE);
    let mut from = START;
    for (c1, c2, to) in LOBES {
        for step in 1..=PER_LOBE {
            let t = step as f32 / PER_LOBE as f32;
            let u = 1.0 - t;
            points.push([
                u * u * u * from[0]
                    + 3.0 * u * u * t * c1[0]
                    + 3.0 * u * t * t * c2[0]
                    + t * t * t * to[0],
                u * u * u * from[1]
                    + 3.0 * u * u * t * c1[1]
                    + 3.0 * u * t * t * c2[1]
                    + t * t * t * to[1],
            ]);
        }
        from = to;
    }
    points
});

/// The icon in the shape's own 200-unit space, computed once: the fit and the table are constants, so
/// the arithmetic does not belong in a frame.
static ICON: std::sync::LazyLock<Vec<[f32; 2]>> = std::sync::LazyLock::new(|| fitted(SURFACE));

/// The same as a closed outline, for a hollow mark.
static ICON_CLOSED: std::sync::LazyLock<Vec<[f32; 2]>> = std::sync::LazyLock::new(|| {
    let mut points = ICON.clone();
    points.push(points[0]);
    points
});

/// The icon's points, scaled about the box's centre so the frame keeps its margin.
fn fitted(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    points
        .iter()
        // The icon table is authored in a unit box; the shape is authored in the standard's 200-unit
        // space, so this is where the two meet — one conversion, and `view_box` does the rest.
        .map(|[x, y]| {
            let (x, y) = ((x - 0.5) * ICON_FIT + 0.5, (y - 0.5) * ICON_FIT + 0.5);
            [x * 200.0, y * 200.0]
        })
        .collect()
}

/// One hull's mark: the frame, and the icon for what it is.
///
/// **Status is ours, not the standard's** (`INV-01`/`INV-03` keep their meaning): a live hull is drawn
/// in the accent, a stale one in dimmed ink, and a backfilled one is hollow — the frame is stroked and
/// the icon is not filled, which reads as an outline rather than a hull that is here now.
pub fn mark(id_unit: i64, glyph: Glyph, size_pt: f32) -> El {
    let (ink, filled) = match glyph {
        Glyph::Live => (tokens::accent(), true),
        Glyph::Stale => (tokens::ink_dim(), true),
        Glyph::History => (tokens::ink_dim(), false),
    };
    let stroke = tokens::SYMBOL_STROKE * size_pt;
    let mut shape = Shape::new()
        .view_box(200.0, 200.0)
        .stroke(QUATREFOIL.clone(), stroke, ink);
    if filled {
        shape = shape.fill(ICON.clone(), ink);
    } else {
        // Hollow: the icon is an outline too, so nothing about a backfilled hull reads as current.
        shape = shape.stroke(ICON_CLOSED.clone(), stroke * 0.7, ink);
    }
    el().key(format!("marker-{id_unit}"))
        .size(size_pt, size_pt)
        .shape(shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The size is a decision from #33's measurement, not a taste: at 12 pt the icon inside the
    /// quatrefoil was 6.9 pt across with a 0.86 pt stroke, and a symbol that cannot be read is worse
    /// than the plain glyph it replaced.
    #[test]
    fn a_mark_is_the_symbol_box_and_not_a_dot() {
        // The number is the decision, and it is pinned here so a quiet edit to the token is a failing
        // test rather than a change nobody measured.
        assert_eq!(tokens::SYMBOL, 24.0);
        // And the mark builds for every state, hollow included.
        for glyph in [Glyph::Live, Glyph::Stale, Glyph::History] {
            let _ = super::mark(12, glyph, tokens::SYMBOL);
        }
    }

    /// The icon is drawn inside the frame's inner box, never to the box's edge: `ICON_FIT` is derived
    /// from the frame's canonical height, and an icon that touches its frame is one blob.
    #[test]
    fn the_icon_keeps_clear_of_the_frame() {
        for point in fitted(SURFACE) {
            assert!(
                (0.0..=200.0).contains(&point[0]) && (0.0..=200.0).contains(&point[1]),
                "the icon stays inside the 200-unit box: {point:?}"
            );
        }
        let outside = fitted(SURFACE)
            .iter()
            .filter(|[x, y]| {
                let margin = 200.0 * (1.0 - ICON_FIT) / 2.0;
                *x < margin || *y < margin || *x > 200.0 - margin || *y > 200.0 - margin
            })
            .count();
        assert_eq!(
            outside, 0,
            "no icon vertex may sit in the frame's own band (ICON_FIT {ICON_FIT})"
        );
    }
}
