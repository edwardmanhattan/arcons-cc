//! `layout.toml` — **the wall as the operator arranged it** (#153), as opposed to what a deployment
//! configured (`config.toml`) and what somebody clicked once (`settings.toml`).
//!
//! Three files, three owners, and one rule each. This one is **the operator's arrangement**: which
//! workspaces exist and what they are called, which panes they hold, where each pane's camera is looking,
//! whether it hides the bases, and what its title is. It is written by the display, read at startup, and
//! it is **not** a place for anything a wall was told in confidence:
//!
//! - **No secret, host, username or token** (`INV-08`). A layout is geometry and words somebody typed.
//!   There is no field here that could hold one, and `deny_unknown_fields` means a hand-written file
//!   cannot smuggle one in beside them.
//! - **Unreadable is not fatal**: a missing, corrupt or half-understood file is a log line and the wall's
//!   default arrangement. The wall's job is to draw the sea; it cannot be made to refuse that by a typo.
//! - **Nothing is invented**: a saved value the layout cannot draw (a zoom past the range, a latitude at
//!   the pole, a split at 0.999) is clamped or refused here, and the value in memory is always one the
//!   wall can draw.
//!
//! This module is the **file format and its arithmetic**: which keys exist, what a usable one means, and
//! the two conversions a wall needs (`Saved` ← `toml`, `Saved` → `toml`). It knows nothing about `State`:
//! `ui::saved` is what turns an arrangement into this and back, which is where the model's invariants
//! live.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The format's version. A file from a newer wall is read for what it understands and the rest is dropped
/// with a line — refusing to draw because a file has a key from tomorrow is the failure this whole module
/// is written to avoid.
pub const VERSION: u32 = 1;

/// The latitudes a camera may centre on: the projection is finite and a wall at the pole has nothing
/// below it to show.
pub const LAT_MAX: f64 = 85.0;

/// The precision a camera is written at: **six decimals**, roughly a tenth of a metre.
///
/// It exists because a file is compared and diffed: `Camera::START.center.1` is `106.90999999999997` in
/// `f64`, and a write that stores all of it makes every launch's file differ from the last one in digits
/// nobody can see. The model keeps its own precision; the file keeps six places.
pub const COORD_PLACES: f64 = 1e6;

/// A coordinate as the file writes it.
pub fn round_coord(value: f64) -> f64 {
    if value.is_finite() {
        (value * COORD_PLACES).round() / COORD_PLACES
    } else {
        value
    }
}

/// The shares a split may take. A ratio of exactly 0 or 1 is a pane with no pixels, which the layout
/// cannot draw and the operator cannot grab.
pub const RATIO_RANGE: (f32, f32) = (0.05, 0.95);

/// What the display remembers about the wall's arrangement.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Saved {
    /// [`VERSION`], as written. Read is not gated on it: see the module's note.
    pub version: u32,
    /// Which workspace is on screen, as an index into `workspace`.
    pub active: usize,
    /// The workspaces, in the order the chips draw. An empty list is the wall's default arrangement,
    /// because a file that names no workspace is a file that says nothing.
    pub workspace: Vec<SavedWorkspace>,
}

/// One workspace: its name, its panes, and how they tile.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SavedWorkspace {
    /// The operator's name, or empty for the default one (`Workspace N`) — a name nobody typed is not a
    /// name (#83, #113), so an unnamed workspace saves nothing here rather than inventing one.
    pub name: String,
    /// Which pane had the focus, as an index into `pane`. A workspace that names none focuses its first.
    pub focused: Option<u32>,
    /// The tiling, as a tree of leaves that name `pane` indices. `None`, or one that does not match the
    /// panes it has, is laid out as a row: an arrangement the wall cannot read is still a wall.
    pub tiling: Option<SavedNode>,
    /// The panes, in the order the tree's leaves refer to them.
    pub pane: Vec<SavedPane>,
}

