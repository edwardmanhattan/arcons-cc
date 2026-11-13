//! **The control plane** — one vocabulary for every input, and the one door state changes through (#170,
//! epic #162).
//!
//! `docs/adr/0005-control-plane.md` is this module's decision: a client sends **intents** and the wall that
//! owns the pixels renders them, over a unix socket whose permissions are the authentication. This module is
//! the vocabulary and the door; the socket and the CLI are #165, the cursor is #167, the menu's keys are #164.
//!
//! ## The rule
//!
//! **An intent is a settled change.** `FocusPane`, `Rename { target, name }`, `Camera { pane, camera }`: a
//! caller says what it wants and the wall decides. A continuous gesture is **not** an intent per event — a pan
//! computes its camera on the client side and hands one `Camera` over when the hand stops, because a wall that
//! applied 60 intents a second would be a wall whose state is its input rate.
//!
//! ## What is deliberately not here
//!
//! - **The pointer's held gestures** — a pane in the air (`Interaction::Lifted`), a border being dragged
//!   (`Interaction::Resizing`), a menu opened *at a position*, a filter panel anchored to its toggle. Those are
//!   surfaces the wall draws around a pointer, and a client that has no pointer gets them from the cursor
//!   vocabulary (#167), not from a second spelling of them here.
//! - **Data folding** — a fix from `live-feed`, a reply from the reads thread. Those are not a client's ask;
//!   they are the wall hearing from Minos, and `poll` applies them.
//!
//! ## Refusals
//!
//! A refusal is a **name**, never silence and never a default (`INV-07`'s rule, applied to verbs). The rule
//! behind the name is the one the UI already owns: [`apply`]`(ClosePane)` asks `State::can_close`, the same
//! predicate the menu row is built from (#141), so the control and the action cannot disagree.

use crate::map::geometry::Camera;
use crate::ui::{self, PaneContent, RenameTarget, State, ViewId};

// **The transport is a unix socket**, so the listener, the wire it speaks and the CLI that talks to it are
// behind `unix` (#165). The vocabulary above is not: `Intent` and `apply` are the wall's own state machine, and
// a Windows build (which the gate's `check` job compiles) still has them — it simply has no plane to drive them
// through until someone decides on a named pipe.
pub mod wire;

#[cfg(unix)]
pub mod ctl;
#[cfg(unix)]
pub mod server;

#[cfg(unix)]
pub use server::{Job, Server};

/// **The protocol's version**, on every message in both directions (`{"v": 1, …}`).
///
/// A client that sends another number is refused by name rather than guessed at: an intent vocabulary is not
/// something to negotiate, and a wall that silently accepted yesterday's verbs would be a wall two clients
/// disagree about.
pub const PROTOCOL: u32 = 1;

/// The socket's file name, inside the run directory. Named rather than spelled out at the call site so the ADR,
/// the CLI and the listener cannot drift apart.
pub const SOCKET_NAME: &str = "control.sock";

/// The socket's mode: owner read and write, nothing for anyone else.
///
/// **This is the credential** (`INV-08`). There is no token, no handshake and no secret to log, so the file's
/// permissions are what decide who may drive the wall — created at this mode, never chmod'ed after the fact
/// (a window between `create` and `chmod` is a window a local process can win).
#[cfg(unix)]
pub const SOCKET_MODE: u32 = 0o600;

/// The refusal a `ClosePane` earns on the wall's last pane (#141's predicate, named once).
pub const LAST_PANE: &str = "the last pane on the wall";

