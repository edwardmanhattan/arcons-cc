//! **`arcons-cc ctl`** — the second client (#165).
//!
//! ```text
//! arcons-cc ctl pane focus 2
//! arcons-cc ctl pane close
//! arcons-cc ctl workspace new
//! arcons-cc ctl surface close
//! ```
//!
//! The CLI does exactly what a hand does: it builds one [`Intent`], sends it as one line to the socket
//! `src/control.rs` names, and prints the wall's answer. It exits `0` when the wall changed, `1` when the wall
//! refused (the rule's name on stderr), and `2` when there is no wall to talk to — which is what a script wants
//! to branch on without parsing English.
//!
//! **The argv is the vocabulary's nouns and verbs**, joined with a dot: `pane focus` is `Intent::verb()`'s
//! `pane.focus`. Nothing here invents a name; `tests` fails if the two disagree.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use super::{Intent, PROTOCOL, wire};

/// Why a `ctl` run did not answer, in the three kinds a script branches on.
///
/// The distinction is the point: **a refusal is the wall's answer** (exit 1, the rule's name), **no wall is the
/// absence of one** (exit 2, start the display), and a **usage error is ours** (exit 64, the same code the flag
/// parser uses). Collapsing them into one exit code would make "the wall said no" and "there is no wall"
/// indistinguishable to the script that has to decide what to do next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The words were not a command this CLI has.
    Usage(String),
    /// Nothing is listening at the socket.
    NoWall(String),
    /// The wall answered, and said no.
    Refused(String),
}

impl Failure {
    /// The exit code, as `docs/control.md` documents it.
    pub fn code(&self) -> u8 {
        match self {
            Failure::Usage(_) => 64,
            Failure::NoWall(_) => 2,
            Failure::Refused(_) => 1,
        }
    }

