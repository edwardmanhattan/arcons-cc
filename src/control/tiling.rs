//! **The tiling, as a contract** (#220): the geometry a client that is not this wall has to agree with.
//!
//! A phone lays the same arrangement into **its own rectangle**. Three numbers decide where a drop lands
//! and where a pane's edge is, and none of them is on the wire:
//!
//! | number | where | what a client that guesses it gets wrong |
//! | --- | --- | --- |
//! | [`tokens::PANE_EDGE_BAND`] = 0.3 | `ui/tokens.rs` | inside the band of a pane's span an edge wins; outside it is a swap. Guess 20% and the phone sends `centre` where the wall's own hand sends `left` |
//! | [`tokens::MIN_PANE_PT`] = 160 | `ui/tokens.rs` | the width below which the clamp stops a split, so a client drawing its own rectangles draws a pane the wall will not |
//! | [`crate::config::layout::RATIO_RANGE`] = 0.05…0.95 | `config/layout.rs` | the share a pane may take, which is what a `pane.resize` is clamped to (#219) |
//!
//! Plus the gap the drawn rectangle is inset by ([`tokens::GAP`] = 8): the cells are the tiling, the drawn
//! panes are the cells pulled in by it.
//!
//! Those numbers are in this file's recording rather than in a sentence a client's author has to trust, and
//! so are **the rectangles they produce**: one JSON file per case under `fixtures/control/tiling/`, written by
//! `cargo run -- --record`. A case is a *wall* — built through the gestures the operator has, never written
//! by hand — so a recording of an arrangement the wall would not draw cannot be in the directory at all, and a
//! test says so.
//!
//! **The numbers are not copied into the file by this module's arithmetic.** [`record_one`] reads them from
//! the tokens and the layout config every time it writes, so a changed token changes the recording — and the
//! test that every file's numbers are the wall's fails until a human re-records, which is the moment to ask
//! whether a client in another language is still right.
//!
//! What a case carries:
//!
//! | field | what it is |
//! | --- | --- |
//! | `viewport` | the panel's own points — the rectangle the tiling lands in |
//! | `numbers` | the four numbers above, as the tokens say them |
//! | `panes` | the pane **ids**, in the order the tiling's leaves refer to them |
//! | `tiling` | the arrangement, in `layout.toml`'s spelling ([`SavedNode`]): leaves are indices into `panes` |
//! | `cells` | [`Layout::cells`] — what a point is hit-tested against, and what a client must reproduce |
//! | `rects` | [`view::panes_on`] at `numbers.gap` — the rectangles the operator sees |
//! | `drops` | what a release at a point does ([`Layout::drop_for`]): a retile on an edge, a swap, or nothing |
//!
//! The last one is here because the band's failure is not a cosmetic one: it is the phone and the wall
//! disagreeing about what the operator did, which is the thing this plane exists to prevent. A client replays
//! `drops` against its own reading of the band instead of discovering the difference by dragging a pane on
//! somebody's wall.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::layout::{RATIO_RANGE, SavedNode};
use crate::map::geometry::Viewport;
use crate::ui::ViewId;
use crate::ui::tokens;
use crate::ui::view::{self, Dir, Drop, Layout, Rect, Side};

/// Where the recordings live, relative to the crate root. `cargo run -- --record` writes here.
pub const DIR: &str = "fixtures/control/tiling";

/// The viewport every case is recorded in unless it is a wall of its own: the crate's default window. The
/// wall's real viewport is the picture panel, which is not the window; a client is given a rectangle and
/// mirrors the arithmetic, so the rectangle here is whatever the case says.
const CASE_VIEWPORT: Viewport = Viewport {
    w: tokens::WINDOW_SIZE.0 as f64,
    h: tokens::WINDOW_SIZE.1 as f64,
};

