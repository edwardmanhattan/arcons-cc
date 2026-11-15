//! **The projection** — what a client may read (#181, `ADR-0005` decision 7).
//!
//! Every intent answers "did it work". This is the other half: a small, versioned snapshot of the wall's
//! **control** state — the workspaces, the panes in them, which is on screen, which has the keys, what is full
//! size, what each pane is looking at.
//!
//! **What is deliberately not here**, because a client that reads it starts to be a renderer:
//!
//! - the fleet, the fixes, the ages — a client that knows where the hulls are has the picture;
//! - the cards, the story text, the chain of command — the wall's *content*, which is what it was built to show;
//! - the frames and the pixels — `INV-04` is a claim about this process's drawing surface.
//!
//! **Absence is not zero** in the projection too (`INV-01`): a pane that has no camera says `null` rather than
//! `0, 0, 0`; a workspace with no panes is `empty: true` and still in the list, because it is still on the wall
//! with its name (#141); a pane with no card says `selected: null`.
//!
//! The whole thing is built in [`of`], from `State`, in one function — so the projection cannot drift from the
//! wall without the wall having changed.

use serde::{Deserialize, Serialize};

use crate::config::layout::Axis;
use crate::ui::view::{Dir, Shape};
use crate::ui::{PaneContent, State, ViewId};

/// The wall, as a controller sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wall {
    /// The workspaces, in chip order.
    pub workspaces: Vec<WorkspaceView>,
    /// Which workspace is on screen, as an index into `workspaces`.
    pub active: usize,
    /// The pane with the keys. `null` when the active workspace has no pane — which is a wall #141 allows.
    pub focused: Option<ViewId>,
    /// The pane drawn over the wall (#85), or `null`.
    pub maximised: Option<ViewId>,
}

/// One workspace: its name, the panes in it, and which of them the wall remembers.
///
/// A workspace is **never removed by closing its last pane** (#141): `empty` says it has no tiling, and `panes`
/// is then empty, but the name and the chip stay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceView {
    pub name: String,
    /// The pane this workspace would focus when it comes back on screen, or `null` when it holds none.
    pub focused: Option<ViewId>,
    /// Whether it holds no tiling at all — #141's legal, empty workspace.
    pub empty: bool,
    /// **The arrangement** (#218): how this workspace's panes tile, with **pane ids** for leaves. `null` for a
    /// workspace the wall cannot tile — an empty one, which a client draws as a row, exactly as the wall does.
    pub tiling: Option<Tiling>,
    pub panes: Vec<PaneView>,
}

/// **What a pane is** (#116, #136), in the words the wire uses: `map`, `tree`, `newsroom`.
///
/// An enum rather than a string, so a client that receives one it does not know is a client that can say so
/// (`INV-07`'s rule at the far end), and so this file cannot spell a fourth kind by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Map,
    Tree,
    Newsroom,
}

/// One pane: what it is, what it is called, and where it is looking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneView {
    pub id: ViewId,
    pub kind: Kind,
    /// What the name plate draws: the operator's name, or the pane's number.
    pub title: String,
    /// Where a map pane is looking. **`null` for a pane that projects nothing** — a tree or a newsroom has no
    /// camera, and an invented one would be a wall claiming to look somewhere it cannot.
    pub camera: Option<CameraView>,
    pub focused: bool,
    /// The hull whose card this pane is showing, or `null` (#99).
    pub selected: Option<i64>,
}

/// A camera, in the shape a `map.camera` intent takes — so a client can read one and send it back.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraView {
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
}

/// **The arrangement**, a workspace's tiling (#218).
///
/// The **same spelling** `layout.toml` and the `Scene` use — `split`, `ratio`, `a`, `b`, `pane` — so a reader that
/// parses one parses the other, with one deliberate difference: a leaf is a **pane id**, the integer every intent
/// names, and not an index into the workspace's pane list.
///
/// The difference is the whole type. A file is written and read whole, so an index is a shorthand; a client holds a
/// projection across changes, and after a `pane.close` every index past the gap names a different pane — a phone
/// would draw the right arrangement of the wrong panes. Indices never travel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Tiling {
    /// One pane, by the id every intent about a pane names.
    Leaf { pane: ViewId },
    /// Two nodes beside each other (`side`: `a` left, `b` right) or one above the other (`stack`: `a` on top).
    ///
    /// `ratio` is **`a`'s share of the split** and it is always present: a file may omit it and mean one half, a
    /// projection always says what it means (`INV-01`'s rule, where an absence would be a guess).
    Split {
        split: Axis,
        ratio: f32,
        a: Box<Tiling>,
        b: Box<Tiling>,
    },
}

/// A layout's shape, in the wire's terms: leaves stay **ids**.
///
/// The wall's own tree is the truth — `Layout::shape` — and this is the one spelling of it a client may see. It
/// cannot invent an arrangement `layout.toml` would not write, because both are read from the same tree.
pub(crate) fn tiling_of(shape: &Shape) -> Tiling {
    match shape {
        Shape::Leaf(id) => Tiling::Leaf { pane: *id },
        Shape::Split { dir, ratio, a, b } => Tiling::Split {
            split: match dir {
                Dir::Horizontal => Axis::Side,
                Dir::Vertical => Axis::Stack,
            },
            ratio: *ratio,
            a: Box::new(tiling_of(a)),
            b: Box::new(tiling_of(b)),
        },
    }
}

