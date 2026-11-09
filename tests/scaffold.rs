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
