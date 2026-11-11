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
//! precedent), `rune::Icon`/`El::icon` for a post's icon and its echelon mark, and `El::scroll` for a
//! tree taller or wider than its pane. Every size and colour is a [`tokens`] token.
//!
//! **The geometry is one function.** [`layout`] answers where every node and every elbow is, and
//! `verify` calls the same function the view draws from — the discipline `Layout::rects` and
//! `map::geometry::project` keep, for the same reason: a tree drawn in one place and asserted in
//! another is a capture that agrees with itself.

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

/// Where the tree's canvas starts inside its pane: the scroll viewport's padding.
///
/// One function, used by the view and by `verify` — the reason `view::name_probe` and
/// `view::chrome_buttons` exist: a capture and its assertion that compute an offset twice come to
/// disagree about where a post is.
pub fn canvas_origin() -> (f32, f32) {
    (tokens::TREE_PAD, tokens::TREE_TOP)
}

/// The part of a node's plate that is **always** fill when the plate is drawn: its bottom-left corner,
/// inside the padding and below the last line of text.
///
/// `verify` asks this rectangle for the plate's colour. Asking the whole plate would pass on a node
/// drawn as text with no surface behind it, which is the falsifier "one node per post" needs.
pub fn node_probe(rect: Rect) -> Rect {
    let side = tokens::GAP / 2.0;
    Rect {
        x: rect.x + side,
        // A plate is as tall as its lines plus its padding (`plate_height`), so the last line ends above
        // this corner: the probe sits in the plate's bottom padding, which is fill whether the post says
        // two lines or four.
        y: rect.y + rect.h - side - tokens::HAIRLINE,
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

/// A tree pane's content: the scroll viewport over the tree, and the card a selected post asked for.
pub fn content(state: &State, pane: ViewId, rect: Rect, tree: &Tree) -> El {
    let (canvas_x, canvas_y) = canvas_origin();
    let mut canvas = el()
        .key(format!("tree-canvas-{pane}"))
        .w(tree.width.max(1.0))
        .h(tree.height.max(1.0));
    // **Connectors under the nodes**, so a run that meets a plate ends under it rather than across its
    // text.
    for link in &tree.links {
        canvas = canvas.child(link_element(pane, link));
    }
    for node in &tree.nodes {
        canvas = canvas.child(node_element(state, pane, node));
    }
    let viewport = col()
        .key(format!("tree-{pane}"))
        .w_full()
        .h_full()
        .pt(canvas_y)
        .px(canvas_x)
        .pb(canvas_x)
        // A viewport needs a bounded height; the pane is that bound (`h_full` inside it), and a deep or
        // wide tree scrolls inside the pane rather than drawing past its edge. Both axes, because a
        // forest is as wide as its branches and a branch may be the one that overflows.
        .scroll_xy()
        .child(canvas);
    let mut body = col()
        .key(format!("tree-body-{pane}"))
        .w_full()
        .h_full()
        .child(viewport);
    // **A post that is a hull selects it in this pane and shows its card** (`PaneView`, #99). The card
    // is drawn in the pane's own points, anchored to the post that was pressed — a tree pane has no
    // marker to hang it from.
    if let Some(card) = state.card_in(pane) {
        let anchor = tree
            .nodes
            .iter()
            .find(|node| node.id_unit == Some(card.unit.id_unit))
            .map(|node| {
                (
                    f64::from(canvas_x + node.rect.x + node.rect.w),
                    f64::from(canvas_y + node.rect.y + node.rect.h / 2.0),
                )
            })
            .unwrap_or((0.0, 0.0));
        let placement = crate::units::card::place(anchor, (f64::from(rect.w), f64::from(rect.h)));
        body = body.child(crate::ui::unitcard::card(card, placement, state.now()));
    }
    body
}

/// One post's plate.
fn node_element(state: &State, pane: ViewId, node: &Placed) -> El {
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
        .on_press(move |state: &mut State| press_post(state, pane, id_unit));

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
    // The post's level, when it states one: the catalogue's mark and the level's own name. A post with
    // no level draws neither — a rank of 0 is never sent, so nothing here may default to one (`INV-01`).
    if let Some(echelon) = &node.echelon {
        body = body.child(echelon_row(Some(echelon.clone())));
    } else if let Some(rank) = node.echelon_rank {
        body = body.child(echelon_row(Some(rank.to_string())));
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