/// One node of a workspace's tiling: a leaf naming a pane, or a split of two nodes.
///
/// An untagged struct rather than an enum, because this is TOML: a table with a `pane` key is a leaf, a
/// table with `split`, `a` and `b` is a split, and the reader says which shape it found. An externally
/// tagged enum would put `[workspace.tiling.Pane]` in an operator's file, which is a file nobody wants to
/// read, let alone hand-edit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SavedNode {
    /// The leaf: an index into the workspace's `pane` list.
    pub pane: Option<u32>,
    /// The split's axis: `side` for side by side, `stack` for one above the other.
    pub split: Option<Axis>,
    /// The share the `a` side takes, clamped to [`RATIO_RANGE`].
    pub ratio: Option<f32>,
    pub a: Option<Box<SavedNode>>,
    pub b: Option<Box<SavedNode>>,
}

/// How two nodes sit beside each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// Side by side: `a` on the left, `b` on the right.
    Side,
    /// Stacked: `a` on top, `b` below.
    Stack,
}

/// What a pane is, for the file. The model's own vocabulary, spelled the way an operator would read it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Content {
    #[default]
    Map,
    Tree,
    Newsroom,
}

/// One pane: what it is, where it looks, and what it hides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SavedPane {
    pub content: Content,
    /// The structure a tree pane is about; absent is **the chain of command as a whole** (#116).
    pub organisation: Option<i64>,
    /// The camera. A map pane's own; a tree or newsroom pane keeps one too, because a pane that changes
    /// content and changes back should come back where it was.
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
    /// The operator's name for this pane, or absent for "nobody named this pane" (#83).
    pub name: Option<String>,
    /// The tree pane's own camera (#127) — `None` on every other kind of pane.
    pub tree: Option<SavedTree>,
    /// The newsroom's tab and page (#136) — `None` on every other kind of pane.
    pub news: Option<SavedNews>,
    /// What this pane hides (#112). Absent means it hides nothing, which is the wall's default.
    pub filter: Option<SavedFilter>,
}

impl Default for SavedPane {
    /// **A default pane is a drawable one**: the wall's start camera, because a `zoom = 0` in a file is
    /// not "the default zoom", it is a value the projection cannot use.
    fn default() -> SavedPane {
        SavedPane {
            content: Content::Map,
            organisation: None,
            lat: crate::map::geometry::Camera::START.center.0,
            lon: crate::map::geometry::Camera::START.center.1,
            zoom: crate::map::geometry::Camera::START.zoom,
            name: None,
            tree: None,
            news: None,
            filter: None,
        }
    }
}

/// A tree pane's camera: where the tree's own points sit, and whether a hand has taken it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SavedTree {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    /// **A fit is not a position.** Until a hand takes the camera, a resize re-fits (#127), and that is the
    /// state a pane opens in — so a pane the operator only ever fitted saves as untouched.
    pub touched: bool,
}

impl Default for SavedTree {
    fn default() -> SavedTree {
        SavedTree {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
            touched: false,
        }
    }
}

/// A newsroom pane's place in the feed: the tab it was reading and the page it was on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SavedNews {
    /// The scope's wire word (`international`, `regional`, `national`) — the server's vocabulary, not a
    /// translated label, so a file stays readable when the wall's words change.
    pub scope: String,
    /// 1-based. **The page the operator was on**, not the newest one: a wall that comes back on page 1 has
    /// forgotten where the reading was.
    pub page: u32,
}

/// What a pane hides (#112): three layers, and the backend's two vocabularies as ids.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SavedFilter {
    pub hulls: bool,
    pub installations: bool,
    pub objects: bool,
    pub hidden_types: Vec<i64>,
    pub hidden_domains: Vec<i64>,
    pub hidden_categories: Vec<i64>,
}

impl Default for SavedFilter {
    fn default() -> SavedFilter {
        SavedFilter {
            hulls: true,
            installations: true,
            objects: true,
            hidden_types: Vec::new(),
            hidden_domains: Vec::new(),
            hidden_categories: Vec::new(),
        }
    }
}

