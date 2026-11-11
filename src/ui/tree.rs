//! The chain of command (#116): a pane that is a tree, not a map.
//!
//! The data is #107's and the pane furniture is #111's. What is here is the drawing, and three
//! decisions are load-bearing:
//!
//! - **A forest is a forest.** `GET /unit-organisations/{id}` returns `nodes` as the *roots* of a
//!   forest — "more than one root is normal" — and each root nests its own subtree. So [`forest`]
//!   returns every root and [`layout`] places them side by side. Flattening the forest into one root
//!   would draw a shape the server never described, and a node would hang off a parent the wire did
//!   not name.
//! - **The supreme command is the root above the forest.** `/tni-commander` is one post shared by
//!   every branch, which is why it is not a node in any structure: a structure cannot share a node.
//!   [`layout`] draws it as a **single root** with one connector down to each branch root, so TNI AL,
//!   TNI AD and TNI AU are siblings under it and never children of one another.
//! - **Absence is not zero** (`INV-01`). A post with no `echelon_name`/`echelon_rank` has no level, a
//!   post with no `commander` is vacant, and an empty `icon` is no icon. Each draws nothing rather
//!   than a zero, a placeholder name or a default glyph.
//!
//! Everything that moves is Rune's own kit: `El::matched`/`matched_crop` for a node that changes
//! place, `El::shape` + `rune::shape::Shape` for the orthogonal connectors (`src/ui/symbol.rs` is the
//! precedent), `rune::Icon`/`El::icon` for a post's icon and its echelon mark, and — since #127 —
//! `El::scale`/`El::offset` for the **canvas**: one transform over the laid-out tree, so a narrow pane
//! fits the whole forest instead of scrolling or cutting it. Every size and colour is a [`tokens`] token.
//!
//! **The geometry is one function.** [`layout`] answers where every node and every elbow is in tree
//! space, [`crate::ui::view::tree_point`] answers where tree space lands in the pane, and `verify` calls
//! both — the discipline `Layout::rects` and `map::geometry::project` keep, for the same reason: a tree
//! drawn in one place and asserted in another is a capture that agrees with itself.

use rune::{El, col, el, row, shape::Shape, text};

use crate::data::types::{TniCommandPost, UnitOrganisationNode};
use crate::ui::icons;
use crate::ui::tokens;
use crate::ui::{PaneContent, Rect, State, ViewId};

/// One post, as the wire described it, before it is placed.
///
/// The fields are the wire's, resolved once: `label` is what the structure calls the box (`name`),
/// `title` is the post that runs it (`position_title`), `holder` is the person appointed
/// (`commander.name`) — absent for a vacant post — and `icon` is the palette's name for the post's
/// mark. Nothing defaults: an absent field stays `None` all the way to the drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    /// The post's identity **in the drawn tree**. Node ids are unique in a structure's table, but the
    /// supreme command is a different resource with its own id — the recorded post is id 1, and so is
    /// the Navy's root — so a drawn post is keyed by where it came from as well as its id.
    pub key: String,
    pub id: i64,
    pub label: String,
    pub title: Option<String>,
    pub holder: Option<String>,
    pub echelon: Option<String>,
    pub echelon_rank: Option<i32>,
    pub icon: Option<String>,
    pub id_unit: Option<i64>,
    pub children: Vec<Post>,
}

impl Post {
    /// This post at a rectangle: the fields the node draws, and where it goes.
    fn placed(&self, rect: Rect, depth: usize) -> Placed {
        Placed {
            key: self.key.clone(),
            id: self.id,
            label: self.label.clone(),
            title: self.title.clone(),
            holder: self.holder.clone(),
            echelon: self.echelon.clone(),
            echelon_rank: self.echelon_rank,
            icon: self.icon.clone(),
            id_unit: self.id_unit,
            rect,
            depth,
        }
    }
}

/// One post, placed in the tree's own points.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// Unique across the whole tree: `org<id_organisation>-node<id>`, or `tni-<id>` for the supreme
    /// command, whose id space is its own resource's.
    pub key: String,
    pub id: i64,
    pub label: String,
    /// The post that runs the box (`position_title`), absent when none is recorded.
    pub title: Option<String>,
    /// The person appointed (`commander.name`), absent for a vacant post.
    pub holder: Option<String>,
    pub echelon: Option<String>,
    pub echelon_rank: Option<i32>,
    pub icon: Option<String>,
    pub id_unit: Option<i64>,
    pub rect: Rect,
    /// The level the post sits at in the drawn tree: the supreme command at 0, each branch root at 1.
    pub depth: usize,
}

/// One parent-to-child run: an orthogonal elbow from the parent's bottom **edge** to the child's top
/// edge, through a bus halfway between the two levels.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub from: String,
    pub to: String,
    /// The polyline, in the tree's own points. Four points: down from the edge, across, down again.
    pub points: Vec<[f32; 2]>,
}

/// A drawn chain of command: every post, every connector, and the box they need.
#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    pub nodes: Vec<Placed>,
    pub links: Vec<Link>,
    pub width: f32,
    pub height: f32,
}

/// **A tree pane's camera** (#127): where the tree's own points sit in the pane.
///
/// The transform is one function (`view::tree_point`): `pane = tree * zoom + pan`. It is a value on the
/// pane, like the map's [`crate::map::geometry::Camera`], so two tree panes keep two views — and it is
/// applied with Rune's `El::scale`/`El::offset` over the already-laid-out tree, never by recomputing
/// every node's rectangle and font size. Layout keeps the unscaled size, so a zoom does not re-flow the
/// tree, text rasterizes at the scaled size once it rests, and hit testing follows the scaled rect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeCamera {
    /// The pane-space point the tree's own origin is drawn at.
    pub pan: (f32, f32),
    pub zoom: f32,
    /// **Has a hand taken the camera?** Until it has, a resize re-fits — which is the sentence #127
    /// exists to make true: narrowing a pane shows the whole forest instead of cutting it.
    pub touched: bool,
    /// The pane's own size, as the last layout pass measured it. The fit is a function of this and the
    /// tree, and a gesture needs it before it may recompute one.
    pub(crate) size: (f32, f32),
    /// The pan a drag started on, so a pan is absolute rather than a sum of deltas.
    pub(crate) drag_origin: Option<(f32, f32)>,
}

