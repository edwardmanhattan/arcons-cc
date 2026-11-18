//! **The wire** — an [`Intent`] as JSON, and a reply as JSON (#165).
//!
//! One line in, one line out, no framing to get wrong:
//!
//! ```text
//! {"v": 1, "intent": {"verb": "pane.focus", "pane": 2}}
//! {"v": 1, "ok": true, "changed": true}
//! {"v": 1, "ok": false, "why": "the last pane on the wall"}
//! ```
//!
//! The shapes are the ADR's (`docs/adr/0005-control-plane.md`) and the prose is `docs/control.md`. Three rules
//! this module keeps, each of which has a test:
//!
//! - **A verb is `Intent::verb()`** and nothing else. The names live there; this file only spells the fields.
//! - **An unknown verb, version or field is a named refusal**, never a default (`INV-07`'s rule applied to a
//!   protocol: a new case is a visible branch, not a silent fall-through).
//! - **A round trip is the identity**: `from_wire(&to_wire(intent)) == intent` for every variant, which is what
//!   keeps the two halves of this file — and the CLI that builds the same JSON — from drifting apart.

use serde_json::{Map, Value, json};

use super::projection::Wall;
use super::snapshot::Scene;
use super::watch::Watch;
use super::{Changed, Intent, PROTOCOL, Paired, RefusalKind, Refused};
use crate::ui::{PaneContent, RenameTarget};

/// An intent as the JSON a client sends.
pub fn to_wire(intent: &Intent) -> Value {
    let mut object = Map::new();
    object.insert("verb".to_string(), json!(intent.verb()));
    match intent {
        Intent::FocusPane(pane) => {
            object.insert("pane".into(), json!(pane));
        }
        Intent::ToggleFullSize(pane) => {
            object.insert("pane".into(), json!(pane));
        }
        Intent::ClosePane(pane) => {
            object.insert("pane".into(), json!(pane));
        }
        Intent::BeginRename(target) => {
            object.insert("target".into(), target_json(*target));
        }
        Intent::Rename { target, name } => {
            object.insert("target".into(), target_json(*target));
            object.insert("name".into(), json!(name));
        }
        Intent::SplitPane { pane, content } => {
            object.insert("pane".into(), json!(pane));
            object.insert("content".into(), content_json(content));
        }
        Intent::SplitAt { pane, at } => {
            object.insert("pane".into(), json!(pane));
            object.insert("at".into(), json!([at.0, at.1]));
        }
        Intent::SplitToWorkspace(pane) => {
            object.insert("pane".into(), json!(pane));
        }
        Intent::MovePane { pane, workspace } => {
            object.insert("pane".into(), json!(pane));
            object.insert("workspace".into(), json!(workspace));
        }
        Intent::Drop { pane, onto, edge } => {
            object.insert("pane".into(), json!(pane));
            object.insert("onto".into(), json!(onto));
            // **The edge is a word, never a number** (#219): a client says `top`, not `2` — a slot is a place in the
            // tiling, and an index would be a second spelling of it that drifts the moment the enum grows.
            object.insert("edge".into(), json!(edge.word()));
        }
        Intent::Resize { pane, other, ratio } => {
            object.insert("pane".into(), json!(pane));
            object.insert("other".into(), json!(other));
            object.insert("ratio".into(), ratio_json(*ratio));
        }
        Intent::CloseOtherPanes | Intent::CloseAllPanes | Intent::CloseOtherWorkspaces => {}
        Intent::CloseWorkspace(index) => {
            object.insert("index".into(), json!(index));
        }
        Intent::OpenPane(content) => {
            object.insert("content".into(), content_json(content));
        }
        Intent::Follow { pane, unit, zoom } => {
            object.insert("pane".into(), json!(pane));
            object.insert("unit".into(), json!(unit));
            object.insert("zoom".into(), json!(zoom));
        }
        Intent::Select { pane, unit } => {
            object.insert("pane".into(), json!(pane));
            object.insert("unit".into(), json!(unit));
        }
        Intent::FocusWorkspace(index) => {
            object.insert("index".into(), json!(index));
        }
        Intent::NewWorkspace => {}
        Intent::Camera { pane, camera } => {
            object.insert("pane".into(), json!(pane));
            object.insert(
                "camera".into(),
                json!({
                    "lat": camera.center.0,
                    "lon": camera.center.1,
                    "zoom": camera.zoom,
                }),
            );
        }
        Intent::MenuFocus { row } => {
            object.insert("row".into(), json!(row));
        }
        Intent::MenuSubmenu { open } => {
            object.insert("open".into(), json!(open));
        }
        Intent::MenuChoose => {}
        Intent::CursorMove { at, origin } => {
            object.insert("at".into(), json!([at.0, at.1]));
            if let Some(origin) = origin {
                object.insert("origin".into(), json!(origin.as_str()));
            }
        }
        Intent::CursorPress { origin } | Intent::CursorRelease { origin } => {
            if let Some(origin) = origin {
                object.insert("origin".into(), json!(origin.as_str()));
            }
        }
        Intent::PairBegin => {}
        Intent::PairClaim { code, name } => {
            object.insert("code".into(), json!(code));
            object.insert("name".into(), json!(name));
        }
        Intent::PairForget { name } => {
            object.insert("name".into(), json!(name));
        }
        Intent::OpenSettings | Intent::CloseSurfaces => {}
    }
    Value::Object(object)
}