impl Saved {
    /// Read the file, and say what happened in one line for the log.
    ///
    /// Never fails. `None` means "no arrangement to restore", which the wall draws as its default — an
    /// absent file is that with no complaint, and an unusable one is that with a sentence naming the file
    /// and the reason. The settings file's rule, because it is the same kind of file.
    pub fn read(path: &Path) -> (Option<Saved>, String) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (
                    None,
                    format!("layout: no {}, opening the default wall", path.display()),
                );
            }
            Err(error) => {
                return (
                    None,
                    format!(
                        "layout: {} is unreadable ({error}), opening the default wall",
                        path.display()
                    ),
                );
            }
        };
        match Saved::parse(&text) {
            Ok(saved) => (Some(saved), format!("layout: read {}", path.display())),
            Err(error) => (
                None,
                format!(
                    "layout: {} is not usable ({error}), opening the default wall",
                    path.display()
                ),
            ),
        }
    }

    /// Parse and make drawable: the camera ranges, the split shares, and a shape a wall can build.
    ///
    /// A **file** problem is an error here and the default wall at the caller; a **value** problem is
    /// clamped, because an operator who hand-edited a zoom to 40 wanted "as close as it goes", not an
    /// error. That is the settings file's distinction (`pane_gap = 400` is clamped) applied to a bigger
    /// file.
    pub fn parse(text: &str) -> Result<Saved, String> {
        let mut saved: Saved = toml::from_str(text).map_err(|error| error.to_string())?;
        saved.make_drawable();
        Ok(saved)
    }

    /// Clamp every value to what the wall can draw, in place.
    pub fn make_drawable(&mut self) {
        self.version = VERSION;
        if self.active >= self.workspace.len() {
            self.active = 0;
        }
        for workspace in &mut self.workspace {
            workspace.name = workspace.name.trim().to_string();
            for pane in &mut workspace.pane {
                pane.lat = if pane.lat.is_finite() {
                    pane.lat.clamp(-LAT_MAX, LAT_MAX)
                } else {
                    crate::map::geometry::Camera::START.center.0
                };
                pane.lon = if pane.lon.is_finite() {
                    pane.lon.clamp(-180.0, 180.0)
                } else {
                    crate::map::geometry::Camera::START.center.1
                };
                pane.lat = round_coord(pane.lat);
                pane.lon = round_coord(pane.lon);
                pane.zoom = if pane.zoom.is_finite() {
                    pane.zoom.clamp(
                        crate::map::geometry::MIN_ZOOM,
                        crate::map::geometry::MAX_ZOOM,
                    )
                } else {
                    crate::map::geometry::Camera::START.zoom
                };
                if let Some(tree) = &mut pane.tree
                    && (!tree.zoom.is_finite() || tree.zoom <= 0.0)
                {
                    *tree = SavedTree::default();
                }
                if let Some(news) = &mut pane.news
                    && news.page == 0
                {
                    news.page = 1;
                }
                // An empty name is not a name: it is "nobody named this pane".
                if pane
                    .name
                    .as_deref()
                    .is_some_and(|name| name.trim().is_empty())
                {
                    pane.name = None;
                }
            }
            workspace.clamp_tiling();
        }
    }

    /// Write the file atomically, and privately — the settings file's way, for the settings file's reason.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        super::write_atomically(path, &text)
    }
}

impl SavedWorkspace {
    /// The splits into the drawable range, and a tiling that refers to panes this workspace has.
    ///
    /// A tiling that does not is **dropped**, not repaired: the panes are real and the arrangement is a
    /// suggestion, so a wall with four panes and a three-leaf tree comes back as a row of four rather than
    /// losing a pane to make a file read.
    pub fn clamp_tiling(&mut self) {
        let panes = self.pane.len();
        if panes == 0 {
            self.tiling = None;
            self.focused = None;
            return;
        }
        let mut seen = Vec::new();
        if let Some(tiling) = &mut self.tiling {
            let mut ok = clamp_node(tiling, panes, &mut seen);
            // Every pane exactly once: a leaf that names a pane twice, or not at all, is not a tiling of
            // this workspace.
            seen.sort_unstable();
            ok &= seen == (0..panes as u32).collect::<Vec<_>>();
            if !ok {
                self.tiling = None;
            }
        }
        if self.focused.is_none_or(|index| index as usize >= panes) {
            // The first leaf of the tiling, or the first pane: the wall focuses something.
            self.focused = Some(first_leaf(self.tiling.as_ref()).unwrap_or(0));
        }
    }
}