/// **One case**: a tiling the wall would have, and the rectangle it lands in.
pub struct Case {
    /// The file's name, without `.json`. It is the case's identity, so it is also the case's only name.
    pub name: &'static str,
    /// The rectangle the tiling lands in, in the panel's own points.
    pub viewport: Viewport,
    /// The tiling, built through the gestures — a split, a dragged border — so it is a wall and not a shape
    /// somebody typed.
    pub layout: Layout,
}

/// The cases `--record` writes, one file each. **The list is the corpus**: a file no case names is an orphan
/// and a case with no file is missing, and one test says both.
///
/// Every tiling here is built with [`Layout::split_with`] and [`Layout::set_ratio`] — the functions the pane
/// menu and the gutter drag call — so a case cannot be an arrangement the wall refuses to draw.
pub fn cases() -> Vec<Case> {
    // Three of the shapes need a second step — a split inside a split, or a split in a viewport the clamp has
    // to halve — so they are built first. The list below says what each case records.
    let mut three = two(Dir::Horizontal, Some(0.58));
    three.split_with(2, Dir::Vertical, 3);

    let mut four = two(Dir::Horizontal, None);
    four.split_with(1, Dir::Vertical, 3);
    four.split_with(2, Dir::Vertical, 4);

    let mut small = Layout::single_with(1);
    small.split_with(1, Dir::Horizontal, 2);

    vec![
        // One pane: the identity, and the only shape where each of the four numbers has nothing to say.
        Case {
            name: "one-pane",
            viewport: CASE_VIEWPORT,
            layout: Layout::single_with(1),
        },
        // Two panes, an even split: the shape a wall is in after somebody asks for a second pane.
        Case {
            name: "side-by-side",
            viewport: CASE_VIEWPORT,
            layout: two(Dir::Horizontal, None),
        },
        // The same two panes with the border dragged to 58%: the share `panes` sets in code, and the value a
        // client sends as a `pane.resize` (#219). An edge is still not the middle, at any ratio.
        Case {
            name: "side-by-side-dragged",
            viewport: CASE_VIEWPORT,
            layout: two(Dir::Horizontal, Some(0.58)),
        },
        // Stacked: the same numbers on the other axis, because a client that reads a span off the wrong edge
        // gets the side-by-side case right and this one wrong.
        Case {
            name: "stacked",
            viewport: CASE_VIEWPORT,
            layout: two(Dir::Vertical, None),
        },
        // **The clamp at the bottom**: a border dragged all the way left. The layout keeps the ratio and the
        // rectangle function stops it at `MIN_PANE_PT`, so this case is where a client's own numbers either
        // match the wall's or draw a pane the wall never draws.
        Case {
            name: "dragged-to-nothing",
            viewport: CASE_VIEWPORT,
            layout: two(Dir::Horizontal, Some(0.0)),
        },
        // The tightest share a resize may ask for (0.05 of 1280 is 64 pt), which the clamp then lifts to
        // `MIN_PANE_PT`. **The clamp and the range are two rules**, and this is the case that says so.
        Case {
            name: "narrowest-share",
            viewport: CASE_VIEWPORT,
            layout: two(Dir::Horizontal, Some(RATIO_RANGE.0)),
        },
        // Three panes: the right half split again, where a point at 50% of the *window* is 90% of the column
        // under it. A band measured against the window fails here and nowhere else.
        Case {
            name: "three",
            viewport: CASE_VIEWPORT,
            layout: three,
        },
        // Four panes, nested two deep on both sides: every edge of the middle column is a band that is not
        // the window's, on both axes.
        Case {
            name: "four-nested",
            viewport: CASE_VIEWPORT,
            layout: four,
        },
        // A viewport too small to hold two panes on its axis: the split halves rather than starving one, so
        // a client's `MIN_PANE_PT` rule has to know when it stops applying. 300 < 2 × 160.
        Case {
            name: "window-too-small",
            viewport: Viewport { w: 300.0, h: 200.0 },
            layout: small,
        },
    ]
}