    /// What goes to stderr.
    pub fn reason(&self) -> &str {
        match self {
            Failure::Usage(reason) | Failure::NoWall(reason) | Failure::Refused(reason) => reason,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

/// What a client's run concluded, so `main` can exit with a code a script can branch on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The wall changed.
    Changed,
    /// The wall answered, and nothing changed (it was already so).
    Unchanged,
    /// The wall refused, for the reason printed.
    Refused,
    /// There is no wall to ask.
    NoWall,
}

impl Outcome {
    /// The exit code, matching `docs/control.md`: `0` answered, `1` refused, `2` no socket.
    pub fn code(self) -> u8 {
        match self {
            Outcome::Changed | Outcome::Unchanged => 0,
            Outcome::Refused => 1,
            Outcome::NoWall => 2,
        }
    }
}

/// **What a client can be asked to do**: change the wall, or look at it (#181).
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Change(Intent),
    /// `ctl wall`: the projection, printed.
    Wall,
}

/// Run one `ctl` command: parse the words, send it, print the answer.
pub fn run(words: &[String]) -> Result<Outcome, Failure> {
    // **`pane close` with no id** (#181): the one thing a client used to have to look up in the wall's log. It
    // reads the wall, takes the pane that has the keys, and closes that — the same two calls a hand makes, and a
    // refusal (not a guess) when the workspace is empty.
    if words == ["pane", "close"] {
        let wall = wall(&super::socket_path())?;
        let Some(focused) = wall.focused else {
            return Err(Failure::Refused(
                "no pane has the keys: the workspace on screen is empty".to_string(),
            ));
        };
        return send(&super::socket_path(), &Intent::ClosePane(focused));
    }
    match command(words)? {
        Command::Change(intent) => send(&super::socket_path(), &intent),
        Command::Wall => {
            let wall = wall(&super::socket_path())?;
            // The projection is JSON because it *is* one: a client's next step is `jq`, and a table here would
            // be a second shape to keep true.
            let text = serde_json::to_string_pretty(&wall).map_err(|error| {
                Failure::NoWall(format!("the wall's projection is not JSON: {error}"))
            })?;
            println!("{text}");
            Ok(Outcome::Unchanged)
        }
    }
}

/// The words as a command: `wall` is a read, everything else is a change.
pub fn command(words: &[String]) -> Result<Command, Failure> {
    if words == ["wall"] {
        return Ok(Command::Wall);
    }
    parse(words).map(Command::Change)
}

/// **The words as an intent.** `pane focus 2` is `Intent::FocusPane(2)`; every arm reads the arguments the verb
/// needs and says what was missing rather than defaulting (`INV-07`'s rule, at the CLI's edge).
pub fn parse(words: &[String]) -> Result<Intent, Failure> {
    let Some(noun) = words.first() else {
        return Err(Failure::Usage(USAGE.to_string()));
    };
    let Some(verb) = words.get(1) else {
        return Err(Failure::Usage(USAGE.to_string()));
    };
    let rest = &words[2..];
    // The verb the wall knows, spelled the way the wall spells it.
    let name = format!("{noun}.{verb}");
    let arg = |index: usize, what: &str| -> Result<&String, Failure> {
        rest.get(index)
            .ok_or_else(|| Failure::Usage(format!("{name} needs {what}\n\n{USAGE}")))
    };
    let number = |index: usize, what: &str| -> Result<u32, Failure> {
        let word = arg(index, what)?;
        word.parse::<u32>()
            .map_err(|_| Failure::Usage(format!("{name}: {what} is a number, got {word:?}")))
    };
    let intent = match name.as_str() {
        "pane.focus" => Intent::FocusPane(number(0, "a pane id")?),
        "pane.full_size" => Intent::ToggleFullSize(number(0, "a pane id")?),
        // **An id, always**: which pane has the keys is not something a client can read yet (the projection is
        // #168's), so the CLI can name only the panes it opened. `docs/control.md` says so where a reader will
        // look for `pane close` with no argument.
        "pane.close" => Intent::ClosePane(number(0, "a pane id")?),
        "pane.split" => Intent::SplitPane {
            pane: number(0, "a pane id")?,
            content: super::wire::content_from_words(rest.get(1).map(String::as_str))
                .map_err(Failure::Usage)?,
        },
        "pane.open" => Intent::OpenPane(
            super::wire::content_from_words(rest.first().map(String::as_str))
                .map_err(Failure::Usage)?,
        ),
        "pane.move" => Intent::MovePane {
            pane: number(0, "a pane id")?,
            workspace: number(1, "a workspace index")? as usize,
        },
        "pane.select" => {
            let unit = match rest.get(1).map(String::as_str) {
                None | Some("clear") => None,
                Some(id) => Some(id.parse::<i64>().map_err(|_| {
                    Failure::Usage(
                        "pane.select: a hull id is a number, or `clear` to clear it".to_string(),
                    )
                })?),
            };
            Intent::Select {
                pane: number(0, "a pane id")?,
                unit,
            }
        }
        "workspace.focus" => Intent::FocusWorkspace(number(0, "a workspace index")? as usize),
        "workspace.new" => Intent::NewWorkspace,
        "workspace.from_pane" => Intent::SplitToWorkspace(number(0, "a pane id")?),
        "rename.begin" => Intent::BeginRename(target(rest)?),
        "rename.set" => {
            let target = target(rest)?;
            // `rename set workspace 1` with no name clears it, which is a thing an operator means.
            // `rename set workspace 1` with no name clears it, which is a thing an operator means.
            Intent::Rename {
                target,
                name: rest.get(2).cloned(),
            }
        }
        "map.camera" => Intent::Camera {
            pane: number(0, "a pane id")?,
            camera: crate::config::parse_camera(arg(1, "LAT,LON,ZOOM")?).map_err(Failure::Usage)?,
        },
        "cursor.move" => {
            let x = number(0, "an x in points")? as f32;
            let y = number(1, "a y in points")? as f32;
            Intent::CursorMove { at: (x, y) }
        }
        "cursor.press" => Intent::CursorPress,
        "cursor.release" => Intent::CursorRelease,
        "surface.settings" => Intent::OpenSettings,
        "surface.close" => Intent::CloseSurfaces,
        _ => {
            return Err(Failure::Usage(format!("{name} is not a verb\n\n{USAGE}")));
        }
    };
    Ok(intent)
}

/// `pane 2` / `workspace 1` — the same two targets a rename can name.
fn target(words: &[String]) -> Result<crate::ui::RenameTarget, Failure> {
    match (words.first().map(String::as_str), words.get(1)) {
        (Some("pane"), Some(id)) => {
            Ok(crate::ui::RenameTarget::Pane(id.parse::<u32>().map_err(
                |_| Failure::Usage(format!("rename: a pane id is a number, got {id:?}")),
            )?))
        }
        (Some("workspace"), Some(index)) => Ok(crate::ui::RenameTarget::Workspace(
            index.parse::<usize>().map_err(|_| {
                Failure::Usage(format!(
                    "rename: a workspace index is a number, got {index:?}"
                ))
            })?,
        )),
        _ => Err(Failure::Usage(format!(
            "rename needs `pane <id>` or `workspace <index>`\n\n{USAGE}"
        ))),
    }
}

/// **Ask the wall what it looks like** (#181): one line out, one body back.
pub fn wall(path: &std::path::Path) -> Result<super::projection::Wall, Failure> {
    let line = request(path, &serde_json::json!({ "v": PROTOCOL, "read": "wall" }))?;
    let reply: serde_json::Value = serde_json::from_str(&line)
        .map_err(|_| Failure::NoWall(format!("the wall's answer is not JSON: {line:?}")))?;
    if reply.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        let why = reply
            .get("why")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("refused, without a reason");
        return Err(Failure::Refused(why.to_string()));
    }
    serde_json::from_value(
        reply
            .get("wall")
            .cloned()
            .ok_or_else(|| Failure::NoWall("the wall's answer has no \"wall\"".to_string()))?,
    )
    .map_err(|error| Failure::NoWall(format!("the wall's projection is not one: {error}")))
}