impl Default for TreeCamera {
    fn default() -> Self {
        Self {
            pan: (0.0, 0.0),
            zoom: 1.0,
            touched: false,
            size: (0.0, 0.0),
            drag_origin: None,
        }
    }
}

impl TreeCamera {
    /// This camera moved by a travel in pane points: a drag, where the tree follows the hand.
    pub fn panned(mut self, by: (f32, f32)) -> Self {
        self.pan = (self.pan.0 + by.0, self.pan.1 + by.1);
        self.touched = true;
        self
    }

    /// A camera a hand has moved to a place and a zoom. Scenes and tests set one directly; a gesture
    /// goes through the handlers below, which are the only code that changes a camera on a wall.
    pub fn moved_to(pan: (f32, f32), zoom: f32) -> Self {
        Self {
            pan,
            zoom: zoom.clamp(tokens::TREE_ZOOM_MIN, tokens::TREE_ZOOM_MAX),
            touched: true,
            ..Self::default()
        }
    }
}

/// **The fit**: the whole tree inside the pane, centred under the pane's chrome, at the largest zoom
/// that keeps every node and connector on screen.
///
/// `TREE_TOP` is left clear at the top so the supreme command is not drawn under the pane's title and
/// buttons, and `TREE_PAD` around the rest so a plate on the edge is not clipped by the frame. The zoom
/// is clamped into the token range: for the recorded 784 pt forest in a 320 pt pane the fit is ≈0.38,
/// which is small enough to read the *shape* and is why the pane is a canvas.
pub fn fit(tree: &Tree, pane: (f32, f32)) -> TreeCamera {
    let avail_w = (pane.0 - tokens::TREE_PAD * 2.0).max(1.0);
    let avail_h = (pane.1 - tokens::TREE_TOP - tokens::TREE_PAD).max(1.0);
    let zoom = (avail_w / tree.width.max(1.0))
        .min(avail_h / tree.height.max(1.0))
        .clamp(tokens::TREE_ZOOM_MIN, tokens::TREE_ZOOM_MAX);
    let pan = (
        (pane.0 - tree.width * zoom) / 2.0,
        tokens::TREE_TOP + (avail_h - tree.height * zoom) / 2.0,
    );
    TreeCamera {
        pan,
        zoom,
        touched: false,
        size: pane,
        drag_origin: None,
    }
}

/// The camera a pane draws with: **the hand's when a hand has taken it, the fit until then**.
///
/// `rect` is the pane's rectangle, so a resize that arrives before the next gesture is honoured by the
/// drawing even though no handler ran. `verify` calls this same function, so a capture and its assertion
/// cannot disagree about where a node is drawn.
pub fn camera_for(pane: &crate::ui::Pane, tree: &Tree, rect: Rect) -> TreeCamera {
    match pane.tree {
        Some(camera) if camera.touched => camera,
        _ => fit(tree, (rect.w, rect.h)),
    }
}

/// The posts under a forest of roots, in the order the wire sent them.
pub fn forest(nodes: &[UnitOrganisationNode]) -> Vec<Post> {
    nodes.iter().map(post_of).collect()
}

/// One node, with its subtree.
fn post_of(node: &UnitOrganisationNode) -> Post {
    Post {
        key: format!("org{}-node{}", node.id_organisation, node.id),
        id: node.id,
        // `name` is what the structure calls the box (`KORPS MARINIR`); `position_title` is the post
        // that runs it (`Dankormar`). Both are usually needed, and neither stands in for the other.
        label: node.name.clone(),
        title: node.position_title.clone(),
        // The holder is the `commander` convenience field, which is **absent when nobody is
        // appointed** — a vacant post is a normal state, and it draws no name.
        holder: node
            .commander
            .as_ref()
            .map(|commander| commander.name.clone()),
        echelon: node.echelon_name.clone(),
        echelon_rank: node.echelon_rank,
        icon: node.icon.clone(),
        id_unit: node.id_unit,
        children: forest(&node.children),
    }
}

/// The supreme command as the root above the forest: `position_title` is its label, and its
/// `commander` is absent when the post is vacant.
pub fn from_supreme(post: &TniCommandPost) -> Post {
    Post {
        key: format!("tni-{}", post.id),
        id: post.id,
        label: post.position_title.clone(),
        title: None,
        holder: post
            .commander
            .as_ref()
            .map(|commander| commander.name.clone()),
        echelon: post.echelon_name.clone(),
        echelon_rank: Some(post.echelon_rank),
        icon: None,
        id_unit: None,
        children: Vec::new(),
    }
}

/// Where every post and every connector goes.
///
/// A post sits over the middle of its children, and a leaf takes the next slot in reading order — the
/// tidy-tree shape an operator already reads an order of battle with. Sibling subtrees never overlap:
/// a parent is centred inside its own children's span, which is the subtree's span.
///
/// `supreme` is the single root above the forest. Its connectors run to **each** root, so a branch
/// root is a sibling of the other branch roots and never their child.
pub fn layout(supreme: Option<&Post>, roots: &[Post]) -> Tree {
    let mut nodes = Vec::new();
    let mut links = Vec::new();
    let tops = row_tops(supreme, roots);
    let offset = usize::from(supreme.is_some());
    let mut cursor = 0.0;
    for root in roots {
        place(root, offset, &mut cursor, &tops, &mut nodes, &mut links);
    }

    // The forest's own extent. A tree with no post at all still has one node's width, so an empty
    // pane draws an empty canvas rather than a zero-sized one.
    let (mut left, mut right) = (f32::MAX, f32::MIN);
    for node in &nodes {
        left = left.min(node.rect.x);
        right = right.max(node.rect.right());
    }
    if nodes.is_empty() {
        left = 0.0;
        right = tokens::TREE_NODE_W;
    }
    let forest_width = (right - left).max(tokens::TREE_NODE_W);
    let width = forest_width;
    // Centre the forest in the tree's own box. Its levels already start below the supreme post, because
    // `row_tops` measured the forest one level down.
    let shift_x = (width - (right - left)) / 2.0 - left;
    for node in &mut nodes {
        node.rect.x += shift_x;
    }
    for link in &mut links {
        for point in &mut link.points {
            point[0] += shift_x;
        }
    }

    if let Some(supreme) = supreme {
        let rect = Rect {
            x: (width - tokens::TREE_NODE_W) / 2.0,
            y: 0.0,
            w: tokens::TREE_NODE_W,
            h: plate_height(supreme),
        };
        nodes.push(supreme.placed(rect, 0));
        // **One connector per branch root**, from the supreme post's own bottom edge. The roots are
        // siblings under it; no root is drawn under another.
        for root in roots {
            if let Some(node) = nodes.iter().find(|node| node.key == root.key) {
                links.push(Link {
                    from: supreme.key.clone(),
                    to: node.key.clone(),
                    points: elbow(rect, node.rect),
                });
            }
        }
    }

    let height = nodes
        .iter()
        .map(|node| node.rect.bottom())
        .fold(0.0_f32, f32::max);
    Tree {
        nodes,
        links,
        width,
        height,
    }
}

