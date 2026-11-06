//! **Notices drift** — `THIRD-PARTY-NOTICES.md` is a legal document, and legal documents rot.
//!
//! Two halves, and the second is the one an auditor asks for:
//!
//! 1. every **direct** dependency in `Cargo.toml` is named in the notices file (this keeps the file
//!    honest about what we chose);
//! 2. every crate in the dependency graph under a **copyleft** licence (`MPL-*`, `LGPL-*`, `GPL-*`,
//!    `AGPL-*`) is named there too, including the ones we never asked for (this keeps an auditor
//!    from finding one we did not know about).
//!
//! The graph comes from `cargo metadata`, because `Cargo.lock` does not carry licences. It reads the
//! local registry cache; it does not build anything.

use std::process::Command;

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn notices() -> String {
    let path = root().join("THIRD-PARTY-NOTICES.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// `cargo metadata`, parsed with the `serde_json` this crate already depends on.
fn metadata() -> serde_json::Value {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1"])
        .current_dir(root())
        .output()
        .expect("cannot run `cargo metadata`");
    assert!(
        output.status.success(),
        "`cargo metadata` failed: {}\n  The notices check reads the dependency graph; if this is a cold cache, `cargo fetch` fixes it.",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata did not return JSON")
}

/// Crates are named with either `_` or `-`; the notices file is prose, so accept both.
fn named_in(notices: &str, crate_name: &str) -> bool {
    let dashed = crate_name.replace('_', "-");
    let underscored = crate_name.replace('-', "_");
    notices.contains(crate_name) || notices.contains(&dashed) || notices.contains(&underscored)
}

#[test]
fn every_direct_dependency_is_named_in_the_notices() {
    let metadata = metadata();
    let notices = notices();
    let ours = metadata["packages"]
        .as_array()
        .expect("metadata has no packages")
        .iter()
        .find(|package| package["name"] == "arcons-cc")
        .expect("cargo metadata does not know this package");

    let dependencies = ours["dependencies"]
        .as_array()
        .expect("no dependencies list");
    assert!(
        !dependencies.is_empty(),
        "a scaffold with no dependencies is a parser bug, not a fact"
    );

    let mut missing = Vec::new();
    for dependency in dependencies {
        let name = dependency["name"].as_str().unwrap_or_default();
        if !named_in(&notices, name) {
            missing.push(name.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "THIRD-PARTY-NOTICES.md does not name these direct dependencies: {missing:?}.\n  \
         Add a row with its licence and what it is for. The app is proprietary; every component's licence still has to be stated."
    );
}

#[test]
fn every_copyleft_crate_in_the_graph_is_named_in_the_notices() {
    let metadata = metadata();
    let notices = notices();

    let mut copyleft = Vec::new();
    let mut unnamed = Vec::new();
    for package in metadata["packages"]
        .as_array()
        .expect("metadata has no packages")
    {
        let name = package["name"].as_str().unwrap_or_default();
        let Some(license) = package["license"].as_str() else {
            continue;
        };
        let copyleft_here = ["MPL-", "LGPL-", "GPL-", "AGPL-"]
            .iter()
            .any(|family| license.contains(family));
        if !copyleft_here {
            continue;
        }
        copyleft.push(format!("{name} ({license})"));
        if !named_in(&notices, name) {
            unnamed.push(format!("{name} {license}"));
        }
    }

    assert!(
        unnamed.is_empty(),
        "THIRD-PARTY-NOTICES.md does not name these copyleft crates: {unnamed:?}.\n  \
         A copyleft dependency is allowed, an unrecorded one is not: name it, say which licence, and say whether the project takes it or the dual licence."
    );
    // If the graph ever loses every copyleft crate, the notices file keeps a table of MPL-2.0
    // crates that no longer exist. That is drift too, and this is where it surfaces.
    assert!(
        !copyleft.is_empty(),
        "the dependency graph now has no copyleft crate at all. If Rune's audio features were dropped, shrink the copyleft table in THIRD-PARTY-NOTICES.md in the same change — an entry for a crate that is gone is drift, and the next reader has to check the lockfile to know it."
    );
}

#[test]
fn the_notices_state_that_the_app_is_proprietary() {
    let notices = notices();
    // Say it in the file, not only in the README: whoever finds this file is looking for a licence.
    let lower = notices.to_lowercase();
    assert!(
        lower.contains("proprietary"),
        "THIRD-PARTY-NOTICES.md must say the application is proprietary"
    );
    assert!(
        lower.contains("not licensed for redistribution"),
        "THIRD-PARTY-NOTICES.md must say the application is not licensed for redistribution: there is no root LICENSE on purpose, and saying so here stops someone adding one"
    );
}