/// One line out, one line back. The one place a socket is opened, so the path and the framing are one decision.
fn request(path: &std::path::Path, body: &serde_json::Value) -> Result<String, Failure> {
    let mut socket = UnixStream::connect(path).map_err(|error| {
        Failure::NoWall(format!(
            "no wall at {}: {error}\n  Is the display running? `--self-check` names the socket.",
            path.display()
        ))
    })?;
    socket
        .write_all(body.to_string().as_bytes())
        .and_then(|()| socket.write_all(b"\n"))
        .map_err(|error| Failure::NoWall(format!("could not ask the wall: {error}")))?;
    let mut line = String::new();
    BufReader::new(socket)
        .read_line(&mut line)
        .map_err(|error| Failure::NoWall(format!("could not read the wall's answer: {error}")))?;
    if line.trim().is_empty() {
        return Err(Failure::NoWall(
            "the wall closed the connection without answering".to_string(),
        ));
    }
    Ok(line)
}

/// Send one intent to the wall and read its one-line answer.
pub fn send(path: &std::path::Path, intent: &Intent) -> Result<Outcome, Failure> {
    let line = request(
        path,
        &serde_json::json!({ "v": PROTOCOL, "intent": wire::to_wire(intent) }),
    )?;
    interpret(&line)
}

/// Read the wall's one line: `{"ok":true,"changed":true}` or `{"ok":false,"why":"…"}`.
pub fn interpret(line: &str) -> Result<Outcome, Failure> {
    let reply: serde_json::Value = serde_json::from_str(line)
        .map_err(|_| Failure::NoWall(format!("the wall's answer is not JSON: {line:?}")))?;
    match reply.get("ok").and_then(serde_json::Value::as_bool) {
        Some(true) => Ok(
            if reply.get("changed").and_then(serde_json::Value::as_bool) == Some(true) {
                Outcome::Changed
            } else {
                Outcome::Unchanged
            },
        ),
        Some(false) => Err(Failure::Refused(
            reply
                .get("why")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("refused, without a reason")
                .to_string(),
        )),
        None => Err(Failure::NoWall(format!(
            "the wall's answer has no \"ok\": {line:?}"
        ))),
    }
}

