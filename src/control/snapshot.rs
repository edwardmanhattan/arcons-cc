//! **The `Scene`** — what a picture is computed from, numbered (#201, `ADR-0006`).
//!
//! A viewer on another machine draws the same wall from this: the workspaces, the panes in them, where each is
//! looking, what each is hiding, which surface is open, and where the cursors are. `State::scene()` takes one and
//! `State::apply_scene()` builds a wall from it — both in `src/ui/scene.rs`, because the state's fields are the `ui`
//! module's business and this file is only the shape that travels.
//!
//! **What is deliberately not here**, and why: the frame a map thread last drew, the threads themselves, decoded
//! pictures, textures, layout results. Those are the *drawing machine's*, and a viewer makes its own. They are
//! `Pane::frame`, `Pane::map`, `State::pictures`, `State::assets` and the image cache — none of which can be sent,
//! and none of which a picture can be computed from without the machine that drew it.
//!
//! **The water is not here either — yet.** The fleet, the fixes, the cards' text, the newsroom's stories and the
//! facilities are `State`'s data, and moving them into the `Scene` is #203 ("the water moves to the authority").
//! Until then a replay proves the **arrangement** is complete with the water held still, which is the half this
//! ticket owns, and the test says so out loud rather than pretending.
//!
//! **Absence is not zero** (`INV-01`): a pane no one named has `name: null`, a pane looking at nothing has
//! `selected: null`, a workspace whose tiling could not be read has `tiling: null` and the wall falls back to a row.
//! A `0` or an empty string would be a claim the wall does not make.
//!
//! **One vocabulary, two readers.** `Seq` is the whole reason a client never replays: a scene is a value, not a
//! stream, so a viewer that has fallen behind takes a fresh one. `v` is the protocol version a client refuses by name
//! (`unsupported protocol version`, `INV-07`).

use serde::{Deserialize, Serialize};

use crate::config::layout::SavedNode;
use crate::control::projection::CameraView;
use crate::ui::ViewId;

/// The version a client must speak. A viewer that cannot speak it says so instead of guessing at fields.
pub const SCENE_VERSION: u32 = 1;

/// **The wall, as a picture is computed from it.**
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    /// The protocol version — [`SCENE_VERSION`].
    pub v: u32,
    /// A counter that only goes up, so two scenes can be ordered and a gap is detectable. The authority owns it (#202).
    pub seq: u64,
    /// Which workspace is on screen, as an index into [`Scene::workspace`].
    pub active: usize,
    /// The pane with the keys. `null` on a workspace that holds none — legal since #141.
    pub focused: Option<ViewId>,
    /// The pane drawn over the wall (#85), or `null`.
    pub maximised: Option<ViewId>,
    /// The surfaces a hand can leave open. One at a time, by construction (`ADR-0004`).
    pub surface: Surface,
    /// Every cursor the wall draws (#167, #205), in no meaningful order.
    pub cursors: Vec<CursorAt>,
    /// The workspaces, in chip order.
    pub workspace: Vec<Workspace>,
}

/// One workspace: its name, its tiling, and its panes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    /// The pane this workspace would focus, as an index into [`Workspace::pane`]; `null` when it holds none.
    pub focused: Option<u32>,
    /// The tiling, in the **same shape `layout.toml` uses** (#153) — so a scene and the file cannot disagree about
    /// what a split is, and a tiling the wall cannot read falls back to a row in one place.
    pub tiling: Option<SavedNode>,
    pub pane: Vec<Pane>,
}

/// One pane: what it is, where it looks, what it hides, and what it is showing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub id: ViewId,
    pub content: Content,
    /// A camera, for every kind of pane: a tree or newsroom pane keeps one too, because a pane that changes content
    /// and changes back should come back where it was (`layout.toml` says the same).
    #[serde(flatten)]
    pub camera: CameraView,
    /// The operator's name, or `null` for "nobody named this pane" (#83).
    pub name: Option<String>,
    /// The hull this pane's camera is following (#169), or `null`. A follow is a live mode, and a viewer is told
    /// about it because it is what the pane is doing — not because it should be saved.
    pub follow: Option<i64>,
    /// The hull whose card this pane shows (#99), or `null`. The card's *text* is water (#203); the selection is
    /// arrangement, and it is what decides which card is drawn.
    pub selected: Option<i64>,
    pub filter: Filter,
    /// The tree pane's own camera (#127), or `null` on a pane that draws no tree.
    pub tree: Option<Tree>,
    /// The newsroom's tab, page and open story (#136), or `null` on a pane that is not the newsroom.
    pub news: Option<News>,
}

/// **What a pane is** (#116, #136) — the same three kinds the projection names, with the one field a tree pane needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Content {
    Map,
    Tree {
        /// The structure this pane is about. Absent is **every structure the wall knows** (#116), which is a
        /// different claim from "structure 0".
        #[serde(default)]
        organisation: Option<i64>,
    },
    Newsroom,
}