/// An intent from the JSON a client sent, or the reason it is not one.
///
/// The `verb` decides which fields are read, and **the match has no catch-all**: a verb that is not in the
/// vocabulary is `"unknown verb"`, and a known verb with a missing or ill-typed field says which one — a client
/// that half-implemented a verb gets told, not defaulted.
pub fn from_wire(value: &Value) -> Result<Intent, Refused> {
    let object = value
        .as_object()
        .ok_or(Refused("an intent is a JSON object"))?;
    let verb = object
        .get("verb")
        .and_then(Value::as_str)
        .ok_or(Refused("an intent has no verb"))?;
    let intent = match verb {
        "pane.focus" => Intent::FocusPane(pane(object)?),
        "pane.full_size" => Intent::ToggleFullSize(pane(object)?),
        "pane.close" => Intent::ClosePane(pane(object)?),
        "rename.begin" => Intent::BeginRename(target(object)?),
        "rename.set" => Intent::Rename {
            target: target(object)?,
            name: match object.get("name") {
                None | Some(Value::Null) => None,
                Some(Value::String(name)) => Some(name.clone()),
                Some(_) => return Err(Refused("\"name\" is a string or null")),
            },
        },
        "pane.split" => Intent::SplitPane {
            pane: pane(object)?,
            content: content(object)?,
        },
        "pane.split_at" => {
            let at = object
                .get("at")
                .and_then(Value::as_array)
                .filter(|at| at.len() == 2)
                .ok_or(Refused("\"at\" is [x, y] in the pane's frame"))?;
            let x = at[0]
                .as_f64()
                .ok_or(Refused("\"at\" is [x, y] in the pane's frame"))?;
            let y = at[1]
                .as_f64()
                .ok_or(Refused("\"at\" is [x, y] in the pane's frame"))?;
            Intent::SplitAt {
                pane: pane(object)?,
                at: (x, y),
            }
        }
        "workspace.from_pane" => Intent::SplitToWorkspace(pane(object)?),
        "pane.move" => Intent::MovePane {
            pane: pane(object)?,
            workspace: integer(object, "workspace")? as usize,
        },
        "pane.drop" => Intent::Drop {
            pane: pane(object)?,
            onto: integer(object, "onto")? as crate::ui::ViewId,
            // **A word this wall does not use is a refusal, not a default** (#219, `INV-07`): guessing `centre` for
            // an edge a newer wall added would land a pane somewhere the client did not ask for.
            edge: object
                .get("edge")
                .and_then(Value::as_str)
                .and_then(super::Edge::of_word)
                .ok_or(Refused(super::EDGE_WORDS))?,
        },
        "pane.resize" => Intent::Resize {
            pane: pane(object)?,
            other: integer(object, "other")? as crate::ui::ViewId,
            ratio: object
                .get("ratio")
                .and_then(Value::as_f64)
                .ok_or(Refused(super::RATIO_WORDS))? as f32,
        },
        "pane.close_others" => Intent::CloseOtherPanes,
        "pane.close_all" => Intent::CloseAllPanes,
        "workspace.close_others" => Intent::CloseOtherWorkspaces,
        "workspace.close" => Intent::CloseWorkspace(integer(object, "index")? as usize),
        "pane.open" => Intent::OpenPane(content(object)?),
        "object.follow" => Intent::Follow {
            pane: pane(object)?,
            unit: match object.get("unit") {
                None | Some(Value::Null) => None,
                Some(unit) => Some(
                    unit.as_i64()
                        .ok_or(Refused("\"unit\" is a hull id or null"))?,
                ),
            },
            zoom: match object.get("zoom") {
                None | Some(Value::Null) => None,
                Some(zoom) => Some(
                    zoom.as_f64()
                        .ok_or(Refused("\"zoom\" is a number or null"))?,
                ),
            },
        },
        "pane.select" => Intent::Select {
            pane: pane(object)?,
            unit: match object.get("unit") {
                None | Some(Value::Null) => None,
                Some(unit) => Some(
                    unit.as_i64()
                        .ok_or(Refused("\"unit\" is a hull id or null"))?,
                ),
            },
        },
        "workspace.focus" => Intent::FocusWorkspace(integer(object, "index")? as usize),
        "workspace.new" => Intent::NewWorkspace,
        "map.camera" => {
            let camera = object
                .get("camera")
                .and_then(Value::as_object)
                .ok_or(Refused("\"camera\" is {lat, lon, zoom}"))?;
            let lat = camera
                .get("lat")
                .and_then(Value::as_f64)
                .ok_or(Refused("\"camera\" is {lat, lon, zoom}"))?;
            let lon = camera
                .get("lon")
                .and_then(Value::as_f64)
                .ok_or(Refused("\"camera\" is {lat, lon, zoom}"))?;
            let zoom = camera
                .get("zoom")
                .and_then(Value::as_f64)
                .ok_or(Refused("\"camera\" is {lat, lon, zoom}"))?;
            Intent::Camera {
                pane: pane(object)?,
                camera: crate::map::geometry::Camera::new((lat, lon), zoom),
            }
        }
        "menu.focus" => Intent::MenuFocus {
            row: match object.get("row") {
                None | Some(Value::Null) => None,
                Some(row) => Some(
                    row.as_u64()
                        .ok_or(Refused("\"row\" is a row index or null"))?
                        as usize,
                ),
            },
        },
        "menu.submenu" => Intent::MenuSubmenu {
            open: object
                .get("open")
                .and_then(Value::as_bool)
                .ok_or(Refused("\"open\" is true or false"))?,
        },
        "menu.choose" => Intent::MenuChoose,
        "cursor.move" => {
            let at = object
                .get("at")
                .and_then(Value::as_array)
                .filter(|at| at.len() == 2)
                .ok_or(Refused("\"at\" is [x, y] in the picture panel's points"))?;
            let x = at[0]
                .as_f64()
                .ok_or(Refused("\"at\" is [x, y] in the picture panel's points"))?;
            let y = at[1]
                .as_f64()
                .ok_or(Refused("\"at\" is [x, y] in the picture panel's points"))?;
            Intent::CursorMove {
                at: (x as f32, y as f32),
                origin: origin_of(object)?,
            }
        }
        "cursor.press" => Intent::CursorPress {
            origin: origin_of(object)?,
        },
        "cursor.release" => Intent::CursorRelease {
            origin: origin_of(object)?,
        },
        "pair.begin" => Intent::PairBegin,
        "pair.claim" => Intent::PairClaim {
            code: match object.get("code") {
                None => return Err(Refused("\"code\" is the six digits on the wall")),
                Some(Value::String(code)) => code.clone(),
                Some(_) => return Err(Refused("\"code\" is the six digits on the wall")),
            },
            name: device_name(object)?,
        },
        "pair.forget" => Intent::PairForget {
            name: device_name(object)?,
        },
        "surface.settings" => Intent::OpenSettings,
        "surface.close" => Intent::CloseSurfaces,
        _ => return Err(Refused("unknown verb")),
    };
    Ok(intent)
}