/// **The wall as a projection**, from the state that draws it.
///
/// One function, reading `State` and nothing else: no clock, no picture, no thread. That is what makes it safe to
/// answer from the middle of a frame — and what makes a test able to say what a client will see.
pub fn of(state: &State) -> Wall {
    let active = state.active;
    let focused = state.layout().map(|layout| layout.focused());
    Wall {
        active,
        focused,
        maximised: state.maximised(),
        workspaces: state
            .workspaces
            .iter()
            .enumerate()
            .map(|(index, workspace)| WorkspaceView {
                name: workspace.name.clone(),
                focused: workspace.layout.as_ref().map(|layout| layout.focused()),
                empty: workspace.layout.is_none(),
                // **The arrangement, from the wall's own tree** (#218): a workspace holds no layout when it is
                // empty (#141), and that is `null` — never an empty tree, which would be a wall claiming to tile
                // nothing.
                tiling: workspace
                    .layout
                    .as_ref()
                    .and_then(|layout| layout.shape())
                    .map(|shape| tiling_of(&shape)),
                panes: workspace
                    .panes
                    .iter()
                    .map(|pane| PaneView {
                        id: pane.id,
                        kind: match pane.content {
                            PaneContent::Map => Kind::Map,
                            PaneContent::Tree { .. } => Kind::Tree,
                            PaneContent::Newsroom => Kind::Newsroom,
                        },
                        title: pane.title(),
                        camera: match pane.content {
                            // A pane that draws no map has no camera to report (`INV-01`).
                            PaneContent::Map => Some(CameraView {
                                lat: pane.camera.center.0,
                                lon: pane.camera.center.1,
                                zoom: pane.camera.zoom,
                            }),
                            _ => None,
                        },
                        // "Focused" is the *active* workspace's question: a pane in a workspace that is not on
                        // screen has no keys, whatever its workspace remembers.
                        focused: index == active && Some(pane.id) == focused,
                        selected: pane.view.selected,
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::view;

    /// **A pane that draws no map has no camera**, and says so — not a zero, and not a missing field (`INV-01`).
    #[test]
    fn a_feed_pane_has_no_camera_and_says_so() {
        let mut state = State::default();
        let map = state.focused();
        let camera = state.camera_of(map);
        let newsroom = state
            .split_pane_as(map, camera, PaneContent::Newsroom)
            .expect("a newsroom beside the map");
        let wall = of(&state);
        let pane = |id: ViewId| {
            wall.workspaces
                .iter()
                .flat_map(|workspace| workspace.panes.iter())
                .find(|pane| pane.id == id)
                .unwrap_or_else(|| panic!("pane {id} is not in the projection"))
        };
        assert!(
            pane(map).camera.is_some(),
            "a map pane is looking somewhere, and says where"
        );
        assert_eq!(
            pane(newsroom).camera,
            None,
            "a newsroom has no camera; a projection that invents one claims the wall looks somewhere it cannot"
        );
        assert_eq!(pane(newsroom).kind, Kind::Newsroom);
        assert_eq!(pane(map).kind, Kind::Map);
        assert_eq!(
            serde_json::to_value(&wall).expect("serializable")["workspaces"][0]["panes"][1]["kind"],
            serde_json::json!("newsroom"),
            "and on the wire it is the word a client's document uses"
        );
        assert_eq!(
            serde_json::to_value(&wall).expect("serializable")["workspaces"][0]["panes"][1]["camera"],
            serde_json::Value::Null,
            "and on the wire it is null, not absent"
        );
    }

    /// **An empty workspace is on the wall**, with its name and its chip — the state #141 allows and #153
    /// restores. A projection that dropped it would tell a client the workspace was closed.
    #[test]
    fn an_empty_workspace_is_present_and_named() {
        let mut state = State::default();
        let pane = state.focused();
        assert!(
            crate::ui::move_pane_to(&mut state, pane, 1),
            "the pane moves to workspace 2"
        );
        let wall = of(&state);
        assert_eq!(wall.workspaces.len(), 3);
        assert!(
            wall.workspaces[0].empty,
            "workspace 1 gave its only pane away and is still on the wall"
        );
        assert!(wall.workspaces[0].panes.is_empty());
        assert_eq!(
            wall.workspaces[0].focused, None,
            "a workspace with no tiling has no pane to come back to"
        );
        assert!(!wall.workspaces[0].name.is_empty(), "and it is still named");
        assert_eq!(wall.active, 1, "the wall is where the pane went");
        assert_eq!(wall.focused, Some(pane));
    }

    /// **The keys are the active workspace's**: a pane in a workspace that is not on screen is not focused,
    /// whatever that workspace remembers.
    #[test]
    fn focus_is_the_active_workspaces_question() {
        let mut state = State::default();
        let other = state.focused();
        let camera = state.camera_of(other);
        let second = state
            .split_pane_as(other, camera, PaneContent::Map)
            .expect("a second pane");
        state.switch_to(1);
        let wall = of(&state);
        assert_eq!(wall.active, 1);
        assert_eq!(wall.focused, None, "workspace 2 has no pane and no keys");
        assert!(
            wall.workspaces[0].panes.iter().all(|pane| !pane.focused),
            "no pane in an off-screen workspace has the keys"
        );
        // …and the wall still remembers which pane it would return to.
        assert_eq!(wall.workspaces[0].focused, Some(second));
    }

    /// The projection carries **no fleet and no picture**: it is a description of the wall's controls, and a
    /// client that could read hulls from it would be a renderer (`ADR-0005` decision 7).
    #[test]
    fn the_projection_describes_controls_and_not_the_picture() {
        let state = State::default();
        let json = serde_json::to_value(of(&state)).expect("serializable");
        let text = json.to_string();
        for forbidden in [
            "lat_", "lon_", "fix", "hull", "card", "story", "speed", "heading",
        ] {
            assert!(
                !text.contains(forbidden),
                "the projection mentions {forbidden:?}: the wall's content is not a client's to read"
            );
        }
        let keys: Vec<&str> = json
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            ["active", "focused", "maximised", "workspaces"],
            "the top-level shape is the document's shape"
        );
    }

    /// **The arrangement travels by pane id** (#218).
    ///
    /// A client holds a projection across changes. After a `pane.close`, an index into the pane list names a
    /// different pane — so a phone drawing from indices would show the right arrangement of the wrong panes. The
    /// leaves are the ids every intent names, and the axes are the file's own words, so one reader parses both.
    #[test]
    fn the_arrangement_names_panes_and_not_their_slots() {
        let mut state = State::default();
        let a = state.focused();
        let camera = state.camera_of(a);
        let b = state.split_pane(a, camera).expect("a second pane");
        let c = state
            .split_pane_as(b, camera, PaneContent::Newsroom)
            .expect("a third pane");
        // **The shape is set here, not inherited** from whichever edge a split happens to take: the axes are the
        // claim, and a test that hopes for them fails on another window size.
        let shape = view::Shape::Split {
            dir: view::Dir::Horizontal,
            ratio: 0.6,
            a: Box::new(view::Shape::Leaf(a)),
            b: Box::new(view::Shape::Split {
                dir: view::Dir::Vertical,
                ratio: 0.5,
                a: Box::new(view::Shape::Leaf(b)),
                b: Box::new(view::Shape::Leaf(c)),
            }),
        };
        *state.layout_mut().expect("the wall has a layout") =
            view::Layout::from_shape(shape).expect("a tiling of the three panes it has");

        let wall = of(&state);
        assert_eq!(
            wall.workspaces[0].tiling,
            Some(Tiling::Split {
                split: Axis::Side,
                ratio: 0.6,
                a: Box::new(Tiling::Leaf { pane: a }),
                b: Box::new(Tiling::Split {
                    split: Axis::Stack,
                    ratio: 0.5,
                    a: Box::new(Tiling::Leaf { pane: b }),
                    b: Box::new(Tiling::Leaf { pane: c }),
                }),
            }),
            "the tree the wall tiles by, with the ids a client's intents name"
        );

        // Every pane appears exactly once: a tiling is a partition of the panes the workspace has.
        assert_eq!(
            wall.workspaces[0]
                .panes
                .iter()
                .map(|pane| pane.id)
                .collect::<Vec<_>>(),
            vec![a, b, c],
            "a leaf that named a pane twice, or not at all, would not be this workspace's tiling"
        );

        // And on the wire it is the file's own spelling, so a scene's reader parses a projection.
        let json = serde_json::to_value(wall.workspaces[0].tiling.as_ref().expect("the tiling"))
            .expect("serializable");
        assert_eq!(
            json["split"],
            serde_json::json!("side"),
            "`side` is `a` then `b`, left to right"
        );
        assert_eq!(
            json["a"],
            serde_json::json!({ "pane": a }),
            "a leaf is an id and nothing else"
        );
        assert_eq!(json["b"]["split"], serde_json::json!("stack"));
        assert_eq!(json["b"]["ratio"], serde_json::json!(0.5));
        assert!(
            json["ratio"].is_number(),
            "a split always says its share: there is no default on the wire (INV-01)"
        );

        // An empty workspace is #141's legal one — no tiling to draw, said as `null` and not as an empty tree.
        assert!(
            wall.workspaces[1].empty,
            "workspace 2 holds no pane and is still on the wall"
        );
        assert_eq!(wall.workspaces[1].tiling, None);
        assert_eq!(
            serde_json::to_value(&wall).expect("serializable")["workspaces"][1]["tiling"],
            serde_json::Value::Null,
            "a client draws a row for a workspace with no tiling: null, not an empty tree"
        );
    }
}