/// How tall one post's plate is: its padding, one line per thing it says, and the gaps between them.
///
/// A post says what the structure calls the box, the post that runs it, the person appointed and its
/// level — each **only when the wire says it** (`INV-01`). So a two-line post wears a two-line plate,
/// and a four-line post is not clipped by a height chosen for the common case.
pub fn plate_height(post: &Post) -> f32 {
    let mut lines = 1.0;
    if post.title.is_some() {
        lines += 1.0;
    }
    if post.holder.is_some() {
        lines += 1.0;
    }
    if post.echelon.is_some() || post.echelon_rank.is_some() {
        lines += 1.0;
    }
    tokens::GAP * 2.0 + lines * tokens::TREE_LINE + (lines - 1.0) * tokens::CARD_GAP
}

/// The y each level starts at: each level is as tall as its tallest post, and the levels are a row gap
/// apart. The supreme post, when there is one, is level 0 above the forest.
fn row_tops(supreme: Option<&Post>, roots: &[Post]) -> Vec<f32> {
    fn measure(post: &Post, depth: usize, rows: &mut Vec<f32>) {
        if rows.len() <= depth {
            rows.resize(depth + 1, 0.0);
        }
        rows[depth] = rows[depth].max(plate_height(post));
        for child in &post.children {
            measure(child, depth + 1, rows);
        }
    }
    let mut rows = Vec::new();
    if let Some(supreme) = supreme {
        measure(supreme, 0, &mut rows);
    }
    let offset = rows.len();
    for root in roots {
        measure(root, offset, &mut rows);
    }
    let mut tops = Vec::with_capacity(rows.len());
    let mut y = 0.0;
    for height in rows {
        tops.push(y);
        y += height + tokens::TREE_ROW_GAP;
    }
    tops
}

/// Place one post and its subtree; answer the post's centre-x, which its parent is centred on.
fn place(
    post: &Post,
    depth: usize,
    cursor: &mut f32,
    tops: &[f32],
    nodes: &mut Vec<Placed>,
    links: &mut Vec<Link>,
) -> f32 {
    let mut centres = Vec::with_capacity(post.children.len());
    for child in &post.children {
        centres.push(place(child, depth + 1, cursor, tops, nodes, links));
    }
    let centre = match (centres.first(), centres.last()) {
        (Some(first), Some(last)) => (first + last) / 2.0,
        _ => {
            // A leaf takes the next slot in reading order.
            let left = *cursor;
            *cursor += tokens::TREE_NODE_W + tokens::TREE_COL_GAP;
            left + tokens::TREE_NODE_W / 2.0
        }
    };
    let rect = Rect {
        x: centre - tokens::TREE_NODE_W / 2.0,
        y: tops.get(depth).copied().unwrap_or(0.0),
        w: tokens::TREE_NODE_W,
        h: plate_height(post),
    };
    nodes.push(post.placed(rect, depth));
    for child in &post.children {
        if let Some(node) = nodes.iter().find(|node| node.key == child.key) {
            links.push(Link {
                from: post.key.clone(),
                to: node.key.clone(),
                points: elbow(rect, node.rect),
            });
        }
    }
    centre
}

/// The orthogonal elbow from a parent to a child.
///
/// **From the parent's bottom edge, not its centre**: a run that started at the centre would be hidden
/// under the plate it leaves, so the structure would read as nodes with no line between them. Down to
/// a bus halfway between the two levels, across, then down to the child's top edge — one path per
/// parent-to-child run, and every segment axis-aligned.
pub fn elbow(parent: Rect, child: Rect) -> Vec<[f32; 2]> {
    let x0 = parent.x + parent.w / 2.0;
    let y0 = parent.y + parent.h;
    let x1 = child.x + child.w / 2.0;
    let y1 = child.y;
    let bus = y0 + ((y1 - y0) / 2.0).max(0.0);
    vec![[x0, y0], [x0, bus], [x1, bus], [x1, y1]]
}

/// The tree a pane draws, or `None` for a pane that is a map.
///
/// `organisation: Some(id)` is that structure's forest; `None` is **every structure the wall knows**,
/// which is the chain of command as a whole — the header control's pane. The supreme command is the
/// root above either one.
pub fn tree_for(state: &State, pane: ViewId) -> Option<Tree> {
    let PaneContent::Tree { organisation } = &state.pane(pane)?.content else {
        return None;
    };
    let mut roots = Vec::new();
    match organisation {
        Some(id) => {
            if let Some(detail) = state.organisation_trees.get(id) {
                roots.extend(forest(&detail.nodes));
            }
        }
        None => {
            for organisation in &state.organisations {
                if let Some(detail) = state.organisation_trees.get(&organisation.id) {
                    roots.extend(forest(&detail.nodes));
                }
            }
        }
    }
    let supreme = state.tni_command.as_ref().map(from_supreme);
    Some(layout(supreme.as_ref(), &roots))
}

/// The part of a node's plate that is **always** fill when the plate is drawn: its bottom-left corner,
/// inside the padding and below the last line of text.
///
/// `verify` asks this rectangle for the plate's colour. Asking the whole plate would pass on a node
/// drawn as text with no surface behind it, which is the falsifier "one node per post" needs. The probe
/// is scaled with the plate, because the plate's padding is: at an overview zoom a fixed 4 pt probe
/// would reach into the text.
pub fn node_probe(rect: Rect, zoom: f32) -> Rect {
    let zoom = if zoom > 0.0 { zoom } else { 1.0 };
    let side = (tokens::GAP / 2.0 * zoom).max(0.75);
    Rect {
        x: rect.x + side,
        // A plate is as tall as its lines plus its padding (`plate_height`), so the last line ends above
        // this corner: the probe sits in the plate's bottom padding, which is fill whether the post says
        // two lines or four.
        y: rect.y + rect.h - side - tokens::HAIRLINE * zoom,
        w: side,
        h: side,
    }
}

