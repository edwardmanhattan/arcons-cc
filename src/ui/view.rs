//! The pane tree: N views, one window, and the arithmetic that turns the tree into rectangles.
//!
//! Multi View is a tree because that is what tiling is: a pane is split, and a split remembers which
//! way and in what proportion. Two decisions are load-bearing, and both are here rather than in the
//! view:
//!
//! - **One function produces the rectangles.** The view positions panes with [`Layout::rects`] and
//!   `verify` checks a capture by calling the same function against the panel it found. There is no
//!   second layout pass — no flex `grow` ratios here, no measured offsets there — so a pane cannot be
//!   drawn in one place and asserted in another, which is the same reason `map::geometry::project` is
//!   the only projection in the crate.
//! - **The clamp lives in the split, not in the drag.** A ratio can be loaded from a saved layout or
//!   written by a future gesture; clamping where the rectangle is computed means no input can produce
//!   a zero-width pane, and a pane below [`MIN_PANE_PT`] cannot be drawn into a state the assertion
//!   disagrees with.
//!
//! Nothing here touches Rune: this module is pure, so the tree is property-tested in the gate with no
//! window, no GPU and no capture.

use crate::map::geometry::Viewport;
use crate::ui::tokens;

/// What a pane is called. Small numbers, unique for the life of the layout.
pub type ViewId = u32;

/// Which way a split runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Side by side: `a` on the left, `b` on the right. The split line is vertical.
    Horizontal,
    /// Stacked: `a` on top, `b` below. The split line is horizontal.
    Vertical,
}

/// A rectangle in the window's points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    /// Whether a point in the window's own points is inside. Used by the hit tests, which take window
    /// coordinates; the edges belong to the rectangle they open.
    pub fn contains(&self, at: (f32, f32)) -> bool {
        at.0 >= self.x && at.0 < self.right() && at.1 >= self.y && at.1 < self.bottom()
    }
    /// The longer edge, which is the direction a new pane splits along: a plot grows the way the
    /// operator's eye already reads it, not always the same way.
    pub fn longer_edge(&self) -> Dir {
        if self.w >= self.h {
            Dir::Horizontal
        } else {
            Dir::Vertical
        }
    }
}

/// The frame a **headless** pane draws with: its own laid-out rectangle, in device pixels.
///
/// A wall gets its frames from the map thread, which renders at exactly this size; a capture has no
/// thread, so the rectangle it laid the pane out at *is* the frame. One function, because `verify`
/// reconstructs the same frame to check a capture — a second copy of this rule is how a capture and
/// its assertion come to disagree about where a marker belongs.
pub fn synthetic_frame(
    camera: crate::map::geometry::Camera,
    rect: Rect,
    scale: f32,
) -> crate::map::geometry::Rendered {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    crate::map::geometry::Rendered {
        camera,
        width: (rect.w * scale).round().max(1.0) as u32,
        height: (rect.h * scale).round().max(1.0) as u32,
    }
}

/// A point in a pane's own coordinates, as the viewport knows it.
///
/// **`Drag::x`/`y` are element-local** — `runtime/input/gesture.rs` builds them from the hit node's local
/// point, and `El::on_drag` hands them over unchanged — while every question the layout answers
/// (`border_at`, `drop_for`, `Border::ratio_for`) is asked in the **viewport's** points. A handler on an
/// element that fills its pane therefore adds the pane's origin, exactly once, here.
///
/// It is not a rounding error to skip it: the point lands in whichever pane owns the window's top-left
/// corner, so the first pane behaves and the rest do not — which is how #80 survived the milestone that
/// introduced panes, and why this has a test whose pane origin is not (0, 0).
pub fn viewport_point(rect: Rect, local: (f32, f32)) -> (f32, f32) {
    (rect.x + local.0, rect.y + local.1)
}

/// A point in a pane's **handle**, as the viewport knows it.
///
/// The handle is drawn at the top centre of its pane (see `ui::handle`), so a drag that starts on it
/// reports a point local to a 28 × 4 box that is itself at an offset inside the pane. Both offsets are
/// added here, once, for the same reason as [`viewport_point`]: the layout asks in viewport points.
pub fn handle_point(rect: Rect, local: (f32, f32)) -> (f32, f32) {
    let (w, _h) = crate::ui::tokens::PANE_HANDLE;
    viewport_point(
        rect,
        (
            rect.w / 2.0 - w / 2.0 + local.0,
            crate::ui::tokens::GAP / 2.0 + local.1,
        ),
    )
}

/// Where a lifted pane **will land**: the slot `Layout::preview` puts it in.
///
/// `Layout::preview` is the layout the release will apply, so this is the pane's rectangle after the drop —
/// one function, used by the picture (the ghost stands on it and its camera is projected against it) and by
/// `verify` (which projects the pane's hulls into it). A second copy of "where the pane lands" is how a
/// capture and its assertion come to disagree about where a pane is, which is the sin `INV-04` is about.
pub fn landing_rect(layout: &Layout, drop: Drop, viewport: Viewport, pane: ViewId) -> Option<Rect> {
    layout.preview(drop)?.rect_of(viewport, pane)
}

/// A rectangle pulled in by `by` on every side: the **plate** a lifted pane is drawn on, narrower than the
/// pane so the target's own hulls stay visible around it while a drag is in the air (#70, #84).
pub fn inset_rect(rect: Rect, by: f32) -> Rect {
    Rect {
        x: rect.x + by,
        y: rect.y + by,
        w: (rect.w - by * 2.0).max(1.0),
        h: (rect.h - by * 2.0).max(1.0),
    }
}