/// **One settled change**, asked for by a hand or a socket.
///
/// Every variant is a value a caller can produce without knowing anything about the wall's internals — which is
/// what makes the same vocabulary serve the pointer, the CLI and, later, a phone.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    // ---- panes
    /// Give this pane the keys (`State::focus_pane`).
    FocusPane(ViewId),
    /// Draw this pane over the wall, or put it back if it is already there (#85, #111).
    ToggleFullSize(ViewId),
    /// Remove this pane — refused on the wall's last one (#141).
    ClosePane(ViewId),
    /// Open the rename sheet on this pane or workspace (#83, #139, #154), without switching to it.
    BeginRename(RenameTarget),
    /// Set a name now, with no sheet: a socket's rename is a name, not nine keystrokes.
    /// `None` clears a pane's name (it falls back to its number); a workspace falls back to `Workspace N`.
    Rename {
        target: RenameTarget,
        name: Option<String>,
    },
    /// Split this pane, the new one showing the same water: the header's tree and newsroom controls (#116,
    /// #136) and a CLI's `pane split`.
    SplitPane { pane: ViewId, content: PaneContent },
    /// Split this pane **at a point in its own frame** — `Focus on new pane` (#77), which shows the water the
    /// operator clicked rather than the water the old pane was looking at.
    SplitAt { pane: ViewId, at: (f64, f64) },
    /// The same pane on a workspace of its own (#140).
    SplitToWorkspace(ViewId),
    /// Carry this pane into another workspace, threads and all (#140).
    MovePane { pane: ViewId, workspace: usize },
    /// Open a pane of this kind beside the focused one (the header's controls, a CLI's `pane open`).
    OpenPane(PaneContent),
    /// Put this pane's selection on one hull, or clear it with `None` (#99).
    Select { pane: ViewId, unit: Option<i64> },

    // ---- workspaces
    /// Make this workspace the wall on screen (#138).
    FocusWorkspace(usize),
    /// Append an empty workspace and switch to it (#138).
    NewWorkspace,

    // ---- map
    /// Put this pane's camera here — a **value**, so a pan is one intent when the hand stops, not sixty.
    Camera { pane: ViewId, camera: Camera },

    // ---- surfaces
    /// The settings sheet (#82).
    OpenSettings,
    /// One surface at a time, in the ladder `Escape` walks (#140): the waterfall, then the menu, then the
    /// interaction, then the maximised pane, then the newsroom's detail, then the selection.
    CloseSurfaces,
}

impl Intent {
    /// The verb this intent is on the wire, as `docs/control.md` names it (#165).
    ///
    /// One place, so a document, a CLI and a test cannot each spell it differently.
    pub fn verb(&self) -> &'static str {
        match self {
            Intent::FocusPane(_) => "pane.focus",
            Intent::ToggleFullSize(_) => "pane.full_size",
            Intent::ClosePane(_) => "pane.close",
            Intent::BeginRename(_) => "rename.begin",
            Intent::Rename { .. } => "rename.set",
            Intent::SplitPane { .. } => "pane.split",
            Intent::SplitAt { .. } => "pane.split_at",
            Intent::SplitToWorkspace(_) => "workspace.from_pane",
            Intent::MovePane { .. } => "pane.move",
            Intent::OpenPane(_) => "pane.open",
            Intent::Select { .. } => "pane.select",
            Intent::FocusWorkspace(_) => "workspace.focus",
            Intent::NewWorkspace => "workspace.new",
            Intent::Camera { .. } => "map.camera",
            Intent::OpenSettings => "surface.settings",
            Intent::CloseSurfaces => "surface.close",
        }
    }
}

/// Whether the wall changed on screen. A refused intent is not a change, and a handler that asked for one
/// should not dirty the layout for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Changed(pub bool);

/// Why an intent did not happen, in the words a client gets back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused(pub &'static str);