/// What the CLI can be asked, in the words the vocabulary uses.
pub const USAGE: &str = "\
arcons-cc ctl <noun> <verb> [args]

  pane focus <id>            give a pane the keys
  pane full_size <id>        draw it over the wall, or put it back
  pane close [id]            close a pane (the wall's last one is refused). With no id, the
                             pane that has the keys — read from `wall` (#181)
  pane split <id> [kind]     a new pane beside it: map (default), tree, newsroom
  pane open <kind>           the same, beside the focused pane
  pane move <id> <index>     carry a pane into another workspace
  pane select <id> [hull]    put a pane's card on a hull, or `clear`
  workspace focus <index>    make a workspace the wall on screen
  workspace new              append an empty workspace
  workspace from_pane <id>   a workspace of that pane's own
  rename begin pane <id>     open the sheet
  rename begin workspace <i>
  rename set pane <id> <name>
  rename set workspace <i> <name>
  map camera <id> L,L,Z      put a pane's camera at a latitude, longitude and zoom
  cursor move <x> <y>        put a client's cursor at a point in the picture panel
  cursor press               the hull under it (its card), or the pane it is over
  cursor release             let go; the cursor fades from here
  surface settings           open the settings sheet
  surface close              dismiss one surface (what Escape does)

  wall                       print the wall's projection as JSON (workspaces, panes,
                             cameras, focus) — what a client can *read* (#181)

The socket is $XDG_RUNTIME_DIR/arcons-cc/control.sock. Exit 0 answered, 1 refused (the
reason on stderr), 2 no wall. docs/control.md is the reference.
";

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    /// **The argv is the vocabulary**: every verb the CLI accepts is one `Intent::verb()` names, and every
    /// intent the vocabulary has can be reached from the CLI. A verb nobody can type is a control a script
    /// cannot reach; a verb the CLI invents is a word the wall does not answer to.
    #[test]
    fn the_cli_speaks_the_walls_vocabulary() {
        let cases = [
            "pane focus 2",
            "pane full_size 2",
            "pane close 2",
            "pane split 2 map",
            "pane split 2 newsroom",
            "pane open tree",
            "pane move 2 1",
            "pane select 2 512345",
            "workspace focus 1",
            "workspace new",
            "workspace from_pane 2",
            "rename begin pane 2",
            "rename set workspace 0 Straits watch",
            "map camera 2 -6.1,106.8,10",
            "cursor move 100 80",
            "cursor press",
            "cursor release",
            "surface settings",
            "surface close",
        ];
        let mut reached = Vec::new();
        for case in cases {
            let intent = match command(&words(case)) {
                Ok(Command::Change(intent)) => intent,
                other => panic!("{case}: {other:?}"),
            };
            let known: Vec<&'static str> = crate::control::vocabulary()
                .iter()
                .map(Intent::verb)
                .collect();
            assert!(
                known.contains(&intent.verb()),
                "{case} builds {}, which the vocabulary does not declare",
                intent.verb()
            );
            reached.push(intent.verb());
        }
        for verb in crate::control::vocabulary().iter().map(Intent::verb) {
            if verb == "pane.split_at" {
                // A point in a pane's frame is a pointer's, and a CLI has no pointer: the menu sends it, the
                // CLI sends `pane.split`, which is the same change at the pane's own centre.
                continue;
            }
            assert!(
                reached.contains(&verb),
                "{verb} is in the vocabulary and the CLI cannot type it (#165)"
            );
        }
    }

    /// **A round trip through the wire**, from the CLI's words to the wall's parse: what `ctl` builds is what
    /// the listener reads back as the same intent.
    #[test]
    fn the_cli_builds_what_the_wall_parses() {
        for case in [
            "pane focus 2",
            "pane move 2 1",
            "rename set workspace 0 Straits",
            "map camera 2 -6.1,106.8,10.25",
            "surface close",
        ] {
            let intent = parse(&words(case)).expect(case);
            let line =
                serde_json::json!({ "v": PROTOCOL, "intent": wire::to_wire(&intent) }).to_string();
            assert_eq!(
                wire::request(&line),
                Ok(crate::control::wire::Request::Intent(intent.clone())),
                "{case} did not survive the wire"
            );
        }
    }

    /// A bad argv says what was wrong rather than sending something else.
    #[test]
    fn a_bad_command_says_what_is_missing() {
        assert!(parse(&[]).is_err());
        assert!(parse(&words("pane")).is_err());
        assert!(parse(&words("pane teleport 2")).is_err());
        assert!(
            parse(&words("pane focus")).is_err(),
            "a focus with no pane is not a request the wall can guess"
        );
        assert!(parse(&words("pane focus two")).is_err());
    }

    /// The answers a script branches on: changed, unchanged, refused, and a wall that is not there.
    #[test]
    fn an_answer_reads_as_an_outcome() {
        assert_eq!(
            interpret(r#"{"changed":true,"ok":true,"v":1}"#),
            Ok(Outcome::Changed)
        );
        assert_eq!(
            interpret(r#"{"changed":false,"ok":true,"v":1}"#),
            Ok(Outcome::Unchanged)
        );
        assert_eq!(
            interpret(r#"{"ok":false,"v":1,"why":"the last pane on the wall"}"#),
            Err(Failure::Refused("the last pane on the wall".to_string()))
        );
        assert_eq!(Failure::NoWall(String::new()).code(), 2);
        assert_eq!(Failure::Usage(String::new()).code(), 64);
        assert_eq!(Outcome::Changed.code(), 0);
        assert_eq!(Outcome::Refused.code(), 1);
        assert_eq!(Outcome::NoWall.code(), 2);

        // And a wall that is not there is `NoWall`, not an error a caller has to translate.
        let nowhere = PathBuf::from("/nonexistent/arcons-cc/control.sock");
        assert!(send(&nowhere, &Intent::OpenSettings).is_err());
    }
}
