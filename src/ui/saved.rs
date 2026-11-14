//! **The wall as the operator arranged it** (#153): the model's half of `layout.toml`.
//!
//! `config::layout` owns the file — which keys exist, what a drawable value means, how it is written.
//! This module owns the **conversion**, and it is here rather than there because a layout is a `State`:
//! workspaces, panes, ids, cameras, filters. `config` must not learn what a `Pane` is; `ui` already reads
//! `config` for the settings, the window and the server.
//!
//! Three decisions worth stating, because they are the ones a reviewer would otherwise have to infer:
//!
//! - **Ids are re-minted on load.** A pane id is the wall's (#138), minted by [`State::allocate_pane`], and
//!   a file that names ids would let two panes claim one number. The file names panes **by index inside
//!   their workspace**, so the tiling survives and the ids do not.
//! - **The write is off the UI thread** (`INV-05`). Serialising is pure and cheap, so `poll` does that and
//!   hands the text to a thread for the file: a camera gesture is a hundred changes a second, and a
//!   `fsync` on the thread that draws the sea is a stutter on every one of them.
//! - **Nothing is saved that is not an arrangement.** No selected hull, no card, no headlines: those are
//!   answers the wall has read, and a wall that came back showing yesterday's answer as if it were
//!   today's would be lying. The filter is the exception and it is deliberate — a filter is a *question*,
//!   and a pane that hid the bases yesterday asked to hide them until told otherwise.

use std::path::PathBuf;

use super::*;
use crate::config::layout::{
    Axis, Content, Saved, SavedFilter, SavedNews, SavedNode, SavedPane, SavedTree, SavedWorkspace,
};

/// How long an arrangement must hold still before it is written, on the frame clock.
///
/// A drag of a camera, a border or a gesture is a stream of changes; the file wants the state they settled
/// into. 1.5 s is longer than a double-click and shorter than an operator's patience for "did it save?".
pub const SAVE_AFTER: f64 = 1.5;

/// Where the wall's arrangement is remembered.
pub fn saved_layout_path() -> PathBuf {
    crate::config::layout_file()
}

impl State {
    /// **Restore the arrangement** (#153), for a wall: a capture never reads the file, so a golden cannot
    /// depend on the machine it was rendered on — the settings file's rule, applied to the bigger file.
    pub fn with_saved_layout(mut self) -> State {
        if rune::is_headless() {
            return self;
        }
        let path = saved_layout_path();
        let (saved, note) = Saved::read(&path);
        tracing::info!("{note}");
        if let Some(saved) = saved
            && !apply_saved(&mut self, &saved)
        {
            tracing::warn!(
                "layout: {} named no workspace the wall could build, opening the default wall",
                path.display()
            );
        }
        self
    }

    /// **Something about the arrangement changed.** Cheap on purpose: it records the frame at which the
    /// change happened, and the save waits for the changes to stop.
    pub fn arranged(&mut self) {
        if !self.headless {
            self.arranged_at = Some(self.logical);
        }
    }

    /// Whether the arrangement has held still long enough to write.
    pub fn save_due(&self) -> bool {
        !self.headless
            && self
                .arranged_at
                .is_some_and(|at| self.logical - at >= SAVE_AFTER)
    }

