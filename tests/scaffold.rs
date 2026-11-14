//! Scaffold-era structural checks: the two rules from #2 that can be checked by reading the tree,
//! and that would otherwise rot silently.
//!
//! These are not invariants (`docs/invariants.md` owns those) — they are the cheap tripwires #2's
//! own `## Proof` line names: "an AGENTS.md that does not point at the verification contract", and
//! "naming and formatting drift". #12 turns the rest of the contract into machinery.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every `.rs` file under `src/`, with its path relative to the crate root.
fn rust_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
            .map(|entry| entry.expect("readable directory entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
                let relative = path
                    .strip_prefix(root())
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                out.push((relative, text));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root().join("src"), &mut out);
    assert!(!out.is_empty(), "no Rust sources found under src/");
    out
}

#[test]
fn agents_md_points_at_the_gate() {
    // A wall display is judged by a command, and the agent that lands the next ticket starts here.
    // An AGENTS.md that has drifted from the contract sends that agent somewhere else.
    let agents = read("AGENTS.md");
    for required in [
        "scripts/verify.sh",
        "docs/verification.md",
        "docs/invariants.md",
        "## Proof",
    ] {
        assert!(
            agents.contains(required),
            "AGENTS.md must name {required:?}: a ticket is done when scripts/verify.sh says so"
        );
    }
}