/// **Whose pointer** (#205), when a client says. Absent is the local client's — a `ctl` over the socket — and a phone
/// will name itself once pairing gives it a name (#194). A name is trimmed and otherwise the client's own.
fn origin_of(object: &Map<String, Value>) -> Result<Option<crate::ui::Origin>, Refused> {
    match object.get("origin") {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(|name| Some(crate::ui::Origin::named(name)))
            .ok_or(Refused("\"origin\" is a client name")),
    }
}

/// A device's name (#194): a string that is not blank. The same words `pair.claim`'s own blank-name refusal uses,
/// so a client that sent `{"name": 3}` and one that sent `{"name": " "}` read one sentence.
fn device_name(object: &Map<String, Value>) -> Result<String, Refused> {
    match object.get("name") {
        Some(Value::String(name)) if !name.trim().is_empty() => Ok(name.clone()),
        _ => Err(Refused("\"name\" is a device name")),
    }
}

fn pane(object: &Map<String, Value>) -> Result<crate::ui::ViewId, Refused> {
    Ok(integer(object, "pane")? as crate::ui::ViewId)
}

fn integer(object: &Map<String, Value>, key: &str) -> Result<i64, Refused> {
    match object.get(key) {
        None => Err(Refused(match key {
            "pane" => "\"pane\" is a pane id",
            "onto" => "\"onto\" is a pane id",
            "other" => "\"other\" is a pane id",
            "workspace" => "\"workspace\" is an index",
            "index" => "\"index\" is an index",
            _ => "a field is missing",
        })),
        Some(value) => value
            .as_i64()
            .filter(|value| *value >= 0)
            .ok_or(Refused("an id is a non-negative integer")),
    }
}

fn target_json(target: RenameTarget) -> Value {
    match target {
        RenameTarget::Pane(pane) => json!({ "pane": pane }),
        RenameTarget::Workspace(index) => json!({ "workspace": index }),
    }
}

fn target(object: &Map<String, Value>) -> Result<RenameTarget, Refused> {
    let target = object
        .get("target")
        .and_then(Value::as_object)
        .ok_or(Refused("\"target\" is {\"pane\": n} or {\"workspace\": n}"))?;
    if target.contains_key("pane") {
        Ok(RenameTarget::Pane(
            integer(target, "pane")? as crate::ui::ViewId
        ))
    } else if target.contains_key("workspace") {
        Ok(RenameTarget::Workspace(
            integer(target, "workspace")? as usize
        ))
    } else {
        Err(Refused("\"target\" is {\"pane\": n} or {\"workspace\": n}"))
    }
}

/// A share as a client reads it: **six places**, the same rule `config::layout` keeps for a file.
///
/// An `f32` widened to `f64` prints as `0.5799999833106995`, and a client that reads that back into an `f32` gets
/// exactly the share that was meant — so the rounding costs nothing and the line says what an operator typed. The
/// value is unchanged on the way in: [`from_wire`] reads whatever decimal the client sent and keeps the `f32`.
fn ratio_json(ratio: f32) -> Value {
    json!((f64::from(ratio) * 1e6).round() / 1e6)
}

