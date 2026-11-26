//! **The exchange corpus** (#224): what this wall answers, recorded so a client in another language can
//! replay it.
//!
//! `docs/control.md` keeps this repo's documents honest against this repo's code, and it cannot see Kotlin.
//! The phone of #217 is written from that document and its author's reading of it, so the first
//! disagreement would otherwise be found by an operator in another room. The fix is the repo's own rule —
//! **fixtures come from the wire** — applied to the one wire we own: the control plane's socket.
//!
//! ## What a file is
//!
//! `fixtures/control/session.jsonl` is one JSON object per line:
//!
//! ```text
//! {"send":{"read":"wall","v":1},"expect":{"ok":true,"v":1,"wall":{…}}}
//! {"send":{"intent":{"pane":2,"verb":"pane.focus"},"v":1},"expect":{"changed":true,"ok":true,"v":1}}
//! ```
//!
//! `send` is the request **as a client sends it** — the object a line carries, or a string when the line is
//! deliberately not JSON (a client's typo is a refusal too). `expect` is the answer line **verbatim**, kept
//! as the bytes the wall wrote. [`line_of`] is the one rule that turns a recorded `send` back into a line.
//!
//! ## How it is recorded
//!
//! Never by hand from the structs. [`recorded_bytes`] builds a wall from a `fixtures/intents/` script (so no
//! deployment's pane names reach the tree), then sends every [`exchanges`] request through **the wall's own
//! session loop** — [`crate::control::server`]'s reader over a [`Loopback`] pair, the same door the unix
//! socket uses (#195) — and writes what came back. The requests are input, like a capture's script; the
//! answers are recorded, and a re-record replaces them.
//!
//! ## What is deliberately not in it
//!
//! A `watch` stream (#221). A coalesced publish is a **race**, and a race is not a fixture: it is covered by
//! Rust tests, where a thread can be held at the exact moment a coalesce happens. A corpus line has to be
//! the same on every machine and on every run (`INV-11`'s rule for a file that is compared), so the corpus
//! carries the settled request/answer pairs and nothing timed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;

use crate::control::Job;
use crate::control::carrier::Loopback;
use crate::control::server;
use crate::ui::State;

/// Where the recordings live, relative to the crate root. `cargo run -- --record` writes here.
pub const DIR: &str = "fixtures/control";

/// The corpus's file name, inside [`DIR`].
pub const SESSION: &str = "session.jsonl";

/// The `fixtures/intents/` script the recorded wall is built from. Its steps go through `State::apply`,
/// and a step that refuses is an error rather than a wall half built — a recording against the wrong wall
/// would still replay, which is exactly the false green this file exists to prevent.
pub const SETUP: &str = "control-corpus";

/// How long the recording waits for the wall, on either side of the session loop. The boundary is the
/// session's own (`server::ANSWER_WITHIN`): the driver plays the UI thread, so a loop that stopped
/// answering must fail the recording with a sentence rather than hang a gate.
const WITHIN: Duration = server::ANSWER_WITHIN;

/// **A request line**, kept as the bytes a client sends rather than as our reading of them.
///
/// The JSON is a [`RawValue`] on purpose: parsing an answer into `serde_json::Value` and writing it back can
/// move a float by one unit in the last place (serde_json's fast path is not correctly rounded), which would
/// make the corpus a near-copy of the wall's answer rather than the answer. A corpus a client compares byte
/// for byte cannot have a paraphrase in it.
///
/// A `send` that is a JSON **string** is a line that is deliberately not JSON — a client's typo is a refusal
/// too, and [`line_of`] is the rule: send the string's contents, or the JSON itself.
pub type Request = Box<RawValue>;

/// **One recorded exchange**: a request line and the answer line it earned.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    /// The request as a client sends it.
    pub send: Request,
    /// The wall's answer, **verbatim**: the exact bytes of the line it wrote.
    pub expect: Box<RawValue>,
}

/// The line a recorded `send` stands for.
///
/// A JSON string is a raw line (the corpus's `not json` probe, and any future typo a client can make); every
/// other value is the JSON a client sends. One rule, used by the recorder, the replay and a reader in
/// another language.
pub fn line_of(send: &RawValue) -> String {
    match serde_json::from_str::<Value>(send.get()) {
        Ok(Value::String(text)) => text,
        _ => send.get().to_string(),
    }
}