    /// Write the arrangement now, off the UI thread (`INV-05`), and forget that it was dirty.
    ///
    /// Serialising happens here — it is pure and it is what proves the file can be built — and the syscall
    /// happens on a thread. A wall that loses power mid-write comes back with the previous file: the writer
    /// renames a temp file into place (`config::write_atomically`).
    pub fn save_layout(&mut self) {
        if self.headless {
            return;
        }
        self.arranged_at = None;
        let saved = self.snapshot();
        let path = saved_layout_path();
        let text = match toml::to_string_pretty(&saved) {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!("layout: the wall cannot describe itself ({error})");
                return;
            }
        };
        std::thread::spawn(
            move || match crate::config::write_atomically(&path, &text) {
                Ok(()) => tracing::debug!("layout: wrote {}", path.display()),
                Err(error) => tracing::warn!("layout: {error}"),
            },
        );
    }

    /// **The arrangement, as the file's own vocabulary.** Pure: a test can compare two of these, and a
    /// capture can never produce one, because a capture is headless and [`State::arranged`] ignores it.
    pub fn snapshot(&self) -> Saved {
        Saved {
            version: crate::config::layout::VERSION,
            active: self.active.min(self.workspaces.len().saturating_sub(1)),
            workspace: self
                .workspaces
                .iter()
                .map(|workspace| SavedWorkspace {
                    // **A name nobody typed is not a name** (#83, #113): the default title is drawn, not
                    // saved, so a wall whose names were never touched writes no names at all.
                    name: workspace_name(&workspace.name),
                    focused: workspace
                        .layout
                        .as_ref()
                        .and_then(|layout| {
                            workspace
                                .panes
                                .iter()
                                .position(|pane| pane.id == layout.focused())
                        })
                        .map(|index| index as u32),
                    tiling: workspace.layout.as_ref().and_then(|layout| {
                        let slots: Vec<ViewId> =
                            workspace.panes.iter().map(|pane| pane.id).collect();
                        layout.shape().and_then(|shape| save_shape(&shape, &slots))
                    }),
                    pane: workspace.panes.iter().map(save_pane).collect(),
                })
                .collect(),
        }
    }

    /// **Write the arrangement before the process leaves.** Called where the wall stops, because a
    /// `SIGTERM` a second after a split must not lose the split.
    pub fn flush_layout(&mut self) {
        self.flush_layout_at(&saved_layout_path());
    }

    /// The same write, at a path the caller names — **so the order of a stop can be proved against a file a test
    /// owns** (#174): the deadline is armed only after this returned, because a truncated `layout.toml` is worse
    /// than a slow stop.
    ///
    /// Returns whether anything was written: nothing to write is not a failure, and a caller that arms a deadline
    /// anyway is a caller that does not need to know.
    pub fn flush_layout_at(&mut self, path: &std::path::Path) -> bool {
        // A capture is not a wall, and a scene built by handlers that ran before `headless` was set must
        // not write a file on its way out: the guard is the same one `arranged` and `save_layout` carry.
        if self.headless {
            return false;
        }
        if self.arranged_at.take().is_none() {
            return false;
        }
        let saved = self.snapshot();
        match toml::to_string_pretty(&saved) {
            Ok(text) => {
                if let Err(error) = crate::config::write_atomically(path, &text) {
                    tracing::warn!("layout: {error}");
                    false
                } else {
                    tracing::info!("layout: wrote {} on the way out", path.display());
                    true
                }
            }
            Err(error) => {
                tracing::warn!("layout: the wall cannot describe itself ({error})");
                false
            }
        }
    }
}

/// A workspace's saved name: **the operator's, or nothing at all**.
fn workspace_name(name: &str) -> String {
    // A default name is `Workspace N`, which the wall derives from the index; saving it would freeze the
    // numbering into the file, so a wall that comes back with different workspaces keeps its own numbers.
    if name.starts_with("Workspace ") && name["Workspace ".len()..].parse::<usize>().is_ok() {
        String::new()
    } else {
        name.to_string()
    }
}

fn save_pane(pane: &Pane) -> SavedPane {
    let filter = &pane.view.filter;
    SavedPane {
        content: match pane.content {
            PaneContent::Map => Content::Map,
            PaneContent::Tree { .. } => Content::Tree,
            PaneContent::Newsroom => Content::Newsroom,
        },
        organisation: match pane.content {
            PaneContent::Tree { organisation } => organisation,
            _ => None,
        },
        // Six decimals: the file is diffed, and `106.90999999999997` is not a coordinate anybody typed.
        lat: crate::config::layout::round_coord(pane.camera.center.0),
        lon: crate::config::layout::round_coord(pane.camera.center.1),
        zoom: pane.camera.zoom,
        name: pane.name.clone(),
        tree: pane.tree.as_ref().map(|tree| SavedTree {
            pan_x: tree.pan.0,
            pan_y: tree.pan.1,
            zoom: tree.zoom,
            touched: tree.touched,
        }),
        news: pane.news.as_ref().map(|view| SavedNews {
            scope: scope_word(view.scope).to_string(),
            page: view.page,
        }),
        filter: Some(SavedFilter {
            hulls: filter.hulls,
            installations: filter.installations,
            objects: filter.objects,
            hidden_types: filter.hidden_types.iter().copied().collect(),
            hidden_domains: filter.hidden_domains.iter().copied().collect(),
            hidden_categories: filter.hidden_categories.iter().copied().collect(),
        }),
    }
}

