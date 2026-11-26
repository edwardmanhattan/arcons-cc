//! **The control plane** — one vocabulary for every input, and the one door state changes through (#170,
//! epic #162).
//!
//! `docs/adr/0005-control-plane.md` is this module's decision: a client sends **intents** and the wall that
//! owns the pixels renders them, over a unix socket whose permissions are the authentication. This module is
//! the vocabulary and the carriers; the door itself is `ui::State::apply`. The socket and the CLI are #165, the cursor is #167, the menu's keys are #164.
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
//! behind the name is the one the UI already owns: [`ui::State::apply`]`(ClosePane)` asks `State::can_close`, the same
//! predicate the menu row is built from (#141), so the control and the action cannot disagree.

#[cfg(test)]
use crate::ui::State;
use crate::verb::{Edge, Intent};
#[cfg(test)]
use crate::verb::{PaneContent, RenameTarget, ViewId};

// **The transport is a unix socket**, so the listener, the wire it speaks and the CLI that talks to it are
// behind `unix` (#165). The vocabulary above is not: `Intent` and `apply` are the wall's own state machine, and
// a Windows build (which the gate's `check` job compiles) still has them — it simply has no plane to drive them
// through until someone decides on a named pipe.
// A viewer's half needs a carrier, and Windows has none (#202): the vocabulary stays compiled so a role can be parsed
// and reported by `--self-check`, and the parts no Windows build can reach are allowed to be unused there rather than
// cut away by `cfg` into a second vocabulary that could drift.
#[cfg_attr(not(unix), allow(dead_code))]
pub mod authority;
pub mod carrier;
// **A client's rules** (#197) are the phone's half of the plane, over a carrier (#195). Like the corpus, it is
// driven through the session loop, which is a unix socket's reader: a Windows build has the vocabulary and no plane.
#[cfg(unix)]
pub mod client;
// **The exchange corpus** (#224) is recorded through the session loop, which is a unix socket's reader
// (`server`). A Windows build has the vocabulary and no plane, so it has no corpus either.
pub mod projection;
// **The relay** (#196) is the wall's one outbound carrier. Its frames compile everywhere — `tests/doc_claims.rs`
// compares them with `docs/control.md` on every platform — and the socket it dials is a unix one, so a Windows build
// has the vocabulary and no carrier, exactly as it does for a viewer's session.
#[cfg_attr(not(unix), allow(dead_code))]
pub mod relay;
// **The wall's queue, and the rules between two carriers** (#222). Unix only, with the listener and the carrier the
// queue is fed by: a Windows build has the vocabulary and no plane.
#[cfg(unix)]
pub mod sessions;
pub mod snapshot;
// **A watch** (#221) is the one thing on this plane that is not request/answer. It compiles everywhere: the wire's
// three frames are the vocabulary, and the carrier that can actually carry a stream is a unix one.
pub mod watch;
pub mod wire;

#[cfg(unix)]
pub mod ctl;
#[cfg(unix)]
pub mod host;
#[cfg(unix)]
pub mod server;

#[cfg(unix)]
pub use host::Host;
#[cfg(unix)]
pub use server::Server;
#[cfg(unix)]
pub use sessions::Job;

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

/// **The refusal `workspace.close` earns when the workspace still holds panes** (#242): an arrangement is not
/// reconstructible from the wire — a name can be typed again, a pane's camera, tiling and shares cannot — so one
/// press may not destroy it. Emptying is `pane.close_all` (#241), a separate settled change the operator can see.
pub const WORKSPACE_HAS_PANES: &str = "the workspace holds panes";

/// **The last workspace on the wall is not closable** (#242): the same rule `pane.close` keeps one level down.
/// A wall with no workspace is not a wall, so the request is refused whole rather than leaving a hole.
pub const LAST_WORKSPACE: &str = "the last workspace on the wall";

/// The refusal a drop earns when both names are the same pane (#219): a swap with itself is a rule, not a
/// tiling that failed — the pointer cannot ask for it (`Layout::drop_for` answers `None` over the pane it is
/// dragging), and a client that says it meant it gets the rule's name.
pub const SELF_DROP: &str = "the pane cannot be swapped with itself";

/// The refusal a resize earns when no border separates the two panes (#219): a border names both of its sides,
/// so a pair with no border between them has no ratio to set.
pub const NOT_NEIGHBOURS: &str = "those panes are not neighbours";

/// The refusal an unknown edge earns (#219): the five words, spelled out, because a client's mistake is a word
/// and telling it "bad edge" would leave it guessing which.
pub const EDGE_WORDS: &str = "\"edge\" is left, right, top, bottom or centre";
/// The refusal a ratio outside [`crate::config::layout::RATIO_RANGE`] earns (#219): **the range, in the words**.
/// `config::layout::RATIO_RANGE` is the number and a test keeps the two from drifting apart — a refusal that said
/// "out of range" would leave a client guessing an end it could have been told.
pub const RATIO_WORDS: &str = "\"ratio\" is a share from 0.05 to 0.95";

/// Whether the wall changed on screen. A refused intent is not a change, and a handler that asked for one
/// should not dirty the layout for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Changed(pub bool);

/// Why an intent did not happen, in the words a client gets back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused(pub &'static str);

/// **A device that has just paired** (#194): the name it chose and the key it keeps.
///
/// This value exists for one trip: `pair.claim` hands it back to the carrier, which writes the key into the
/// client's answer, once. The wall keeps only the hash — so this is the **only** copy of the key, and its `Debug`
/// redacts it, which is what makes a log line that formatted a whole answer safe (`INV-08`).
#[derive(Clone, Debug, PartialEq)]
pub struct Paired {
    pub name: String,
    pub key: crate::config::devices::Key,
}

/// **What one intent concluded.** Almost every verb changes the wall or refuses; `pair.claim` is the one verb
/// that also answers with something the client has to keep.
///
/// It is an outcome rather than two doors (`INV-07`'s rule at this layer): [`apply`] stays the only place an
/// input changes the wall, and a caller that only wants "did anything move?" asks [`Applied::changed`].
#[derive(Clone, Debug, PartialEq)]
pub enum Applied {
    Changed(Changed),
    Paired(Paired),
}

impl Applied {
    /// Whether a pixel moved. A pairing changed what the wall remembers, so it is a change too.
    pub fn changed(&self) -> bool {
        match self {
            Applied::Changed(changed) => changed.0,
            Applied::Paired(_) => true,
        }
    }
}

