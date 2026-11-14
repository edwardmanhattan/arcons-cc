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
use super::{Changed, Intent, PROTOCOL, Refused};
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
        Intent::CloseOtherPanes | Intent::CloseOtherWorkspaces => {}
        Intent::OpenPane(content) => {
            object.insert("content".into(), content_json(content));
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
        Intent::CursorMove { at } => {
            object.insert("at".into(), json!([at.0, at.1]));
        }
        Intent::CursorPress | Intent::CursorRelease => {}
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
        "pane.close_others" => Intent::CloseOtherPanes,
        "workspace.close_others" => Intent::CloseOtherWorkspaces,
        "pane.open" => Intent::OpenPane(content(object)?),
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
            }
        }
        "cursor.press" => Intent::CursorPress,
        "cursor.release" => Intent::CursorRelease,
        "surface.settings" => Intent::OpenSettings,
        "surface.close" => Intent::CloseSurfaces,
        _ => return Err(Refused("unknown verb")),
    };
    Ok(intent)
}

fn pane(object: &Map<String, Value>) -> Result<crate::ui::ViewId, Refused> {
    Ok(integer(object, "pane")? as crate::ui::ViewId)
}

fn integer(object: &Map<String, Value>, key: &str) -> Result<i64, Refused> {
    match object.get(key) {
        None => Err(Refused(match key {
            "pane" => "\"pane\" is a pane id",
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

/// **What a client asked for**: a change (an `Intent`), or a look at the wall (#181).
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Intent(Intent),
    Wall,
}

/// **The wall's answer**, in the shape the listener sends back (#165, #181).
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Changed(Result<Changed, Refused>),
    Wall(Wall),
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
    // **One or the other, never both** (#181): a line that changes the wall and reads it at once cannot say
    // which answer the client is waiting for.
    match (object.get("intent"), object.get("read")) {
        (Some(intent), None) => from_wire(intent).map(Request::Intent),
        (None, Some(read)) => match read.as_str() {
            Some("wall") => Ok(Request::Wall),
            _ => Err(Refused("unknown read")),
        },
        (Some(_), Some(_)) => Err(Refused(
            "a request carries \"intent\" or \"read\", not both",
        )),
        (None, None) => Err(Refused("a request carries \"intent\" or \"read\"")),
    }
}

/// The reply to one request, as one line.
pub fn reply(answer: Answer) -> String {
    match answer {
        Answer::Changed(Ok(changed)) => {
            json!({ "v": PROTOCOL, "ok": true, "changed": changed.0 }).to_string()
        }
        Answer::Changed(Err(Refused(why))) => {
            json!({ "v": PROTOCOL, "ok": false, "why": why }).to_string()
        }
        // **A read is a body, and a good one**: `wall` beside `ok`, and no `changed` — a client that asked to
        // change nothing is not told something did.
        Answer::Wall(wall) => json!({ "v": PROTOCOL, "ok": true, "wall": wall }).to_string(),
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
                "a request carries \"intent\" or \"read\", not both"
            ))
        );
        assert_eq!(
            request(r#"{"v":1}"#),
            Err(Refused("a request carries \"intent\" or \"read\""))
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
    /// whether silence meant success.
    #[test]
    fn a_reply_says_whether_it_worked() {
        let ok = reply(Answer::Changed(Ok(Changed(true))));
        assert_eq!(ok, r#"{"changed":true,"ok":true,"v":1}"#);
        let refused = reply(Answer::Changed(Err(Refused("unknown verb"))));
        assert_eq!(refused, r#"{"ok":false,"v":1,"why":"unknown verb"}"#);
    }
}
