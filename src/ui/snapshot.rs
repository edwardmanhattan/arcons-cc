//! **The `Scene` from a `State`, and back** (#201, `ADR-0006`).
//!
//! `crate::control::snapshot` is the shape that travels; this is the two functions that fill and apply it. They live
//! here because a `State`'s fields are this module's business — the same arrangement as `layout.toml`
//! (`config::layout` holds the file's types, `ui::saved` writes and reads it).
//!
//! **A scene is the arrangement, not the water.** The fleet, the cards' text and the newsroom's stories stay on the
//! machine that read them until #203 moves them into the scene. So applying a scene to the state that made it is a
//! round trip that must not change a pixel — which is what `tests/scene_replay.rs` proves, scene by scene, through
//! the real binary: the only way to know the arrangement's vocabulary is complete is to draw the wall from it.

use crate::config::layout::SavedNode;
use crate::control::projection::CameraView;
use crate::control::snapshot::{
    Content, CursorAt, Filter, News, Pane as ScenePane, SCENE_VERSION, Scene, Surface, Target,
    Tree, Workspace as SceneWorkspace,
};
use crate::ui::view::Layout;
use crate::ui::{
    Camera, Interaction, Pane, PaneContent, Rename, RenameTarget, State, ViewId, Workspace,
    default_workspace_name,
};

impl State {
    /// **The wall, as a picture is computed from it** (#201).
    ///
    /// One function reading `State` and nothing else: no clock, no picture's pixels, no thread. A closure's order is
    /// the order of the workspaces and the panes, because that order *is* the chips and the tiling's slots.
    pub fn scene(&self) -> Scene {
        Scene {
            v: SCENE_VERSION,
            seq: self.scene_seq,
            active: self.active,
            focused: self.layout().map(|layout| layout.focused()),
            maximised: self.maximised(),
            surface: surface_of(&self.interaction),
            // **The water travels with the arrangement** (#203): the fleet, its fixes, and the ages this process
            // measured — so a viewer draws the same markers and the same ages without reading Minos at all.
            water: Some(self.picture.water(self.now())),
            cursors: self
                .cursors
                .iter()
                .map(|(origin, cursor)| CursorAt {
                    origin: origin.as_str().to_string(),
                    at: cursor.at,
                    down: cursor.down,
                    moved_at: cursor.moved_at,
                })
                .collect(),
            workspace: self
                .workspaces
                .iter()
                .map(|workspace| SceneWorkspace {
                    name: workspace.name.clone(),
                    focused: workspace.layout.as_ref().and_then(|layout| {
                        workspace
                            .panes
                            .iter()
                            .position(|pane| pane.id == layout.focused())
                            .map(|index| index as u32)
                    }),
                    tiling: workspace
                        .layout
                        .as_ref()
                        .and_then(|layout| layout.shape())
                        .and_then(|shape| {
                            let slots: Vec<ViewId> =
                                workspace.panes.iter().map(|pane| pane.id).collect();
                            crate::ui::saved::save_shape(&shape, &slots)
                        }),
                    pane: workspace.panes.iter().map(scene_of_pane).collect(),
                })
                .collect(),
        }
    }

    /// **A viewer's way in**: take this wall's `Scene`, empty the arrangement, and build it back from the scene alone
    /// (#201).
    ///
    /// Not the same claim as applying a scene to the state that produced it — that comparison is blind, because both
    /// sides come from the same function and an omission cancels out. Emptying first is what a viewer actually does:
    /// the arrangement it holds is the scene's and nothing else. The water (this machine's copy of what the authority
    /// sent) stays where it is until #203 makes it part of the scene.
    pub fn rebuild_from_scene(&mut self) -> bool {
        let scene = self.scene();
        // **The water is the scene's too** (#203): emptied first, so a scene that forgot a hull is a scene that draws
        // fewer markers — which is a claim the replay test can fail on rather than one it cannot see.
        self.picture = crate::units::Picture::default();
        self.apply_scene_forced(&scene)
    }