/// An edge as the word a CLI types (#219): `left`, `right`, `top`, `bottom` or `centre`.
///
/// `None` is a refusal rather than a default: an edge is the one field of `pane.drop` that says **where**, and a
/// guessed one would land a pane in a slot nobody named.
pub fn edge_from_words(word: Option<&str>) -> Result<super::Edge, String> {
    word.and_then(super::Edge::of_word).ok_or_else(|| {
        format!(
            "an edge is left, right, top, bottom or centre, got {}",
            word.map(|word| format!("{word:?}"))
                .unwrap_or_else(|| "nothing".to_string())
        )
    })
}

/// A pane kind as the words a CLI types (#165): `map`, `tree`, `newsroom`, and nothing else.
pub fn content_from_words(word: Option<&str>) -> Result<PaneContent, String> {
    match word.unwrap_or("map") {
        "map" => Ok(PaneContent::Map),
        "newsroom" => Ok(PaneContent::Newsroom),
        "tree" => Ok(PaneContent::Tree { organisation: None }),
        other => Err(format!(
            "a pane kind is map, tree or newsroom, got {other:?}"
        )),
    }
}

fn content_json(content: &PaneContent) -> Value {
    match content {
        PaneContent::Map => json!({ "kind": "map" }),
        PaneContent::Newsroom => json!({ "kind": "newsroom" }),
        PaneContent::Tree { organisation } => {
            json!({ "kind": "tree", "organisation": organisation })
        }
    }
}

fn content(object: &Map<String, Value>) -> Result<PaneContent, Refused> {
    let content = object
        .get("content")
        .and_then(Value::as_object)
        .ok_or(Refused(
            "\"content\" is {kind: \"map\" | \"newsroom\" | \"tree\"}",
        ))?;
    let kind = content
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(Refused("\"content\" needs a \"kind\""))?;
    match kind {
        "map" => Ok(PaneContent::Map),
        "newsroom" => Ok(PaneContent::Newsroom),
        "tree" => Ok(PaneContent::Tree {
            organisation: match content.get("organisation") {
                None | Some(Value::Null) => None,
                Some(organisation) => Some(
                    organisation
                        .as_i64()
                        .ok_or(Refused("\"organisation\" is a structure id or null"))?,
                ),
            },
        }),
        _ => Err(Refused("unknown pane kind")),
    }
}

/// **What a client asked for**: a change (an `Intent`), a look at the wall (#181), or a stream of them (#221).
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Intent(Intent),
    Wall,
    /// **The whole arrangement, for a viewer** (#201, #202): what a picture is computed from, so a second screen can
    /// draw the same wall. Not a projection — that is for a controller, and it deliberately leaves the picture out.
    Scene,
    /// **A watch** (#221): the projection, and then a line whenever it changed. Not an intent — nothing on the wall
    /// moves for it — and not a read, because the answer keeps coming. Whether a carrier can carry one is the
    /// carrier's question, not this enum's.
    Watch(Watch),
}

/// **The wall's answer**, in the shape the listener sends back (#165, #181, #221).
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Changed(Result<Changed, Refused>),
    /// **A device has paired** (#194): the row is written and the key travels back with this answer, once.
    Paired(Paired),
    Wall(Wall),
    /// The arrangement a viewer draws (#202), with the `seq` that says how old it is.
    Scene(Box<Scene>),
    /// **A watch was granted** (#221): the one answer to the request, and what follows is the stream.
    Watching(Watch),
    /// **A projection the wall published** (#221): the body a read answers with, and the watch it belongs to.
    ///
    /// The `watch` on the line is what tells a publish from the answer to a request a client has in flight — both
    /// carry a `wall`, and a client that could not tell them apart would take a publish for its read.
    Published(Watch, Wall),
}

/// **One request line** — the version, and the intent inside it.
///
/// The version is compared, not negotiated: a client from another protocol speaks a language the wall does not,
/// and guessing would be a wall two clients disagree about.
pub fn request(line: &str) -> Result<Request, Refused> {
    let value: Value = serde_json::from_str(line).map_err(|_| Refused("not JSON"))?;
    let object = value
        .as_object()
        .ok_or(Refused("a request is a JSON object"))?;
    match object.get("v").and_then(Value::as_u64) {
        Some(v) if v == u64::from(PROTOCOL) => {}
        Some(_) => return Err(Refused("unsupported protocol version")),
        None => return Err(Refused("a request carries \"v\"")),
    }
    // **One of the three, never two** (#181, #221): a line that changes the wall and reads it at once cannot say
    // which answer the client is waiting for, and a watch on the same line would say it twice.
    match (
        object.get("intent"),
        object.get("read"),
        object.get("watch"),
    ) {
        (Some(intent), None, None) => from_wire(intent).map(Request::Intent),
        (None, Some(read), None) => match read.as_str() {
            Some("wall") => Ok(Request::Wall),
            Some("scene") => Ok(Request::Scene),
            _ => Err(Refused("unknown read")),
        },
        // **A word this wall does not carry is a refusal, not a default** (`INV-07`): guessing `wall` for a word a
        // newer wall added would start a stream the client did not ask for.
        (None, None, Some(watch)) => watch
            .as_str()
            .and_then(Watch::of_word)
            .map(Request::Watch)
            .ok_or(Refused("\"watch\" is \"wall\"")),
        (None, None, None) => Err(Refused(
            "a request carries \"intent\", \"read\" or \"watch\"",
        )),
        // The remaining cases are two of the three at once, and the match says so in the order it answers them:
        // the three singles above are exhaustive for one field, so this arm is exactly "more than one".
        (Some(_), _, _) | (_, Some(_), _) => Err(Refused(
            "a request carries \"intent\", \"read\" or \"watch\", not two",
        )),
    }
}