/// The wire word for a scope: the file speaks the server's vocabulary, not the wall's labels.
///
/// `NewsScope::Unknown` is a real scope on the wire and **not a tab the wall draws**, so a pane can never
/// be reading it: a file that says `unknown` comes back on the international tab, which is the tab the
/// panel opens on.
fn scope_word(scope: crate::data::types::NewsScope) -> &'static str {
    match scope {
        crate::data::types::NewsScope::Unknown => {
            crate::data::types::NewsScope::International.as_str()
        }
        known => known.as_str(),
    }
}

fn scope_from_word(word: &str) -> crate::data::types::NewsScope {
    use crate::data::types::NewsScope;
    match word {
        "regional" => NewsScope::Regional,
        "national" => NewsScope::National,
        _ => NewsScope::International,
    }
}

/// A layout's shape, in the file's terms: leaves become **indices into the workspace's panes**.
fn save_shape(shape: &view::Shape, slots: &[ViewId]) -> Option<SavedNode> {
    match shape {
        view::Shape::Leaf(id) => {
            let index = slots.iter().position(|slot| slot == id)?;
            Some(SavedNode {
                pane: Some(index as u32),
                ..SavedNode::default()
            })
        }
        view::Shape::Split { dir, ratio, a, b } => Some(SavedNode {
            pane: None,
            split: Some(match dir {
                Dir::Horizontal => Axis::Side,
                Dir::Vertical => Axis::Stack,
            }),
            ratio: Some(*ratio),
            a: Some(Box::new(save_shape(a, slots)?)),
            b: Some(Box::new(save_shape(b, slots)?)),
        }),
    }
}

fn load_shape(node: &SavedNode, ids: &[ViewId]) -> Option<view::Shape> {
    match (&node.pane, node.split, &node.a, &node.b) {
        (Some(index), None, None, None) => Some(view::Shape::Leaf(*ids.get(*index as usize)?)),
        (None, Some(axis), Some(a), Some(b)) => {
            let ratio = node.ratio.unwrap_or(0.5);
            Some(view::Shape::Split {
                dir: match axis {
                    Axis::Side => Dir::Horizontal,
                    Axis::Stack => Dir::Vertical,
                },
                ratio,
                a: Box::new(load_shape(a, ids)?),
                b: Box::new(load_shape(b, ids)?),
            })
        }
        _ => None,
    }
}

/// **Build the wall the file describes.** `false` when the file names nothing a wall can be built from,
/// which the caller reports and draws as the default wall.
///
/// The panes are built **whatever their kind** and no thread is started here: `poll` gives a thread to
/// any pane that draws a picture (`State::spawn_missing_maps`, #160) — including these, on the first
/// frame, at the camera the file gave each one — so a restored wall opens on the operator's sea, not on
/// the default one and then a jump. Spawning here was #153's mistake: a fourth place that has to
/// remember is a place that forgets.
pub fn apply_saved(state: &mut State, saved: &Saved) -> bool {
    if saved.workspace.is_empty() {
        return false;
    }
    // Any panes the wall already had are the default wall's: they go, and their threads are joined by the
    // drop. `close_maps` is not called, because the wall is not leaving.
    state.workspaces.clear();
    state.next_pane = 1;
    state.interaction = Interaction::Idle;
    state.active = 0;

    let mut built = Vec::with_capacity(saved.workspace.len());
    for (index, saved_workspace) in saved.workspace.iter().enumerate() {
        let name = if saved_workspace.name.trim().is_empty() {
            default_workspace_name(index)
        } else {
            saved_workspace.name.trim().to_string()
        };
        let ids: Vec<ViewId> = saved_workspace
            .pane
            .iter()
            .map(|_| state.allocate_pane())
            .collect();
        let panes: Vec<Pane> = saved_workspace
            .pane
            .iter()
            .zip(ids.iter().copied())
            .map(|(saved, id)| load_pane(saved, id))
            .collect();
        let layout = saved_workspace
            .tiling
            .as_ref()
            .and_then(|node| load_shape(node, &ids))
            .and_then(Layout::from_shape)
            // **An arrangement the wall cannot read is still a wall**: a row of the panes it has, which is
            // what a scene builds and what the operator can rearrange from.
            .or_else(|| row_layout(&ids, saved_workspace.focused));
        let focused = saved_workspace
            .focused
            .and_then(|slot| ids.get(slot as usize).copied());
        let mut workspace = Workspace {
            name,
            layout,
            panes,
        };
        if let (Some(layout), Some(focused)) = (workspace.layout.as_mut(), focused) {
            layout.focus(focused);
        }
        built.push(workspace);
    }
    state.workspaces = built;
    state.active = saved.active.min(state.workspaces.len() - 1);
    #[cfg(feature = "map")]
    {
        // A restored wall's panes have threads that have never been told a cadence: the next frame's
        // `apply_cadence` does that, and this is what makes it run.
        state.cadenced = None;
    }
    state.layout_dirty = true;
    true
}