    /// **Build the wall a scene describes** (#201), in place.
    ///
    /// In place because the water is this state's and stays: a viewer's fleet is its own copy of what the authority
    /// sent, and #203 will make that copy come *with* the scene. Today, applying a scene the state itself produced is
    /// a round trip that must not move a pixel.
    ///
    /// Returns whether the arrangement actually changed, so a client fed the same scene twice does not redraw. `seq`
    /// is compared out: it is the counter that says *when*, not *what*.
    pub fn apply_scene(&mut self, scene: &Scene) -> bool {
        self.apply_scene_body(scene, false)
    }

    /// The same, **without** the "nothing changed" shortcut: a viewer that has just been given a scene builds it even
    /// if the scene it makes from what it holds looks the same. The shortcut compares two scenes made by the same
    /// function, so a field that function forgets cancels out — this is the way in that does not (`#201`).
    pub fn apply_scene_forced(&mut self, scene: &Scene) -> bool {
        self.apply_scene_body(scene, true)
    }

    fn apply_scene_body(&mut self, scene: &Scene, forced: bool) -> bool {
        let before = {
            let mut mine = self.scene();
            mine.seq = scene.seq;
            mine
        };
        if !forced && before == *scene {
            self.scene_seq = scene.seq;
            return false;
        }

        // **The frames are this machine's** (`INV-04`): they are what markers project against, they come from this
        // viewer's own map threads, and a scene does not carry them. A pane that keeps its id keeps the frame it was
        // drawn at, so applying a scene does not blank a map while the thread re-renders.
        let frames: Vec<(ViewId, crate::map::geometry::Rendered)> = self
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.panes.iter())
            .filter_map(|pane| pane.frame.map(|frame| (pane.id, frame)))
            .collect();

        // The old panes go, and their map threads are joined by the drop — a viewer that is told a new arrangement
        // has already closed what the old one held.
        self.workspaces.clear();
        self.interaction = Interaction::Idle;
        self.active = 0;
        self.cursors.clear();

