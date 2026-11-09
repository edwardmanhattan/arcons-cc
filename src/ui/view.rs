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