#[test]
fn only_tokens_names_a_colour_or_a_size() {
    // docs/verification.md rule 3: assert against tokens, not literals. The way that rule survives
    // contact with six more tickets is that the palette lives in exactly one file.
    let mut offenders: Vec<String> = Vec::new();
    for (path, text) in rust_sources() {
        if path == "src/ui/tokens.rs" {
            continue;
        }
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("").trim();
            if code.is_empty() {
                continue;
            }
            let named_colour = ["oklch(", "oklcha(", "Color::", "Color {"]
                .iter()
                .any(|needle| code.contains(needle));
            let named_size = [
                ".padding(",
                ".gap(",
                ".radius(",
                ".size(",
                ".border(",
                ".max_w(",
                ".min_h(",
            ]
            .iter()
            .any(|needle| match code.split_once(needle) {
                Some((_, rest)) => rest.starts_with(|c: char| c.is_ascii_digit()),
                None => false,
            });
            if named_colour || named_size {
                offenders.push(format!("{path}:{}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "only src/ui/tokens.rs may name a colour or a size; these lines do not go through a token:\n  {}",
        offenders.join("\n  ")
    );
}

/// The view reads one clock: [`crate::ui::State::now`]. Nothing in a drawing or change-detection path
/// may read the wall clock, because `poll`'s answer decides when Rune rebuilds the element tree — a
/// real-clock comparison there makes *when* something is drawn depend on the machine's speed, which is
/// how a capture stops being reproducible (#35: a two-second scene drew `4 s old` on a fast machine and
/// `5 s old` on a slow one, and the two-run check caught it as flakiness rather than as a clock).
///
/// The one exception is the origin of the clock itself.
#[test]
fn the_view_reads_no_clock_but_the_frame_clock() {
    for file in ["src/ui/mod.rs", "src/ui/markers.rs", "src/ui/unitcard.rs"] {
        let source = read(file);
        let code = match source.find("#[cfg(test)]") {
            Some(at) => &source[..at],
            None => &source[..],
        };
        for (number, line) in code.lines().enumerate() {
            let code_only = !line.trim_start().starts_with("//");
            let reads_clock = line.contains("Instant::now()") || line.contains("SystemTime::now()");
            if !code_only || !reads_clock {
                continue;
            }
            assert!(
                line.contains("origin:"),
                "{file}:{} reads the wall clock:\n  {line}\n  \
                 Use `State::now()`. If this really is the clock's origin, it must be the `origin:` \
                 line and nowhere else.",
                number + 1
            );
        }
    }
}

/// **A wall's failures go to the log, because the log is the only thing a wall keeps.**
///
/// `--log-file` is how a machine in a cupboard is debugged, and `eprintln!` bypasses it: the line lands on a
/// stderr nobody is reading. `#53` was exactly that — a picture and a register that failed said so to a
/// console that did not exist, and the wall drew symbols with nothing recorded anywhere.
///
/// The display and the reads thread must therefore log through `tracing`. Startup paths that run before the
/// log exists (`platform::init_logging` itself, `main`) are out of scope by construction, not by permission.
#[test]
fn the_walls_failures_are_logged_and_not_printed_to_a_console_nobody_reads() {
    for file in [
        "src/ui/mod.rs",
        "src/ui/markers.rs",
        "src/ui/unitcard.rs",
        "src/data/http.rs",
        "src/data/thread.rs",
        "src/data/mod.rs",
    ] {
        let source = read(file);
        let code = match source.find("#[cfg(test)]") {
            Some(at) => &source[..at],
            None => &source[..],
        };
        for (number, line) in code.lines().enumerate() {
            if line.trim_start().starts_with("//") || line.trim_start().starts_with("///") {
                continue;
            }
            assert!(
                !line.contains("eprintln!"),
                "{file}:{} prints to a console a wall does not have:\n  {line}\n  \
                 Log it with `tracing` — `--log-file` is the only record a deployment keeps (#53).",
                number + 1
            );
        }
    }
}

/// **A handler may not change the wall except through the control plane** (#173, epic #162).
///
/// `ADR-0005` says a client sends intents and the wall renders them; `#170` made that one door
/// (`control::apply`). The day a closure in the view assigns a field "just this once", there are two dialects of
/// state change again and the socket is the one that drifts. So this test reads every event handler the view
/// builds — `El::on_press`, `on_secondary_press`, the `_local` variants, `on_drag`, `on_wheel`, `on_submit`,
/// `on_hover` — and fails when one of them calls a **settled** mutator directly.
///
/// The exceptions are not hidden: they are the handlers' own helper functions, named below with the reason each
/// one is allowed to touch the wall itself. A new helper is a deliberate line in this list, not a quiet bypass.
#[test]
fn no_handler_changes_the_wall_except_through_control() {
    // The wall's settled changes: the mutators `control::apply` calls. A handler calling one of these has gone
    // around the door.
    const SETTLED: &[&str] = &[
        "focus_pane",
        "close_pane",
        "switch_to",
        "add_workspace",
        "maximise",
        "restore_tiling",
        "split_pane_as",
        "split_pane",
        "set_camera_of",
        "select_in",
        "clear_selection",
        "press_marker",
        "press_background",
        "apply_drop",
        "set_ratio",
        "move_pane_to",
        "new_workspace_from_pane",
        "begin",
        "end",
    ];

    // The functions a handler may call instead. Each is either the door itself, a surface the wall anchors to a
    // pointer, a held gesture, or a function that builds an intent and calls the door.
    const HELPERS: &[&str] = &[
        // The door.
        "apply",
        // A surface: the filter panel's toggle and its rows, the menu's trigger, the settings sheet's switch and
        // exits. Each opens or closes a surface the wall draws around a pointer (#111, #140, #82).
        "toggle_filter",
        "toggle_in",
        "open_submenu",
        "toggle_pane_gap",
        "close_settings",
        // The held gestures: a pane in the air, a border dragged, a camera panned, the resizer between panes.
        // Their *result* is an intent (see `drag_map`'s release, #173); the moves themselves are the viewer's.
        "lift",
        "drag_pane",
        "drag_gutter",
        "drag_map",
        "wheel",
        // A press on the water or on a pane: three intents inside (`press_in`), and a menu anchored where the
        // pointer was (`open_menu`).
        "press_in",
        "open_menu",
        // The rename sheet's Enter, which ends the sheet and calls the one function a sent name also uses.
        "commit_rename",
        // A menu row, which builds its intent and applies it (#170).
        "run",
    ];

    let source = read("src/ui/mod.rs");
    // Only the view: the tests below it are allowed to poke whatever they like.
    let body = source
        .split_once("\nmod tests {")
        .map(|(body, _)| body)
        .unwrap_or(&source);

    let mut handlers = 0;
    let mut offenders: Vec<String> = Vec::new();
    for handler in handler_closures(body) {
        // **A layout pass is not an input** (#173): `on_layout` is where the wall learns its own geometry — the
        // panel's size, each pane's rectangle, the frame markers project against (`INV-04`) — and the renderer
        // runs it, not a hand or a socket. The rule is about what a *client* may do.
        if handler.kind.contains("layout") {
            continue;
        }
        handlers += 1;
        for settled in SETTLED {
            let call = format!(".{settled}(");
            if handler.body.contains(&call) {
                offenders.push(format!(
                    "line {}: `{}` calls `{settled}` — that is the door's job (#173); call \
                     `control::apply` with the intent it means",
                    handler.line, handler.kind
                ));
            }
        }
        for called in direct_calls(&handler.body) {
            if !HELPERS.contains(&called.as_str()) {
                offenders.push(format!(
                    "line {}: `{}` calls `{called}(...)`, which is not one of the handlers' helpers. If it \
                     changes the wall, it is a bypass (#173); if it is a surface or a held gesture, add it to \
                     HELPERS with the reason",
                    handler.line, handler.kind
                ));
            }
        }
    }
    assert!(
        handlers > 20,
        "only {handlers} handlers found — the scan is not reading `src/ui/mod.rs` as it is written, so it \
         proves nothing (#173)"
    );
    assert!(
        offenders.is_empty(),
        "handlers that go around the control plane (#173):\n  {}",
        offenders.join("\n  ")
    );
}

/// One `El::on_*` handler: what it was attached with, the closure's argument text, and where it starts.
struct Handler {
    kind: String,
    body: String,
    line: usize,
}

/// Every `.on_* (…)` closure in the view, with its balanced argument.
fn handler_closures(source: &str) -> Vec<Handler> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while let Some(found) = source[i..].find(".on_") {
        let start = i + found;
        let rest = &source[start + 1..];
        let Some(open_in_rest) = rest.find('(') else {
            break;
        };
        // The name is everything between `.on_` and `(`: `press`, `secondary_press_local`, `drag`…
        let name = &rest[..open_in_rest];
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            i = start + 1;
            continue;
        }
        let open = start + 1 + open_in_rest;
        let mut depth = 0usize;
        let mut end = None;
        for (offset, byte) in bytes[open..].iter().enumerate() {
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        out.push(Handler {
            kind: name.to_string(),
            body: source[open..end].to_string(),
            line: source[..start].lines().count(),
        });
        i = end;
    }
    out
}

/// The free functions called in a closure — `apply(state, …)` yes, `state.maximised()` no.
fn direct_calls(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let begin = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &body[begin..i];
            let before = body[..begin].chars().rev().find(|c| !c.is_whitespace());
            let after = body[i..].trim_start();
            // A method call (`state.maximised()`, `args.ok()`) is a read or a builder, not a handler helper, and
            // a path-qualified name (`Intent::OpenPane`, `PaneContent::Tree`) is a value being built.
            let is_method = before == Some('.');
            let is_path = before == Some(':');
            if after.starts_with('(') && !is_method && !is_path && !KEYWORDS.contains(&word) {
                out.push(word.to_string());
            }
            continue;
        }
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

/// Words that look like calls to this scan and are not: the language, and the combinators a closure uses.
const KEYWORDS: &[&str] = &[
    "if", "for", "while", "match", "loop", "move", "let", "return", "Some", "None", "Ok", "Err",
    "vec", "format", "matches", "assert", "std", "rune",
];

/// **The two paths are one picture** (#172, epic #162).
///
/// A scene driven only by intents against the same wall a pointer script drives is only worth capturing if its
/// golden is the pointer's — so the remote golden is that file, byte for byte, and this test says so. Without
/// it the two goldens could drift one `--update-goldens` at a time until the remote scene proved nothing.
#[test]
fn the_remote_twin_s_golden_is_the_pointer_twin_s() {
    let pointer = root().join("fixtures/golden/workspaces-new.png");
    let remote = root().join("fixtures/golden/workspaces-new-remote.png");
    let pointer_bytes = std::fs::read(&pointer)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", pointer.display()));
    let remote_bytes =
        std::fs::read(&remote).unwrap_or_else(|e| panic!("cannot read {}: {e}", remote.display()));
    assert_eq!(
        pointer_bytes.len(),
        remote_bytes.len(),
        "fixtures/golden/workspaces-new-remote.png must be fixtures/golden/workspaces-new.png, and it \
         is not: a client-driven capture whose golden drifted proves nothing (#172)"
    );
    assert!(
        pointer_bytes == remote_bytes,
        "fixtures/golden/workspaces-new-remote.png differs from workspaces-new.png by some byte: the two \
         paths must draw one wall, and the golden is the claim (#172)"
    );
}

/// **Every refusal the code can make has a category, and the table has no extras** (#198).
///
/// The rule this enforces is `INV-07`'s at the newest layer: a refusal is a named rule, so it cannot arrive without
/// being classified — and a category that names nothing is a table row that aged. Both directions, because they fail
/// differently: an unclassified refusal is a client that has to read prose to decide, and a stale row is a category
/// nobody can trust.
///
/// The source is read as text on purpose. `Refused("…")` is a literal everywhere it is made, and a *new* one that
/// nobody classified is exactly the thing that would otherwise be caught by a client in production.
#[test]
fn every_refusal_has_a_category() {
    use std::collections::BTreeSet;

    let mut made: BTreeSet<String> = BTreeSet::new();
    let mut walk = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(path) = walk.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a source file is text");
            // A comment may quote the call (`Refused("…")` in prose): a line that starts with `//` is documentation, not
            // a refusal this wall can send.
            let code: String = text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            let mut rest = code.as_str();
            while let Some(at) = rest.find("Refused(\"") {
                rest = &rest[at + 9..];
                let mut message = String::new();
                let mut chars = rest.chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            if let Some(next) = chars.next() {
                                message.push(next);
                            }
                        }
                        '"' => break,
                        other => message.push(other),
                    }
                }
                made.insert(message);
            }
        }
    }

    let table: BTreeSet<String> = arcons_cc::control::REFUSALS
        .iter()
        .map(|(why, _)| (*why).to_string())
        .collect();
    let missing: Vec<&String> = made.difference(&table).collect();
    assert!(
        missing.is_empty(),
        "these refusals have no category in `control::REFUSALS` (#198): {missing:?}. A client that has to read the \
         words to decide between retry, fix-the-call and update-the-app is a client coupled to a sentence."
    );
    let stale: Vec<&String> = table.difference(&made).collect();
    assert!(
        stale.is_empty(),
        "`control::REFUSALS` names refusals the code no longer makes (#198): {stale:?}. A category for a message \
         nobody sends is a table row that aged."
    );
}