/// One request, built from the JSON a request line carries.
fn send(value: Value) -> Request {
    RawValue::from_string(value.to_string()).expect("a serde_json value is valid JSON")
}

/// The directory the recordings live in, from the crate root — the same root `env!("CARGO_MANIFEST_DIR")`
/// gives the tests, so `--record` and the test that reads what it wrote cannot mean two directories.
pub fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DIR)
}

/// The corpus's path inside `dir`.
pub fn session_path(dir: &Path) -> PathBuf {
    dir.join(SESSION)
}

/// **The wall the corpus is recorded against**, built from its `fixtures/intents/` script through
/// `State::apply` — the same door a scene and a socket use.
///
/// The script is read strictly here, unlike `scene::load_intents` (a capture degrades to the wall its scene
/// built and the golden says so). A recording that silently skipped a step would write a corpus of
/// `no such pane` refusals and still replay, so a missing `{"at", "intent"}` is an error naming the step.
pub fn wall() -> Result<State, String> {
    let path = crate::scene::intents_path(SETUP);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let steps: Vec<Value> = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not a list of steps: {error}", path.display()))?;
    // **A recording is headless** (`INV-11`): it binds no socket, writes no `devices.toml`, and draws its
    // pairing code from the frame clock instead of the OS — so `pair.begin`'s answer, and a wrong code's,
    // are the same bytes on every machine (#194, #224). A random code would make the corpus flake one run
    // in a million, and the *right* guess would write a device key into the tree.
    let mut state = State::headless();
    for (index, step) in steps.iter().enumerate() {
        if step.get("at").and_then(Value::as_f64).is_none() {
            return Err(format!(
                "{} step {index} has no \"at\": a setup script is read as {{\"at\": seconds, \
                 \"intent\": {{…}}}}",
                path.display()
            ));
        }
        let intent = step
            .get("intent")
            .ok_or_else(|| format!("{} step {index} has no \"intent\"", path.display()))
            .and_then(|intent| {
                crate::control::wire::from_wire(intent).map_err(|why| {
                    format!(
                        "{} step {index} is not an intent: {}",
                        path.display(),
                        why.0
                    )
                })
            })?;
        state.apply(intent.clone()).map_err(|why| {
            format!(
                "{} step {index} ({}) was refused: {} — the wall the corpus is recorded against was \
                 never built, so every exchange after it would be a refusal of a pane that does not exist",
                path.display(),
                intent.verb(),
                why.0
            )
        })?;
    }
    Ok(state)
}