        let mut built = Vec::with_capacity(scene.workspace.len());
        for (index, saved) in scene.workspace.iter().enumerate() {
            // The scene's own ids: a pane keeps its number across a viewer, because the name plate draws it (`#113`)
            // and a client reading `panes[].id` must be able to act on what it read.
            let ids: Vec<ViewId> = saved.pane.iter().map(|pane| pane.id).collect();
            let panes: Vec<Pane> = saved.pane.iter().map(pane_from_scene).collect();
            let layout = saved
                .tiling
                .as_ref()
                .and_then(|node| crate::ui::saved::load_shape(node, &ids))
                .and_then(Layout::from_shape)
                // **An arrangement the wall cannot read is still a wall**: the row of the panes it has, which is
                // what `layout.toml` does with a file it cannot follow (#153).
                .or_else(|| crate::ui::saved::row_layout(&ids, saved.focused));
            let focused = saved
                .focused
                .and_then(|slot| ids.get(slot as usize).copied());
            let mut workspace = Workspace {
                name: if saved.name.trim().is_empty() {
                    default_workspace_name(index)
                } else {
                    saved.name.trim().to_string()
                },
                layout,
                panes,
            };
            if let (Some(layout), Some(focused)) = (workspace.layout.as_mut(), focused) {
                layout.focus(focused);
            }
            built.push(workspace);
        }
        self.workspaces = built;
        if self.workspaces.is_empty() {
            // A scene with no workspace is not a wall: the default one, so a viewer is never a black screen.
            self.workspaces = vec![Workspace {
                name: default_workspace_name(0),
                layout: None,
                panes: Vec::new(),
            }];
        }
        self.next_pane = self
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.panes.iter().map(|pane| pane.id))
            .max()
            .map_or(1, |highest| highest + 1);
        self.active = scene.active.min(self.workspaces.len() - 1);

        // A maximised pane is a layout fact (`#85`), so it is set after the tiling exists.
        if let Some(pane) = scene.maximised
            && let Some(layout) = self.layout_mut()
        {
            layout.maximise(pane);
        }

        // **A selection rebuilds its card**, because a card is drawn from the water and a viewer's water is its own.
        // Setting the field alone would draw a card-shaped hole (#99).
        for (pane, wanted) in scene
            .workspace
            .iter()
            .flat_map(|workspace| workspace.pane.iter())
            .map(|pane| (pane.id, pane.selected))
        {
            if let Some(unit) = wanted {
                self.select_in(pane, unit);
            }
        }

        for (id, frame) in frames {
            if let Some(pane) = self.pane_mut(id) {
                pane.frame = Some(frame);
            }
        }

        // **The water arrives with it** (#203). A scene from a wall that has not read Minos carries none, which is
        // "not asked" (`INV-09`) — so a viewer keeps what it had rather than being emptied by a wall that is still
        // waking up.
        if let Some(water) = &scene.water {
            self.picture.adopt(water, self.now());
        }

        self.interaction = interaction_of(&scene.surface);
        self.cursors = scene
            .cursors
            .iter()
            .map(|cursor| {
                (
                    crate::ui::Origin::named(&cursor.origin),
                    crate::ui::Cursor {
                        at: cursor.at,
                        down: cursor.down,
                        moved_at: cursor.moved_at,
                    },
                )
            })
            .collect();
        self.scene_seq = scene.seq;
        self.layout_dirty = true;
        #[cfg(feature = "map")]
        {
            // The threads that arrive with these panes have never been told a cadence; the next frame's
            // `apply_cadence` does, and this is what makes it run (`#71`).
            self.cadenced = None;
        }
        true
    }
}

/// One pane, from its scene.
fn scene_of_pane(pane: &Pane) -> ScenePane {
    ScenePane {
        id: pane.id,
        content: match pane.content {
            PaneContent::Map => Content::Map,
            PaneContent::Tree { organisation } => Content::Tree { organisation },
            PaneContent::Newsroom => Content::Newsroom,
        },
        camera: CameraView {
            lat: pane.camera.center.0,
            lon: pane.camera.center.1,
            zoom: pane.camera.zoom,
        },
        name: pane.name.clone(),
        follow: pane.follow,
        selected: pane.view.selected,
        filter: Filter {
            hulls: pane.view.filter.hulls,
            installations: pane.view.filter.installations,
            objects: pane.view.filter.objects,
            hidden_types: pane.view.filter.hidden_types.iter().copied().collect(),
            hidden_domains: pane.view.filter.hidden_domains.iter().copied().collect(),
            hidden_categories: pane.view.filter.hidden_categories.iter().copied().collect(),
        },
        tree: pane.tree.as_ref().map(|tree| Tree {
            pan_x: tree.pan.0,
            pan_y: tree.pan.1,
            zoom: tree.zoom,
            touched: tree.touched,
        }),
        news: pane.news.as_ref().map(|news| News {
            scope: crate::ui::saved::scope_word(news.scope).to_string(),
            page: news.page,
            selected: news.selected,
        }),
    }
}