/// Two panes side by side or stacked, with the border dragged to `ratio` when one is named.
fn two(dir: Dir, ratio: Option<f32>) -> Layout {
    let mut layout = Layout::single_with(1);
    layout.split_with(1, dir, 2);
    if let Some(ratio) = ratio {
        let border = layout
            .borders(CASE_VIEWPORT)
            .into_iter()
            .next()
            .expect("a split has a border");
        layout.set_ratio(&border, ratio);
    }
    layout
}

/// One case, recorded: every number and rectangle a client would otherwise have to guess.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tiling {
    /// The rectangle the tiling lands in, in the panel's own points.
    pub viewport: Size,
    /// The four numbers, as the tokens and the layout config say them today.
    pub numbers: Numbers,
    /// The pane ids, in the order the `tiling`'s leaves refer to them.
    pub panes: Vec<ViewId>,
    /// The arrangement, in `layout.toml`'s spelling: a leaf names its pane by index into `panes`.
    pub tiling: SavedNode,
    /// [`Layout::cells`]: the tiling, which is what a point is hit-tested against.
    pub cells: Vec<Cell>,
    /// [`view::panes_on`] at `numbers.gap`: the rectangles the operator sees.
    pub rects: Vec<Cell>,
    /// What a release at a point does, for the probes that pin the band.
    pub drops: Vec<Probe>,
}

/// A viewport, as the file writes it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

/// **The numbers a client must not guess.**
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Numbers {
    /// The smallest a pane may be dragged to: [`tokens::MIN_PANE_PT`].
    pub min_pane_pt: f32,
    /// The share of a pane's span, from an edge, that reads as "drop against this edge": the band.
    pub edge_band: f32,
    /// The space between two drawn panes: every cell is pulled in by half of it on each side.
    pub gap: f32,
    /// The share a split may take: [`RATIO_RANGE`], which is what a resize is clamped to.
    pub ratio_min: f32,
    pub ratio_max: f32,
}

/// One pane's rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub pane: ViewId,
    pub rect: Rect,
}

/// **A release, recorded**: a hand holding `dragged` let go at `at`, in the viewport's points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub dragged: ViewId,
    pub at: (f32, f32),
    /// `null` when the release does nothing: over the dragged pane's own slot, or outside every pane.
    pub drop: Option<DropRecord>,
}

/// A [`Drop`], in the file's words. `side` is the word `pane.drop` takes (#219), so a client reads one
/// vocabulary for both.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DropRecord {
    /// The two panes trade places: `a` is the pane that was under the pointer, `b` the one in the air.
    Swap { a: ViewId, b: ViewId },
    /// `dragged` takes this edge of `target`.
    Retile { target: ViewId, side: Edge },
}

/// Which edge of a pane a drop names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl From<Side> for Edge {
    fn from(side: Side) -> Self {
        match side {
            Side::Left => Edge::Left,
            Side::Right => Edge::Right,
            Side::Top => Edge::Top,
            Side::Bottom => Edge::Bottom,
        }
    }
}

impl From<Drop> for DropRecord {
    fn from(drop: Drop) -> Self {
        match drop {
            Drop::Swap { a, b } => DropRecord::Swap { a, b },
            Drop::Retile { target, side, .. } => DropRecord::Retile {
                target,
                side: Edge::from(side),
            },
        }
    }
}

impl Tiling {
    /// The viewport, as the layout asks for it.
    pub fn viewport(&self) -> Viewport {
        Viewport::new(self.viewport.w, self.viewport.h)
    }

    /// **The tiling, as a layout** — through `layout.toml`'s own reader ([`crate::ui::saved::load_shape`]),
    /// so a recording and a saved arrangement cannot disagree about what a split is.
    ///
    /// `None` when the tree names no pane, or one pane twice: that is not a tiling of these panes, and the
    /// caller should say so rather than draw half a wall.
    pub fn layout(&self) -> Option<Layout> {
        let shape = crate::ui::saved::load_shape(&self.tiling, &self.panes)?;
        Layout::from_shape(shape)
    }
}