/// A workspace's panes as a row, for a tiling the file did not give: the layout a wall can always draw.
fn row_layout(ids: &[ViewId], focused: Option<u32>) -> Option<Layout> {
    let (first, rest) = ids.split_first()?;
    let mut layout = Layout::single_with(*first);
    let mut at = *first;
    for id in rest {
        at = layout.split_side_with(at, view::Side::Right, *id)?;
    }
    if let Some(id) = focused.and_then(|slot| ids.get(slot as usize)) {
        layout.focus(*id);
    }
    Some(layout)
}

fn load_pane(saved: &SavedPane, id: ViewId) -> Pane {
    let camera = Camera::new((saved.lat, saved.lon), saved.zoom);
    let content = match saved.content {
        Content::Map => PaneContent::Map,
        Content::Tree => PaneContent::Tree {
            organisation: saved.organisation,
        },
        Content::Newsroom => PaneContent::Newsroom,
    };
    let mut pane = match content {
        PaneContent::Map => Pane::new(id, camera),
        PaneContent::Tree { organisation } => Pane::tree(id, camera, organisation),
        PaneContent::Newsroom => Pane::newsroom(id, camera),
    };
    pane.name = saved.name.clone();
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
        view.scope = scope_from_word(&news.scope);
        view.page = news.page.max(1);
        // **The rows are not saved.** The pane comes back on the page it was reading, reading: `asked` is
        // `None` with `stories` empty, which is the state `ask_restored_feeds` recognises and turns into
        // the one read the pane needs. Showing yesterday's stories as if they were today's would be a wall
        // lying about what it knows.
        view.asked = None;
        view.stories.clear();
        view.pagination = None;
        view.state = crate::ui::newsroom::ReadState::Reading;
    }
    if let Some(filter) = &saved.filter {
        pane.view.filter.hulls = filter.hulls;
        pane.view.filter.installations = filter.installations;
        pane.view.filter.objects = filter.objects;
        pane.view.filter.hidden_types = filter.hidden_types.iter().copied().collect();
        pane.view.filter.hidden_domains = filter.hidden_domains.iter().copied().collect();
        pane.view.filter.hidden_categories = filter.hidden_categories.iter().copied().collect();
    }
    pane
}