/// **A request as one line** — the reverse of [`request`], so a client builds lines with the same vocabulary the wall
/// reads them with (#202). One encoder, one decoder, and a round-trip test between them.
pub fn request_line(request: &Request) -> String {
    request_line_at(PROTOCOL, request)
}

/// **The same request at another version** (#197): a client speaks the one protocol it was built against, and a
/// test has to be able to be a client from another build. The body is [`request_line`]'s, so the two cannot
/// disagree about anything but the number.
pub fn request_line_at(version: u32, request: &Request) -> String {
    match request {
        Request::Intent(intent) => json!({ "v": version, "intent": to_wire(intent) }).to_string(),
        Request::Wall => json!({ "v": version, "read": "wall" }).to_string(),
        Request::Scene => json!({ "v": version, "read": "scene" }).to_string(),
        Request::Watch(watch) => json!({ "v": version, "watch": watch.word() }).to_string(),
    }
}

/// **A reply line a client cannot use** (#197): the wall's "no", with the kind it gave it, or a line that is not
/// an answer this protocol has.
///
/// The words are enough for a viewer, which only logs them. A *client* decides between retrying, fixing the call and
/// telling its user to update, so it needs the category beside the sentence (#198) — and reading prose to find it is
/// the coupling `RefusalKind` exists to remove. The kind is read through [`RefusalKind::of_word`], the one place the
/// wall's four words are spelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unreadable {
    /// **The wall said no by name** (`INV-07`), and said what kind of no it is (#198).
    Refused(Refusal),
    /// The line is not an answer this protocol has, in this client's words.
    Malformed(String),
}

/// **A refusal as a client reads it**: the rule's name, and the category a client acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The wall's own words — a name in `REFUSALS`, never a paraphrase (`INV-07`).
    pub why: String,
    /// What a client may do about it (#198).
    pub kind: RefusalKind,
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::Refused(refusal) => write!(f, "{} ({})", refusal.why, refusal.kind.word()),
            Unreadable::Malformed(why) => f.write_str(why),
        }
    }
}