/// The numbers, read from the tokens and the layout config — **never** written as literals here.
pub fn numbers() -> Numbers {
    Numbers {
        min_pane_pt: tokens::MIN_PANE_PT,
        edge_band: tokens::PANE_EDGE_BAND,
        gap: tokens::GAP,
        ratio_min: RATIO_RANGE.0,
        ratio_max: RATIO_RANGE.1,
    }
}

/// One case, as the file that records it: the wall's own answer, taken from the wall's own functions.
pub fn record_one(case: &Case) -> Tiling {
    let viewport = case.viewport;
    let numbers = numbers();
    let layout = &case.layout;
    let panes = layout.ids();
    let shape = layout.shape().expect("a layout has a shape");
    Tiling {
        viewport: Size {
            w: viewport.w,
            h: viewport.h,
        },
        numbers,
        cells: layout
            .cells(viewport)
            .into_iter()
            .map(|(pane, rect)| Cell { pane, rect })
            .collect(),
        rects: view::panes_on(layout, viewport, numbers.gap)
            .into_iter()
            .map(|(pane, rect)| Cell { pane, rect })
            .collect(),
        drops: probes(layout, viewport),
        tiling: crate::ui::saved::save_shape(&shape, &panes)
            .expect("every pane is in the list its shape is recorded against"),
        panes,
    }
}

/// The probes that pin the band, for one layout: the middle and each edge of every pane but the dragged
/// one, **and the point just outside each band**, which is the half a client's reading of `0.3` gets wrong.
///
/// The points are fractions of a *pane's own rectangle*, never of the window: a band measured against the
/// window is the bug this recording exists to catch, and the fractions are what a client will use.
fn probes(layout: &Layout, viewport: Viewport) -> Vec<Probe> {
    let cells = layout.cells(viewport);
    let Some(dragged) = cells.last().map(|(id, _)| *id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let release = |at: (f32, f32)| Probe {
        dragged,
        at,
        drop: layout.drop_for(viewport, dragged, at).map(DropRecord::from),
    };
    for (target, rect) in &cells {
        if *target == dragged {
            continue;
        }
        let at = |fx: f32, fy: f32| (rect.x + rect.w * fx, rect.y + rect.h * fy);
        for point in [
            // In the band, then a step outside it: 0.25 reads as an edge, 0.35 does not. The pair is the
            // band's number made checkable from the outside, on both axes.
            at(0.25, 0.5),
            at(0.35, 0.5),
            at(0.75, 0.5),
            at(0.65, 0.5),
            at(0.5, 0.25),
            at(0.5, 0.35),
            at(0.5, 0.75),
            at(0.5, 0.65),
            // The middle of the pane, which is a swap and never an edge.
            at(0.5, 0.5),
        ] {
            out.push(release(point));
        }
    }
    // Over the dragged pane's own slot, and outside the wall: both are **no drop**, and a client that
    // previewed a landing for either would show a pane moving where the operator let it go.
    if let Some((_, rect)) = cells.last() {
        out.push(release((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)));
    }
    out.push(release((
        viewport.w as f32 + 10.0,
        viewport.h as f32 + 10.0,
    )));
    out
}

/// The bytes `--record` writes for one case: pretty JSON and a final newline, so the file diffs line by line.
pub fn bytes(tiling: &Tiling) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(tiling).expect("a recording is a plain data structure")
    )
}

/// The directory the recordings live in, from the crate root — the same root `env!("CARGO_MANIFEST_DIR")`
/// gives the tests, so `--record` and the test that reads what it wrote cannot mean two directories.
pub fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DIR)
}

/// The recorded files, sorted by name.
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    out.sort();
    out
}