/// One pane, from a scene.
fn pane_from_scene(saved: &ScenePane) -> Pane {
    let camera = Camera::new((saved.camera.lat, saved.camera.lon), saved.camera.zoom);
    let mut pane = match saved.content {
        Content::Map => Pane::new(saved.id, camera),
        Content::Tree { organisation } => Pane::tree(saved.id, camera, organisation),
        Content::Newsroom => Pane::newsroom(saved.id, camera),
    };
    pane.name = saved.name.clone();
    pane.follow = saved.follow;
    // The selection is set through `select_in` by the caller, so a card is built from the water this state has.
    pane.view.selected = None;
    pane.view.filter.hulls = saved.filter.hulls;
    pane.view.filter.installations = saved.filter.installations;
    pane.view.filter.objects = saved.filter.objects;
    pane.view.filter.hidden_types = saved.filter.hidden_types.iter().copied().collect();
    pane.view.filter.hidden_domains = saved.filter.hidden_domains.iter().copied().collect();
    pane.view.filter.hidden_categories = saved.filter.hidden_categories.iter().copied().collect();
    if let Some(tree) = saved.tree {
        pane.tree = Some(crate::ui::tree::TreeCamera {
            pan: (tree.pan_x, tree.pan_y),
            zoom: tree.zoom,
            touched: tree.touched,
            ..crate::ui::tree::TreeCamera::default()
        });
    }
    if let Some(news) = &saved.news {
        let view = pane.news.get_or_insert_with(Default::default);
        view.scope = crate::ui::saved::scope_from_word(&news.scope);
        view.page = news.page.max(1);
        // The rows are water (#203): a viewer is told what the pane is showing, and the stories arrive with the
        // fleet. Until then the pane asks for its page, which is what a restored pane does (#153).
        view.asked = None;
        view.stories.clear();
        view.pagination = None;
        view.selected = news.selected;
        view.state = crate::ui::newsroom::ReadState::Reading;
    }
    pane
}

/// The surface a scene carries, without the gestures a hand holds (`ADR-0005`: those are outside the door).
fn surface_of(interaction: &Interaction) -> Surface {
    match interaction {
        Interaction::Idle | Interaction::Lifted(_) | Interaction::Resizing(_) => Surface::Idle,
        Interaction::Menu {
            pane,
            at,
            open,
            ring,
        } => Surface::Menu {
            pane: *pane,
            at: *at,
            row: open.map(str::to_string),
            ring: *ring,
        },
        Interaction::Filtering { pane } => Surface::Filtering { pane: *pane },
        Interaction::Renaming(rename) => Surface::Renaming {
            target: match rename.target {
                RenameTarget::Pane(pane) => Target::Pane { pane },
                RenameTarget::Workspace(index) => Target::Workspace { index },
            },
            text: rename.edit.text.clone(),
            // The caret is a byte offset in the field; a scene carries the character it sits before, because a
            // client's text is characters and a byte offset into somebody else's encoding is not a fact.
            caret: rename.edit.text.chars().take(rename.edit.focus).count(),
        },
        Interaction::Settings => Surface::Settings,
    }
}

/// The interaction a scene's surface describes.
fn interaction_of(surface: &Surface) -> Interaction {
    match surface {
        Surface::Idle => Interaction::Idle,
        Surface::Menu {
            pane,
            at,
            row,
            ring,
        } => Interaction::Menu {
            pane: *pane,
            at: *at,
            // The key is `&'static str` in the interaction, because a row's key is a compile-time name (#140). A
            // scene that names a row the wall does not have therefore opens the first surface: the alternative is
            // leaking a string for the life of the process, and a row key is not a value a client may invent.
            open: row.as_deref().and_then(crate::ui::menu_key),
            ring: *ring,
        },
        Surface::Filtering { pane } => Interaction::Filtering { pane: *pane },
        Surface::Renaming {
            target,
            text,
            caret,
        } => {
            let target = match *target {
                Target::Pane { pane } => RenameTarget::Pane(pane),
                Target::Workspace { index } => RenameTarget::Workspace(index),
            };
            let mut rename = Rename::new(target, Some(text.as_str()));
            rename.edit.focus = (*caret).min(text.len());
            rename.edit.anchor = rename.edit.focus;
            Interaction::Renaming(rename)
        }
        Surface::Settings => Interaction::Settings,
    }
}

/// A tiling, for a test that wants one without building a wall.
#[doc(hidden)]
pub fn tiling_of(shape: &SavedNode) -> Option<SavedNode> {
    Some(shape.clone())
}