/// A pane's filter (#111) — what it draws and what it hides.
///
/// The sets are sorted vectors rather than sets: JSON has one shape for a list, the order makes two scenes with the
/// same content compare equal, and a client that rebuilds a set gets the same set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub hulls: bool,
    pub installations: bool,
    pub objects: bool,
    #[serde(default)]
    pub hidden_types: Vec<i64>,
    #[serde(default)]
    pub hidden_domains: Vec<i64>,
    #[serde(default)]
    pub hidden_categories: Vec<i64>,
}

/// A tree pane's camera (#127).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    /// Whether a hand has taken the camera — until it has, a resize re-fits (#127).
    pub touched: bool,
}

/// The newsroom pane's view (#136): which tab, which page, and the story it has open.
///
/// The rows are water (#203): a viewer is told what a pane is *showing*, and a story's text arrives with the water
/// because it is what the wall read, not where it is looking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct News {
    /// The tab, in the word the wire and `layout.toml` both use (`international`, `domestic`, …).
    pub scope: String,
    pub page: u32,
    /// The story whose detail is open, or `null` on the list.
    pub selected: Option<i64>,
}

/// **The surfaces a hand can leave open** (`ADR-0004`), without the ones a hand *holds*.
///
/// A lift and a border drag are the pointer's, last for milliseconds, and are deliberately outside the door
/// (`ADR-0005`): a viewer's own gesture is its own, and a scene that carried one would be a scene that could strand a
/// pane in the air on somebody else's screen.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "open", rename_all = "lowercase")]
pub enum Surface {
    #[default]
    Idle,
    Menu {
        pane: ViewId,
        at: (f32, f32),
        /// Which row's waterfall is open (#140), by its key; `null` for the first surface only.
        row: Option<String>,
        /// The row the keyboard is on (#164), or `null` when no row has the ring.
        ring: Option<usize>,
    },
    Filtering {
        pane: ViewId,
    },
    Renaming {
        target: Target,
        /// What is in the field. The caret goes with it, because a sheet's caret is drawn.
        text: String,
        caret: usize,
    },
    Settings,
}

/// What a rename sheet is naming (#83, #139, #154).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "what", rename_all = "lowercase")]
pub enum Target {
    Pane { pane: ViewId },
    Workspace { index: usize },
}

/// One cursor (#167), as a scene carries it.
///
/// There is one today and a list already, because #205 makes it one per origin: a pointer is a hand, and a fleet has
/// more than one. `moved_at` is on the **frame clock**, so a fade is a function of the picture and not of a wall.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CursorAt {
    pub at: (f32, f32),
    pub down: bool,
    pub moved_at: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A scene survives a round trip through JSON**, which is what a viewer does with it — and the shape is the
    /// one the document names, so a field that arrives as something else is caught here rather than on a phone.
    #[test]
    fn a_scene_round_trips_through_its_own_json() {
        let scene = Scene {
            v: SCENE_VERSION,
            seq: 7,
            active: 0,
            focused: Some(2),
            maximised: None,
            surface: Surface::Menu {
                pane: 2,
                at: (10.0, 20.0),
                row: None,
                ring: Some(3),
            },
            cursors: vec![CursorAt {
                at: (1.0, 2.0),
                down: false,
                moved_at: 0.5,
            }],
            workspace: vec![Workspace {
                name: "Straits watch".to_string(),
                focused: Some(0),
                tiling: None,
                pane: vec![Pane {
                    id: 2,
                    content: Content::Tree {
                        organisation: Some(4),
                    },
                    camera: CameraView {
                        lat: -6.1,
                        lon: 106.8,
                        zoom: 9.0,
                    },
                    name: Some("Chain".to_string()),
                    follow: Some(12),
                    selected: None,
                    filter: Filter {
                        hulls: true,
                        ..Filter::default()
                    },
                    tree: Some(Tree {
                        pan_x: 1.0,
                        pan_y: 2.0,
                        zoom: 1.5,
                        touched: true,
                    }),
                    news: None,
                }],
            }],
        };
        let text = serde_json::to_string(&scene).expect("a scene serialises");
        let back: Scene = serde_json::from_str(&text).expect("and parses");
        assert_eq!(back, scene, "the round trip is the identity: {text}");
    }

    /// **A content kind a client does not know is a refusal, not a default** (`INV-07`): the enum is tagged, so a
    /// fourth kind fails to parse instead of arriving as a map pane.
    #[test]
    fn an_unknown_kind_of_pane_is_not_a_map_pane() {
        let json = r#"{"id":1,"content":{"kind":"radio"},"lat":0,"lon":0,"zoom":1,
            "name":null,"follow":null,"selected":null,"filter":{},"tree":null,"news":null}"#;
        assert!(
            serde_json::from_str::<Pane>(json).is_err(),
            "a pane kind nothing draws must be refused by name, not quietly become a map"
        );
    }
}