/// The point on a connector's **elbow**: the middle of its horizontal run, between the parent's
/// column and the child's.
///
/// `verify` samples it, so a diagonal from one plate to another — or a run that never turned — is not
/// a connector. The points are the tree's own, the same space [`layout`] answers in.
pub fn link_probe(link: &Link) -> (f32, f32) {
    let a = link.points.get(1).copied().unwrap_or([0.0, 0.0]);
    let b = link.points.get(2).copied().unwrap_or(a);
    ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0)
}

/// A tree pane's content: **a canvas** over the tree, and the card a selected post asked for.
///
/// The canvas is sized to the pane (never to the tree) and the laid-out tree inside it is drawn with one
/// transform — `El::scale(zoom).origin(0, 0).offset(pan)` — so layout keeps the tree's own size and the
/// drawing, text rasterisation and hit testing all follow the same camera (`view::tree_point`). A root
/// sized to the *tree* would lay out a wide canvas in a narrow pane and clip it, which is the bug this
/// ticket is about, moved.
pub fn content(state: &State, pane: ViewId, rect: Rect, tree: &Tree) -> El {
    let camera = state
        .pane(pane)
        .map(|pane| camera_for(pane, tree, rect))
        .unwrap_or_else(|| fit(tree, (rect.w, rect.h)));
    let mut world = el()
        .key(format!("tree-world-{pane}"))
        .absolute(0.0, 0.0)
        .w(tree.width.max(1.0))
        .h(tree.height.max(1.0))
        // The pivot is the tree's own origin, so `pane = tree * zoom + pan` holds exactly and the wheel
        // can keep the point under the pointer still.
        .origin(0.0, 0.0)
        .scale(camera.zoom)
        .offset(camera.pan.0, camera.pan.1);
    // **Connectors under the nodes**, so a run that meets a plate ends under it rather than across its
    // text.
    for link in &tree.links {
        world = world.child(link_element(pane, link));
    }
    for node in &tree.nodes {
        world = world.child(node_element(state, pane, node, camera.zoom));
    }
    // The canvas root is the pane's size and clips; the tree may overflow it in either direction, and a
    // pan is what reveals the rest. Every gesture is on this element and on each node, because Rune
    // routes a press to the nearest element under the pointer that handles one: a drag that started on a
    // post must pan the canvas rather than fall through to nothing.
    let mut body = el()
        .key(format!("tree-canvas-{pane}"))
        .w_full()
        .h_full()
        .clip_rounded(0.0)
        .on_drag(move |state: &mut State, drag, _tx| tree_drag(state, pane, drag))
        .on_wheel(move |state: &mut State, event: rune::WheelEvent, _tx| {
            tree_wheel(state, pane, &event)
        })
        .on_double_press(move |state: &mut State| fit_now(state, pane))
        .on_layout(move |state: &mut State, [_, _, w, h]| note_size(state, pane, (w, h)))
        .child(world);
    // **A post that is a hull selects it in this pane and shows its card** (`PaneView`, #99). The card
    // is drawn in the pane's own points and is **not scaled**: it is chrome about the post, and a card
    // zoomed to 38 % is a card nobody reads. It is anchored where the post's camera puts its edge.
    if let Some(card) = state.card_in(pane) {
        let anchor = tree
            .nodes
            .iter()
            .find(|node| node.id_unit == Some(card.unit.id_unit))
            .map(|node| {
                let (x, y) = crate::ui::view::tree_point(
                    &camera,
                    (node.rect.right(), node.rect.y + node.rect.h / 2.0),
                );
                (f64::from(x), f64::from(y))
            })
            .unwrap_or((0.0, 0.0));
        let placement = crate::units::card::place(anchor, (f64::from(rect.w), f64::from(rect.h)));
        body = body.child(crate::ui::unitcard::card(card, placement, state.now()));
    }
    body
}

/// The camera a gesture starts from: the hand's, or the fit when no hand has taken it.
fn gesture_camera(state: &State, pane: ViewId) -> Option<(Tree, TreeCamera)> {
    let stored = state.pane(pane)?.tree?;
    let tree = tree_for(state, pane)?;
    let camera = if stored.touched {
        stored
    } else {
        fit(&tree, stored.size)
    };
    Some((tree, camera))
}

/// Put a camera back on the pane and redraw. The one way a gesture changes the view.
fn set_camera(state: &mut State, pane: ViewId, camera: TreeCamera) {
    if let Some(pane) = state.pane_mut(pane) {
        pane.tree = Some(camera);
    }
    state.layout_dirty = true;
}

/// A drag on a tree pane **pans** it: a canvas that moves when a hand drags it, the feel the map has.
///
/// Absolute rather than incremental on purpose: [`rune::Drag::dx`]/`dy` are travel since the press in
/// window points, so the pan is the origin plus that travel. Summing per-event deltas would compound and
/// the tree would run away from the pointer.
fn tree_drag(state: &mut State, pane: ViewId, drag: &rune::Drag) {
    let Some((_, camera)) = gesture_camera(state, pane) else {
        return;
    };
    match drag.phase {
        rune::Phase::Down => {
            // Remember where the hand took hold. A press that never moves is a click on a post, and
            // leaves `touched` false, so a resize still re-fits.
            set_camera(
                state,
                pane,
                TreeCamera {
                    drag_origin: Some(camera.pan),
                    ..camera
                },
            );
        }
        rune::Phase::Move => {
            let origin = camera.drag_origin.unwrap_or(camera.pan);
            set_camera(
                state,
                pane,
                TreeCamera {
                    pan: (origin.0 + drag.dx, origin.1 + drag.dy),
                    touched: true,
                    drag_origin: Some(origin),
                    ..camera
                },
            );
        }
        rune::Phase::Up => {
            set_camera(
                state,
                pane,
                TreeCamera {
                    drag_origin: None,
                    ..camera
                },
            );
        }
    }
}

/// How much one point of wheel travel changes the tree's zoom, in log-zoom. A wheel is a gesture, not a
/// measurement: the map's own constant is in ground zoom levels and means nothing on a text canvas.
const WHEEL_ZOOM_PER_PT: f32 = 0.002;

