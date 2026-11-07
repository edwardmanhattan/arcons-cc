//! **Doc drift** — claims that can be checked are checked, so they do not age silently.
//!
//! Three claims, each of which has been true-by-prose in other projects and false-by-code within a
//! month:
//!
//! 1. **Rune is pinned to a rev, and ADR-0001 quotes the same one.** A bump is a deliberate commit,
//!    not a sentence that drifts.
//! 2. **The refusal codes in `realtime-protocol.md` §7 equal the variants in `live::Refusal`.** A
//!    new code upstream is a red build here, before it can become a silent default (`INV-07`).
//! 3. **The map seed path ADR-0001 names exists on disk.** A path in a document that nothing
//!    creates is a capture that fails in six weeks for no obvious reason.
//!
//! Plus the gate's own claim: CI runs `scripts/verify.sh` and never its individual parts.

use std::path::{Path, PathBuf};

use arcons_cc::live::Refusal;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every `rev = "…"` in `Cargo.toml`, which for the Rune pin is the whole claim.
fn pinned_revs(manifest: &str) -> Vec<String> {
    manifest
        .lines()
        .filter(|line| line.contains("git = \"https://github.com/anomalyco/rune\""))
        .filter_map(|line| line.split_once("rev = \"").map(|(_, rest)| rest))
        .filter_map(|rest| rest.split_once('"').map(|(rev, _)| rev.to_string()))
        .collect()
}

#[test]
fn rune_is_pinned_by_rev_and_the_adr_quotes_it() {
    let manifest = read("Cargo.toml");
    let revs = pinned_revs(&manifest);
    assert!(
        !revs.is_empty(),
        "Cargo.toml pins no Rune rev. Rune is pinned by rev, never `branch = \"dev\"` — `dev` moves, and a wall display that silently changes renderer is not verifiable."
    );
    let first = &revs[0];
    for rev in &revs {
        assert_eq!(
            rev, first,
            "rune and rune-kit must move together: found {revs:?}"
        );
    }
    assert_eq!(
        first.len(),
        40,
        "a Rune pin is a full 40-character commit id, got {first:?}"
    );

    let adr = read("docs/adr/0001-rune-and-maplibre.md");
    assert!(
        adr.contains(first.as_str()),
        "docs/adr/0001-rune-and-maplibre.md does not quote the Rune rev pinned in Cargo.toml ({first}).\n  \
         A rev bump is one commit that changes both, and says which revs and why — otherwise the ADR is a sentence that ages."
    );
}

/// The refusal codes in §7 of the pinned realtime protocol: `| \`101\` | … |`.
fn documented_refusal_codes() -> Vec<u16> {
    let doc = read("docs/spec/realtime-protocol.md");
    let section = doc
        .split_once("## 7.")
        .and_then(|(_, rest)| rest.split_once("## 8.").map(|(section, _)| section))
        .expect(
            "docs/spec/realtime-protocol.md has no §7 … §8 pair to read the refusal table from",
        );
    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start().strip_prefix("| `")?;
            let (code, _) = trimmed.split_once('`')?;
            code.parse::<u16>().ok()
        })
        .collect()
}

#[test]
fn the_refusal_codes_in_the_protocol_equal_the_enum() {
    let mut documented = documented_refusal_codes();
    documented.sort_unstable();
    assert_eq!(
        documented.len(),
        6,
        "§7 of the realtime protocol must list six refusal codes (101, 103, 105, 107, 109, 100); found {documented:?}. \
         If upstream added one, add the variant to live::Refusal — a new code must be a compile-visible decision, never a silent default."
    );

    let mut modelled: Vec<u16> = Refusal::ALL.iter().map(|refusal| refusal.code()).collect();
    modelled.sort_unstable();
    assert_eq!(
        modelled, documented,
        "live::Refusal and §7 of docs/spec/realtime-protocol.md disagree about the refusal codes.\n  \
         documented: {documented:?}\n  modelled:   {modelled:?}\n  \
         Fix the enum and its action mapping — and re-vendor docs/spec/ only if the contract itself changed."
    );
}

#[test]
fn the_map_seed_path_in_the_adr_exists() {
    let adr = read("docs/adr/0001-rune-and-maplibre.md");
    let paths: Vec<&str> = adr
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| token.starts_with("assets/") && !token.contains(' '))
        .collect();
    assert!(
        !paths.is_empty(),
        "ADR-0001 must name the map seed path (in backticks, starting `assets/`) so this check can follow it"
    );
    for path in paths {
        assert!(
            Path::new(&root()).join(path).exists(),
            "ADR-0001 names the map seed at `{path}`, and it does not exist on disk.\n  \
             An empty directory is a seed that is not recorded yet (#5); a missing one is a cold start that cannot draw."
        );
    }
}

#[test]
fn ci_runs_the_gate_and_never_its_parts() {
    let workflow = read(".github/workflows/ci.yml");

    // Every command the workflow runs, whether `run: cmd` or a `run: |` block.
    let mut commands: Vec<String> = Vec::new();
    let mut in_block = false;
    for line in workflow.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("run:") {
            let value = rest.trim();
            in_block = value == "|" || value == ">";
            if !in_block && !value.is_empty() {
                commands.push(value.to_string());
            }
            continue;
        }
        if in_block {
            if trimmed.is_empty() || line.starts_with("      ") {
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    commands.push(trimmed.to_string());
                }
            } else {
                in_block = false;
            }
        }
    }

    assert!(
        commands
            .iter()
            .any(|command| command == "scripts/verify.sh"),
        "CI must run `scripts/verify.sh` verbatim. Commands found: {commands:#?}"
    );
    for command in &commands {
        let is_gate = command.trim() == "scripts/verify.sh";
        if command.contains("scripts/verify.sh") && !is_gate {
            panic!(
                "CI runs {command:?}: the gate has one invocation and no flags in CI. \
                 `--fast` skips the captures and the budgets, which is exactly what CI is for."
            );
        }
        for part in ["cargo ", "rustfmt", "clippy", "cargo fmt"] {
            assert!(
                !command.contains(part),
                "CI runs {command:?}, which invokes {part:?} directly.\n  \
                 The point of the gate is that CI and a human run the same command: CI must never run a part of it on its own."
            );
        }
    }
}

/// The wall's binary and the gate's captures are built by different routes, and #26 is why: the local
/// C++ checkout at `-O2` crashes on the first map frame, so the wall takes upstream's amalgam.
///
/// If this fails, `scripts/build-wall.sh` stopped doing one of the two things that make it a wall
/// build, or it went away while the runbook still tells an operator to run it.
#[test]
fn the_wall_build_takes_the_precompiled_route() {
    let script = read("scripts/build-wall.sh");
    assert!(
        script.contains("MLN_PRECOMPILE=1"),
        "scripts/build-wall.sh must select the precompiled core (#26)"
    );
    assert!(
        script.contains("MLN_LOCAL_REPOSITORY= "),
        "scripts/build-wall.sh must **clear** MLN_LOCAL_REPOSITORY: it wins over MLN_PRECOMPILE in the \
         vendored build script, and the local route is the one that segfaults in release (#26)"
    );
    assert!(
        script.contains("--release") && script.contains("--features map"),
        "a wall binary is release and has the map: scripts/build-wall.sh must pass both"
    );
    let runbook = read("docs/runbook.md");
    assert!(
        runbook.contains("scripts/build-wall.sh"),
        "docs/runbook.md must tell the operator how the wall binary is built (§6)"
    );
}