/// A draggable border between two panes: the split it belongs to, and where the line is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    /// A pane on the `a` side, and one on the `b` side. The pair names the split exactly — an id alone
    /// cannot, because a pane has a border on each of its sides.
    pub a: ViewId,
    pub b: ViewId,
    pub dir: Dir,
    /// The line's own coordinate along the split axis.
    pub at: f32,
    /// The split's length along that axis, and where it starts, so a pointer reads as a ratio.
    pub start: f32,
    pub span: f32,
}

impl Border {
    /// The ratio a pointer asks for. Clamped to `(0, 1)`: the minimum pane size is the rectangle
    /// function's job, and a ratio of exactly 0 or 1 would have no side to grow back into.
    pub fn ratio_for(&self, pointer: f32) -> f32 {
        if self.span <= 0.0 {
            return 0.5;
        }
        ((pointer - self.start) / self.span).clamp(0.05, 0.95)
    }
}

/// Which edge of a target pane a dragged pane was released over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn dir(self) -> Dir {
        match self {
            Side::Left | Side::Right => Dir::Horizontal,
            Side::Top | Side::Bottom => Dir::Vertical,
        }
    }

    /// Whether the dropped pane takes the `a` side (left or top) of the split it creates.
    fn on_a(self) -> bool {
        matches!(self, Side::Left | Side::Top)
    }
}

/// What a released pane does to the layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drop {
    /// The two panes trade places.
    Swap { a: ViewId, b: ViewId },
    /// The dragged pane takes the `side` edge of the target, which is split in two.
    Retile {
        dragged: ViewId,
        target: ViewId,
        side: Side,
    },
}