/// **The requests the corpus records**, in order. Input, not data: a script is curated (there is no wall to
/// drive at a keyboard in a gate), and the answers it earns are recorded from the wall.
///
/// The order is the story: a read before any change and one after it; a settlement of every verb; then a
/// refusal for every rule and every malformed field a client can send. A state-changing request is placed
/// where the requests after it need the wall it leaves — so this list and [`SETUP`] are the whole of what a
/// replay depends on.
pub fn exchanges() -> Vec<Request> {
    use serde_json::json;
    vec![
        // ---- the wall, before any change, and after one (#181)
        send(json!({ "v": 1, "read": "wall" })),
        // ---- **closing a workspace that holds panes is refused by name** (#242): the arrangement is not
        // reconstructible from the wire, so this is the first rule a client meets. The empty one closed below is
        // the verb's one success.
        send(json!({ "v": 1, "intent": { "verb": "workspace.close", "index": 0 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.focus", "pane": 2 } })),
        send(json!({ "v": 1, "read": "wall" })),
        // ---- a pane drawn over the wall, and put back (#85)
        send(json!({ "v": 1, "intent": { "verb": "pane.full_size", "pane": 2 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.full_size", "pane": 2 } })),
        // ---- every rename: a sheet, a name, a workspace's name, and a clear (#139, #154)
        send(json!({ "v": 1, "intent": { "verb": "rename.begin", "target": { "pane": 1 } } })),
        send(
            json!({ "v": 1, "intent": { "verb": "rename.set", "target": { "pane": 1 }, "name": "Straits watch" } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "rename.set", "target": { "workspace": 0 }, "name": "Straits watch" } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "rename.set", "target": { "workspace": 0 }, "name": null } }),
        ),
        send(json!({ "v": 1, "intent": { "verb": "surface.close" } })),
        // ---- a pane of each kind, and one at the pane's own centre (#77, #116, #136)
        send(
            json!({ "v": 1, "intent": { "verb": "pane.split", "pane": 1, "content": { "kind": "map" } } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.split", "pane": 5, "content": { "kind": "tree", "organisation": null } } }),
        ),
        send(json!({ "v": 1, "intent": { "verb": "pane.open", "content": { "kind": "map" } } })),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.split_at", "pane": 1, "at": [100.0, 100.0] } }),
        ),
        // ---- the water a pane cannot draw, and a card it can (#99, #169)
        send(json!({ "v": 1, "intent": { "verb": "object.follow", "pane": 3, "unit": 999999 } })),
        send(json!({ "v": 1, "intent": { "verb": "object.follow", "pane": 1, "unit": 999999 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.select", "pane": 1, "unit": 512345 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.select", "pane": 1, "unit": null } })),
        send(
            json!({ "v": 1, "intent": { "verb": "map.camera", "pane": 1, "camera": { "lat": -6.108, "lon": 106.91, "zoom": 11 } } }),
        ),
        // ---- workspaces: a pane of its own, a new empty one, and a pane carried into it (#138, #140)
        send(json!({ "v": 1, "intent": { "verb": "workspace.from_pane", "pane": 1 } })),
        // ---- **the workspace on screen emptied on purpose** (#241): the workspace `from_pane` just made holds one
        // pane and the wall holds others, so `pane.close_all` closes it and #141 lands the wall on the nearest
        // previous workspace with a pane. The read is the claim: the emptied workspace is still in the list, with
        // `empty: true`, which is the state this verb exists to let a client ask for.
        send(json!({ "v": 1, "intent": { "verb": "pane.close_all" } })),
        send(json!({ "v": 1, "read": "wall" })),
        send(json!({ "v": 1, "intent": { "verb": "workspace.new" } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.close_others" } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.move", "pane": 4, "workspace": 4 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.move", "pane": 4, "workspace": 4 } })),
        send(json!({ "v": 1, "intent": { "verb": "workspace.focus", "index": 0 } })),
        // ---- a drag, settled: a border moved, and a pane landed on an edge (#219)
        send(
            json!({ "v": 1, "intent": { "verb": "pane.resize", "pane": 1, "other": 3, "ratio": 0.58 } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.resize", "pane": 1, "other": 2, "ratio": 2.0 } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.resize", "pane": 1, "other": 9, "ratio": 0.58 } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.resize", "pane": 2, "other": 3, "ratio": 0.58 } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.drop", "pane": 1, "onto": 1, "edge": "top" } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.drop", "pane": 1, "onto": 4, "edge": "top" } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.drop", "pane": 2, "onto": 1, "edge": "top" } }),
        ),
        // ---- a client's cursor, and the hull a press reaches (#167)
        send(json!({ "v": 1, "intent": { "verb": "cursor.press" } })),
        send(json!({ "v": 1, "intent": { "verb": "cursor.move", "at": [420.0, 180.0] } })),
        send(json!({ "v": 1, "intent": { "verb": "cursor.release" } })),
        send(json!({ "v": 1, "intent": { "verb": "cursor.press" } })),
        send(json!({ "v": 1, "intent": { "verb": "cursor.release" } })),
        // ---- the one modal surface, and the ladder Escape walks (#82, #140)
        send(json!({ "v": 1, "intent": { "verb": "surface.settings" } })),
        send(json!({ "v": 1, "intent": { "verb": "surface.close" } })),
        // ---- the menu's three verbs never open a menu: a press does (#164). A client that asks anyway is
        // told by name rather than guessed at.
        send(json!({ "v": 1, "intent": { "verb": "menu.focus", "row": 1 } })),
        send(json!({ "v": 1, "intent": { "verb": "menu.submenu", "open": true } })),
        send(json!({ "v": 1, "intent": { "verb": "menu.choose" } })),
        // ---- a close that lands, then the wall's last pane (#141)
        send(json!({ "v": 1, "intent": { "verb": "pane.close", "pane": 7 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.close", "pane": 99 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.focus", "pane": 2 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.close_others" } })),
        send(json!({ "v": 1, "intent": { "verb": "workspace.close_others" } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.close", "pane": 2 } })),
        // ---- and the same rule on the verb that asks for the whole workspace at once (#241): the wall's last pane
        // is not the workspace's to close, and the refusal names the wall rather than this workspace.
        send(json!({ "v": 1, "intent": { "verb": "pane.close_all" } })),
        send(json!({ "v": 1, "read": "wall" })),
        // ---- **the composition this ticket's decision names** (#242): a workspace made by `from_pane`, emptied
        // by `pane.close_all` (#241), and then **closed** by `workspace.close`. The close lands where a pane close
        // lands — the workspace that still has a pane — and the read is the claim that the place is gone.
        send(json!({ "v": 1, "intent": { "verb": "workspace.from_pane", "pane": 2 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.close_all" } })),
        send(json!({ "v": 1, "intent": { "verb": "workspace.close", "index": 1 } })),
        send(json!({ "v": 1, "read": "wall" })),
        // ---- and the other refusal: a wall with one workspace, even an empty one, has no workspace to give up
        // (#242). `workspace.close_others` leaves the new empty one on the wall so the request is refused by the
        // rule rather than by a pane it still holds.
        send(json!({ "v": 1, "intent": { "verb": "workspace.new" } })),
        send(json!({ "v": 1, "intent": { "verb": "workspace.close_others" } })),
        send(json!({ "v": 1, "intent": { "verb": "workspace.close", "index": 0 } })),
        send(json!({ "v": 1, "read": "wall" })),
        // ---- every rule a request can break
        send(json!({ "v": 1, "intent": { "verb": "pane.focus", "pane": 99 } })),
        send(json!({ "v": 1, "intent": { "verb": "workspace.focus", "index": 9 } })),
        send(json!({ "v": 1, "intent": { "verb": "workspace.close", "index": 9 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.focus", "pane": "two" } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.focus" } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.split", "pane": 1 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.split", "pane": 1, "content": {} } })),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.split", "pane": 1, "content": { "kind": "hologram" } } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.split", "pane": 1, "content": { "kind": "tree", "organisation": "navy" } } }),
        ),
        send(json!({ "v": 1, "intent": { "verb": "rename.begin", "target": {} } })),
        send(
            json!({ "v": 1, "intent": { "verb": "rename.set", "target": { "pane": 1 }, "name": 5 } }),
        ),
        send(json!({ "v": 1, "intent": { "verb": "pane.split_at", "pane": 1, "at": "middle" } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.drop", "pane": 1, "onto": 2 } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.drop", "pane": 1, "edge": "top" } })),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.drop", "pane": 1, "onto": 2, "edge": "north" } }),
        ),
        send(json!({ "v": 1, "intent": { "verb": "pane.resize", "pane": 1, "ratio": 0.5 } })),
        send(
            json!({ "v": 1, "intent": { "verb": "pane.resize", "pane": 1, "other": 2, "ratio": "half" } }),
        ),
        send(json!({ "v": 1, "intent": { "verb": "menu.submenu", "open": "yes" } })),
        send(json!({ "v": 1, "intent": { "verb": "menu.focus", "row": "two" } })),
        send(json!({ "v": 1, "intent": { "verb": "pane.select", "pane": 1, "unit": "seven" } })),
        send(
            json!({ "v": 1, "intent": { "verb": "object.follow", "pane": 1, "unit": 1, "zoom": "far" } }),
        ),
        send(json!({ "v": 1, "intent": { "verb": "cursor.move", "at": [7.0] } })),
        send(json!({ "v": 1, "intent": { "verb": "cursor.move", "at": [1.0, 2.0], "origin": 5 } })),
        send(json!({ "v": 1, "intent": { "verb": "map.camera", "pane": 1, "camera": 5 } })),
        // ---- the envelope: a version, a body, and a verb this wall does not answer (#165)
        send(json!({ "v": 99, "intent": { "verb": "pane.focus", "pane": 1 } })),
        send(json!({})),
        send(json!({ "v": 1 })),
        send(json!({ "v": 1, "intent": { "verb": "pane.focus", "pane": 1 }, "read": "wall" })),
        send(json!({ "v": 1, "read": "fleet" })),
        // ---- a watch (#221) is a **carrier's capability**, and the corpus's carrier is the socket's stand-in: the ask
        // is refused by name, which is what a `ctl` script gets. The carried half is not recordable — a coalesced
        // publish is a race, and `fixtures/README.md` says why a file cannot hold one.
        send(json!({ "v": 1, "watch": "wall" })),
        send(json!({ "v": 1, "watch": "scene" })),
        send(json!({ "v": 1, "intent": 5 })),
        send(json!({ "v": 1, "intent": {} })),
        send(json!({ "v": 1, "intent": { "verb": "pane.teleport", "pane": 1 } })),
        send(json!([])),
        send(json!("not json")),
        // ---- pairing (#194): the code, the claim, and the three refusals a client can earn without a key
        //
        // The order is the wall's: a claim before anything is open, then the code on the wall, then a wrong
        // guess, then two malformed claims, then a forget that removes nothing. **A successful claim is not
        // recorded**: its answer carries the device key, and a key never enters the tree (`INV-08`).
        send(
            json!({ "v": 1, "intent": { "verb": "pair.claim", "code": "481902", "name": "phone" } }),
        ),
        send(json!({ "v": 1, "intent": { "verb": "pair.begin" } })),
        send(
            json!({ "v": 1, "intent": { "verb": "pair.claim", "code": "000000", "name": "phone" } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pair.claim", "code": 481902, "name": "phone" } }),
        ),
        send(
            json!({ "v": 1, "intent": { "verb": "pair.claim", "code": "481902", "name": "   " } }),
        ),
        send(json!({ "v": 1, "intent": { "verb": "pair.forget", "name": "nobody" } })),
    ]
}

/// **One request through the wall's own session loop** (#195), and the lines it answered.
///
/// The loop runs on a thread of its own, over a [`Loopback`] pair, exactly as the unix socket serves a
/// client. This thread is the **UI thread**: it takes each [`Job`] the loop hands over, answers it with
/// `control::authority::answer` (the same call the view's `poll` makes), and never lets the loop apply
/// anything itself — `INV-05`, held by the same architecture a wall holds it with.
fn through_the_session(state: &mut State, lines: Vec<String>) -> Result<Vec<String>, String> {
    let (wall_end, peer) = Loopback::pair();
    let (tx, jobs) = mpsc::channel::<Job>();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let serving = std::thread::Builder::new()
        .name("corpus-session".to_string())
        .spawn(move || server::serve_session(wall_end, &tx, &stopping))
        .map_err(|error| format!("cannot start the session loop: {error}"))?;

    let mut answers = Vec::with_capacity(lines.len());
    let mut failure = None;
    'exchange: for line in lines {
        peer.send(&line);
        // **A line the session cannot parse is answered by the loop itself** (`not JSON`, a bad version,
        // an unknown verb before it reaches the wall): a `Job` and an answer are the two things that can come
        // back, and the driver looks for both. Waiting on the queue alone would hang on the first typo the
        // corpus deliberately records.
        let until = std::time::Instant::now() + WITHIN;
        let job = loop {
            match jobs.try_recv() {
                Ok(job) => break job,
                Err(mpsc::TryRecvError::Disconnected) => {
                    failure = Some(format!("the session loop stopped before {line:?}"));
                    break 'exchange;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if let Some(answer) = peer.try_recv() {
                answers.push(answer);
                continue 'exchange;
            }
            if std::time::Instant::now() >= until {
                failure = Some(format!(
                    "the session loop neither handed over nor answered {line:?}: nothing is serving the \
                     wall"
                ));
                break 'exchange;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        // A malformed line was answered above (the `continue`); a parsed one is the UI thread's, and this
        // is that thread — through the same door `poll` uses, so the recording is the wall's own answer (#222).
        state.answer(job);
        match peer.recv_within(WITHIN) {
            Some(answer) => answers.push(answer),
            None => {
                failure = Some(format!(
                    "the session loop did not answer {line:?}: the wall it serves stopped"
                ));
                break;
            }
        }
    }
    // End of file, exactly as a client hanging up: the loop's read returns `None` and its thread ends.
    drop(peer);
    stop.store(true, Ordering::SeqCst);
    let served = serving.join().map_err(|_| {
        "the session loop panicked — a carrier failure is `Ok(None)`, so this is a bug".to_string()
    })?;
    if let Some(failure) = failure {
        return Err(failure);
    }
    served.map_err(|error| format!("the session loop failed: {error}"))?;
    Ok(answers)
}

/// **The corpus this wall records today**, as the bytes `--record` writes.
///
/// Every request goes through [`through_the_session`], and each answer becomes the exchange's `expect`
/// **as the bytes the loop wrote** — a [`RawValue`], never a value parsed and printed back, so the file
/// cannot quietly become a paraphrase of what the wall said.
pub fn recorded_bytes() -> Result<String, String> {
    let mut state = wall()?;
    let sends = exchanges();
    let lines: Vec<String> = sends.iter().map(|send| line_of(send)).collect();
    let answers = through_the_session(&mut state, lines)?;
    let mut exchanges = Vec::with_capacity(sends.len());
    for (index, (send, answer)) in sends.into_iter().zip(answers).enumerate() {
        let expect = RawValue::from_string(answer).map_err(|error| {
            format!(
                "exchange {index} ({}) was answered with a line that is not JSON: {error}",
                line_of(&send)
            )
        })?;
        exchanges.push(Exchange { send, expect });
    }
    Ok(bytes(&exchanges))
}

/// The bytes of a corpus: one compact JSON object per line, and a final newline so the file diffs line by
/// line.
pub fn bytes(exchanges: &[Exchange]) -> String {
    let mut out = String::new();
    for exchange in exchanges {
        out.push_str(
            &serde_json::to_string(exchange).expect("an exchange is a plain data structure"),
        );
        out.push('\n');
    }
    out
}

/// One recorded file, read. The error names the line, because a corpus is a list and "the file is wrong"
/// does not say where.
pub fn read(path: &Path) -> Result<Vec<Exchange>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut out = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let exchange: Exchange = serde_json::from_str(line).map_err(|error| {
            format!(
                "{} line {} is not {{\"send\": {{…}}, \"expect\": {{…}}}}: {error}",
                path.display(),
                number + 1
            )
        })?;
        out.push(exchange);
    }
    Ok(out)
}

/// **Replay a corpus through this wall**, comparing each answer with the file line for line.
///
/// This is the gate's half and `scripts/conformance.sh`'s half in one function, so a client that reads the
/// same file and a gate that checks it cannot disagree. The first difference is the error, with the line
/// and both answers, because "the corpus is stale" is not a line anybody can act on.
pub fn replay(path: &Path) -> Result<Vec<(Exchange, String)>, String> {
    let recorded = read(path)?;
    let mut state = wall()?;
    let lines: Vec<String> = recorded
        .iter()
        .map(|exchange| line_of(&exchange.send))
        .collect();
    let answers = through_the_session(&mut state, lines)?;
    let mut out = Vec::with_capacity(recorded.len());
    for (index, (exchange, answer)) in recorded.into_iter().zip(answers).enumerate() {
        let expected = exchange.expect.get();
        if answer != expected {
            return Err(format!(
                "{} line {} ({}) answered {answer:?}, and the corpus expects {expected:?} — the wall \
                 and the file disagree, so a client would replay the wrong contract",
                path.display(),
                index + 1,
                line_of(&exchange.send)
            ));
        }
        out.push((exchange, answer));
    }
    Ok(out)
}

/// **Write the corpus**: one re-record from the wall, over the file a human reads.
///
/// It is a tool, not a capture: nothing here reads the wall's arrangement or its settings, and every
/// request/answer pair comes through the session loop, so a recording is the same bytes on every machine —
/// `INV-11`'s rule, for a file that is compared rather than drawn.
pub fn record(dir: &Path) -> Result<PathBuf, String> {
    let recorded = recorded_bytes()?;
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("cannot make {}: {error}", dir.display()))?;
    let path = session_path(dir);
    std::fs::write(&path, recorded)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(path)
}
