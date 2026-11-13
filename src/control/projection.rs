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
}