/// **A newsroom pane that came back from the file has asked for nothing yet** (#153): give it its one read.
///
/// A view restored on page 2 is `Reading` with no query in flight and no rows — the state only a restore
/// produces, because every other way of getting a newsroom pane asks on the frame it is opened. Returns
/// whether anything was asked, so the frame that asked is redrawn.
pub fn ask_restored_feeds(state: &mut State) -> bool {
    if state.headless || state.backend.is_none() {
        return false;
    }
    let mut queries = Vec::new();
    for pane in state.panes_mut() {
        let Some(view) = pane.news.as_mut() else {
            continue;
        };
        let waiting = view.asked.is_none()
            && view.stories.is_empty()
            && matches!(view.state, crate::ui::newsroom::ReadState::Reading);
        if waiting {
            queries.push(view.ask());
        }
    }
    let asked = !queries.is_empty();
    if let Some(backend) = &state.backend {
        for query in queries {
            backend.request(crate::data::Command::Stories(query));
        }
    }
    asked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall with three workspaces: the first two panes on two cameras, one named; the second a newsroom
    /// on page 2 with a filter; the third empty.
    fn arranged() -> State {
        let mut state = State::default();
        let first = state.focused();
        state.set_camera_of(first, Camera::new((1.0, 100.0), 9.0));
        if let Some(pane) = state.pane_mut(first) {
            pane.name = Some("Straits".to_string());
        }
        let second = state
            .split_pane_as(first, Camera::new((2.5, 101.5), 6.5), PaneContent::Map)
            .expect("a second pane");
        let index = 1;
        state.switch_to(index);
        let news_id = state.allocate_pane();
        let news = Pane::newsroom(news_id, Camera::START);
        let mut news = news;
        if let Some(view) = news.news.as_mut() {
            view.scope = crate::data::types::NewsScope::National;
            view.page = 2;
        }
        news.view.filter.objects = false;
        news.view.filter.hidden_domains.insert(4);
        state.workspaces[index].layout = Some(Layout::single_with(news_id));
        state.workspaces[index].panes.push(news);
        state.workspaces[index].name = "Straits watch".to_string();
        state.switch_to(0);
        state.focus_pane(second);
        state
    }

    /// **What the wall writes, the wall reads back** (#153): the workspaces, their names and order, the
    /// panes and their kinds, the cameras, the tiling's ratio, the names, the filter and the page.
    #[test]
    fn the_arrangement_survives_a_round_trip() {
        let state = arranged();
        let saved = state.snapshot();
        let mut restored = State::default();
        assert!(apply_saved(&mut restored, &saved));

        assert_eq!(
            restored.workspaces.len(),
            3,
            "three workspaces, and the empty one"
        );
        assert_eq!(
            restored.workspaces[0].name, "Workspace 1",
            "a default name is derived, not saved"
        );
        assert_eq!(restored.workspaces[1].name, "Straits watch");
        assert!(restored.workspaces[2].panes.is_empty());
        assert!(restored.workspaces[2].layout.is_none());
        assert_eq!(restored.active, 0);

        let first = &restored.workspaces[0].panes[0];
        assert_eq!(first.name.as_deref(), Some("Straits"));
        assert_eq!(first.camera.center.0, 1.0);
        assert_eq!(first.camera.center.1, 100.0);
        assert_eq!(first.camera.zoom, 9.0);
        let second = &restored.workspaces[0].panes[1];
        assert_eq!(second.camera.center.0, 2.5);
        assert_eq!(
            restored.workspaces[0].layout.as_ref().map(|l| l.len()),
            Some(2)
        );
        assert_eq!(
            restored.workspaces[0]
                .layout
                .as_ref()
                .map(|layout| layout.focused()),
            Some(second.id),
            "the focused pane of a workspace is remembered"
        );

        let news = &restored.workspaces[1].panes[0];
        assert!(news.news.is_some(), "a newsroom comes back a newsroom");
        let view = news.news.as_ref().expect("its view");
        assert_eq!(view.scope, crate::data::types::NewsScope::National);
        assert_eq!(view.page, 2, "the page the operator was reading");
        assert!(!news.view.filter.objects, "the pane's filter came back");
        assert!(news.view.filter.hidden_domains.contains(&4));
        assert!(
            view.stories.is_empty(),
            "no rows: the pane asks for its page rather than showing yesterday's answer"
        );

        // The ids are the wall's and are re-minted: no two panes share one, and none is the default's.
        let mut ids: Vec<ViewId> = restored
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.panes.iter().map(|pane| pane.id))
            .collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "every pane has its own id");
        assert_eq!(ids[0], 1, "the ids are minted from one, on this wall");
    }

    /// **A restored wall claims a thread for every map pane, and for nothing else** (#160).
    ///
    /// #153 rebuilt the panes from the file and the only spawn left was `setup`'s, which takes the
    /// focused pane whatever it is: a restored wall drew `AWAITING THE PICTURE` on every map, with a
    /// MapLibre context burning behind a newsroom that would never draw one. This is the answer `poll`
    /// asks of the wall, and it is asked here on the rung that needs no GPU.
    #[test]
    fn a_restored_wall_asks_for_a_thread_for_every_map_pane_and_no_other() {
        // Every kind on one wall: the two maps of `arranged`, a tree, and the newsroom.
        let mut state = arranged();
        let target = state.workspaces[0].panes[0].id;
        let tree = state
            .split_pane_as(
                target,
                Camera::new((3.0, 102.0), 5.0),
                PaneContent::Tree { organisation: None },
            )
            .expect("a tree pane");
        let newsroom = state.workspaces[1].panes[0].id;
        let saved = state.snapshot();
        let mut restored = State::default();
        assert!(apply_saved(&mut restored, &saved));

        let missing = restored.panes_without_maps();
        let named: Vec<ViewId> = missing.iter().map(|(id, _, _)| *id).collect();
        let maps: Vec<ViewId> = restored
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.panes.iter())
            .filter(|pane| pane.content.is_map())
            .map(|pane| pane.id)
            .collect();
        assert_eq!(
            named, maps,
            "every map pane is waiting for a picture, and only a map pane is"
        );
        assert!(
            !named.contains(&tree),
            "a tree pane draws a structure: nothing to start and nothing to project"
        );
        assert!(
            !named.contains(&newsroom),
            "a newsroom is rows, not a picture — the pane that must not own a context"
        );

        // The thread is asked for at the camera the file gave the pane, and at that pane's own slot —
        // not at the window's size, which is what a pane in the wall's corner would render blurred.
        let slots: std::collections::HashMap<ViewId, Rect> =
            restored.pane_rects().into_iter().collect();
        for (id, camera, size) in missing {
            let pane = restored.pane(id).expect("a pane the wall named");
            assert_eq!(camera, pane.camera, "the camera the pane is holding");
            let rect = slots.get(&id).expect("a pane on screen");
            assert_eq!(
                size,
                (rect.w.max(1.0), rect.h.max(1.0)),
                "pane {id} asked for a frame that is not its slot"
            );
        }
    }

    /// The engine's half of #160: the list empties, and no pane that draws something other than a
    /// picture is left holding a context. Ignored because it opens MapLibre:
    ///
    /// ```sh
    /// cargo test --features map --lib -- --ignored --nocapture restored_wall_starts
    /// ```
    #[cfg(feature = "map")]
    #[test]
    #[ignore = "map engine: needs a GPU and, when the cache is cold, a style load"]
    fn a_restored_wall_starts_a_thread_for_every_map_pane() {
        let saved = arranged().snapshot();
        let mut restored = State::default();
        assert!(apply_saved(&mut restored, &saved));
        assert!(
            !restored.panes_without_maps().is_empty(),
            "the wall came back with maps to start"
        );
        assert!(restored.spawn_missing_maps(), "and this starts them");
        assert!(
            restored.panes_without_maps().is_empty(),
            "every map pane on the restored wall has a thread"
        );
        assert!(
            restored
                .panes()
                .iter()
                .filter(|pane| pane.content.is_map())
                .all(|pane| pane.map.is_some()),
            "every map pane draws its own picture"
        );
        assert!(
            restored
                .panes()
                .iter()
                .filter(|pane| !pane.content.is_map())
                .all(|pane| pane.map.is_none()),
            "no pane that draws something else owns a map context"
        );
        restored.close_maps();
    }

    /// **The file describes panes by index, not by id**: the same file read twice builds the same wall twice,
    /// with the same tiling, whatever ids the wall mints.
    #[test]
    fn the_file_does_not_carry_ids() {
        let state = arranged();
        let saved = state.snapshot();
        let mut first = State::default();
        let mut second = State::default();
        apply_saved(&mut first, &saved);
        apply_saved(&mut second, &saved);
        assert_eq!(first.snapshot(), second.snapshot());
        // **A launch is a fixed point, or the wall loses a pane every time it opens.** The text is
        // compared rather than the values: TOML's float formatting is not the model's, and that is a
        // difference in the file, not in the wall.
        let once = toml::to_string_pretty(&saved).expect("the wall describes itself");
        let twice = toml::to_string_pretty(&first.snapshot()).expect("and again");
        let thrice = toml::to_string_pretty(&second.snapshot()).expect("and again");
        assert_eq!(once, twice);
        assert_eq!(twice, thrice);
    }

    /// **An unusable file is the default wall**, and a file that names no workspace is not a wall at all:
    /// `false` tells the caller to draw the default one.
    #[test]
    fn a_file_naming_no_workspace_is_refused() {
        let mut state = State::default();
        let before = state.workspaces.len();
        assert!(!apply_saved(&mut state, &Saved::default()));
        assert_eq!(
            state.workspaces.len(),
            before,
            "the wall it had is still there"
        );
    }

    /// **A workspace whose tiling the file got wrong is still a workspace**: its panes come back as a row,
    /// because the panes are real and the arrangement is a suggestion.
    #[test]
    fn a_workspace_with_no_tiling_comes_back_as_a_row() {
        let mut saved = Saved::default();
        saved.workspace.push(SavedWorkspace {
            name: String::new(),
            focused: None,
            tiling: None,
            pane: vec![SavedPane::default(), SavedPane::default()],
        });
        let mut state = State::default();
        assert!(apply_saved(&mut state, &saved));
        let layout = state.workspaces[0].layout.as_ref().expect("a layout");
        assert_eq!(layout.len(), 2);
        assert_eq!(layout.ids().len(), 2);
        assert_eq!(
            state.pane_count(),
            2,
            "both panes are in the wall on screen"
        );
    }

    /// A workspace nobody named keeps the wall's own numbering: a name is not invented by the file, and a
    /// default name is not frozen into it.
    #[test]
    fn the_default_names_are_derived_and_not_saved() {
        let state = State::default();
        let saved = state.snapshot();
        assert!(
            saved
                .workspace
                .iter()
                .all(|workspace| workspace.name.is_empty()),
            "the wall's default names are drawn, not written: {:?}",
            saved.workspace.iter().map(|w| &w.name).collect::<Vec<_>>()
        );
        let mut restored = State::default();
        apply_saved(&mut restored, &saved);
        assert_eq!(restored.workspaces[0].name, "Workspace 1");
        assert_eq!(restored.workspaces[2].name, "Workspace 3");
    }

    /// **A burst of changes is one write** (#153): the arrangement is written when it has held still for
    /// [`SAVE_AFTER`], not once per gesture — a camera drag is a hundred `arranged()` calls a second.
    #[test]
    fn a_burst_of_changes_is_one_write() {
        let mut state = State {
            logical: 10.0,
            ..State::default()
        };
        state.arranged();
        assert!(
            !state.save_due(),
            "a change just now is not a settled arrangement"
        );
        state.logical = 11.0;
        state.arranged();
        assert!(
            !state.save_due(),
            "and the second change moved the deadline"
        );
        state.logical = 11.0 + SAVE_AFTER;
        assert!(
            state.save_due(),
            "a beat after the last change, it is written"
        );
    }

    /// **The deadline is armed after the arrangement is written, never before** (#174): a truncated `layout.toml`
    /// is worse than a slow stop, so a slow disk must not be able to lose the file the deadline exists to protect.
    ///
    /// The closure runs where the wall arms the deadline, so what it can see is what the wall would see.
    #[test]
    fn the_deadline_is_armed_after_the_layout_is_written() {
        let directory = std::env::temp_dir().join(format!("arcons-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let path = directory.join("layout.toml");
        let mut state = State {
            logical: 10.0,
            ..State::default()
        };
        state.arranged();
        assert!(
            state.arranged_at.is_some(),
            "the state has something to write"
        );
        let armed = std::cell::Cell::new(0_u32);
        crate::ui::stop(&mut state, &path, &|| {
            armed.set(armed.get() + 1);
            let text = std::fs::read_to_string(&path).expect(
                "the layout file must be on disk before the deadline is armed — a slow disk would otherwise be a \
                 truncated file that the deadline then kills the process over (#174)",
            );
            assert!(
                text.contains("version"),
                "the file is written whole, not merely opened: got {text:?}"
            );
        });
        assert_eq!(armed.get(), 1, "the deadline is armed exactly once");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A capture is not a wall: nothing a scene does marks the arrangement dirty, so a golden can never
    /// depend on — or write — the machine's file.
    #[test]
    fn a_capture_never_saves() {
        let mut state = crate::scene::state_for("panes");
        // A scene's `setup` is what marks it headless; a state a test builds by hand says so itself, and
        // `with_saved_layout` checks `rune::is_headless` for the same reason in a real capture. Building a
        // scene runs the handlers that mark a *wall* dirty, so the mark a scene made while it was being
        // built is cleared here — the claim is about what it does once it is a capture.
        state.headless = true;
        state.arranged_at = None;
        state.arranged();
        assert!(
            state.arranged_at.is_none(),
            "a scene records no arrangement"
        );
        assert!(!state.save_due());
    }
}