impl Drop {
    /// The pane that moves.
    pub fn dragged(self) -> ViewId {
        match self {
            Drop::Swap { a: _, b } => b,
            Drop::Retile { dragged, .. } => dragged,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Leaf(ViewId),
    Split {
        dir: Dir,
        /// The share `a` takes, in `(0, 1)`. Clamped where the rectangles are computed.
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

/// The panes, how they tile, and which one the operator is working in.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    root: Node,
    focused: ViewId,
    next: ViewId,
}

impl Default for Layout {
    fn default() -> Self {
        Self::single()
    }
}

impl Layout {
    /// One pane filling the window: the wall as it has always been.
    pub fn single() -> Self {
        Self {
            root: Node::Leaf(1),
            focused: 1,
            next: 2,
        }
    }

    pub fn focused(&self) -> ViewId {
        self.focused
    }

    /// Every pane, left-to-right and top-to-bottom — the order the tree draws in.
    pub fn ids(&self) -> Vec<ViewId> {
        let mut ids = Vec::new();
        collect_leaves(&self.root, &mut ids);
        ids
    }

    pub fn len(&self) -> usize {
        self.ids().len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// One pane, and nothing to tile: the case a capture of the wall as it was renders.
    pub fn is_single(&self) -> bool {
        matches!(self.root, Node::Leaf(_))
    }

    pub fn contains(&self, id: ViewId) -> bool {
        self.ids().contains(&id)
    }

    /// Put the keyboard and the focus ring on a pane.
    pub fn focus(&mut self, id: ViewId) -> bool {
        if !self.contains(id) {
            return false;
        }
        self.focused = id;
        true
    }

    /// Split a pane in two, in the direction the pane's own shape asks for. The new pane becomes the
    /// focused one: the operator just asked for it, so that is where they are looking.
    pub fn split(&mut self, target: ViewId, dir: Dir) -> Option<ViewId> {
        let id = self.next;
        let mut split = false;
        self.root = replace_leaf(self.root.clone(), target, dir, id, &mut split);
        if !split {
            return None;
        }
        self.next += 1;
        self.focused = id;
        Some(id)
    }

    /// Remove a pane and give its space to its sibling: **collapse**, never a hole. The last pane
    /// cannot be closed — a window with no map is not a view, it is a bug.
    pub fn close(&mut self, id: ViewId) -> bool {
        if self.len() <= 1 || !self.contains(id) {
            return false;
        }
        let mut removed = false;
        self.root = remove_leaf(self.root.clone(), id, &mut removed);
        if !removed {
            return false;
        }
        if !self.contains(self.focused) {
            self.focused = self.ids()[0];
        }
        true
    }

    /// Set the ratio of the split a border belongs to — the drag on the border between two panes.
    ///
    /// The border names both sides, because an id names only one: with three panes, "the border at
    /// pane 2" is ambiguous between its left and its right edge, and picking the wrong one drags a
    /// line the operator is not holding.
    pub fn set_ratio(&mut self, border: &Border, ratio: f32) -> bool {
        let mut set = false;
        self.root = set_ratio_on(self.root.clone(), border, ratio, &mut set);
        set
    }

    /// The border nearest a point in the viewport's points, if the point is within `grip / 2` of it.
    ///
    /// The **deepest** border wins: in a three-pane layout the split between two panes and the outer
    /// split can be a pixel apart at a corner, and the one the operator sees under the pointer is the
    /// inner one.
    pub fn border_at(&self, viewport: Viewport, at: (f32, f32), grip: f32) -> Option<Border> {
        let mut found = None;
        find_border(
            &self.root,
            Rect {
                x: 0.0,
                y: 0.0,
                w: viewport.w as f32,
                h: viewport.h as f32,
            },
            at,
            grip.max(1.0) / 2.0,
            &mut found,
        );
        found
    }

    /// What a pane released at a point would do: swap with the pane under the middle of the pointer,
    /// re-tile against the edge it is nearest.
    ///
    /// A point over the dragged pane itself is **no drop**, which is what makes "released where it
    /// started" a no-op rather than a swap with itself.
    pub fn drop_for(&self, viewport: Viewport, dragged: ViewId, at: (f32, f32)) -> Option<Drop> {
        let target = self.pane_at(viewport, at)?;
        if target == dragged {
            return None;
        }
        let rect = self.rect_of(viewport, target)?;
        let band = crate::ui::tokens::PANE_EDGE_BAND;
        let edges = [
            (Side::Left, at.0 - rect.x, rect.w),
            (Side::Right, rect.right() - at.0, rect.w),
            (Side::Top, at.1 - rect.y, rect.h),
            (Side::Bottom, rect.bottom() - at.1, rect.h),
        ];
        let mut nearest: Option<(Side, f32)> = None;
        for (side, distance, span) in edges {
            if span <= 0.0 || distance / span >= band {
                continue;
            }
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((side, distance));
            }
        }
        Some(match nearest {
            Some((side, _)) => Drop::Retile {
                dragged,
                target,
                side,
            },
            None => Drop::Swap {
                a: target,
                b: dragged,
            },
        })
    }

    /// The layout this drop results in — **the same function the release calls**, so the gap a drag
    /// previews is the place the pane lands. A preview computed some other way is two layouts that
    /// disagree, which is the bug the whole "one function" discipline exists to prevent.
    pub fn preview(&self, drop: Drop) -> Option<Layout> {
        let mut preview = self.clone();
        preview.apply(drop).then_some(preview)
    }

    /// Perform a drop. `false` when it cannot be done (a pane that is no longer there, a swap with
    /// itself, the last pane) — in which case the layout is untouched.
    pub fn apply(&mut self, drop: Drop) -> bool {
        match drop {
            Drop::Swap { a, b } => {
                if a == b || !self.contains(a) || !self.contains(b) {
                    return false;
                }
                self.root = swap_leaves(self.root.clone(), a, b);
                true
            }
            Drop::Retile {
                dragged,
                target,
                side,
            } => {
                if dragged == target || self.len() < 2 || !self.contains(dragged) {
                    return false;
                }
                // Collapse the dragged pane out first, then split the target it lands on. The other
                // order would renumber nothing but could split a pane that is about to disappear.
                if !self.close(dragged) {
                    return false;
                }
                // **The dropped pane keeps its id**: its camera, its map thread and its frame live on
                // that id, and a re-tile that minted a new one would drop the pane's whole state and
                // open a fresh map where the operator's picture used to be.
                self.splice(target, side.dir(), side.on_a(), dragged)
                    .is_some()
            }
        }
    }

    /// Split a pane, putting the new one on a named side (the drop's side, not always `b`).
    pub fn split_side(&mut self, target: ViewId, side: Side) -> Option<ViewId> {
        let id = self.next;
        self.splice(target, side.dir(), side.on_a(), id)
    }

    /// The one place a leaf becomes a split, for a caller that already knows the id it wants — a
    /// re-tile reuses the dragged pane's id, a new pane takes the next one.
    fn splice(&mut self, target: ViewId, dir: Dir, on_a: bool, id: ViewId) -> Option<ViewId> {
        let mut split = false;
        self.root = replace_leaf_side(self.root.clone(), target, dir, id, on_a, &mut split);
        if !split {
            return None;
        }
        self.next = self.next.max(id + 1);
        self.focused = id;
        Some(id)
    }

    /// The rectangles, one per pane, **covering the viewport exactly**: no gaps between panes, no
    /// overlap, and every rectangle at least [`MIN_PANE_PT`] unless the window itself is smaller than
    /// two panes.
    pub fn rects(&self, viewport: Viewport) -> Vec<(ViewId, Rect)> {
        let mut out = Vec::new();
        walk_rects(
            &self.root,
            Rect {
                x: 0.0,
                y: 0.0,
                w: (viewport.w as f32).max(0.0),
                h: (viewport.h as f32).max(0.0),
            },
            &mut out,
        );
        out
    }

    /// One pane's rectangle, for a gesture that needs to know where it happened.
    pub fn rect_of(&self, viewport: Viewport, id: ViewId) -> Option<Rect> {
        self.rects(viewport)
            .into_iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| rect)
    }

    /// The pane under a point in the viewport's points, if any.
    pub fn pane_at(&self, viewport: Viewport, at: (f32, f32)) -> Option<ViewId> {
        self.rects(viewport)
            .into_iter()
            .find(|(_, rect)| rect.contains(at))
            .map(|(id, _)| id)
    }
}

fn collect_leaves(node: &Node, out: &mut Vec<ViewId>) {
    match node {
        Node::Leaf(id) => out.push(*id),
        Node::Split { a, b, .. } => {
            collect_leaves(a, out);
            collect_leaves(b, out);
        }
    }
}

fn replace_leaf(node: Node, target: ViewId, new_dir: Dir, id: ViewId, split: &mut bool) -> Node {
    match node {
        Node::Leaf(leaf) if leaf == target => {
            *split = true;
            Node::Split {
                dir: new_dir,
                ratio: 0.5,
                a: Box::new(Node::Leaf(leaf)),
                b: Box::new(Node::Leaf(id)),
            }
        }
        Node::Leaf(leaf) => Node::Leaf(leaf),
        Node::Split { dir, ratio, a, b } => Node::Split {
            dir,
            ratio,
            a: Box::new(replace_leaf(*a, target, new_dir, id, split)),
            b: Box::new(replace_leaf(*b, target, new_dir, id, split)),
        },
    }
}

/// Remove a leaf by replacing its parent with the sibling — the collapse that leaves no hole.
fn remove_leaf(node: Node, target: ViewId, removed: &mut bool) -> Node {
    match node {
        Node::Split { dir, ratio, a, b } => {
            if matches!(*a, Node::Leaf(leaf) if leaf == target) {
                *removed = true;
                return *b;
            }
            if matches!(*b, Node::Leaf(leaf) if leaf == target) {
                *removed = true;
                return *a;
            }
            Node::Split {
                dir,
                ratio,
                a: Box::new(remove_leaf(*a, target, removed)),
                b: Box::new(remove_leaf(*b, target, removed)),
            }
        }
        leaf => leaf,
    }
}

fn set_ratio_on(node: Node, border: &Border, ratio: f32, set: &mut bool) -> Node {
    match node {
        Node::Leaf(leaf) => Node::Leaf(leaf),
        Node::Split {
            dir,
            ratio: current,
            a,
            b,
        } => {
            if contains_leaf(&a, border.a) && contains_leaf(&b, border.b) {
                *set = true;
                let clamped = if ratio.is_finite() {
                    ratio.clamp(0.05, 0.95)
                } else {
                    current
                };
                return Node::Split {
                    dir,
                    ratio: clamped,
                    a,
                    b,
                };
            }
            Node::Split {
                dir,
                ratio: current,
                a: Box::new(set_ratio_on(*a, border, ratio, set)),
                b: Box::new(set_ratio_on(*b, border, ratio, set)),
            }
        }
    }
}

/// Depth-first, so the last border found at a point is the innermost one.
fn find_border(node: &Node, rect: Rect, at: (f32, f32), reach: f32, found: &mut Option<Border>) {
    let Node::Split { dir, ratio, a, b } = node else {
        return;
    };
    let inside = at.0 >= rect.x - reach
        && at.0 <= rect.right() + reach
        && at.1 >= rect.y - reach
        && at.1 <= rect.bottom() + reach;
    if !inside {
        return;
    }
    let (a_rect, b_rect) = split_rects(*dir, *ratio, rect);
    find_border(a, a_rect, at, reach, found);
    find_border(b, b_rect, at, reach, found);
    // `at`/`start`/`span` are the split's own axis — what a ratio is measured along. The line's
    // *extent* is the other axis, and only the hit test uses it.
    let (line, start, span) = match dir {
        Dir::Horizontal => (b_rect.x.min(rect.right()), rect.x, rect.w),
        Dir::Vertical => (b_rect.y.min(rect.bottom()), rect.y, rect.h),
    };
    let (along_from, along_to) = match dir {
        Dir::Horizontal => (rect.y, rect.bottom()),
        Dir::Vertical => (rect.x, rect.right()),
    };
    let across = match dir {
        Dir::Horizontal => at.0,
        Dir::Vertical => at.1,
    };
    let along = match dir {
        Dir::Horizontal => at.1,
        Dir::Vertical => at.0,
    };
    let on_line = (across - line).abs() <= reach;
    let on_span = along >= along_from - reach && along <= along_to + reach;
    if on_line && on_span {
        *found = Some(Border {
            a: first_leaf(a),
            b: first_leaf(b),
            dir: *dir,
            at: line,
            start,
            span,
        });
    }
}

fn swap_leaves(node: Node, a: ViewId, b: ViewId) -> Node {
    match node {
        Node::Leaf(id) if id == a => Node::Leaf(b),
        Node::Leaf(id) if id == b => Node::Leaf(a),
        Node::Leaf(id) => Node::Leaf(id),
        Node::Split {
            dir,
            ratio,
            a: na,
            b: nb,
        } => Node::Split {
            dir,
            ratio,
            a: Box::new(swap_leaves(*na, a, b)),
            b: Box::new(swap_leaves(*nb, a, b)),
        },
    }
}

fn replace_leaf_side(
    node: Node,
    target: ViewId,
    dir: Dir,
    id: ViewId,
    on_a: bool,
    split: &mut bool,
) -> Node {
    match node {
        Node::Leaf(leaf) if leaf == target => {
            *split = true;
            let (a, b) = if on_a {
                (Node::Leaf(id), Node::Leaf(leaf))
            } else {
                (Node::Leaf(leaf), Node::Leaf(id))
            };
            Node::Split {
                dir,
                ratio: 0.5,
                a: Box::new(a),
                b: Box::new(b),
            }
        }
        Node::Leaf(leaf) => Node::Leaf(leaf),
        Node::Split { dir, ratio, a, b } => Node::Split {
            dir,
            ratio,
            a: Box::new(replace_leaf_side(*a, target, dir, id, on_a, split)),
            b: Box::new(replace_leaf_side(*b, target, dir, id, on_a, split)),
        },
    }
}

fn first_leaf(node: &Node) -> ViewId {
    match node {
        Node::Leaf(id) => *id,
        Node::Split { a, .. } => first_leaf(a),
    }
}

fn contains_leaf(node: &Node, id: ViewId) -> bool {
    match node {
        Node::Leaf(leaf) => *leaf == id,
        Node::Split { a, b, .. } => contains_leaf(a, id) || contains_leaf(b, id),
    }
}

fn walk_rects(node: &Node, rect: Rect, out: &mut Vec<(ViewId, Rect)>) {
    match node {
        Node::Leaf(id) => out.push((*id, rect)),
        Node::Split { dir, ratio, a, b } => {
            let (a_rect, b_rect) = split_rects(*dir, *ratio, rect);
            walk_rects(a, a_rect, out);
            walk_rects(b, b_rect, out);
        }
    }
}

/// The two rectangles a split makes of one. **The only place the ratio becomes pixels**, so the drawn
/// panes, the border a drag grabs and a capture's assertion cannot disagree about where a pane is.
fn split_rects(dir: Dir, ratio: f32, rect: Rect) -> (Rect, Rect) {
    match dir {
        Dir::Horizontal => {
            let a_w = split_length(rect.w, ratio);
            (
                Rect { w: a_w, ..rect },
                Rect {
                    x: rect.x + a_w,
                    w: (rect.w - a_w).max(0.0),
                    ..rect
                },
            )
        }
        Dir::Vertical => {
            let a_h = split_length(rect.h, ratio);
            (
                Rect { h: a_h, ..rect },
                Rect {
                    y: rect.y + a_h,
                    h: (rect.h - a_h).max(0.0),
                    ..rect
                },
            )
        }
    }
}

/// How much of `length` the `a` side takes: the ratio, clamped so neither side falls below
/// [`MIN_PANE_PT`] unless both are already too small to hold one.
fn split_length(length: f32, ratio: f32) -> f32 {
    if length <= 0.0 {
        return 0.0;
    }
    let min = tokens::MIN_PANE_PT;
    let wanted = ratio.clamp(0.0, 1.0) * length;
    if length <= min * 2.0 {
        // Too small for two panes: split it in half rather than give one an empty rectangle.
        return length / 2.0;
    }
    wanted.clamp(min, length - min)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo's property-test generator, small enough to read: a 64-bit LCG.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 11
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn between(&mut self, low: f64, high: f64) -> f64 {
            low + (self.next() as f64 / u64::MAX as f64) * (high - low)
        }
    }

    fn viewport() -> Viewport {
        Viewport::new(1280.0, 720.0)
    }

    /// Split and close a random tree, so the property test sees shapes nobody wrote by hand.
    fn random_layout(rng: &mut Rng) -> Layout {
        let mut layout = Layout::single();
        for _ in 0..12 {
            let ids = layout.ids();
            let target = ids[rng.below(ids.len())];
            match rng.below(3) {
                0 if layout.len() < 6 => {
                    let dir = if rng.below(2) == 0 {
                        Dir::Horizontal
                    } else {
                        Dir::Vertical
                    };
                    layout.split(target, dir);
                }
                1 if layout.len() > 1 => {
                    layout.close(target);
                }
                _ => {
                    // A drag on a border: found the way the gesture finds it, from a point on the line.
                    let point = (
                        rng.between(1.0, 1279.0) as f32,
                        rng.between(1.0, 719.0) as f32,
                    );
                    if let Some(border) = layout.border_at(viewport(), point, 400.0) {
                        let ratio = rng.between(0.0, 1.0) as f32;
                        layout.set_ratio(&border, ratio);
                    }
                }
            }
        }
        layout
    }

    #[test]
    fn the_layout_covers_the_viewport_exactly() {
        let mut rng = Rng(0x5EED_0068_ABCD_1111);
        for round in 0..300 {
            let layout = random_layout(&mut rng);
            let rects = layout.rects(viewport());
            assert_eq!(
                rects.len(),
                layout.len(),
                "round {round}: {} rectangles for {} panes",
                rects.len(),
                layout.len()
            );
            let mut area = 0.0;
            for (_, rect) in &rects {
                assert!(
                    rect.w > 0.0 && rect.h > 0.0,
                    "round {round}: a pane with no area: {rect:?}"
                );
                assert!(
                    rect.x >= -0.001
                        && rect.y >= -0.001
                        && rect.right() <= viewport().w as f32 + 0.001
                        && rect.bottom() <= viewport().h as f32 + 0.001,
                    "round {round}: {rect:?} leaves the viewport"
                );
                area += f64::from(rect.w) * f64::from(rect.h);
            }
            // Coverage exactly: gaps and overlaps both break this, and neither can hide.
            let viewport_area = viewport().w * viewport().h;
            assert!(
                (area - viewport_area).abs() < 0.5,
                "round {round}: the panes cover {area} of {viewport_area} — a gap or an overlap"
            );
            for (index, (id, rect)) in rects.iter().enumerate() {
                for (other, other_rect) in rects.iter().skip(index + 1) {
                    let overlap_w =
                        (rect.right().min(other_rect.right()) - rect.x.max(other_rect.x)).max(0.0);
                    let overlap_h = (rect.bottom().min(other_rect.bottom())
                        - rect.y.max(other_rect.y))
                    .max(0.0);
                    assert!(
                        overlap_w * overlap_h < 0.0001,
                        "round {round}: panes {id} and {other} overlap by {} pt²",
                        overlap_w * overlap_h
                    );
                }
            }
        }
    }

    #[test]
    fn a_split_takes_the_pane_it_was_aimed_at_and_no_other() {
        let mut layout = Layout::single();
        let first = layout.ids()[0];
        let second = layout.split(first, Dir::Horizontal).expect("a split");
        assert_eq!(layout.len(), 2);
        assert_eq!(
            layout.focused(),
            second,
            "the new pane is where the eye goes"
        );
        assert_eq!(layout.ids(), vec![first, second], "a is left of b");
        let rects = layout.rects(viewport());
        assert_eq!(rects[0].0, first);
        assert!(
            (rects[0].1.w - 640.0).abs() < 0.001,
            "an even split of 1280 is 640, got {}",
            rects[0].1.w
        );

        // A third splits the SECOND pane, not the first.
        let third = layout.split(second, Dir::Vertical).expect("a split");
        let rects = layout.rects(viewport());
        assert_eq!(rects.len(), 3);
        let third_rect = rects.iter().find(|(id, _)| *id == third).unwrap().1;
        assert!(
            (third_rect.x - 640.0).abs() < 0.001,
            "the third pane belongs to the right half, started at {}",
            third_rect.x
        );
        assert_eq!(layout.split(999, Dir::Horizontal), None, "no such pane");
    }

    #[test]
    fn closing_a_pane_gives_its_space_to_its_sibling_and_never_leaves_a_hole() {
        let mut layout = Layout::single();
        let first = layout.ids()[0];
        let second = layout.split(first, Dir::Horizontal).unwrap();
        let _third = layout.split(second, Dir::Vertical).unwrap();

        assert!(layout.close(second), "closing a pane that has a sibling");
        let ids = layout.ids();
        assert_eq!(ids.len(), 2);
        assert!(!ids.contains(&second), "the closed pane is gone: {ids:?}");
        let rects = layout.rects(viewport());
        let area: f64 = rects
            .iter()
            .map(|(_, r)| f64::from(r.w) * f64::from(r.h))
            .sum();
        assert!(
            (area - viewport().w * viewport().h).abs() < 0.5,
            "after a close the panes still cover the viewport: {area}"
        );
        assert!(layout.contains(layout.focused()), "focus is on a live pane");

        // The last pane cannot close: a window with no map is a bug, not a view.
        let last = layout.ids()[0];
        layout.close(layout.ids()[1]);
        assert_eq!(layout.len(), 1);
        assert!(!layout.close(last), "the last pane stays");
        assert_eq!(layout.len(), 1);
    }

    #[test]
    fn a_ratio_below_the_minimum_clamps_instead_of_emptying_a_pane() {
        let mut layout = Layout::single();
        let first = layout.ids()[0];
        let second = layout.split(first, Dir::Horizontal).unwrap();
        // Drag the border all the way: the ratio is clamped, and the pane keeps a usable width.
        let border = layout
            .border_at(viewport(), (639.0, 360.0), tokens::PANE_GRIP)
            .expect("the border between the two panes");
        assert_eq!((border.a, border.b), (first, second));
        assert_eq!(border.dir, Dir::Horizontal);
        assert!(layout.set_ratio(&border, 0.0));
        let rects = layout.rects(viewport());
        let (narrow, other) = if rects[0].0 == first {
            (rects[0].1, rects[1].1)
        } else {
            (rects[1].1, rects[0].1)
        };
        assert!(
            (narrow.w - tokens::MIN_PANE_PT).abs() < 0.001,
            "a dragged-to-nothing pane is {0} pt, and {1} is the minimum",
            narrow.w,
            tokens::MIN_PANE_PT
        );
        assert!(
            (other.w - (viewport().w as f32 - tokens::MIN_PANE_PT)).abs() < 0.001,
            "the other side takes the rest: {}",
            other.w
        );
        // A viewport too small for two panes halves rather than starving one.
        let tiny = layout.rects(Viewport::new(f64::from(tokens::MIN_PANE_PT), 100.0));
        assert!(tiny.iter().all(|(_, r)| r.w > 0.0), "{tiny:?}");
    }

    #[test]
    fn the_new_pane_lands_on_the_side_the_pane_was_opened_on() {
        let wide = Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 300.0,
        };
        let tall = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 800.0,
        };
        assert_eq!(
            wide.longer_edge(),
            Dir::Horizontal,
            "a wide pane splits left/right"
        );
        assert_eq!(
            tall.longer_edge(),
            Dir::Vertical,
            "a tall pane splits up/down"
        );
    }

    /// #80: a gesture's point arrives in the **element's** coordinates, and the layout's questions are
    /// asked in the viewport's. The conversion is `viewport_point`, and this test is the bug: the second
    /// pane's left edge is 2 pt below the pointer, and read un-converted that point is 2 pt from the
    /// window's left edge — in the *first* pane, nowhere near a border.
    #[test]
    fn a_press_is_measured_from_the_pane_it_landed_in() {
        let mut layout = Layout::single();
        let first = layout.ids()[0];
        let second = layout.split(first, Dir::Horizontal).unwrap();
        let grip = tokens::PANE_GRIP;
        let rect = layout
            .rect_of(viewport(), second)
            .expect("the right-hand pane");
        assert!(
            rect.x > 0.0,
            "the point of this test: an origin that is not the window's"
        );

        // 2 pt inside the second pane's left edge, half a grip from the border.
        let local = (2.0, 200.0);
        let unconverted = layout.border_at(viewport(), local, grip);
        assert!(
            unconverted.is_none(),
            "read as a viewport point, a press inside pane 2 lands at {local:?}, in pane 1: {unconverted:?}"
        );

        let at = viewport_point(rect, local);
        let border = layout
            .border_at(viewport(), at, grip)
            .unwrap_or_else(|| panic!("the border beside the pane the press landed in, at {at:?}"));
        assert_eq!((border.a, border.b), (first, second));

        // And the ratio follows the same point: a hand 128 pt to the right of the line — stated as the
        // pane it is holding sees it — asks for a ratio 128 pt to the right of the line, not for the
        // far edge of the window.
        let moved = viewport_point(rect, (border.at - rect.x + 128.0, local.1));
        assert!(
            (border.ratio_for(moved.0) - 0.6).abs() < 1e-3,
            "{moved:?} asks for {}",
            border.ratio_for(moved.0)
        );

        // The single-pane wall is the identity: its pane's origin is the window's, which is why the
        // bug looked like "panes work until there are two".
        let one = Rect {
            x: 0.0,
            y: 0.0,
            w: viewport().w as f32,
            h: viewport().h as f32,
        };
        assert_eq!(viewport_point(one, (12.0, 34.0)), (12.0, 34.0));
    }

    /// #69: the border is grabbed on the line, and only there — a drag that starts a few pixels away
    /// pans the map under it, which is what an operator expects from a grip that is 6 pt wide.
    #[test]
    fn a_border_is_grabbed_only_on_the_line() {
        let mut layout = Layout::single();
        let first = layout.ids()[0];
        let second = layout.split(first, Dir::Horizontal).unwrap();
        let grip = tokens::PANE_GRIP;

        let on = layout
            .border_at(viewport(), (640.0, 400.0), grip)
            .expect("the border at the middle of a two-pane layout");
        assert_eq!((on.a, on.b), (first, second));
        assert_eq!(on.dir, Dir::Horizontal);
        assert!((on.at - 640.0).abs() < 0.001, "the line is at {0}", on.at);
        // Half a grip either side is still the border; twice that is the map.
        assert!(
            layout
                .border_at(viewport(), (640.0 + grip / 2.0, 400.0), grip)
                .is_some()
        );
        assert!(
            layout
                .border_at(viewport(), (640.0 - grip / 2.0, 400.0), grip)
                .is_some()
        );
        assert!(
            layout
                .border_at(viewport(), (640.0 + grip, 400.0), grip)
                .is_none(),
            "a point a grip away from the line is not on it"
        );
        assert!(layout.border_at(viewport(), (300.0, 400.0), grip).is_none());

        // A point on the line asks for the ratio it points at, clamped so the split can always grow
        // back: 0 and 1 are not ratios, they are a pane that has been deleted.
        assert!((on.ratio_for(640.0) - 0.5).abs() < 1e-6);
        assert!((on.ratio_for(768.0) - 0.6).abs() < 1e-3);
        assert_eq!(on.ratio_for(0.0), 0.05, "clamped, not zero");
        assert_eq!(on.ratio_for(5000.0), 0.95);

        // Three panes: the deepest border under the pointer wins, so the one the operator sees is the
        // one that moves.
        let mut three = Layout::single();
        let left = three.ids()[0];
        let right = three.split(left, Dir::Horizontal).unwrap();
        let bottom = three.split(right, Dir::Vertical).unwrap();
        let deepest = three
            .border_at(viewport(), (960.0, 360.0), 8.0)
            .expect("the border inside the right half");
        assert_eq!(deepest.dir, Dir::Vertical, "{deepest:?}");
        assert_eq!((deepest.a, deepest.b), (right, bottom));
    }

    /// #84: a pane picked up takes hold where the hand closed. The handle is a 28 × 4 box at the top
    /// centre of its pane, so a press on it is local to *that box*, inside the pane — and the pane's
    /// origin is the second offset. Both, once, or the drop is computed from near the window's corner.
    #[test]
    fn a_lifted_pane_is_measured_from_the_handle_its_hand_closed_on() {
        let mut layout = Layout::single();
        let first = layout.ids()[0];
        let second = layout.split(first, Dir::Vertical).unwrap();
        let rect = layout.rect_of(viewport(), second).expect("the lower pane");
        assert!(
            rect.y > 0.0,
            "the point of the test: a pane whose origin is not the window's"
        );

        let (w, h) = tokens::PANE_HANDLE;
        // The handle's centre in the handle's coordinates: the press that picks the pane up.
        let at = handle_point(rect, (w / 2.0, h / 2.0));
        assert_eq!(
            layout.pane_at(viewport(), at),
            Some(second),
            "a press on the handle reports a point inside its own pane, not near the window corner \
             (invariant: the lift's drop is asked about this point)"
        );

        // The handle's top edge is a gap/2 below the pane's, and its left edge is centred: read as a
        // pane-local point those are the two offsets, and a lift that skipped either would drop the pane
        // somewhere it was never dragged to.
        assert!((at.0 - (rect.x + rect.w / 2.0)).abs() < 0.001, "{at:?}");
        assert!(
            (at.1 - (rect.y + tokens::GAP / 2.0 + h / 2.0)).abs() < 0.001,
            "{at:?}"
        );
    }

    /// #70: the middle of a pane swaps, an edge re-tiles against that edge, and a release over the
    /// pane's own slot is nothing at all.
    #[test]
    fn a_drop_is_a_swap_in_the_middle_and_a_re_tile_on_an_edge() {
        let mut layout = Layout::single();
        let left = layout.ids()[0];
        let right = layout.split(left, Dir::Horizontal).unwrap();
        let rect = layout.rect_of(viewport(), right).unwrap();
        let middle = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);

        assert_eq!(
            layout.drop_for(viewport(), left, middle),
            Some(Drop::Swap { a: right, b: left }),
            "the middle is a swap"
        );
        // **All four bands**, and not just the two a live pane happens to have on one side: a mirrored
        // pair (`Right`/`Bottom`) is exactly the half a test that only tries `Left`/`Top` never checks,
        // and it is where #84's sideways-only drop lived.
        for (name, at, side) in [
            ("left", (rect.x + rect.w * 0.1, middle.1), Side::Left),
            ("right", (rect.x + rect.w * 0.9, middle.1), Side::Right),
            ("top", (middle.0, rect.y + rect.h * 0.08), Side::Top),
            ("bottom", (middle.0, rect.y + rect.h * 0.92), Side::Bottom),
        ] {
            assert_eq!(
                layout.drop_for(viewport(), left, at),
                Some(Drop::Retile {
                    dragged: left,
                    target: right,
                    side
                }),
                "the {name} band of the target pane re-tiles on that side: {at:?}"
            );
        }
        // Over its own slot: no drop, so a release where the drag began changes nothing.
        assert_eq!(layout.drop_for(viewport(), left, (100.0, 300.0)), None);
        assert_eq!(layout.drop_for(viewport(), left, (5000.0, 5000.0)), None);
    }

    /// The gap a drag previews is the place the pane lands: the preview and the release are one
    /// function, so this compares the two answers rather than trusting them.
    #[test]
    fn the_preview_of_a_drop_is_the_layout_the_drop_makes() {
        let mut layout = Layout::single();
        let first = layout.ids()[0];
        let second = layout.split(first, Dir::Horizontal).unwrap();
        let third = layout.split(second, Dir::Vertical).unwrap();

        for drop in [
            Drop::Swap { a: first, b: third },
            Drop::Retile {
                dragged: third,
                target: first,
                side: Side::Right,
            },
            Drop::Retile {
                dragged: first,
                target: third,
                side: Side::Top,
            },
        ] {
            let preview = layout.preview(drop).expect("a preview");
            let mut applied = layout.clone();
            assert!(applied.apply(drop));
            assert_eq!(
                preview, applied,
                "the preview and the release disagree about {drop:?} — the gap would open where the \
                 pane does not land"
            );
            // And the rectangle the gap is drawn at is the pane's rectangle after the drop.
            let moved = drop.dragged();
            assert_eq!(
                preview.rect_of(viewport(), moved).map(|rect| (
                    rect.x.round(),
                    rect.y.round(),
                    rect.w.round(),
                    rect.h.round()
                )),
                applied.rect_of(viewport(), moved).map(|rect| (
                    rect.x.round(),
                    rect.y.round(),
                    rect.w.round(),
                    rect.h.round()
                )),
            );
        }
    }

    #[test]
    fn a_swap_moves_both_panes_and_a_re_tile_splits_the_target() {
        let mut layout = Layout::single();
        let first = layout.ids()[0];
        let second = layout.split(first, Dir::Horizontal).unwrap();
        let before = layout.rects(viewport());
        let first_rect = before[0].1;

        assert!(layout.apply(Drop::Swap {
            a: first,
            b: second
        }));
        let after = layout.rects(viewport());
        assert_eq!(
            after[0].0, second,
            "the pane that was on the right is now on the left"
        );
        assert_eq!(after[0].1, first_rect, "and it took the left pane's slot");
        // The layout still covers the viewport: a swap is a renaming, not new geometry.
        let area: f64 = after
            .iter()
            .map(|(_, r)| f64::from(r.w) * f64::from(r.h))
            .sum();
        assert!((area - viewport().w * viewport().h).abs() < 0.5);

        // Re-tiling puts the dragged pane on the named side of its target.
        let mut layout = Layout::single();
        let a = layout.ids()[0];
        let b = layout.split(a, Dir::Horizontal).unwrap();
        assert!(layout.apply(Drop::Retile {
            dragged: a,
            target: b,
            side: Side::Top,
        }));
        let rects = layout.rects(viewport());
        let moved = rects.iter().find(|(id, _)| *id == a).unwrap().1;
        let stayed = rects.iter().find(|(id, _)| *id == b).unwrap().1;
        assert_eq!(moved.y, 0.0, "the dragged pane took the top");
        assert_eq!(moved.x, stayed.x, "inside the target's own column");
        assert!(moved.h < stayed.h + 1.0);

        // A drop that cannot be done leaves the layout alone.
        let untouched = layout.clone();
        assert!(!layout.apply(Drop::Swap { a, b: a }));
        assert_eq!(layout, untouched);
        assert!(!layout.apply(Drop::Retile {
            dragged: a,
            target: a,
            side: Side::Left
        }));
        assert_eq!(layout, untouched);
    }

    #[test]
    fn a_point_belongs_to_exactly_one_pane() {
        let mut layout = Layout::single();
        let first = layout.ids()[0];
        layout.split(first, Dir::Horizontal).unwrap();
        let mut rng = Rng(0x0068_F00D_2222_3333);
        for _ in 0..500 {
            let at = (
                rng.between(0.0, 1280.0) as f32,
                rng.between(0.0, 720.0) as f32,
            );
            assert!(
                layout.pane_at(viewport(), at).is_some(),
                "({}, {}) belongs to no pane",
                at.0,
                at.1
            );
        }
        // The far edge belongs to the last pane, and just outside belongs to none.
        assert!(layout.pane_at(viewport(), (1279.9, 719.9)).is_some());
        assert_eq!(layout.pane_at(viewport(), (1280.0, 400.0)), None);
    }
}