/// Zoom by `factor` about a **pane point**, so the tree point under it stays under it.
///
/// One function, used by the wheel and by the test that holds the promise: the point under the pointer
/// is the point the operator is looking at, and a zoom that moved it would be a zoom about the corner.
pub fn zoom_about(camera: TreeCamera, at: (f32, f32), factor: f32) -> TreeCamera {
    let under = crate::ui::view::tree_from_pane(&camera, at);
    let zoom = (camera.zoom * factor).clamp(tokens::TREE_ZOOM_MIN, tokens::TREE_ZOOM_MAX);
    TreeCamera {
        pan: (at.0 - under.0 * zoom, at.1 - under.1 * zoom),
        zoom,
        touched: true,
        drag_origin: None,
        ..camera
    }
}

/// A wheel step **zooms about the pointer**, so the post under the cursor stays under it — the map's
/// gesture, so the wall has one way to zoom.
fn tree_wheel(state: &mut State, pane: ViewId, event: &rune::WheelEvent) -> bool {
    let Some((_, camera)) = gesture_camera(state, pane) else {
        return false;
    };
    if event.dy == 0.0 {
        return false;
    }
    let next = zoom_about(
        camera,
        (event.x, event.y),
        (-event.dy * WHEEL_ZOOM_PER_PT).exp(),
    );
    if (next.zoom - camera.zoom).abs() < 1e-4 {
        return true;
    }
    set_camera(state, pane, next);
    true
}

/// A double press **fits** the tree again, and gives the camera back to the fit until a hand moves it.
fn fit_now(state: &mut State, pane: ViewId) {
    let Some((tree, camera)) = gesture_camera(state, pane) else {
        return;
    };
    set_camera(state, pane, fit(&tree, camera.size));
}

/// The pane's own size, as the layout pass measured it. The fit is recomputed from this on every draw
/// until a hand takes the camera, so a resize never needs a handler to be right.
fn note_size(state: &mut State, pane: ViewId, size: (f32, f32)) {
    let Some(camera) = state.pane_mut(pane).and_then(|pane| pane.tree.as_mut()) else {
        return;
    };
    if (camera.size.0 - size.0).abs() > 0.5 || (camera.size.1 - size.1).abs() > 0.5 {
        camera.size = size;
        state.layout_dirty = true;
    }
}

/// One post's plate.
///
/// `zoom` is the camera's, and it decides one thing here: **below `TREE_SECONDARY_MIN` only the name
/// draws**. Four lines a post at an overview scale is a grey block, and the shape is what is read; the
/// plate keeps its measured height so the tree does not re-flow, it simply carries less.
fn node_element(state: &State, pane: ViewId, node: &Placed, zoom: f32) -> El {
    let selected = node.id_unit.is_some() && state.selected_in(pane) == node.id_unit;
    let id_unit = node.id_unit;
    let mut body = col()
        // **The matched key is the post's**, so a post that is re-parented glides from where it was to
        // where it now sits instead of blinking — the same mechanism the panes use for a swap.
        .key(format!("tree-node-{pane}-{}", node.key))
        .matched(format!("tree-node-{pane}-{}", node.key))
        .matched_crop()
        .absolute(node.rect.x, node.rect.y)
        .size(tokens::TREE_NODE_W, node.rect.h)
        .padding(tokens::GAP)
        .gap(tokens::CARD_GAP)
        // A plate is as tall as what it says; the lines sit at its top, so two posts side by side read
        // from the same line rather than from their own centres.
        .justify_start()
        .radius(tokens::PANEL_RADIUS)
        // A post is a plate of chrome, so it wears the plate's material; a selected post is marked by
        // the same edge the focused pane uses, because the selection is the pane's and so is the ring.
        .fill(tokens::name_plate())
        .border(
            tokens::HAIRLINE,
            if selected {
                tokens::focus_edge()
            } else {
                tokens::card_edge()
            },
        )
        .on_press(move |state: &mut State| press_post(state, pane, id_unit))
        // A drag that starts **on a post** still pans the canvas (#127): Rune routes a gesture to the
        // nearest element under the pointer that handles one, so the handler has to be here as well as
        // on the canvas, or dragging a plate would do nothing.
        .on_drag(move |state: &mut State, drag, _tx| tree_drag(state, pane, drag))
        .on_double_press(move |state: &mut State| fit_now(state, pane));

    // What the structure calls the box, with the post's own icon when the wire names one.
    let mut label = row().w_full().gap(tokens::CARD_GAP).items_center();
    if let Some(icon) = node.icon.as_deref().and_then(icons::post_icon) {
        label = label.child(rune::icon(icon, tokens::TREE_ICON).color(tokens::ink()));
    }
    label = label.child(
        text(node.label.clone())
            .font(tokens::LABEL)
            .color(tokens::ink())
            .grow(1.0)
            .truncate(),
    );
    body = body.child(label);

    // At an overview zoom the name is the post. The secondary lines come back as the hand zooms in.
    if zoom >= tokens::TREE_SECONDARY_MIN {
        // The post that runs the box, when the wire records one.
        if let Some(title) = &node.title {
            body = body.child(
                text(title.clone())
                    .font(tokens::LABEL)
                    .color(tokens::ink_dim())
                    .truncate(),
            );
        }
        // The person appointed, when somebody is. A vacant post draws no name rather than an invented one.
        if let Some(holder) = &node.holder {
            body = body.child(
                text(holder.clone())
                    .font(tokens::LABEL)
                    .color(tokens::ink_dim())
                    .truncate(),
            );
        }
        // The post's level, when it states one: the catalogue's mark and the level's own name. A post
        // with no level draws neither — a rank of 0 is never sent, so nothing here may default to one
        // (`INV-01`).
        if let Some(echelon) = &node.echelon {
            body = body.child(echelon_row(Some(echelon.clone())));
        } else if let Some(rank) = node.echelon_rank {
            body = body.child(echelon_row(Some(rank.to_string())));
        }
    }
    body
}