/// **One reply line, read back** (#202): a viewer takes the line a wall wrote and turns it into the answer, so neither
/// side has a private spelling.
pub fn answer(line: &str) -> Result<Answer, Unreadable> {
    let value: Value =
        serde_json::from_str(line).map_err(|_| Unreadable::Malformed("not JSON".to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| Unreadable::Malformed("an answer is a JSON object".to_string()))?;
    if object.get("ok").and_then(Value::as_bool) == Some(false) {
        // A refusal the wall named is the wall's own words (`INV-07`), passed on as text: a client does not re-spell
        // it, and the reason a viewer logs is the reason the authority gave.
        let why = object
            .get("why")
            .and_then(Value::as_str)
            .ok_or_else(|| Unreadable::Malformed("a refusal carries \"why\"".to_string()))?;
        // **The category travels beside the words** (#198), and a "no" without one is a wall from another protocol
        // rather than a rule this client could guess an action for.
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .and_then(RefusalKind::of_word)
            .ok_or_else(|| {
                Unreadable::Malformed(format!(
                    "a refusal carries a kind this client knows: {why:?}"
                ))
            })?;
        return Err(Unreadable::Refused(Refusal {
            why: why.to_string(),
            kind,
        }));
    }
    if let Some(paired) = object.get("paired") {
        let name = paired.get("name").and_then(Value::as_str).ok_or_else(|| {
            Unreadable::Malformed("a paired answer carries a device name".to_string())
        })?;
        let key = paired
            .get("key")
            .and_then(Value::as_str)
            .and_then(crate::config::devices::Key::from_hex)
            .ok_or_else(|| Unreadable::Malformed("a paired answer carries a key".to_string()))?;
        return Ok(Answer::Paired(Paired {
            name: name.to_string(),
            key,
        }));
    }
    if let Some(scene) = object.get("scene") {
        let scene: Scene = serde_json::from_value(scene.clone()).map_err(|_| {
            Unreadable::Malformed("the scene is not a scene this client knows".to_string())
        })?;
        return Ok(Answer::Scene(Box::new(scene)));
    }
    // **The watch's two lines** (#221), read before the plain `wall` answer because a publish carries both: the
    // `watch` it belongs to is what tells it from the answer to a read.
    if let Some(watch) = object.get("watch") {
        let watch = watch
            .as_str()
            .and_then(Watch::of_word)
            .ok_or_else(|| Unreadable::Malformed("a watch this client knows".to_string()))?;
        let Some(wall) = object.get("wall") else {
            return Ok(Answer::Watching(watch));
        };
        let wall: Wall = serde_json::from_value(wall.clone()).map_err(|_| {
            Unreadable::Malformed("the wall is not a projection this client knows".to_string())
        })?;
        return Ok(Answer::Published(watch, wall));
    }
    if let Some(wall) = object.get("wall") {
        let wall: Wall = serde_json::from_value(wall.clone()).map_err(|_| {
            Unreadable::Malformed("the wall is not a projection this client knows".to_string())
        })?;
        return Ok(Answer::Wall(wall));
    }
    match object.get("changed").and_then(Value::as_bool) {
        Some(changed) => Ok(Answer::Changed(Ok(Changed(changed)))),
        None => Err(Unreadable::Malformed(
            "an answer carries \"changed\", \"wall\", \"scene\" or \"why\"".to_string(),
        )),
    }
}

/// The reply to one request, as one line.
pub fn reply(answer: Answer) -> String {
    match answer {
        Answer::Changed(Ok(changed)) => {
            json!({ "v": PROTOCOL, "ok": true, "changed": changed.0 }).to_string()
        }
        Answer::Changed(Err(Refused(why))) => {
            // **The words and the category** (#198): a client decides what to do from `kind`, and a person reads `why`.
            // A message with no category is a message nobody classified — `tests/scaffold.rs` refuses that, so this
            // branch is unreachable in a green tree rather than a default nobody notices.
            let mut reply = json!({ "v": PROTOCOL, "ok": false, "why": why });
            if let Some(kind) = super::refusal_kind(why) {
                reply["kind"] = json!(kind.word());
            }
            reply.to_string()
        }
        // **The one time a key crosses the carrier** (#194): the row is already a hash, so this line is the only
        // copy the client will have, and it is not written anywhere (`INV-08`).
        Answer::Paired(paired) => json!({
            "v": PROTOCOL,
            "ok": true,
            "paired": { "name": paired.name, "key": paired.key.hex() },
        })
        .to_string(),
        // **A read is a body, and a good one**: `wall` beside `ok`, and no `changed` — a client that asked to
        // change nothing is not told something did.
        Answer::Wall(wall) => json!({ "v": PROTOCOL, "ok": true, "wall": wall }).to_string(),
        Answer::Scene(scene) => json!({ "v": PROTOCOL, "ok": true, "scene": scene }).to_string(),
        // **The grant says what is being watched, and nothing else** (#221): the projection follows on the stream, so
        // the client is told the stream exists rather than being handed a body it has just read.
        Answer::Watching(watch) => {
            json!({ "v": PROTOCOL, "ok": true, "watch": watch.word() }).to_string()
        }
        // **A publish is a read's body with the watch on it**: one encoder, and a client that has a request in flight
        // can still tell which line is which.
        Answer::Published(watch, wall) => {
            json!({ "v": PROTOCOL, "ok": true, "watch": watch.word(), "wall": wall }).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::vocabulary;

    /// **A round trip is the identity**, for every variant in the vocabulary — the test that keeps the two
    /// halves of this file, and the CLI that builds the same JSON, from drifting apart (#165).
    #[test]
    fn every_intent_survives_the_wire() {
        for intent in vocabulary() {
            let wire = to_wire(&intent);
            let back = from_wire(&wire).unwrap_or_else(|why| {
                panic!(
                    "{} did not survive the wire: {why:?} (as {wire})",
                    intent.verb()
                )
            });
            assert_eq!(back, intent, "{} came back changed", intent.verb());
        }
    }

    /// **The verb in the JSON is `Intent::verb()`**, so a rename of a verb is a red test rather than a client
    /// that speaks a word the wall no longer answers to.
    #[test]
    fn the_wire_speaks_the_intents_own_verbs() {
        for intent in vocabulary() {
            let wire = to_wire(&intent);
            assert_eq!(wire["verb"], json!(intent.verb()));
        }
    }

    /// An unknown verb is a **named refusal**, not a default and not silence (`INV-07`).
    #[test]
    fn an_unknown_verb_is_refused_by_name() {
        let value = json!({ "verb": "pane.teleport", "pane": 1 });
        assert_eq!(from_wire(&value), Err(Refused("unknown verb")));
    }

    /// A known verb with a field missing says which field, and a field of the wrong type says what it should be
    /// — the difference between "you are wrong" and "we disagree about the shape".
    #[test]
    fn a_bad_field_says_which_field() {
        assert_eq!(
            from_wire(&json!({ "verb": "pane.focus" })),
            Err(Refused("\"pane\" is a pane id"))
        );
        assert_eq!(
            from_wire(&json!({ "verb": "pane.focus", "pane": "two" })),
            Err(Refused("an id is a non-negative integer"))
        );
        assert_eq!(
            from_wire(
                &json!({ "verb": "pane.split", "pane": 1, "content": { "kind": "hologram" } })
            ),
            Err(Refused("unknown pane kind"))
        );
    }

    /// **The two verbs a drag settles into have field shapes of their own** (#219), and a shape is what a client is
    /// told: a slot is a word from a closed list and a share is a number, so an edge this wall does not know is a
    /// mistake in the call — `input`, not a rule to wait out — and a missing field says which one is missing.
    #[test]
    fn a_drop_and_a_resize_say_what_a_client_sent() {
        // Round-trips, in both directions, through the shapes `docs/control.md` prints.
        let drop = json!({ "verb": "pane.drop", "pane": 2, "onto": 1, "edge": "top" });
        assert_eq!(
            from_wire(&drop),
            Ok(Intent::Drop {
                pane: 2,
                onto: 1,
                edge: super::super::Edge::Top,
            })
        );
        assert_eq!(
            to_wire(&Intent::Drop {
                pane: 2,
                onto: 1,
                edge: super::super::Edge::Top
            }),
            drop
        );
        let resize = json!({ "verb": "pane.resize", "pane": 1, "other": 2, "ratio": 0.58 });
        assert_eq!(
            from_wire(&resize),
            Ok(Intent::Resize {
                pane: 1,
                other: 2,
                ratio: 0.58,
            })
        );
        assert_eq!(
            to_wire(&Intent::Resize {
                pane: 1,
                other: 2,
                ratio: 0.58,
            }),
            resize,
            "a ratio is a number a client reads back as the one it sent"
        );

        // **An unknown edge is `input`, not `rule`** (#219): the client fixes the word and retries; nothing on the
        // wall has to change first.
        let unknown =
            from_wire(&json!({ "verb": "pane.drop", "pane": 1, "onto": 2, "edge": "north" }))
                .expect_err("north is not an edge");
        assert_eq!(unknown, Refused(super::super::EDGE_WORDS));
        assert_eq!(
            super::super::refusal_kind(unknown.0),
            Some(super::super::RefusalKind::Input),
            "an edge this wall does not use is a call to fix, and a client decides from `kind`"
        );
        // A ratio that is not a number is the same kind of mistake, and the words name the range.
        let bad_ratio =
            from_wire(&json!({ "verb": "pane.resize", "pane": 1, "other": 2, "ratio": "half" }))
                .expect_err("half is not a share");
        assert_eq!(bad_ratio, Refused(super::super::RATIO_WORDS));

        // A missing field names itself, as `pane` does — a client is told which one, not that something was wrong.
        assert_eq!(
            from_wire(&json!({ "verb": "pane.drop", "pane": 1, "edge": "top" })),
            Err(Refused("\"onto\" is a pane id"))
        );
        assert_eq!(
            from_wire(&json!({ "verb": "pane.resize", "pane": 1, "ratio": 0.5 })),
            Err(Refused("\"other\" is a pane id"))
        );
        assert_eq!(
            from_wire(&json!({ "verb": "pane.drop", "pane": 1, "onto": 2 })),
            Err(Refused("\"edge\" is left, right, top, bottom or centre"))
        );
    }

    /// **A read is a request like any other**, and a line carries one or the other — never both (#181).
    #[test]
    fn a_read_is_a_request_and_carries_one_or_the_other() {
        assert_eq!(request(r#"{"v":1,"read":"wall"}"#), Ok(Request::Wall));
        assert_eq!(
            request(r#"{"v":1,"read":"fleet"}"#),
            Err(Refused("unknown read")),
            "a client cannot read what the projection does not carry"
        );
        assert_eq!(
            request(r#"{"v":1,"intent":{"verb":"pane.focus","pane":1},"read":"wall"}"#),
            Err(Refused(
                "a request carries \"intent\", \"read\" or \"watch\", not two"
            ))
        );
        assert_eq!(
            request(r#"{"v":1}"#),
            Err(Refused(
                "a request carries \"intent\", \"read\" or \"watch\""
            ))
        );
    }

    /// The request line carries the version, and the version is compared rather than negotiated.
    #[test]
    fn the_request_line_checks_the_version() {
        let good = r#"{"v":1,"intent":{"verb":"pane.focus","pane":2}}"#;
        assert_eq!(request(good), Ok(Request::Intent(Intent::FocusPane(2))));
        assert_eq!(
            request(r#"{"v":2,"intent":{"verb":"pane.focus","pane":2}}"#),
            Err(Refused("unsupported protocol version"))
        );
        assert_eq!(
            request(r#"{"intent":{"verb":"pane.focus","pane":2}}"#),
            Err(Refused("a request carries \"v\""))
        );
        assert_eq!(request("{"), Err(Refused("not JSON")));
    }

    /// A reply is one line with the version on it, whether it worked or not — a client never has to guess
    /// whether silence meant success. **A refusal carries its category beside its words** (#198).
    #[test]
    fn a_reply_says_whether_it_worked_and_why_in_a_word() {
        let ok = reply(Answer::Changed(Ok(Changed(true))));
        assert_eq!(ok, r#"{"changed":true,"ok":true,"v":1}"#);
        let refused = reply(Answer::Changed(Err(Refused("unknown verb"))));
        assert_eq!(
            refused, r#"{"kind":"input","ok":false,"v":1,"why":"unknown verb"}"#,
            "a client decides from `kind`: `unknown verb` is a call to fix, not a rule to wait out"
        );
        // The three families, each through the one table: the wall's rule, a version to update, a wall to retry.
        for (why, kind) in [
            ("the last pane on the wall", "rule"),
            ("unsupported protocol version", "version"),
            ("the wall has stopped", "busy"),
        ] {
            let line = reply(Answer::Changed(Err(Refused(why))));
            let value: Value = serde_json::from_str(&line).expect("a reply is JSON");
            assert_eq!(value["kind"], json!(kind), "{why} is a {kind}");
        }
    }
    /// **The watch's three lines** (#221): the ask, the grant, and a publish — which is a read's body with the watch
    /// on it, so a client that has a request in flight can tell a publish from the answer it is waiting for.
    ///
    /// The falsifiers are the two shapes that would have been easier: a publish identical to a read's answer (a client
    /// cannot tell them apart, and would take one for the other) and a session that had to parse the relay's frames
    /// itself (a second vocabulary, which is the bug #196 exists to make impossible).
    #[test]
    fn a_watch_asks_is_granted_and_publishes_a_body_a_read_would_answer_with() {
        use super::super::watch::Watch;

        let ask = json!({ "v": 1, "watch": "wall" });
        assert_eq!(
            request(&ask.to_string()),
            Ok(Request::Watch(Watch::Wall)),
            "the ask is a request like any other: a version and one of the three fields"
        );
        assert_eq!(request_line(&Request::Watch(Watch::Wall)), ask.to_string());
        assert_eq!(
            reply(Answer::Watching(Watch::Wall)),
            r#"{"ok":true,"v":1,"watch":"wall"}"#,
            "the grant says which stream is open and carries no projection: the client has just read one"
        );

        let wall = crate::control::projection::of(&crate::ui::State::default());
        let published = reply(Answer::Published(Watch::Wall, wall.clone()));
        assert_eq!(
            answer(&published),
            Ok(Answer::Published(Watch::Wall, wall.clone())),
            "a publish comes back as itself: {published}"
        );
        let read = reply(Answer::Wall(wall.clone()));
        assert_eq!(
            answer(&read),
            Ok(Answer::Wall(wall)),
            "and a read is still a read"
        );
        let value: Value = serde_json::from_str(&published).expect("a publish is JSON");
        assert_eq!(
            value["watch"],
            json!("wall"),
            "a publish belongs to a watch"
        );
        assert!(
            value["wall"].is_object(),
            "and carries the projection a read answers with, not a patch"
        );
        assert!(
            serde_json::from_str::<Value>(&read)
                .expect("a read is JSON")
                .get("watch")
                .is_none(),
            "a read carries no watch: that field is the only thing that tells the two apart (#221)"
        );

        // **A word this wall does not carry is refused by name**, and so is a line that asks for two things.
        assert_eq!(
            request(r#"{"v":1,"watch":"scene"}"#),
            Err(Refused("\"watch\" is \"wall\""))
        );
        assert_eq!(
            request(r#"{"v":1,"watch":5}"#),
            Err(Refused("\"watch\" is \"wall\""))
        );
        assert_eq!(
            request(r#"{"v":1,"watch":"wall","read":"wall"}"#),
            Err(Refused(
                "a request carries \"intent\", \"read\" or \"watch\", not two"
            ))
        );
    }

    /// **A scene travels as a line and comes back as itself** (#202): one encoder, one decoder, and the identity in
    /// between — which is what lets a viewer on another machine draw the wall the authority owns.
    #[test]
    fn a_scene_round_trips_through_the_lines_a_socket_carries() {
        assert_eq!(
            request(&request_line(&Request::Scene)).expect("parses"),
            Request::Scene
        );
        assert_eq!(
            request(&request_line(&Request::Wall)).expect("parses"),
            Request::Wall,
            "the projection and the scene are different reads, and both survive the trip"
        );

        let scene = crate::ui::State::default().scene();
        let line = reply(Answer::Scene(Box::new(scene.clone())));
        match answer(&line).expect("the reply parses") {
            Answer::Scene(back) => assert_eq!(*back, scene, "the scene is the identity: {line}"),
            other => panic!("a scene reply read as {other:?}"),
        }
    }

    /// **A refusal the wall named comes back as the wall's words and its kind**, not as a default and not as a
    /// client's paraphrase (`INV-07`): a client that has to decide between retrying and updating reads the category,
    /// and a person reads the sentence.
    #[test]
    fn a_refusal_travels_with_its_reason_and_its_kind() {
        let line = reply(Answer::Changed(Err(Refused("the last pane on the wall"))));
        assert_eq!(
            answer(&line),
            Err(Unreadable::Refused(Refusal {
                why: "the last pane on the wall".to_string(),
                kind: RefusalKind::Rule,
            })),
            "a client gets the rule's own name and the action it may take (#198)"
        );
    }
}