/// **What a client can do about a refusal** (#198): the category of a "no", beside its words.
///
/// A refusal is a named rule, and the words are for the person at the screen (`INV-07`). A *client* is not a person at
/// a terminal: it has to choose between retrying, fixing the call it sent, and telling its user the app is out of date.
/// Reading prose to decide that is a client coupled to a sentence, so every refusal carries its category too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalKind {
    /// **The wall will not do that.** Understood and refused; sending it again changes nothing until something else on
    /// the wall does — `the last pane on the wall`, `no such pane`.
    Rule,
    /// **This client speaks a version the wall does not.** Send nothing more: update the app.
    Version,
    /// **The request was malformed** — a field of the wrong type, a verb that does not exist. Fix the call, retry.
    Input,
    /// **The wall is not answering now** — mid-frame, or going away. Retry, and do not read it as a refusal of the
    /// change itself.
    Busy,
}

impl RefusalKind {
    /// The word on the wire and in `docs/control.md`. A client matches on these four and never on a sentence.
    pub fn word(self) -> &'static str {
        match self {
            RefusalKind::Rule => "rule",
            RefusalKind::Version => "version",
            RefusalKind::Input => "input",
            RefusalKind::Busy => "busy",
        }
    }

    /// The category a word names, or `None` for a word this wall does not use (`INV-07`: no default).
    pub fn of_word(word: &str) -> Option<RefusalKind> {
        [
            RefusalKind::Rule,
            RefusalKind::Version,
            RefusalKind::Input,
            RefusalKind::Busy,
        ]
        .into_iter()
        .find(|kind| kind.word() == word)
    }
}

/// **Every refusal this wall can make, and what kind it is** (#198).
///
/// One table, in one place, beside the messages. A refusal that is not here is a refusal with no category — and
/// `tests/scaffold.rs` walks the source for every `Refused("…")` literal and fails when the two disagree, in both
/// directions. That is what makes "a new refusal is a visible branch" true without a `match` that could grow a `_`.
pub const REFUSALS: &[(&str, RefusalKind)] = &[
    ("\"at\" is [x, y] in the pane's frame", RefusalKind::Input),
    (
        "\"at\" is [x, y] in the picture panel's points",
        RefusalKind::Input,
    ),
    ("\"camera\" is {lat, lon, zoom}", RefusalKind::Input),
    ("\"code\" is the six digits on the wall", RefusalKind::Input),
    (
        "\"content\" is {kind: \"map\" | \"newsroom\" | \"tree\"}",
        RefusalKind::Input,
    ),
    ("\"content\" needs a \"kind\"", RefusalKind::Input),
    (
        "\"edge\" is left, right, top, bottom or centre",
        RefusalKind::Input,
    ),
    ("\"name\" is a device name", RefusalKind::Input),
    ("\"name\" is a string or null", RefusalKind::Input),
    ("\"onto\" is a pane id", RefusalKind::Input),
    ("\"open\" is true or false", RefusalKind::Input),
    (
        "\"organisation\" is a structure id or null",
        RefusalKind::Input,
    ),
    ("\"origin\" is a client name", RefusalKind::Input),
    ("\"other\" is a pane id", RefusalKind::Input),
    ("\"pane\" is a pane id", RefusalKind::Input),
    ("\"ratio\" is a share from 0.05 to 0.95", RefusalKind::Input),
    ("\"row\" is a row index or null", RefusalKind::Input),
    (
        "\"target\" is {\"pane\": n} or {\"workspace\": n}",
        RefusalKind::Input,
    ),
    ("\"unit\" is a hull id or null", RefusalKind::Input),
    ("\"watch\" is \"wall\"", RefusalKind::Input),
    ("\"zoom\" is a number or null", RefusalKind::Input),
    (
        "a request carries \"intent\", \"read\" or \"watch\", not two",
        RefusalKind::Input,
    ),
    (
        "a request carries \"intent\", \"read\" or \"watch\"",
        RefusalKind::Input,
    ),
    ("a request carries \"v\"", RefusalKind::Input),
    ("a request is a JSON object", RefusalKind::Input),
    ("an id is a non-negative integer", RefusalKind::Input),
    ("an intent has no verb", RefusalKind::Input),
    ("an intent is a JSON object", RefusalKind::Input),
    ("no menu is open", RefusalKind::Rule),
    ("no pairing is open", RefusalKind::Rule),
    ("no pane has the keys", RefusalKind::Rule),
    ("no relay is configured", RefusalKind::Rule),
    ("no row has the ring", RefusalKind::Rule),
    ("no submenu is open", RefusalKind::Rule),
    ("no such pane", RefusalKind::Rule),
    ("no such workspace", RefusalKind::Rule),
    ("not JSON", RefusalKind::Input),
    ("that code is not the code", RefusalKind::Rule),
    ("that device is already paired", RefusalKind::Rule),
    ("that device is not paired", RefusalKind::Rule),
    ("the cursor has not been placed", RefusalKind::Rule),
    ("the cursor is not pressed", RefusalKind::Rule),
    ("the hull has not reported", RefusalKind::Rule),
    ("the last pane on the wall", RefusalKind::Rule),
    ("the last workspace on the wall", RefusalKind::Rule),
    ("the workspace holds panes", RefusalKind::Rule),
    ("the pane cannot be swapped with itself", RefusalKind::Rule),
    ("the pane draws no map", RefusalKind::Rule),
    ("the pane has no frame yet", RefusalKind::Rule),
    ("the pane is already in that workspace", RefusalKind::Rule),
    ("the relay is not answering", RefusalKind::Busy),
    ("the relay refused the wall", RefusalKind::Rule),
    ("the ring is on no row", RefusalKind::Rule),
    ("the row opens nothing", RefusalKind::Rule),
    ("the tiling refused the pane", RefusalKind::Rule),
    ("the wall did not answer", RefusalKind::Busy),
    ("the wall has no source of chance", RefusalKind::Busy),
    ("the wall has stopped", RefusalKind::Busy),
    // **A carrier's capability, not a verb of the plane** (#221): a socket answers one request with one answer, and
    // a client that asked for a stream on it is told which wall it is talking to.
    ("this carrier does not carry a watch", RefusalKind::Rule),
    ("those panes are not neighbours", RefusalKind::Rule),
    ("unknown pane kind", RefusalKind::Input),
    ("unknown read", RefusalKind::Input),
    ("unknown verb", RefusalKind::Input),
    ("unsupported protocol version", RefusalKind::Version),
];