/// **The door.** Apply one intent, or say why not.
///
/// This is the only place an *input* changes the wall: the pointer's handlers, `on_key` and (from #165) the
/// socket all come here, so "the mouse can do it and the CLI cannot" stops being a category of bug. What it
/// calls is the same code the wall already trusted — the point is not a new implementation of closing a pane,
/// it is that there is one entrance to the old one.
pub fn apply(state: &mut State, intent: Intent) -> Result<Changed, Refused> {
    let changed = match intent {
        Intent::FocusPane(pane) => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            state.focus_pane(pane)
        }
        Intent::ToggleFullSize(pane) => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            ui::toggle_full_size(state, pane)
        }
        Intent::ClosePane(pane) => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            // **The same predicate the menu row is built from** (#141): the control and the action cannot
            // disagree, and a refusal is the rule's name rather than a no-op.
            if !state.can_close(pane) {
                return Err(Refused(LAST_PANE));
            }
            ui::close_from_menu(state, pane)
        }
        Intent::BeginRename(target) => {
            if !target_exists(state, target) {
                return Err(Refused(no_such(target)));
            }
            ui::begin_rename_target(state, target);
            true
        }
        Intent::Rename { target, name } => {
            if !target_exists(state, target) {
                return Err(Refused(no_such(target)));
            }
            ui::rename_to(state, target, name.as_deref())
        }
        Intent::SplitPane { pane, content } => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            ui::open_pane_beside(state, pane, content).is_some()
        }
        Intent::SplitAt { pane, at } => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            // No frame means no camera to unproject against, and a pane opened at a guessed point shows the
            // wrong water — the failure `INV-04` exists to prevent (#77).
            if state.pane(pane).and_then(|pane| pane.frame).is_none() {
                return Err(Refused("the pane has no frame yet"));
            }
            ui::split_at(state, pane, at).is_some()
        }
        Intent::SplitToWorkspace(pane) => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            ui::new_workspace_from_pane(state, pane).is_some()
        }
        Intent::MovePane { pane, workspace } => {
            let Some(source) = state.workspace_of(pane) else {
                return Err(Refused("no such pane"));
            };
            if workspace >= state.workspaces.len() {
                return Err(Refused("no such workspace"));
            }
            if workspace == source {
                return Err(Refused("the pane is already in that workspace"));
            }
            if !ui::move_pane_to(state, pane, workspace) {
                return Err(Refused("the tiling refused the pane"));
            }
            true
        }
        Intent::OpenPane(content) => ui::open_pane_focused(state, content).is_some(),
        Intent::Select { pane, unit } => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            match unit {
                Some(unit) => {
                    state.select_in(pane, unit);
                }
                None => state.clear_selection(pane),
            }
            true
        }
        Intent::FocusWorkspace(index) => {
            if index >= state.workspaces.len() {
                return Err(Refused("no such workspace"));
            }
            state.switch_to(index)
        }
        Intent::NewWorkspace => {
            state.add_workspace();
            true
        }
        Intent::Camera { pane, camera } => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            state.set_camera_of(pane, camera);
            true
        }
        Intent::OpenSettings => ui::open_settings(state),
        Intent::CloseSurfaces => ui::close_surfaces(state),
    };
    if changed {
        state.layout_dirty = true;
    }
    Ok(Changed(changed))
}

/// Whether the thing a rename names is on the wall.
fn target_exists(state: &State, target: RenameTarget) -> bool {
    match target {
        RenameTarget::Pane(pane) => state.pane(pane).is_some(),
        RenameTarget::Workspace(index) => index < state.workspaces.len(),
    }
}

fn no_such(target: RenameTarget) -> &'static str {
    match target {
        RenameTarget::Pane(_) => "no such pane",
        RenameTarget::Workspace(_) => "no such workspace",
    }
}

/// The socket's path: `$XDG_RUNTIME_DIR/arcons-cc/control.sock`, or the configuration directory when the run
/// directory is missing.
///
/// The run directory is the right home for it — it is per-user, it is the machine's own (not a network mount),
/// and it is cleared at logout, so a stale socket does not survive a reboot. A container or a systemd unit
/// without `XDG_RUNTIME_DIR` would otherwise have no wall to drive, so the fallback keeps the plane reachable
/// there; either way the directory is created `0700` before the socket is bound (#165).
#[cfg(unix)]
pub fn socket_path() -> std::path::PathBuf {
    socket_path_in(std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from))
}

/// The decision above, with the environment passed in: which directory the socket belongs in.
///
/// Split out so the branch is a **test** rather than a machine's luck — a test that reads the environment can
/// only assert what this machine happens to have, and a relative `XDG_RUNTIME_DIR` must not resolve against the
/// working directory (a wall started from `/` would bind `./arcons-cc/control.sock`).
#[cfg(unix)]
fn socket_path_in(run: Option<std::path::PathBuf>) -> std::path::PathBuf {
    match run.filter(|path| path.is_absolute()) {
        Some(dir) => dir.join("arcons-cc").join(SOCKET_NAME),
        None => crate::config::config_dir().join(SOCKET_NAME),
    }
}