/// The echelon mark and its name: Rune's catalogue icon plus a word, because a chevron alone is a glyph
/// nobody can name.
fn echelon_row(level: Option<String>) -> El {
    let mut row = row()
        .w_full()
        .gap(tokens::CARD_GAP)
        .items_center()
        .child(rune::icon(&icons::ECHELON, tokens::TREE_ICON).color(tokens::ink_dim()));
    if let Some(level) = level {
        row = row.child(
            text(level)
                .font(tokens::LABEL)
                .color(tokens::ink_dim())
                .grow(1.0)
                .truncate(),
        );
    }
    row
}

/// One connector: a `Shape` over its own bounding box, so the element is only as big as the line.
fn link_element(pane: ViewId, link: &Link) -> El {
    let pad = tokens::TREE_LINK_W;
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for [x, y] in &link.points {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    // A stroke is centred on its points, so the box is grown by the stroke's own width: a 1.5 pt line
    // on a point at the box's edge is half outside it, and an element clips its own drawing.
    min_x -= pad;
    min_y -= pad;
    let w = (max_x - min_x + pad).max(1.0);
    let h = (max_y - min_y + pad).max(1.0);
    let points: Vec<[f32; 2]> = link
        .points
        .iter()
        .map(|[x, y]| [x - min_x, y - min_y])
        .collect();
    let shape =
        Shape::new()
            .view_box(w, h)
            .stroke(points, tokens::TREE_LINK_W, tokens::tree_link());
    el().key(format!("tree-link-{pane}-{}-{}", link.from, link.to))
        .absolute(min_x, min_y)
        .size(w, h)
        .shape(shape)
}

/// A press on a post: the pane takes the keys, and a post that **is a hull** selects it in this pane.
///
/// A post without an `id_unit` is a command post, not a hull: it selects nothing. Inventing a card for
/// it would be a card about no unit, which is a different ticket (#116).
pub fn press_post(state: &mut State, pane: ViewId, id_unit: Option<i64>) {
    state.layout.focus(pane);
    if let Some(id_unit) = id_unit {
        if state.selected_in(pane) == Some(id_unit) {
            state.clear_selection(pane);
        } else {
            state.select_in(pane, id_unit);
        }
    }
    state.layout_dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{Envelope, TniCommandPost, UnitOrganisationDetail};
    use crate::scene;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
    }

    /// The scene's structure: the recording, with one `parent_id` removed so the forest has two roots.
    fn detail() -> UnitOrganisationDetail {
        let envelope: Envelope<UnitOrganisationDetail> =
            serde_json::from_str(&fixture("scenes/chain-of-command.json"))
                .expect("the derived structure decodes");
        envelope.data
    }

    fn supreme() -> TniCommandPost {
        let envelope: Envelope<TniCommandPost> =
            serde_json::from_str(&fixture("rest/tni-commander.json"))
                .expect("the recorded supreme command decodes");
        envelope.data
    }

    fn node_of<'a>(tree: &'a Tree, key: &str) -> &'a Placed {
        tree.nodes
            .iter()
            .find(|node| node.key == key)
            .unwrap_or_else(|| panic!("no post {key} in the tree"))
    }

    /// **A forest renders as a forest.** `nodes` is the roots of a forest, and more than one root is
    /// normal; a layout that took the first root, or flattened the second under it, draws one root
    /// where the server sent two. The falsifier is the flattened tree below, built from the same
    /// posts: one root, and a branch that has become another branch's child.
    #[test]
    fn a_forest_renders_as_a_forest() {
        let detail = detail();
        assert_eq!(
            detail.nodes.len(),
            2,
            "the derived fixture is a forest of two roots"
        );
        let posts = forest(&detail.nodes);
        let supreme = from_supreme(&supreme());
        let tree = layout(Some(&supreme), &posts);

        let roots: Vec<&Placed> = tree.nodes.iter().filter(|node| node.depth == 1).collect();
        assert_eq!(
            roots.len(),
            2,
            "two roots, not one: a flattened forest draws a single root"
        );
        assert_eq!(
            tree.nodes.len(),
            8,
            "one node per post: seven recorded posts and the supreme command above them"
        );
        // The roots are side by side, not stacked, and each hangs off the supreme post.
        assert!(
            (roots[0].rect.x - roots[1].rect.x).abs() > 1.0,
            "two roots are two boxes: {:?} and {:?}",
            roots[0].rect,
            roots[1].rect
        );
        for root in &roots {
            assert!(
                tree.links
                    .iter()
                    .any(|link| link.from == supreme.key && link.to == root.key),
                "post {} hangs off the supreme command",
                root.id
            );
        }
        // **Never children of each other**: no connector runs between two branch roots.
        let keys: Vec<&String> = roots.iter().map(|root| &root.key).collect();
        assert!(
            tree.links
                .iter()
                .all(|link| !(keys.contains(&&link.from) && keys.contains(&&link.to))),
            "a branch root is a sibling of the other roots, never their child"
        );

        // The falsifier, from the same posts: flatten the forest into one root and the shape changes.
        let mut flat = posts.clone();
        let second = flat.remove(1);
        flat[0].children.push(second);
        let flattened = layout(Some(&supreme), &flat);
        assert_eq!(
            flattened
                .nodes
                .iter()
                .filter(|node| node.depth == 1)
                .count(),
            1,
            "flattened, the forest draws one root — which is the shape this test refuses"
        );
        assert_eq!(
            flattened.nodes.len(),
            tree.nodes.len(),
            "flattening keeps every post, so the root count is the only difference"
        );
    }

    /// **The gestures the canvas promises, on the state they promise it for** (#127): a wheel zooms
    /// about the pointer, and a double press gives the camera back to the fit. The drag is the same
    /// arithmetic with a travel instead of a wheel; `rune::Drag`'s fields are the runtime's, so a drag is
    /// the one gesture only a pointer capture can drive.
    #[test]
    fn a_wheel_zooms_about_the_pointer_and_a_double_press_fits_again() {
        let mut state = scene::state_for("pane-tree");
        let tree_id = state
            .pane_rects()
            .into_iter()
            .map(|(id, _)| id)
            .find(|id| state.pane(*id).is_some_and(|pane| !pane.content.is_map()))
            .expect("the scene's tree pane");
        let rect = state
            .pane_rects()
            .into_iter()
            .find(|(id, _)| *id == tree_id)
            .expect("its rectangle")
            .1;
        let tree = tree_for(&state, tree_id).expect("its tree");
        // The size a layout pass would have recorded: the handlers need it before they may re-fit.
        if let Some(camera) = state.pane_mut(tree_id).and_then(|pane| pane.tree.as_mut()) {
            camera.size = (rect.w, rect.h);
        }
        let fitted = camera_for(state.pane(tree_id).expect("pane"), &tree, rect);
        assert!(!fitted.touched);
        let at = (300.0, 200.0);
        let under = crate::ui::view::tree_from_pane(&fitted, at);
        let event = rune::WheelEvent::new(0.0, -50.0).at(at.0, at.1);
        assert!(
            tree_wheel(&mut state, tree_id, &event),
            "the wheel is taken"
        );
        let zoomed = state
            .pane(tree_id)
            .and_then(|pane| pane.tree)
            .expect("the camera");
        assert!(zoomed.touched, "a hand has taken the camera");
        assert!(
            zoomed.zoom > fitted.zoom,
            "wheel-up zooms in: {} -> {}",
            fitted.zoom,
            zoomed.zoom
        );
        let after = crate::ui::view::tree_point(&zoomed, under);
        assert!(
            (after.0 - at.0).abs() < 0.5 && (after.1 - at.1).abs() < 0.5,
            "the point under the pointer moved from {at:?} to {after:?}"
        );
        fit_now(&mut state, tree_id);
        let refit = state
            .pane(tree_id)
            .and_then(|pane| pane.tree)
            .expect("the camera");
        assert!(
            !refit.touched,
            "a double press gives the camera back to the fit"
        );
        assert!(
            (refit.zoom - fitted.zoom).abs() < 1e-4,
            "the fit is the same fit: {} vs {}",
            refit.zoom,
            fitted.zoom
        );
    }

    /// **A fit puts a wide forest inside a narrow pane.** The recorded forest is 784 pt wide; a 320 pt
    /// pane at 1:1 cuts it. The fit shrinks it and centres it under the pane's chrome, which is the
    /// operator's ask (#127) and the reason the canvas is allowed below the hand's comfort zoom.
    #[test]
    fn a_fit_puts_a_wide_forest_inside_a_narrow_pane() {
        let tree = Tree {
            nodes: Vec::new(),
            links: Vec::new(),
            width: 784.0,
            height: 396.0,
        };
        let pane = (320.0, 650.0);
        let camera = fit(&tree, pane);
        assert!(!camera.touched, "a fit is not a hand's camera");
        assert!(
            camera.zoom < 1.0,
            "784 pt does not fit 320 pt at 1:1: {}",
            camera.zoom
        );
        assert!((camera.zoom - 296.0 / 784.0).abs() < 1e-4, "{camera:?}");
        let (x, y) = crate::ui::view::tree_point(&camera, (0.0, 0.0));
        let (right, bottom) = crate::ui::view::tree_point(&camera, (tree.width, tree.height));
        assert!(
            x >= crate::ui::tokens::TREE_PAD - 0.5
                && y >= crate::ui::tokens::TREE_TOP - 0.5
                && right <= pane.0 - crate::ui::tokens::TREE_PAD + 0.5
                && bottom <= pane.1 - crate::ui::tokens::TREE_PAD + 0.5,
            "the whole tree is inside the pane: ({x}, {y})..({right}, {bottom}) of {pane:?}"
        );
    }

    /// **A wheel zoom keeps the tree point under the pointer still** — the map's gesture, on the tree's
    /// transform. The camera is a value, so this is arithmetic with no window.
    #[test]
    fn a_zoom_about_a_point_keeps_that_point_still() {
        let camera = TreeCamera::moved_to((70.0, -40.0), 0.9);
        let at = (300.0, 200.0);
        let under = crate::ui::view::tree_from_pane(&camera, at);
        let next = zoom_about(camera, at, 1.7);
        assert!(next.touched);
        assert!((next.zoom - 1.53).abs() < 1e-4, "{next:?}");
        let after = crate::ui::view::tree_point(&next, under);
        assert!(
            (after.0 - at.0).abs() < 0.01 && (after.1 - at.1).abs() < 0.01,
            "the point under the pointer moved from {at:?} to {after:?}"
        );
        // The zoom is clamped into the token range, and a clamp still pivots on the pointer.
        let floor = zoom_about(camera, at, 0.0001);
        assert_eq!(floor.zoom, crate::ui::tokens::TREE_ZOOM_MIN);
        let under_floor = crate::ui::view::tree_from_pane(&camera, at);
        let after_floor = crate::ui::view::tree_point(&floor, under_floor);
        assert!(
            (after_floor.0 - at.0).abs() < 0.01 && (after_floor.1 - at.1).abs() < 0.01,
            "a clamped zoom still pivots on the pointer: {after_floor:?}"
        );
    }

    /// **A connector starts at the parent's edge**, not its centre, and every segment is orthogonal.
    /// A run from the centre would be hidden under the plate it leaves, and the elbow's own point —
    /// the one `verify` samples in the capture — is the bus halfway between the two levels.
    #[test]
    fn a_connector_runs_from_the_parents_edge_and_not_its_centre() {
        let posts = forest(&detail().nodes);
        let supreme = from_supreme(&supreme());
        let tree = layout(Some(&supreme), &posts);
        assert!(!tree.links.is_empty());
        for link in &tree.links {
            let parent = node_of(&tree, &link.from);
            let child = node_of(&tree, &link.to);
            let first = link.points.first().expect("a run has a start");
            let last = link.points.last().expect("a run has an end");
            assert!(
                (first[1] - parent.rect.bottom()).abs() < 0.01,
                "the run leaves the parent's bottom edge ({}) and not its centre ({}): {first:?}",
                parent.rect.bottom(),
                parent.rect.y + parent.rect.h / 2.0
            );
            assert!(
                (first[0] - (parent.rect.x + parent.rect.w / 2.0)).abs() < 0.01,
                "the run leaves from the parent's own column"
            );
            assert!(
                (last[1] - child.rect.y).abs() < 0.01,
                "the run meets the child's top edge: {last:?}"
            );
            // Orthogonal: every segment is axis-aligned, which is what makes it an elbow and not a
            // diagonal. The bus is strictly between the two levels.
            for pair in link.points.windows(2) {
                assert!(
                    (pair[0][0] - pair[1][0]).abs() < 0.01
                        || (pair[0][1] - pair[1][1]).abs() < 0.01,
                    "a connector segment is a diagonal: {pair:?}"
                );
            }
            let bus = link.points[1][1];
            assert!(
                bus > parent.rect.bottom() && bus < child.rect.y,
                "the elbow runs in the gap between the levels, got {bus}"
            );
        }
    }

    /// `INV-01` on a post: an absent level draws no level, an empty icon draws no icon, and a vacant
    /// post draws no name. The recording carries all three absences, and the supreme command is the
    /// vacant one.
    #[test]
    fn an_absent_level_icon_and_holder_draw_nothing() {
        let posts = forest(&detail().nodes);
        assert!(
            posts
                .iter()
                .all(|post| post.echelon.is_none() && post.echelon_rank.is_none()),
            "INV-01: the recording's posts have no level, and none may default to a rank of 0"
        );
        assert!(
            posts.iter().all(|post| post.icon.as_deref() == Some("")),
            "the recorded posts carry an empty icon, which is no icon"
        );
        assert!(
            posts
                .iter()
                .all(|post| icons::post_icon(post.icon.as_deref().unwrap_or("")).is_none()),
            "an empty icon name resolves to no catalogue icon"
        );
        assert_eq!(
            icons::post_icon("branch").map(|icon| icon.name),
            Some("branch"),
            "a name the catalogue carries does draw its icon"
        );

        let supreme = from_supreme(&supreme());
        assert_eq!(
            supreme.holder, None,
            "INV-01: the supreme command is vacant, so no name is drawn for it"
        );
        assert_eq!(supreme.echelon.as_deref(), Some("Commander"));
        assert_eq!(
            supreme.echelon_rank,
            Some(7),
            "the level it states, as sent"
        );
        assert!(
            supreme.label.contains("Panglima TNI"),
            "the post's title is its label, got {:?}",
            supreme.label
        );
    }

    /// **The header control opens a tree pane and splits the layout** (#116), like `Focus on new
    /// pane`: the tree takes a pane beside the one the operator was working in, and the wall gains a
    /// pane rather than a picture.
    #[test]
    fn the_header_control_opens_a_tree_pane_and_splits_the_layout() {
        let mut state = State::default();
        let before = state.layout.len();
        let map = state.layout.focused();
        crate::ui::open_tree_pane(&mut state);
        assert_eq!(state.layout.len(), before + 1, "the wall gained a pane");
        let tree = state.layout.focused();
        assert_ne!(tree, map, "the new pane is the one the operator asked for");
        assert!(
            matches!(
                state.pane(tree).map(|pane| &pane.content),
                Some(PaneContent::Tree { organisation: None })
            ),
            "the new pane is the chain of command, not a map"
        );
        assert!(
            state.pane(map).is_some_and(|pane| pane.content.is_map()),
            "the pane it was opened beside is still a map"
        );
    }

    /// **A tree pane has no frame and draws nothing that projects** (`INV-04` holds literally). A
    /// layout pass gives a map pane a frame to project against and gives a tree pane none, so there
    /// is nothing for a marker to be placed against.
    #[test]
    fn a_tree_pane_has_no_frame_and_draws_nothing_that_projects() {
        let mut state = scene::state_for("pane-tree");
        let rects = state.pane_rects();
        assert_eq!(rects.len(), 2, "the scene is a tree beside a map");
        let tree = rects
            .iter()
            .find(|(id, _)| state.pane(*id).is_some_and(|pane| !pane.content.is_map()))
            .map(|(id, _)| *id)
            .expect("the scene has a tree pane");
        let map = rects
            .iter()
            .find(|(id, _)| state.pane(*id).is_some_and(|pane| pane.content.is_map()))
            .map(|(id, _)| *id)
            .expect("the scene has a map pane");

        // The layout pass a wall runs on every frame: a map pane is told its rectangle and answers
        // with a frame; a tree pane is told the same and has nothing to answer with.
        for (id, rect) in rects {
            crate::ui::pane_laid_out(&mut state, id, rect.w, rect.h, 1.0);
        }
        assert!(
            state.pane(map).is_some_and(|pane| pane.frame.is_some()),
            "the map pane projects against its own frame"
        );
        assert!(
            state.pane(tree).is_some_and(|pane| pane.frame.is_none()),
            "INV-04: a tree pane has no frame, so it draws no marker"
        );
        assert!(
            state.pane(tree).is_some_and(|pane| !pane.content.is_map()),
            "the tree pane is not a map"
        );
    }

    /// **A post that is a hull selects it in that pane only** (#99), and a post that is not a hull
    /// selects nothing. The recording's posts carry no `id_unit`, so this derives one — the value is
    /// the fixture feed's own hull, set on the decoded structure rather than written into a fixture.
    #[test]
    fn a_post_with_a_hull_selects_in_that_pane_only() {
        let mut state = scene::state_for("pane-tree");
        let rects = state.pane_rects();
        let tree = rects
            .iter()
            .find(|(id, _)| state.pane(*id).is_some_and(|pane| !pane.content.is_map()))
            .map(|(id, _)| *id)
            .expect("the scene has a tree pane");
        let map = rects
            .iter()
            .find(|(id, _)| state.pane(*id).is_some_and(|pane| pane.content.is_map()))
            .map(|(id, _)| *id)
            .expect("the scene has a map pane");

        // The hull the post is about: the scene's own card hull, which the feed carries a fix for.
        let hull = scene::CARD_UNIT;
        let detail = state
            .organisation_trees
            .get_mut(&scene::TREE_ORGANISATION)
            .expect("the scene's structure");
        detail.nodes[0].id_unit = Some(hull);

        press_post(&mut state, tree, Some(hull));
        assert_eq!(
            state.selected_in(tree),
            Some(hull),
            "a post that is a hull selects it in this pane"
        );
        assert_eq!(
            state.selected_in(map),
            None,
            "INV-10: the selection is the pane's, so the neighbour is untouched"
        );
        assert!(
            state.card_in(tree).is_some(),
            "a post that is a hull shows that hull's card"
        );
        assert!(
            state.card_in(map).is_none(),
            "and no card appears in the pane beside it"
        );

        // A second press closes it, the same way a marker does.
        press_post(&mut state, tree, Some(hull));
        assert_eq!(state.selected_in(tree), None);

        // A post without a hull — a command post — selects nothing at all.
        press_post(&mut state, tree, None);
        assert_eq!(state.selected_in(tree), None);
        assert!(state.card_in(tree).is_none());
    }
}