/// The category of a refusal, by its words — `None` for a message this wall never sends.
pub fn refusal_kind(why: &str) -> Option<RefusalKind> {
    REFUSALS
        .iter()
        .find(|(message, _)| *message == why)
        .map(|(_, kind)| *kind)
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
        let refused = state.apply(Intent::ClosePane(only)).unwrap_err();
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
            state.apply(Intent::ClosePane(first)),
            Ok(Applied::Changed(Changed(true)))
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
            state.apply(Intent::OpenPane(PaneContent::Newsroom)),
            Ok(Applied::Changed(Changed(true)))
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
            state.apply(Intent::MovePane {
                pane: only,
                workspace: 1
            }),
            Ok(Applied::Changed(Changed(true)))
        );
        assert!(
            state.workspaces[0].layout.is_none(),
            "the source workspace is empty (#141), and stays on the wall"
        );
        assert_eq!(state.active, 1, "the move switched to where the pane went");
        // Back to the empty one, then open a pane there.
        assert_eq!(
            state.apply(Intent::FocusWorkspace(0)),
            Ok(Applied::Changed(Changed(true)))
        );
        assert!(state.layout().is_none());
        assert_eq!(
            state.apply(Intent::OpenPane(PaneContent::Newsroom)),
            Ok(Applied::Changed(Changed(true))),
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

    /// **Close every pane but the focused one, in this workspace only** (#166): the panes go one at a time through
    /// `close_pane`, so the refusal, the map thread's join and the cadence are that function's.
    #[test]
    fn closing_the_other_panes_keeps_the_focused_one_and_this_workspace() {
        let mut state = State::default();
        let first = state.focused();
        let camera = state.camera_of(first);
        state
            .split_pane_as(first, camera, PaneContent::Map)
            .expect("a second pane");
        state
            .split_pane_as(first, camera, PaneContent::Map)
            .expect("a third");
        let keep = state.focused();
        assert_eq!(state.pane_count(), 3);
        assert_eq!(
            state.apply(Intent::CloseOtherPanes),
            Ok(Applied::Changed(Changed(true)))
        );
        assert_eq!(state.pane_count(), 1, "one pane is left");
        assert_eq!(state.focused(), keep, "and it is the one with the keys");
        assert_eq!(
            state.apply(Intent::CloseOtherPanes),
            Ok(Applied::Changed(Changed(false))),
            "closing nothing is not a change, and not an error"
        );
        // A wall with panes in another workspace keeps them: the intent is this workspace's.
        let other = state.workspaces.len();
        assert_eq!(other, 3, "the default wall's workspaces are untouched");
        assert!(state.workspaces[1].panes.is_empty());
    }

    /// **An empty workspace has no focused pane to keep**, so the intent is refused by name rather than succeeding
    /// at closing nothing (#141's state, #166's rule).
    #[test]
    fn closing_the_other_panes_on_an_empty_workspace_is_refused() {
        let mut state = State::default();
        let only = state.focused();
        state
            .apply(Intent::MovePane {
                pane: only,
                workspace: 1,
            })
            .expect("the pane moves");
        state
            .apply(Intent::FocusWorkspace(0))
            .expect("back to the empty one");
        assert_eq!(
            state.apply(Intent::CloseOtherPanes),
            Err(Refused("no pane has the keys"))
        );
    }

    /// **Close every workspace but the one on screen** (#166): the chips go, their panes go with them, and the wall
    /// stays where the operator was — the active workspace is never closed and never emptied.
    #[test]
    fn closing_the_other_workspaces_keeps_the_one_on_screen() {
        let mut state = State::default();
        let only = state.focused();
        let camera = state.camera_of(only);
        let second = state
            .split_pane_as(only, camera, PaneContent::Map)
            .expect("a second pane");
        state
            .apply(Intent::MovePane {
                pane: second,
                workspace: 1,
            })
            .expect("a pane in workspace 2");
        state
            .apply(Intent::MovePane {
                pane: second,
                workspace: 2,
            })
            .expect("and in workspace 3");
        assert_eq!(state.workspaces.len(), 3);
        let active_before = state.active;
        assert_eq!(
            state.apply(Intent::CloseOtherWorkspaces),
            Ok(Applied::Changed(Changed(true)))
        );
        assert_eq!(state.workspaces.len(), 1, "one workspace is left");
        assert_eq!(state.active, 0, "and the wall is on it");
        let names: Vec<&str> = state.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(
            names,
            ["Workspace 3"],
            "the one that was on screen, by name"
        );
        assert_eq!(
            state.workspaces[0]
                .panes
                .iter()
                .map(|pane| pane.id)
                .collect::<Vec<_>>(),
            vec![second],
            "with the pane that was in it, and no other"
        );
        let _ = active_before;
        assert_eq!(
            state.apply(Intent::CloseOtherWorkspaces),
            Ok(Applied::Changed(Changed(false))),
            "one workspace is nothing to close"
        );
    }

    /// **A follow centres the pane on the hull's fix, every frame** (#169) — and a camera intent ends it, so the
    /// wall stops pulling the camera back the moment somebody pans.
    #[test]
    fn following_a_hull_centres_the_pane_and_a_camera_ends_it() {
        let mut state = State::default();
        // The default wall has no picture; the fixture is what a hull's fix comes from.
        state.load_fixture(&crate::scene::fixture().expect("the fixture"));
        let pane = state.focused();
        let unit = crate::scene::LIVE_UNIT;
        let Some(fix) = state
            .picture
            .units()
            .iter()
            .find(|held| held.unit.id_unit == unit)
            .and_then(|held| held.drawn(state.now()))
            .map(|pos| (pos.lat, pos.lon))
        else {
            panic!("the fixture's live hull has a fix");
        };
        assert_eq!(
            state.apply(Intent::Follow {
                pane,
                unit: Some(unit),
                zoom: Some(13.0)
            }),
            Ok(Applied::Changed(Changed(true)))
        );
        let camera = state.camera_of(pane);
        assert!(
            (camera.center.0 - fix.0).abs() < 1e-9 && (camera.center.1 - fix.1).abs() < 1e-9,
            "the pane is centred on the hull: {camera:?} against {fix:?}"
        );
        assert_eq!(camera.zoom, 13.0, "and at the zoom the intent asked for");

        // A camera intent is the operator's: the follow goes, and the camera stays where it was put.
        let elsewhere = crate::map::geometry::Camera::new((-5.0, 105.0), 10.0);
        state
            .apply(Intent::Camera {
                pane,
                camera: elsewhere,
            })
            .expect("a camera");
        assert_eq!(
            state.pane(pane).and_then(|pane| pane.follow),
            None,
            "any camera intent ends the follow (#169)"
        );
        assert_eq!(state.camera_of(pane), elsewhere);
        assert!(
            !state.tick_follows(),
            "and nothing moves it back: the hand is driving"
        );
    }

    /// **A hull that has not reported cannot be followed** (`INV-01`): the refusal names it rather than putting a
    /// camera where a guess would.
    #[test]
    fn following_a_hull_with_no_fix_is_refused() {
        let mut state = State::default();
        state.load_fixture(&crate::scene::fixture().expect("the fixture"));
        let pane = state.focused();
        let silent = crate::scene::fixture()
            .expect("the fixture")
            .into_iter()
            .find(|hull| hull.position.is_none())
            .expect("a hull that has never reported")
            .id_unit;
        assert_eq!(
            state.apply(Intent::Follow {
                pane,
                unit: Some(silent),
                zoom: None
            }),
            Err(Refused("the hull has not reported"))
        );
        assert_eq!(
            state.apply(Intent::Follow {
                pane,
                unit: None,
                zoom: None
            }),
            Ok(Applied::Changed(Changed(true))),
            "letting go is never a refusal"
        );
    }

    /// **A camera write moves the frame with it** (#187): markers project against a *frame* (`INV-04`), so a camera
    /// that changed with no gesture to make a new frame would leave the picture drawing the old place. The state was
    /// right and the picture was stale — worse than a refusal, because nothing said so.
    /// **A cursor belongs to a client** (#205): two origins, two cursors, moving one leaves the other where it was,
    /// pressing one does not press the other, and each fades on its own age. The falsification this is written
    /// against: one shared pointer, where the last client to move wins and nobody can tell whose press landed.
    #[test]
    fn two_clients_have_two_cursors_and_one_cannot_move_the_other() {
        let mut state = crate::scene::state_for("panes");
        let phone = crate::verb::Origin::named("phone");
        let tablet = crate::verb::Origin::named("tablet");
        assert!(
            state.cursors.is_empty(),
            "a wall nobody has pointed at has no cursor"
        );

        state.set_frame_clock(1.0);
        assert_eq!(
            state.apply(Intent::CursorMove {
                at: (100.0, 100.0),
                origin: Some(phone.clone()),
            }),
            Ok(Applied::Changed(Changed(true)))
        );
        // A second and a half later — one full fade — the tablet points somewhere else.
        state.set_frame_clock(1.0 + crate::ui::Cursor::FADE);
        assert_eq!(
            state.apply(Intent::CursorMove {
                at: (300.0, 200.0),
                origin: Some(tablet.clone()),
            }),
            Ok(Applied::Changed(Changed(true)))
        );

        assert_eq!(state.cursors.len(), 2, "two hands, two pointers");
        assert_eq!(
            state.cursors[&phone].at,
            (100.0, 100.0),
            "the phone's pointer has not moved"
        );
        assert_eq!(state.cursors[&tablet].at, (300.0, 200.0));

        // **Ages are per cursor**: the phone has been still for one fade and its ring is gone; the tablet's is not.
        let now = 1.0 + crate::ui::Cursor::FADE;
        assert!(!state.cursors[&phone].visible_at(now));
        assert!(state.cursors[&tablet].visible_at(now));

        // A press is one client's: the phone's button goes down, the tablet's does not.
        state
            .apply(Intent::CursorPress {
                origin: Some(phone.clone()),
            })
            .expect("the phone's press");
        assert!(state.cursors[&phone].down);
        assert!(
            !state.cursors[&tablet].down,
            "one hand's press is not another's"
        );

        // And a client that has not pointed is refused **by name**, not handed somebody else's pointer.
        assert_eq!(
            state.apply(Intent::CursorPress {
                origin: Some(crate::verb::Origin::named("stranger")),
            }),
            Err(Refused("the cursor has not been placed"))
        );
    }

    #[test]
    fn a_camera_write_moves_the_frame_with_it() {
        let mut state = State::default();
        crate::ui::seed_single_pane_frame(&mut state);
        let pane = state.focused();
        assert!(state.pane(pane).and_then(|pane| pane.frame).is_some());
        let camera = crate::map::geometry::Camera::new((-5.5, 105.25), 9.5);
        assert_eq!(
            state.apply(Intent::Camera { pane, camera }),
            Ok(Applied::Changed(Changed(true)))
        );
        assert_eq!(state.camera_of(pane), camera);
        assert_eq!(
            state
                .pane(pane)
                .and_then(|pane| pane.frame)
                .map(|frame| frame.camera),
            Some(camera),
            "the frame a marker projects against must be at the camera the pane holds (#187)"
        );
    }

    /// A pane that is not on the wall is refused with its own reason, never a panic and never a default.
    #[test]
    fn an_unknown_pane_is_refused() {
        let mut state = State::default();
        let ghost = 99;
        assert!(state.pane(ghost).is_none());
        assert_eq!(
            state.apply(Intent::FocusPane(ghost)),
            Err(Refused("no such pane"))
        );
        assert_eq!(
            state.apply(Intent::ClosePane(ghost)),
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
            state.apply(Intent::FocusWorkspace(99)),
            Err(Refused("no such workspace"))
        );
        assert_eq!(state.active, before);
        assert_eq!(
            state.apply(Intent::MovePane {
                pane,
                workspace: 99
            }),
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
            state.apply(Intent::MovePane {
                pane,
                workspace: here
            }),
            Err(Refused("the pane is already in that workspace"))
        );
    }

    /// **A socket's rename is a name, not nine keystrokes**: it lands without a sheet, and it lands the same way
    /// the sheet's Enter does — including what an empty name means for a workspace (#139, #170).
    #[test]
    fn a_rename_lands_without_a_sheet() {
        let mut state = State::default();
        assert!(matches!(state.interaction, crate::ui::Interaction::Idle));
        state
            .apply(Intent::Rename {
                target: RenameTarget::Workspace(0),
                name: Some("Straits watch".to_string()),
            })
            .expect("a name is not a change the machine refuses");
        assert_eq!(state.workspaces[0].name, "Straits watch");
        assert!(
            matches!(state.interaction, crate::ui::Interaction::Idle),
            "a rename with no sheet leaves no surface behind"
        );

        // An empty name falls back to the default, because the header has to say something.
        state
            .apply(Intent::Rename {
                target: RenameTarget::Workspace(0),
                name: None,
            })
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
            state.apply(Intent::SplitAt {
                pane,
                at: (10.0, 10.0)
            }),
            Err(Refused("the pane has no frame yet"))
        );
        assert_eq!(state.pane_count(), 1, "the refusal left the wall alone");
    }

    /// A selection is a value: `Some` puts the pane's card on that hull, `None` clears it.
    #[test]
    fn selecting_a_hull_and_clearing_it() {
        let mut state = State::default();
        let pane = state.focused();
        state
            .apply(Intent::Select {
                pane,
                unit: Some(7),
            })
            .expect("a hull");
        assert_eq!(state.selected_in(pane), Some(7));
        state
            .apply(Intent::Select { pane, unit: None })
            .expect("a clear");
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
        assert_eq!(
            state.apply(Intent::CloseSurfaces),
            Ok(Applied::Changed(Changed(true)))
        );
        assert!(
            matches!(
                state.interaction,
                crate::ui::Interaction::Menu { open: None, .. }
            ),
            "the waterfall closes first"
        );
        assert_eq!(
            state.apply(Intent::CloseSurfaces),
            Ok(Applied::Changed(Changed(true)))
        );
        assert!(
            matches!(state.interaction, crate::ui::Interaction::Idle),
            "then the menu"
        );
        assert_eq!(
            state.apply(Intent::CloseSurfaces),
            Ok(Applied::Changed(Changed(false))),
            "and with nothing open, dismissing changes nothing"
        );
    }

    /// Every verb is unique: two intents with one name is a document that cannot describe the wall.
    #[test]
    fn every_intent_has_its_own_verb() {
        let verbs: Vec<&'static str> = crate::verb::vocabulary().iter().map(Intent::verb).collect();
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

/// **The two intents a drag settles into** (#219): `pane.drop` and `pane.resize`.
///
/// A drag is the pointer's — a pane in the air, a border being held — and the settled result of it is an intent, so
/// these are the two verbs a client with no pointer can send. What makes them worth having as intents rather than as
/// a second layout implementation is that they are the wall's **own** calls: `State::apply_drop` (`Layout::apply`)
/// and `State::set_ratio`, the two a release and a border drag end in. The falsifier is a drop that computes its own
/// rectangle: two landings for one gesture, and a preview drawn from `tiling` that disagrees with the wall.
#[cfg(test)]
mod pane_drop {
    use super::*;

    /// The shape `panes` draws, built the way the menu builds it: two panes side by side, the left one split
    /// top/bottom — three panes, two borders, and one pair that is **not** neighbours (the right pane and the
    /// bottom-left one share no border, which the nested split does not draw).
    fn three_panes() -> (State, ViewId, ViewId, ViewId) {
        let mut state = State::default();
        let first = state.focused();
        let camera = state.camera_of(first);
        let second = state
            .split_pane_as(first, camera, PaneContent::Map)
            .expect("a second pane");
        let third = state
            .split_pane_as(first, camera, PaneContent::Map)
            .expect("a third pane");
        (state, first, second, third)
    }

    fn rect(state: &State, id: ViewId) -> crate::ui::Rect {
        state
            .pane_rect(id)
            .unwrap_or_else(|| panic!("pane {id} has a rectangle"))
    }

    fn shape(state: &State) -> Option<crate::ui::view::Shape> {
        state.layout().and_then(crate::ui::Layout::shape)
    }

    /// Whether a landed pane is on `edge`'s side of the pane it was dropped on, and the word for the failure.
    fn lands(
        edge: Edge,
        dropped: crate::ui::Rect,
        target: crate::ui::Rect,
    ) -> (bool, &'static str) {
        match edge {
            Edge::Left => (dropped.right() <= target.x, "left of"),
            Edge::Right => (dropped.x >= target.right(), "right of"),
            Edge::Top => (dropped.bottom() <= target.y, "above"),
            Edge::Bottom => (dropped.y >= target.bottom(), "below"),
            // A swap puts the pane where the target was, so it is "not on the target" in every direction.
            Edge::Centre => (dropped == target, "on"),
        }
    }

    /// **`centre` is a swap**: the two panes trade places and the tiling keeps its shape, because a swap moves the
    /// panes and not the split.
    #[test]
    fn a_drop_on_the_centre_swaps_the_two_panes() {
        let (mut state, first, second, _third) = three_panes();
        let before = (rect(&state, first), rect(&state, second));
        // The split's **line**, which a swap must not move: a swap carries the panes and leaves the tiling's shape
        // alone, so the borders keep their direction, their place and their extent (only their two ids trade).
        let borders_before: Vec<_> = state
            .borders()
            .into_iter()
            .map(|border| (border.dir, border.at, border.start, border.span))
            .collect();
        assert_eq!(
            state.apply(Intent::Drop {
                pane: first,
                onto: second,
                edge: Edge::Centre,
            }),
            Ok(Applied::Changed(Changed(true))),
            "a swap is a change on screen"
        );
        assert_eq!(
            rect(&state, first),
            before.1,
            "the dropped pane stands where the target stood"
        );
        assert_eq!(
            rect(&state, second),
            before.0,
            "and the target stands where the dropped pane stood"
        );
        assert_eq!(
            state
                .borders()
                .into_iter()
                .map(|border| (border.dir, border.at, border.start, border.span))
                .collect::<Vec<_>>(),
            borders_before,
            "a swap moves the panes, not the split: every border is where it was"
        );
    }

    /// **Each of the four edges retiles the target on that side** (#219): the dropped pane takes the named edge's
    /// half of the target's cell, and the two panes tile rather than overlap or leave a hole.
    #[test]
    fn every_edge_lands_the_pane_on_that_side_of_the_target() {
        for (edge, holds) in [
            (Edge::Left, "left of the target"),
            (Edge::Right, "right of the target"),
            (Edge::Top, "above the target"),
            (Edge::Bottom, "below the target"),
        ] {
            let (mut state, first, second, _third) = three_panes();
            assert_eq!(
                state.apply(Intent::Drop {
                    pane: second,
                    onto: first,
                    edge,
                }),
                Ok(Applied::Changed(Changed(true))),
                "a drop on the {edge:?} edge is a change"
            );
            assert_eq!(state.pane_count(), 3, "a retile keeps every pane");
            let dropped = rect(&state, second);
            let target = rect(&state, first);
            let (lands, why) = lands(edge, dropped, target);
            assert!(
                lands,
                "{edge:?}: the dropped pane is at {dropped:?} and the target at {target:?}, so it does not land \
                 {why} it ({holds}) — `Layout::apply` is the landing and the preview is drawn from it"
            );
            // **Half the target's cell each**, because a re-tile splits it rather than squeezing past it.
            let share = match edge {
                Edge::Left | Edge::Right => dropped.w / (dropped.w + target.w),
                Edge::Top | Edge::Bottom => dropped.h / (dropped.h + target.h),
                // The centre is a swap, tested on its own: no cell is split.
                Edge::Centre => 0.5,
            };
            assert!(
                (share - 0.5).abs() < 0.01,
                "{edge:?}: the split put {share:.3} of the cell on the dropped pane's side, and a fresh split is \
                 half"
            );
        }
    }

    /// **The intent and the pointer land the pane in the same place** — the claim that makes a preview honest.
    ///
    /// The pointer's drop is asked of `Layout::drop_for` at a point in the target's edge band, exactly as a hand's
    /// release asks it; the intent's drop is `Edge::drop_on`. If the two `view::Drop` values differ, or if the two
    /// landings differ, this is red — and a drop intent that computed its own layout would fail here and nowhere
    /// else the gate runs.
    #[test]
    fn the_intent_and_the_pointer_land_the_pane_in_the_same_place() {
        let (mut stated, first, second, _third) = three_panes();
        let target = rect(&stated, second);
        let at = (target.x + target.w / 2.0, target.y + target.h * 0.1);
        assert_eq!(
            stated.drop_for(first, at),
            Some(crate::ui::drop_for(Edge::Top, first, second)),
            "a point in the target's top band is the top edge, and the edge's drop is the pointer's drop"
        );

        // And the swap, whose drop carries the pair in the order `drop_for` builds — asked of the wall **before**
        // the drop above moved the target, because a point means something else once the tiling has changed.
        let middle = (target.x + target.w / 2.0, target.y + target.h / 2.0);
        assert_eq!(
            stated.drop_for(first, middle),
            Some(crate::ui::drop_for(Edge::Centre, first, second)),
            "the middle of a pane is a swap, and the centre edge is the same swap"
        );

        // The pointer's landing: the drop `drop_for` answered, applied the way a release applies it.
        let (mut statep, firstp, secondp, _thirdp) = three_panes();
        let Some(pointer) = statep.drop_for(firstp, at) else {
            panic!("the pointer has a drop at a point inside the target");
        };
        assert!(statep.apply_drop(pointer), "the release landed");
        assert_eq!(
            stated.apply(Intent::Drop {
                pane: first,
                onto: second,
                edge: Edge::Top,
            }),
            Ok(Applied::Changed(Changed(true)))
        );
        assert_eq!(
            shape(&stated),
            shape(&statep),
            "the intent and the pointer produce different tilings for one gesture: the intent computes its own \
             layout instead of calling `Layout::apply`"
        );
        for (landed, pointer_landed) in [(first, firstp), (second, secondp)] {
            assert_eq!(
                rect(&stated, landed),
                rect(&statep, pointer_landed),
                "pane {landed} landed somewhere else on the intent's path"
            );
        }
    }

    /// **A border drag is an intent too**: the pair names the border, in either order, and the share the border's
    /// first side takes is the share the gutter lands on.
    #[test]
    fn a_resize_moves_the_gutter_the_pair_names() {
        let (mut state, first, second, _third) = three_panes();
        let panel = state.pane_viewport().w as f32;
        assert_eq!(
            state.apply(Intent::Resize {
                pane: first,
                other: second,
                ratio: 0.58,
            }),
            Ok(Applied::Changed(Changed(true)))
        );
        let split = rect(&state, second).x;
        assert!(
            (split - panel * 0.58).abs() < 0.5,
            "the gutter is at {split:.1} of a {panel} pt panel, and the intent asked for 58%"
        );
        // **The pair names the border**, so the other order is the same request: a client that reads the tiling
        // cannot know which of the two the border calls its first side.
        assert_eq!(
            state.apply(Intent::Resize {
                pane: second,
                other: first,
                ratio: 0.42,
            }),
            Ok(Applied::Changed(Changed(true))),
            "the same border, named the other way round"
        );
        let back = rect(&state, second).x;
        assert!(
            (back - panel * 0.42).abs() < 0.5,
            "the pair is unordered — the border was found at {back:.1} of {panel} pt, and the ratio is the border's \
             own first side"
        );
    }

    /// **A drop on itself is a rule, not a tiling that failed** (#219): every edge, and the centre, says the rule's
    /// name.
    #[test]
    fn a_drop_on_itself_is_refused_by_name() {
        let (mut state, first, _second, _third) = three_panes();
        for edge in crate::verb::EDGES {
            assert_eq!(
                state.apply(Intent::Drop {
                    pane: first,
                    onto: first,
                    edge: *edge,
                }),
                Err(Refused("the pane cannot be swapped with itself")),
                "{edge:?} on the pane itself is the rule, not a layout that refused"
            );
        }
        assert_eq!(state.pane_count(), 3, "a refusal changes nothing");
    }

    /// **The tiling's own `false` is a refusal with a name** (#219): two panes that exist and cannot be landed on
    /// each other — the workspace on screen holds one pane, or the target is in another workspace. A crossing is
    /// `pane.move` and then a drop, two settled changes.
    #[test]
    fn a_drop_the_tiling_refuses_is_refused_by_name() {
        // **The last pane**: the workspace on screen holds one, and the target is on another chip.
        let mut state = State::default();
        let lone = state.focused();
        let camera = state.camera_of(lone);
        let elsewhere = state
            .split_pane_as(lone, camera, PaneContent::Map)
            .expect("a second pane");
        state
            .apply(Intent::MovePane {
                pane: elsewhere,
                workspace: 1,
            })
            .expect("a pane in workspace 2");
        state
            .apply(Intent::FocusWorkspace(0))
            .expect("back to the wall with one pane");
        assert_eq!(state.pane_count(), 1);
        assert_eq!(
            state.apply(Intent::Drop {
                pane: lone,
                onto: elsewhere,
                edge: Edge::Right,
            }),
            Err(Refused("the tiling refused the pane")),
            "a workspace's last pane has no cell to split"
        );
        assert_eq!(state.pane_count(), 1, "the refusal left the wall alone");
    }

    /// **Those panes are not neighbours**: no border separates them, so there is no ratio to set — and the wall is
    /// where it was.
    #[test]
    fn a_resize_between_panes_with_no_border_is_refused_by_name() {
        let (mut state, first, second, third) = three_panes();
        // The right pane and the bottom-left one are both inside the nested split's `b`… in this shape they share no
        // border: the left column's line is between `first` and `third`.
        assert!(
            state
                .borders()
                .iter()
                .all(|border| !(border.a == second && border.b == third)),
            "the fixture must not have a border between {second} and {third}"
        );
        assert_eq!(
            state.apply(Intent::Resize {
                pane: second,
                other: third,
                ratio: 0.5,
            }),
            Err(Refused("those panes are not neighbours"))
        );
        // A pane is not its own neighbour either.
        assert_eq!(
            state.apply(Intent::Resize {
                pane: first,
                other: first,
                ratio: 0.5,
            }),
            Err(Refused("those panes are not neighbours"))
        );
        assert_eq!(state.pane_count(), 3);
    }

    /// **A share the tiling cannot hold is the field's shape**, and the refusal names the range so a client is not
    /// left guessing it — the message and `config::layout::RATIO_RANGE` are one number.
    #[test]
    fn a_ratio_outside_the_range_is_refused_with_the_range_in_it() {
        let (mut state, first, second, _third) = three_panes();
        let (low, high) = crate::config::layout::RATIO_RANGE;
        for ratio in [-0.5, 0.0, low - 0.01, high + 0.01, 1.0, 7.0, f32::NAN] {
            assert_eq!(
                state.apply(Intent::Resize {
                    pane: first,
                    other: second,
                    ratio,
                }),
                Err(Refused("\"ratio\" is a share from 0.05 to 0.95")),
                "{ratio} is not a share a split can take"
            );
        }
        assert!(
            RATIO_WORDS.contains(&format!("{low}")) && RATIO_WORDS.contains(&format!("{high}")),
            "the refusal must say the range: {RATIO_WORDS:?} against RATIO_RANGE {low}..{high}"
        );
        assert_eq!(
            refusal_kind(RATIO_WORDS),
            Some(RefusalKind::Input),
            "a share outside the range is a call to fix, not a rule about panes"
        );
        // The ends of the range are inside it.
        assert_eq!(
            state.apply(Intent::Resize {
                pane: first,
                other: second,
                ratio: low,
            }),
            Ok(Applied::Changed(Changed(true)))
        );
        assert_eq!(
            state.apply(Intent::Resize {
                pane: first,
                other: second,
                ratio: high,
            }),
            Ok(Applied::Changed(Changed(true)))
        );
    }

    /// Both verbs refuse a pane that is not on the wall with the name the rest of the vocabulary uses, and an edge
    /// that is not one of the five is a mistake in the call rather than a rule (`INV-07`: no default).
    #[test]
    fn an_unknown_pane_and_an_unknown_edge_are_named() {
        let (mut state, first, second, _third) = three_panes();
        let ghost = 99;
        assert!(state.pane(ghost).is_none());
        for (pane, onto) in [(ghost, first), (first, ghost)] {
            assert_eq!(
                state.apply(Intent::Drop {
                    pane,
                    onto,
                    edge: Edge::Left,
                }),
                Err(Refused("no such pane"))
            );
        }
        assert_eq!(
            state.apply(Intent::Resize {
                pane: ghost,
                other: first,
                ratio: 0.5,
            }),
            Err(Refused("no such pane"))
        );
        assert_eq!(
            state.apply(Intent::Resize {
                pane: first,
                other: ghost,
                ratio: 0.5,
            }),
            Err(Refused("no such pane"))
        );
        // The words are the wire's, and a word this wall does not use is not an edge.
        assert_eq!(Edge::of_word("north"), None, "INV-07: no default edge");
        assert_eq!(Edge::of_word("centre"), Some(Edge::Centre));
        assert_eq!(
            refusal_kind(EDGE_WORDS),
            Some(RefusalKind::Input),
            "an unknown edge is a call to fix, not a rule to wait out"
        );
        let _ = second;
    }
}

/// **`workspace.close`** (#242): the empty workspace is the one a press may forget, and the two rules that keep a
/// place's arrangement out of a single press.
#[cfg(test)]
mod workspace_close {
    use super::*;

    /// **`workspace.close` closes an empty workspace and lands where a pane close lands** (#242): the active
    /// workspace is emptied, the wall lands on the nearest previous workspace that still has a pane, and the
    /// workspace is gone from the chips.
    #[test]
    fn closing_an_empty_workspace_lands_like_a_pane_close() {
        let mut state = State::default();
        assert_eq!(state.workspaces.len(), 3);
        assert_eq!(
            state.apply(Intent::FocusWorkspace(1)),
            Ok(Applied::Changed(Changed(true)))
        );
        assert!(state.workspaces[1].panes.is_empty());
        assert_eq!(
            state.apply(Intent::CloseWorkspace(1)),
            Ok(Applied::Changed(Changed(true)))
        );
        assert_eq!(state.workspaces.len(), 2, "the workspace is gone");
        assert_eq!(
            state.active, 0,
            "the nearest previous workspace that still has a pane"
        );
        assert_eq!(
            state
                .workspaces
                .iter()
                .map(|w| w.name.as_str())
                .collect::<Vec<_>>(),
            ["Workspace 1", "Workspace 3"],
            "the chip went and the others keep the wall's own numbering"
        );
    }

    /// **A workspace that still holds panes is refused by name** (#242): its arrangement — cameras, shares,
    /// tiling — is not reconstructible from the wire, so one press may not destroy it. Emptying is
    /// `pane.close_all`, a separate settled change (#241).
    #[test]
    fn closing_a_workspace_that_holds_panes_is_refused_whole() {
        let mut state = State::default();
        let only = state.focused();
        assert_eq!(state.workspaces[0].panes.len(), 1);
        assert_eq!(
            state.apply(Intent::CloseWorkspace(0)),
            Err(Refused("the workspace holds panes"))
        );
        assert_eq!(state.workspaces.len(), 3, "the refusal changed nothing");
        assert!(state.pane(only).is_some());
        assert_eq!(
            refusal_kind(WORKSPACE_HAS_PANES),
            Some(RefusalKind::Rule),
            "the wall will not do that: retrying changes nothing until the panes are emptied"
        );
        assert_eq!(WORKSPACE_HAS_PANES, "the workspace holds panes");
    }

    /// **The wall's last workspace is refused**, the rule `pane.close` keeps one level down (#242): a wall with no
    /// workspace is not a wall.
    #[test]
    fn closing_the_last_workspace_on_the_wall_is_refused_by_name() {
        let mut state = State::default();
        // The wall's last workspace, and it is empty: the state #141 allows and #242 still refuses to remove.
        let only = state.focused();
        assert!(state.workspaces[0].close(only));
        state.workspaces.truncate(1);
        state.active = 0;
        assert_eq!(state.workspaces.len(), 1);
        assert!(state.workspaces[0].panes.is_empty());
        assert_eq!(
            state.apply(Intent::CloseWorkspace(0)),
            Err(Refused("the last workspace on the wall"))
        );
        assert_eq!(state.workspaces.len(), 1, "a refusal is a settled no");
        assert_eq!(
            refusal_kind(LAST_WORKSPACE),
            Some(RefusalKind::Rule),
            "the wall will not do that, and no retry makes a last workspace closable"
        );
        assert_eq!(LAST_WORKSPACE, "the last workspace on the wall");
        assert_eq!(
            state.apply(Intent::CloseWorkspace(9)),
            Err(Refused("no such workspace"))
        );
    }
}

/// **`pane.close_all`** (#241): the verb that empties the workspace on screen, and the one thing it may not take —
/// the wall's last pane.
///
/// The decision is written in `docs/control.md`: an **empty workspace is a state an operator may ask for** (#141
/// already draws it — `No panes yet.`, `empty: true` on the projection), so that a phone or a `ctl` script does not
/// have to send N closes it computed from a projection another client may have changed under it
/// (`docs/mobile-remote.md` decision 1). What no intent may do is leave the **wall** with nothing, so the verb's only
/// refusal is `LAST_PANE`, and it refuses the request **whole** — an intent is a settled change, and three panes
/// half-closed is a wall nobody asked for.
///
/// The two verbs are not two spellings of one: `pane.close_others` keeps the pane with the keys and the workspace it
/// is in (#166); `pane.close_all` gives up both. A client that cannot tell them apart cannot say which reset it
/// wants.
#[cfg(test)]
mod close_all {
    use super::*;

    /// Every pane on the wall, whatever workspace holds it — `State::pane_count` is the workspace on screen's count,
    /// and this verb is about the difference between the two.
    fn wall_panes(state: &State) -> usize {
        state.workspaces.iter().map(|w| w.panes.len()).sum()
    }

    /// **A wall with panes in two workspaces**: workspace 1 holds `first` and `second`, workspace 2 holds `third`,
    /// and workspace 1 is on screen with the keys on `second`. The shape `close_all` has to leave something behind.
    fn two_workspaces() -> (State, ViewId, ViewId, ViewId) {
        let mut state = State::default();
        let first = state.focused();
        let camera = state.camera_of(first);
        let second = state
            .split_pane_as(first, camera, PaneContent::Map)
            .expect("a second pane");
        let third = state
            .split_pane_as(first, camera, PaneContent::Map)
            .expect("a third pane");
        // The move the menu's waterfall sends, which takes the wall with it.
        assert!(
            crate::ui::move_pane_to(&mut state, third, 1),
            "the third pane lands in workspace 2"
        );
        assert!(state.switch_to(0), "back to workspace 1");
        state.focus_pane(second);
        assert_eq!(wall_panes(&state), 3);
        assert_eq!(state.pane_count(), 2, "workspace 1 holds two of them");
        (state, first, second, third)
    }

    /// **Every pane in the workspace on screen goes, and the wall keeps every pane that is not in it** (#241). The
    /// workspace is left **empty and on the chips** (#141): `No panes yet.` where the water was, and the wall lands on
    /// the nearest previous workspace that still has a pane, focused on the pane it remembers.
    #[test]
    fn close_all_empties_the_workspace_on_screen_and_keeps_its_chip() {
        let (mut state, first, second, third) = two_workspaces();
        assert_eq!(
            state.apply(Intent::CloseAllPanes),
            Ok(Applied::Changed(Changed(true))),
            "the wall changed on screen"
        );
        assert_eq!(wall_panes(&state), 1, "every pane but the wall's last");
        assert!(
            state.pane(third).is_some(),
            "the pane on the other workspace is the wall's last, and it is not this workspace's to close"
        );
        assert!(
            state.pane(first).is_none() && state.pane(second).is_none(),
            "the workspace on screen held them, so they went — the one with the keys included"
        );
        assert_eq!(
            state.active, 1,
            "#141's landing: the nearest previous workspace with a pane"
        );
        assert_eq!(
            state.focused(),
            third,
            "and its remembered pane has the keys"
        );
        assert_eq!(
            state.workspaces.len(),
            3,
            "an emptied workspace keeps its chip (#141), so nothing was removed"
        );
        assert!(
            state.workspaces[0].layout.is_none() && state.workspaces[0].panes.is_empty(),
            "the workspace on screen is empty, which is a state the wall already draws"
        );
        assert_eq!(
            state.workspaces[0].name, "Workspace 1",
            "and it keeps its name"
        );
    }

    /// **The wall's last pane is refused, and the refusal names the wall** (the ticket's falsifier): a request that
    /// would leave nothing behind it is refused by #141's rule, not by a sentence about this workspace and not
    /// half-applied.
    #[test]
    fn close_all_is_refused_whole_by_the_walls_rule() {
        // Three panes, every one of them in the workspace on screen: closing all of them is closing the wall.
        let mut state = State::default();
        let first = state.focused();
        let camera = state.camera_of(first);
        state
            .split_pane_as(first, camera, PaneContent::Map)
            .expect("a second pane");
        state
            .split_pane_as(first, camera, PaneContent::Map)
            .expect("a third pane");
        assert_eq!(state.pane_count(), 3);
        assert_eq!(
            state.apply(Intent::CloseAllPanes),
            Err(Refused(LAST_PANE)),
            "the rule is `LAST_PANE` — the wall's name, not a rule about the workspace"
        );
        assert_eq!(
            LAST_PANE, "the last pane on the wall",
            "and the words a client reads say which pane cannot go"
        );
        assert_eq!(
            wall_panes(&state),
            3,
            "a refusal is a settled no: an intent is not half-applied"
        );
    }

    /// A wall of one pane refuses with the same name: `close_all` is not a way round #141.
    #[test]
    fn a_wall_of_one_pane_refuses_close_all_by_name() {
        let mut state = State::default();
        let only = state.focused();
        assert_eq!(state.pane_count(), 1);
        assert_eq!(state.apply(Intent::CloseAllPanes), Err(Refused(LAST_PANE)));
        assert!(state.pane(only).is_some(), "the refusal changed nothing");
    }

    /// **An empty workspace is already the answer**: nothing to close is `changed: false`, not a refusal and not a
    /// second name for "there is nothing here" (#241). The client asked for a state the wall is in.
    #[test]
    fn an_empty_workspace_is_already_what_was_asked_for() {
        let (mut state, _first, _second, _third) = two_workspaces();
        let index = state.add_workspace();
        assert_eq!(state.active, index, "a new workspace is on screen");
        assert!(state.workspaces[index].panes.is_empty());
        assert_eq!(
            state.apply(Intent::CloseAllPanes),
            Ok(Applied::Changed(Changed(false))),
            "the workspace on screen holds no pane, so no pixel moved"
        );
        assert_eq!(state.active, index, "and the wall stayed where it was");
    }

    /// **The two verbs are not two spellings of one** (#241): `close_others` keeps the pane with the keys and the
    /// workspace it is in (#166); `close_all` gives up both. A client that cannot tell them apart cannot say which
    /// reset it asked for.
    #[test]
    fn close_others_keeps_a_pane_and_close_all_does_not() {
        let (mut state, first, second, _third) = two_workspaces();
        assert_eq!(
            state.apply(Intent::CloseOtherPanes),
            Ok(Applied::Changed(Changed(true)))
        );
        assert_eq!(
            state.active, 0,
            "`Close others` leaves the workspace on screen"
        );
        assert!(state.pane(second).is_some(), "and the pane with the keys");
        assert!(state.pane(first).is_none());
        assert_eq!(
            wall_panes(&state),
            2,
            "the pane with the keys is still one of them"
        );

        let (mut state, first, second, third) = two_workspaces();
        assert_eq!(
            state.apply(Intent::CloseAllPanes),
            Ok(Applied::Changed(Changed(true)))
        );
        assert_ne!(
            state.active, 0,
            "`Close all` empties the workspace on screen"
        );
        assert!(
            state.pane(first).is_none() && state.pane(second).is_none(),
            "the pane with the keys goes too"
        );
        assert!(state.pane(third).is_some());
    }

    /// The verb is on the wire under `docs/control.md`'s name, and a round trip through the codec is the identity —
    /// the rule the rest of the vocabulary keeps (#165).
    #[test]
    fn close_all_is_a_verb_a_client_can_send() {
        assert_eq!(Intent::CloseAllPanes.verb(), "pane.close_all");
        let wire = wire::to_wire(&Intent::CloseAllPanes);
        assert_eq!(
            wire,
            serde_json::json!({ "verb": "pane.close_all" }),
            "no fields: the workspace on screen is the one every client already knows it is in"
        );
        assert_eq!(wire::from_wire(&wire), Ok(Intent::CloseAllPanes));
    }
}