/// **One sample of every variant**, so a table over the vocabulary is exhaustive rather than representative.
///
/// Public because the tables that must not drift are outside this module: `docs/control.md`'s verb list, the
/// CLI's words and the wire codec are all checked against it, and a test-gated copy would be invisible to an
/// integration test. Adding a variant without adding it here leaves the vocabulary with a hole — which is the
/// point: a table nobody enumerates is a table that stops describing the enum (#170, #165).
pub fn vocabulary() -> Vec<Intent> {
    vec![
        Intent::FocusPane(1),
        Intent::ToggleFullSize(1),
        Intent::ClosePane(1),
        Intent::BeginRename(RenameTarget::Pane(1)),
        Intent::Rename {
            target: RenameTarget::Workspace(0),
            name: Some("Straits watch".to_string()),
        },
        Intent::SplitPane {
            pane: 1,
            content: PaneContent::Map,
        },
        Intent::SplitAt {
            pane: 1,
            at: (0.0, 0.0),
        },
        Intent::SplitToWorkspace(1),
        Intent::MovePane {
            pane: 1,
            workspace: 2,
        },
        Intent::OpenPane(PaneContent::Newsroom),
        Intent::Select {
            pane: 1,
            unit: None,
        },
        Intent::FocusWorkspace(0),
        Intent::NewWorkspace,
        Intent::Camera {
            pane: 1,
            camera: Camera::new((-6.1, 106.8), 10.0),
        },
        Intent::OpenSettings,
        Intent::CloseSurfaces,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    // The socket's path is a unix fact, so are the tests that read it (#165).
    #[cfg(unix)]
    use std::path::{Path, PathBuf};

    /// The run directory, when it is one, is where the socket goes: per-user, per-machine, cleared at logout.
    #[cfg(unix)]
    #[test]
    fn the_socket_lives_in_the_run_directory() {
        let path = socket_path_in(Some(PathBuf::from("/run/user/1000")));
        assert_eq!(
            path,
            Path::new("/run/user/1000/arcons-cc/control.sock"),
            "an absolute XDG_RUNTIME_DIR wins: the socket is that user's, on this machine (#171)"
        );
    }

    /// A wall in a container or a unit without a run directory still has a control plane; it just lives beside
    /// the configuration.
    #[cfg(unix)]
    #[test]
    fn a_missing_run_directory_falls_back_to_the_config_directory() {
        let path = socket_path_in(None);
        assert_eq!(path, crate::config::config_dir().join(SOCKET_NAME));
        assert!(
            path.ends_with(SOCKET_NAME),
            "the fallback is still the control socket, got {}",
            path.display()
        );
    }

    /// A relative `XDG_RUNTIME_DIR` is not a directory the wall may create: it would resolve against whatever
    /// the working directory happened to be.
    #[cfg(unix)]
    #[test]
    fn a_relative_run_directory_is_ignored() {
        let path = socket_path_in(Some(PathBuf::from("run/user/1000")));
        assert_eq!(
            path,
            crate::config::config_dir().join(SOCKET_NAME),
            "a relative XDG_RUNTIME_DIR must not resolve against the working directory (#171)"
        );
    }

    /// The three values the ADR quotes are the three values a client depends on, and none of them is optional.
    #[test]
    fn the_wire_constants_are_what_a_client_is_told() {
        assert_eq!(PROTOCOL, 1, "the protocol's version is on every message");
        assert_eq!(SOCKET_NAME, "control.sock");
        // A mode that lets the group or the world write is a wall anyone on the machine can drive (`INV-08`).
        #[cfg(unix)]
        {
            assert_eq!(
                SOCKET_MODE, 0o600,
                "the socket is the credential: owner read and write, nothing else"
            );
            assert_eq!(
                SOCKET_MODE & 0o077,
                0,
                "no group or other bits on the control socket"
            );
        }
    }

    /// **The refusal the ADR quotes, from the predicate the menu row is built from** (#141, #170).
    ///
    /// This is the claim the whole control plane exists to make good on: a socket asking for what the wall's
    /// last pane cannot do gets the rule's name back, not a silent no-op and not a second copy of the rule.
    #[test]
    fn closing_the_last_pane_on_the_wall_is_refused_by_name() {
        let mut state = State::default();
        let only = state.focused();
        assert!(state.is_single_pane());
        let refused = apply(&mut state, Intent::ClosePane(only)).unwrap_err();
        assert_eq!(
            refused,
            Refused(LAST_PANE),
            "the rule is named once, in LAST_PANE"
        );
        assert_eq!(state.pane_count(), 1, "a refusal changes nothing");
    }

    /// And the same intent on a wall with a sibling does it — one vocabulary, two answers.
    #[test]
    fn closing_a_pane_with_a_sibling_closes_it() {
        let mut state = State::default();
        let first = state.focused();
        let camera = state.camera_of(first);
        state
            .split_pane_as(first, camera, PaneContent::Map)
            .expect("a second pane");
        assert_eq!(
            apply(&mut state, Intent::ClosePane(first)),
            Ok(Changed(true))
        );
        assert_eq!(state.pane_count(), 1);
    }

    /// **Opening a pane is one more pane**, on a wall that has one — the header's tree and newsroom controls
    /// and a CLI's `pane.open` are the same request.
    #[test]
    fn opening_a_pane_adds_one_beside_the_focused_pane() {
        let mut state = State::default();
        let before = state.pane_count();
        assert_eq!(
            apply(&mut state, Intent::OpenPane(PaneContent::Newsroom)),
            Ok(Changed(true))
        );
        assert_eq!(state.pane_count(), before + 1);
        assert!(
            state
                .pane(state.focused())
                .is_some_and(|pane| matches!(pane.content, PaneContent::Newsroom)),
            "the pane the operator asked for is the one that opens and takes the keys"
        );
    }

    /// **An empty workspace has nothing to split** (#141's state), so opening a pane fills it rather than
    /// silently doing nothing — which is what the header's controls used to do there, and what a CLI found.
    #[test]
    fn opening_a_pane_in_an_empty_workspace_fills_it() {
        let mut state = State::default();
        let only = state.focused();
        // Take the wall's only pane out of its workspace: the state #141 allows and #153 restores.
        assert_eq!(
            apply(
                &mut state,
                Intent::MovePane {
                    pane: only,
                    workspace: 1
                }
            ),
            Ok(Changed(true))
        );
        assert!(
            state.workspaces[0].layout.is_none(),
            "the source workspace is empty (#141), and stays on the wall"
        );
        assert_eq!(state.active, 1, "the move switched to where the pane went");
        // Back to the empty one, then open a pane there.
        assert_eq!(
            apply(&mut state, Intent::FocusWorkspace(0)),
            Ok(Changed(true))
        );
        assert!(state.layout().is_none());
        assert_eq!(
            apply(&mut state, Intent::OpenPane(PaneContent::Newsroom)),
            Ok(Changed(true)),
            "opening a pane in an empty workspace is a change, not a no-op"
        );
        assert_eq!(
            state.pane_count(),
            1,
            "the active workspace has its first pane"
        );
        assert_eq!(
            state
                .workspaces
                .iter()
                .map(|w| w.panes.len())
                .sum::<usize>(),
            2,
            "and the moved pane is still on the wall in the other workspace"
        );
        assert!(state.layout().is_some());
    }

    /// A pane that is not on the wall is refused with its own reason, never a panic and never a default.
    #[test]
    fn an_unknown_pane_is_refused() {
        let mut state = State::default();
        let ghost = 99;
        assert!(state.pane(ghost).is_none());
        assert_eq!(
            apply(&mut state, Intent::FocusPane(ghost)),
            Err(Refused("no such pane"))
        );
        assert_eq!(
            apply(&mut state, Intent::ClosePane(ghost)),
            Err(Refused("no such pane"))
        );
    }

    /// A workspace index past the end is a refusal, and the wall does not move to a workspace it does not have.
    #[test]
    fn a_workspace_that_is_not_there_is_refused() {
        let mut state = State::default();
        let before = state.active;
        let pane = state.focused();
        assert_eq!(
            apply(&mut state, Intent::FocusWorkspace(99)),
            Err(Refused("no such workspace"))
        );
        assert_eq!(state.active, before);
        assert_eq!(
            apply(
                &mut state,
                Intent::MovePane {
                    pane,
                    workspace: 99
                }
            ),
            Err(Refused("no such workspace"))
        );
    }

    /// Moving a pane to the workspace it is already in is refused rather than done and undefined.
    #[test]
    fn moving_a_pane_to_where_it_already_is_is_refused() {
        let mut state = State::default();
        let pane = state.focused();
        let here = state
            .workspace_of(pane)
            .expect("the focused pane is in a workspace");
        assert_eq!(
            apply(
                &mut state,
                Intent::MovePane {
                    pane,
                    workspace: here
                }
            ),
            Err(Refused("the pane is already in that workspace"))
        );
    }

    /// **A socket's rename is a name, not nine keystrokes**: it lands without a sheet, and it lands the same way
    /// the sheet's Enter does — including what an empty name means for a workspace (#139, #170).
    #[test]
    fn a_rename_lands_without_a_sheet() {
        let mut state = State::default();
        assert!(matches!(state.interaction, crate::ui::Interaction::Idle));
        apply(
            &mut state,
            Intent::Rename {
                target: RenameTarget::Workspace(0),
                name: Some("Straits watch".to_string()),
            },
        )
        .expect("a name is not a change the machine refuses");
        assert_eq!(state.workspaces[0].name, "Straits watch");
        assert!(
            matches!(state.interaction, crate::ui::Interaction::Idle),
            "a rename with no sheet leaves no surface behind"
        );

        // An empty name falls back to the default, because the header has to say something.
        apply(
            &mut state,
            Intent::Rename {
                target: RenameTarget::Workspace(0),
                name: None,
            },
        )
        .expect("clearing a workspace's name is allowed");
        assert_eq!(state.workspaces[0].name, "Workspace 1");
    }

    /// `SplitAt` needs a frame to unproject against: without one the new pane would show the wrong water, which
    /// is the failure `INV-04` exists to prevent (#77).
    #[test]
    fn a_split_at_a_point_needs_a_frame() {
        let mut state = State::default();
        let pane = state.focused();
        assert!(state.pane(pane).and_then(|pane| pane.frame).is_none());
        assert_eq!(
            apply(
                &mut state,
                Intent::SplitAt {
                    pane,
                    at: (10.0, 10.0)
                }
            ),
            Err(Refused("the pane has no frame yet"))
        );
        assert_eq!(state.pane_count(), 1, "the refusal left the wall alone");
    }

    /// A selection is a value: `Some` puts the pane's card on that hull, `None` clears it.
    #[test]
    fn selecting_a_hull_and_clearing_it() {
        let mut state = State::default();
        let pane = state.focused();
        apply(
            &mut state,
            Intent::Select {
                pane,
                unit: Some(7),
            },
        )
        .expect("a hull");
        assert_eq!(state.selected_in(pane), Some(7));
        apply(&mut state, Intent::Select { pane, unit: None }).expect("a clear");
        assert_eq!(state.selected_in(pane), None);
    }

    /// **Escape is an intent**: the ladder closes the submenu, then the menu, then the interaction — one surface
    /// at a time (#140), which is what a client with no keyboard gets from `surface.close`.
    #[test]
    fn close_surfaces_walks_one_thing_at_a_time() {
        let mut state = State::default();
        let pane = state.focused();
        state.begin(crate::ui::Begin::Menu {
            pane,
            at: (20.0, 30.0),
        });
        crate::ui::open_submenu(&mut state, crate::ui::MENU_MOVE);
        assert!(matches!(
            state.interaction,
            crate::ui::Interaction::Menu { open: Some(_), .. }
        ));
        assert_eq!(apply(&mut state, Intent::CloseSurfaces), Ok(Changed(true)));
        assert!(
            matches!(
                state.interaction,
                crate::ui::Interaction::Menu { open: None, .. }
            ),
            "the waterfall closes first"
        );
        assert_eq!(apply(&mut state, Intent::CloseSurfaces), Ok(Changed(true)));
        assert!(
            matches!(state.interaction, crate::ui::Interaction::Idle),
            "then the menu"
        );
        assert_eq!(
            apply(&mut state, Intent::CloseSurfaces),
            Ok(Changed(false)),
            "and with nothing open, dismissing changes nothing"
        );
    }

    /// Every verb is unique: two intents with one name is a document that cannot describe the wall.
    #[test]
    fn every_intent_has_its_own_verb() {
        let verbs: Vec<&'static str> = super::vocabulary().iter().map(Intent::verb).collect();
        let mut seen = std::collections::BTreeSet::new();
        for verb in &verbs {
            assert!(
                seen.insert(*verb),
                "{verb} names two intents — a client cannot say which one it wants (#170)"
            );
            assert!(
                verb.contains('.'),
                "{verb} is not a `noun.verb` name: the vocabulary is grouped by what it acts on (#170)"
            );
        }
        assert_eq!(
            seen.len(),
            verbs.len(),
            "the table test must build every variant once"
        );
    }
}