/// Clamp a node's ratio and collect the leaves it names; `false` when it is not a leaf or a split of two.
fn clamp_node(node: &mut SavedNode, panes: usize, seen: &mut Vec<u32>) -> bool {
    match (node.pane, node.split, node.a.as_mut(), node.b.as_mut()) {
        (Some(index), None, None, None) => {
            if index as usize >= panes {
                return false;
            }
            seen.push(index);
            true
        }
        (None, Some(_), Some(a), Some(b)) => {
            node.ratio = Some(
                node.ratio
                    .unwrap_or(0.5)
                    .clamp(RATIO_RANGE.0, RATIO_RANGE.1),
            );
            clamp_node(a, panes, seen) & clamp_node(b, panes, seen)
        }
        _ => false,
    }
}

/// The first pane a tiling names, reading left to right, as `Layout::focused()` would.
fn first_leaf(node: Option<&SavedNode>) -> Option<u32> {
    let node = node?;
    match (&node.pane, &node.a, &node.b) {
        (Some(index), _, _) => Some(*index),
        (None, Some(a), _) => first_leaf(Some(a)),
        (None, None, Some(b)) => first_leaf(Some(b)),
        _ => None,
    }
}

/// `$XDG_CONFIG_HOME/arcons-cc/layout.toml`, beside the settings and the credential.
pub fn layout_file() -> PathBuf {
    super::config_dir().join("layout.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("arcons-layout-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn wall() -> Saved {
        Saved {
            version: VERSION,
            active: 1,
            workspace: vec![
                SavedWorkspace {
                    name: "Straits watch".to_string(),
                    focused: Some(1),
                    tiling: Some(SavedNode {
                        pane: None,
                        split: Some(Axis::Side),
                        ratio: Some(0.4),
                        a: Some(Box::new(SavedNode {
                            pane: Some(0),
                            ..SavedNode::default()
                        })),
                        b: Some(Box::new(SavedNode {
                            pane: Some(1),
                            ..SavedNode::default()
                        })),
                    }),
                    pane: vec![
                        SavedPane {
                            content: Content::Map,
                            lat: -1.25,
                            lon: 104.5,
                            zoom: 7.5,
                            name: Some("pane-01".to_string()),
                            ..SavedPane::default()
                        },
                        SavedPane {
                            content: Content::Newsroom,
                            news: Some(SavedNews {
                                scope: "national".to_string(),
                                page: 2,
                            }),
                            filter: Some(SavedFilter {
                                objects: false,
                                hidden_categories: vec![7],
                                ..SavedFilter::default()
                            }),
                            ..SavedPane::default()
                        },
                    ],
                },
                SavedWorkspace::default(),
            ],
        }
    }

    /// **What the wall writes is what it reads** (#153), and nothing else: the file round-trips through
    /// TOML with its names, its tiling, its cameras and its filters intact.
    #[test]
    fn what_the_wall_writes_is_what_it_reads() {
        let dir = temp_dir("round-trip");
        let path = dir.join("layout.toml");
        let written = wall();
        written.write(&path).expect("the file writes");
        let (read, note) = Saved::read(&path);
        assert_eq!(read.expect("a saved wall"), written, "{note}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("the file exists")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "a layout is private, like the settings and the credential"
            );
        }
        assert!(!dir.join("layout.toml.tmp").exists(), "the write renamed");
    }

    /// A file nobody can use is the default wall and a sentence — never a wall that refuses to draw.
    #[test]
    fn an_unusable_file_is_the_default_wall_and_a_sentence() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).expect("the temp dir");
        let path = dir.join("layout.toml");
        for text in [
            "active = \"one\"".to_string(),
            "[[workspace]]\npane = [".to_string(),
            // A key that does not exist is refused rather than ignored: a wall is not a dumping ground.
            "host = \"minos.example\"".to_string(),
            "\u{0}\u{1}\u{2}".to_string(),
        ] {
            std::fs::write(&path, &text).expect("the file writes");
            let (saved, note) = Saved::read(&path);
            assert!(saved.is_none(), "{text:?} parsed");
            assert!(note.contains("not usable"), "{text:?}: {note}");
        }
        let (saved, note) = Saved::read(&dir.join("absent.toml"));
        assert!(saved.is_none());
        assert!(note.contains("no "), "{note}");
    }

    /// **Nothing is invented, and nothing the wall cannot draw is honoured**: a zoom past the range, a
    /// latitude at the pole, a split at 0.999 and a page of zero all come back drawable.
    #[test]
    fn what_the_wall_cannot_draw_is_clamped() {
        let saved = Saved::parse(
            r#"
active = 9

[[workspace]]
name = "  Watch  "

[workspace.tiling]
split = "side"
ratio = 0.999

[workspace.tiling.a]
pane = 0

[workspace.tiling.b]
pane = 1

[[workspace.pane]]
lat = 89.9
lon = 400.0
zoom = 40.0

[[workspace.pane]]
name = "   "
news = { scope = "international", page = 0 }
"#,
        )
        .expect("a drawable wall");
        assert_eq!(saved.active, 0, "an active index past the end is the first");
        let workspace = &saved.workspace[0];
        assert_eq!(
            workspace.name, "Watch",
            "a name is trimmed, and empty is no name"
        );
        assert_eq!(
            workspace.tiling.as_ref().and_then(|node| node.ratio),
            Some(RATIO_RANGE.1)
        );
        assert_eq!(workspace.pane[0].lat, LAT_MAX);
        assert_eq!(workspace.pane[0].lon, 180.0);
        assert_eq!(
            workspace.pane[0].zoom,
            crate::map::geometry::MAX_ZOOM,
            "a zoom past the range is the range's end, not an error"
        );
        assert_eq!(workspace.pane[1].name, None, "whitespace is not a name");
        assert_eq!(
            workspace.pane[1].news.as_ref().map(|news| news.page),
            Some(1)
        );
    }

    /// **A tiling that does not describe the panes it has is dropped, not repaired**: the panes are real
    /// and the arrangement is a suggestion.
    #[test]
    fn a_tiling_that_names_the_wrong_panes_is_dropped() {
        // Two panes, one leaf: a tree that would lose a pane.
        let mut saved = Saved::parse(
            r#"
[[workspace]]

[workspace.tiling]
pane = 0

[[workspace.pane]]
[[workspace.pane]]
"#,
        )
        .expect("a drawable wall");
        assert!(
            saved.workspace[0].tiling.is_none(),
            "a tiling of one leaf for two panes is not this workspace's"
        );
        assert_eq!(saved.workspace[0].focused, Some(0));

        // A leaf that names the same pane twice is not a tiling either.
        let twice = Saved::parse(
            r#"
[[workspace]]

[workspace.tiling]
split = "stack"

[workspace.tiling.a]
pane = 0

[workspace.tiling.b]
pane = 0

[[workspace.pane]]
[[workspace.pane]]
"#,
        )
        .expect("a drawable wall");
        assert!(twice.workspace[0].tiling.is_none());
        // And a workspace with no panes keeps no tiling and no focus.
        saved.workspace[0].pane.clear();
        saved.workspace[0].clamp_tiling();
        assert!(saved.workspace[0].tiling.is_none());
        assert!(saved.workspace[0].focused.is_none());
    }

    /// **A file cannot smuggle in a field that is not a layout** (`INV-08`): `deny_unknown_fields` is the
    /// rule, so a key beside the ones this module names is a file the wall refuses rather than ignores.
    #[test]
    fn a_key_that_is_not_a_layout_is_refused() {
        for text in [
            "token = \"abc\"",
            "[[workspace]]\nhost = \"minos.example\"",
            "[[workspace]]\n[[workspace.pane]]\nusername = \"ops\"",
        ] {
            assert!(
                Saved::parse(text).is_err(),
                "{text:?} was accepted into a layout"
            );
        }
    }
}