/// One recorded file, read. The error is a sentence, because the caller is a test that has to say what is
/// wrong with the corpus: a file that will not parse is a `--record` run that was not re-run.
pub fn read(path: &Path) -> Result<Tiling, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| {
        format!(
            "{} is not a tiling this wall recorded — a field is missing, renamed or hand-written, so a \
             client reading it would have to guess it: {error}",
            path.display()
        )
    })
}

/// **Write the corpus**: one file per case, from [`record_one`].
///
/// It is a tool, not a capture: nothing here reads the wall's arrangement or its settings, so a recording is
/// the same bytes on every machine — `INV-11`'s rule, for a file that is compared rather than drawn.
pub fn record(dir: &Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("cannot make {}: {error}", dir.display()))?;
    let mut written = Vec::new();
    for case in cases() {
        let path = dir.join(format!("{}.json", case.name));
        std::fs::write(&path, bytes(&record_one(&case)))
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every case names its own file: a case with no name, or two cases after the same file, is a corpus
    /// that cannot say which wall it records.
    #[test]
    fn every_case_has_a_name_of_its_own() {
        let cases = cases();
        assert!(!cases.is_empty(), "the corpus is empty");
        let mut names: Vec<&str> = cases.iter().map(|case| case.name).collect();
        for name in &names {
            assert!(!name.is_empty(), "a case with no name writes `.json`");
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            cases.len(),
            "two cases would write the same file, so one of them would never be recorded"
        );
    }

    /// **A case is a wall.** Every tiling in the corpus covers its viewport exactly, keeps every split
    /// inside the range a file may carry, and leaves every pane drawable — because it was built through the
    /// gestures rather than typed, and this test says so before a fixture can claim otherwise.
    #[test]
    fn every_case_is_a_wall() {
        for case in cases() {
            let tiling = record_one(&case);
            let viewport = tiling.viewport();
            assert!(
                viewport.is_usable(),
                "{}: a case with no viewport has nothing to tile",
                case.name
            );
            let area: f64 = tiling
                .cells
                .iter()
                .map(|cell| f64::from(cell.rect.w) * f64::from(cell.rect.h))
                .sum();
            let viewport_area = viewport.w * viewport.h;
            assert!(
                (area - viewport_area).abs() < 0.5,
                "{}: the cells cover {area} of {viewport_area} — a case with a gap or an overlap is not a \
                 wall the panes could tile",
                case.name
            );
            let min = f64::from(tokens::MIN_PANE_PT);
            for cell in &tiling.cells {
                assert!(
                    f64::from(cell.rect.w) >= min - 0.001 || viewport.w < min * 2.0,
                    "{}: pane {} is {} pt wide in a {} pt viewport, and {} is the smallest pane the wall \
                     draws — the clamp would have stopped this split, so the case is not a wall",
                    case.name,
                    cell.pane,
                    cell.rect.w,
                    viewport.w,
                    tokens::MIN_PANE_PT
                );
                assert!(
                    f64::from(cell.rect.h) >= min - 0.001 || viewport.h < min * 2.0,
                    "{}: pane {} is {} pt tall, and {} is the smallest pane the wall draws",
                    case.name,
                    cell.pane,
                    cell.rect.h,
                    tokens::MIN_PANE_PT
                );
            }
            assert!(
                every_ratio(&tiling.tiling),
                "{}: a split outside {RATIO_RANGE:?} is a share the wall clamps before it draws, so these \
                 rectangles are ones nothing would produce",
                case.name
            );
        }
    }

    fn every_ratio(node: &SavedNode) -> bool {
        match (&node.pane, node.split, &node.a, &node.b) {
            (Some(_), None, None, None) => true,
            (None, Some(_), Some(a), Some(b)) => {
                node.ratio
                    .is_some_and(|ratio| (RATIO_RANGE.0..=RATIO_RANGE.1).contains(&ratio))
                    && every_ratio(a)
                    && every_ratio(b)
            }
            _ => false,
        }
    }
}
