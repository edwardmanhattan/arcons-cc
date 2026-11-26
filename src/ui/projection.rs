//! **The wall's projection, built from the wall** (#181, #221): the read model a controller sees.
//!
//! The shape is [`crate::control::projection`]'s; this is the one function that reads `State` and fills it.
//! Keeping the builder here is what lets `control` name no `ui` type: the wire knows its own shape, the wall
//! knows how to build it.

use crate::config::layout::Axis;
use crate::control::projection::{CameraView, Kind, PaneView, Tiling, Wall, WorkspaceView};
use crate::ui::view::{Dir, Shape};
use crate::verb::PaneContent;

use super::State;

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
        seq: state.projection_seq,
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

/// **The projection, versioned** (#221): what a client reads, and the number that says which version it is.
///
/// The version is the **projection's own**. [`of`] is a function of `State`, so "it changed" is a comparison of two
/// projections rather than a promise thirty call sites would have to keep — a wall that changed nothing cannot hand
/// out a new number, which is precisely the falsifier this exists for (a phone that wakes for nothing).
///
/// The first projection is version `0`, so a client's first read is the wall *at* a number rather than one step past
/// nothing; every change the wall answers from then on is one more.
pub fn versioned(state: &mut State) -> Wall {
    let wall = of(state);
    if state.projection_seen.as_ref() == Some(&wall) {
        // Nothing a client reads has moved: the same number says so, and a watcher publishes nothing.
        return wall;
    }
    if state.projection_seen.is_some() {
        state.projection_seq += 1;
    }
    let wall = of(state);
    state.projection_seen = Some(wall.clone());
    wall
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::view;
    use crate::verb::ViewId;

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
            ["active", "focused", "maximised", "seq", "workspaces"],
            "the top-level shape is the document's shape"
        );
    }

    /// **The projection is versioned** (#221): the number moves when — and only when — what a client reads changed, so a
    /// watcher can be told "it is not what you hold" without a diff, and a wall that changed nothing cannot hand out a
    /// new number at all.
    ///
    /// The falsifiers are the two implementations that are easier: a counter bumped by every call (a wall that
    /// publishes on every beat, which wakes a phone for nothing) and a counter bumped by `arranged()` (a save's
    /// debounce and a change nobody can read are one number, so a real change can hide behind them).
    #[test]
    fn the_version_moves_only_when_the_projection_did() {
        let mut state = State::default();
        let first = versioned(&mut state);
        assert_eq!(
            first.seq, 0,
            "the first projection a client reads is version 0"
        );
        assert_eq!(
            versioned(&mut state).seq,
            0,
            "a wall that changed nothing handed out a new version: a watch would publish, and a phone would wake for \
             nothing (INV-01's rule, in a number)"
        );

        // **A change a client can read is one version further on.** A new workspace changes the chips and the active
        // index — both are in the projection.
        state.add_workspace();
        let second = versioned(&mut state);
        assert_eq!(
            second.seq, 1,
            "the projection changed and the number did not follow it: a client cannot put the two in order"
        );
        assert_eq!(
            second.active, 3,
            "and the version is the wall it was taken from: the fourth workspace is the one it made"
        );
        assert_eq!(
            versioned(&mut state).seq,
            1,
            "the same projection twice is one version, however often it is asked for"
        );

        // **An arrangement change a client cannot see is not a version**: `arranged()` is the save's clock and the
        // scene's counter (#201, #153), and a projection that moved with it would publish a wall identical to the one
        // the client holds.
        state.arranged();
        assert_eq!(
            versioned(&mut state).seq,
            1,
            "a change to nothing a client reads moved the projection's version (#221)"
        );

        // **And a change that leaves the projection as it was is not a change**: selecting in a pane the hull it already
        // shows twice is the same projection, so it is the same number. (The pane is workspace 1's, because the
        // workspace just made is the one on screen and it holds none.)
        let pane = state.workspaces[0].panes[0].id;
        state.select_in(pane, 512_345);
        assert_eq!(
            versioned(&mut state).seq,
            2,
            "a selection a client reads is a version"
        );
        state.select_in(pane, 512_345);
        assert_eq!(
            versioned(&mut state).seq,
            2,
            "selecting the hull a pane already shows is not a version: nothing a client reads moved"
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
