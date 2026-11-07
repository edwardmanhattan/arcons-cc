//! **Invariant coverage** — a promise in `docs/invariants.md` with no test is prose.
//!
//! This file walks `docs/invariants.md`, `src/` and `tests/` and asks, for every `INV-nn`: is it
//! cited inside at least one `#[test]` body, or is it on the [`PENDING`] list with the ticket that
//! will do it? Nothing else counts — the citation has to be in the test, because
//! `docs/verification.md` says a red build must name the promise that broke.
//!
//! **How this bites.** `PENDING` is an IOU, not a parking space:
//!
//! - adding an invariant to the document, with neither a test nor an IOU, fails;
//! - landing an invariant's test without deleting its IOU fails (a stale IOU hides a real gap
//!   later);
//! - deleting a landed invariant's test fails, quoting the claim that is no longer proved.
//!
//! Citations in `//` line comments do not count. A comment saying "this is about INV-09" is not a
//! test; an `assert!` message saying `INV-01:` is.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every invariant in `docs/invariants.md`, as `(id, claim)`.
fn documented_invariants() -> Vec<(String, String)> {
    let path = root().join("docs/invariants.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut found = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("| **INV-") else {
            continue;
        };
        let Some((number, rest)) = rest.split_once("**") else {
            continue;
        };
        // The row is: | **INV-nn** | claim | owner | proof |, and `rest` begins after the closing
        // `**`, so the first cell is the empty run before the next `|`.
        let mut cells = rest.split('|');
        let _before = cells.next();
        let claim = cells.next().unwrap_or("").trim().to_string();
        found.push((format!("INV-{number}"), claim));
    }
    assert!(
        !found.is_empty(),
        "docs/invariants.md has no `| **INV-nn** |` rows, so this check would pass vacuously"
    );
    found
}

/// Every `.rs` file in `src/` and `tests/`.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, into: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                walk(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                into.push((path.display().to_string(), text));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root().join("src"), &mut out);
    walk(&root().join("tests"), &mut out);
    assert!(
        !out.is_empty(),
        "no Rust sources found under src/ and tests/"
    );
    out
}

/// Strip `//` line comments, keeping string literals intact.
///
/// The citation that counts lives in an assert message; the one that does not lives in a comment.
fn without_line_comments(text: &str) -> String {
    text.lines()
        .map(|line| {
            let mut in_string = false;
            let bytes: Vec<char> = line.chars().collect();
            let mut out = String::new();
            let mut i = 0;
            while i < bytes.len() {
                let c = bytes[i];
                if c == '"' && (i == 0 || bytes[i - 1] != '\\') {
                    in_string = !in_string;
                }
                if !in_string && c == '/' && bytes.get(i + 1) == Some(&'/') {
                    break;
                }
                out.push(c);
                i += 1;
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The bodies of every `#[test]` function in a source file.
fn test_bodies(text: &str) -> Vec<String> {
    let text = without_line_comments(text);
    let mut bodies = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("#[test]") {
        let attribute_at = from + found;
        let after = attribute_at + "#[test]".len();
        // The body starts at the first `{` after the attribute and ends at its matching `}`.
        let Some(open) = text[after..].find('{').map(|at| after + at) else {
            break;
        };
        let mut depth = 0usize;
        let mut end = None;
        for (index, c) in text[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = end.unwrap_or(text.len());
        bodies.push(text[open..end].to_string());
        from = end;
    }
    bodies
}

fn covered_invariants() -> Vec<String> {
    let mut covered = Vec::new();
    for (_, text) in sources() {
        for body in test_bodies(&text) {
            for number in 1..=99 {
                let id = format!("INV-{number:02}");
                if body.contains(&id) && !covered.contains(&id) {
                    covered.push(id);
                }
            }
        }
    }
    covered
}

/// Invariants with no test yet, and the ticket that lands one.
///
/// Delete a line in the same PR that lands the test. The check fails on a stale entry, on purpose:
/// an IOU that outlives its gap is how the next gap hides.
const PENDING: &[(&str, &str)] = &[("INV-11", "#9")];

#[test]
fn every_invariant_is_either_proved_or_owed_to_a_ticket() {
    let documented = documented_invariants();
    let covered = covered_invariants();

    let claim_of = |id: &str| {
        documented
            .iter()
            .find(|(documented_id, _)| documented_id == id)
            .map(|(_, claim)| claim.clone())
            .unwrap_or_else(|| "<not in docs/invariants.md>".to_string())
    };

    for (id, ticket) in PENDING {
        assert!(
            documented
                .iter()
                .any(|(documented_id, _)| documented_id == id),
            "{id} is owed to {ticket} in tests/invariant_coverage.rs and is not in docs/invariants.md. Invariants are never renumbered: retire one by marking it `retired` instead."
        );
        assert!(
            !covered.contains(&id.to_string()),
            "{id} is still on the PENDING list but a test already cites it: \"{}\".\n  \
             Delete its line from PENDING in the same change — an IOU that outlives its gap is how the next gap hides.",
            claim_of(id)
        );
        assert!(
            ticket.starts_with('#'),
            "{id}'s PENDING entry must name the ticket that will prove it, got {ticket:?}"
        );
    }

    for (id, claim) in &documented {
        let pending = PENDING.iter().any(|(pending_id, _)| pending_id == id);
        let proved = covered.contains(id);
        assert!(
            pending || proved,
            "{id} is written down and neither proved nor owed:\n  \"{claim}\"\n  \
             Write the test that names it in its failure message, or add it to PENDING in tests/invariant_coverage.rs with the ticket that will. \
             A promise with no test is prose, and prose does not go red."
        );
        assert!(
            !(pending && proved),
            "{id} is both covered and pending; delete the PENDING line.\n  \"{claim}\""
        );
    }
}

#[test]
fn the_coverage_scanner_finds_tests_at_all() {
    // If this file's own test list came back empty, every invariant would look unproved for the
    // wrong reason; and a scanner that found nothing would make the check above vacuous.
    let bodies: Vec<String> = sources()
        .into_iter()
        .flat_map(|(_, text)| test_bodies(&text))
        .collect();
    assert!(
        bodies.len() >= 10,
        "the coverage scanner found only {} `#[test]` bodies; it has stopped reading the tree",
        bodies.len()
    );
}
