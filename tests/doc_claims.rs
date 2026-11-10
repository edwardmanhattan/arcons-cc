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

/// The commands a block of workflow text runs, whether `run: cmd` or a `run: |` block. A block ends at
/// the first line indented less than the `run:` key that opened it.
fn commands_in(text: &str) -> Vec<String> {
    let mut commands: Vec<String> = Vec::new();
    let mut block_indent: Option<usize> = None;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if let Some(open) = block_indent {
            if trimmed.is_empty() {
                continue;
            }
            if indent <= open {
                block_indent = None;
            } else {
                if !trimmed.starts_with('#') {
                    commands.push(trimmed.to_string());
                }
                continue;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("run:") {
            let value = rest.trim();
            if value == "|" || value == ">" {
                block_indent = Some(indent);
            } else if !value.is_empty() {
                commands.push(value.to_string());
            }
        }
    }
    commands
}

/// The body of one job under `jobs:`: everything between its `  name:` line and the next job's. A job
/// starts with exactly two spaces of indent and a bare name; steps, keys and block scalars are indented
/// further, which is what tells the two apart.
fn job_body<'a>(workflow: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("\n  {name}:");
    let start = workflow.find(&marker)? + marker.len();
    let rest = &workflow[start..];
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end();
        let is_job = trimmed.starts_with("  ")
            && !trimmed.starts_with("   ")
            && trimmed.ends_with(':')
            && trimmed.len() > 4;
        if is_job {
            return Some(&rest[..offset]);
        }
        offset += line.len();
    }
    Some(rest)
}

/// Job names, in file order, by the same rule `job_body` uses.
fn job_names(workflow: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut in_jobs = false;
    for line in workflow.lines() {
        if line.trim_end() == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            break;
        }
        if let Some(name) = line
            .trim_end()
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|name| !name.is_empty() && !name.starts_with(' ') && !name.starts_with('#'))
        {
            names.push(name.to_string());
        }
    }
    names
}

/// **The gate job runs the gate, and nothing else runs the gate.**
///
/// The claim is about divergence: CI must not be able to pass by running a *part* of `scripts/verify.sh`
/// where a human runs all of it. That is why the rule is scoped to the job that gates — the build matrix
/// added in #31 runs `cargo check` on macOS and Windows on purpose, because `verify.sh` is bash and
/// renders pixels, and those platforms have to prove something cheaper instead.
#[test]
fn the_gate_job_runs_the_gate_and_never_its_parts() {
    let workflow = read(".github/workflows/ci.yml");
    let names = job_names(&workflow);
    assert!(
        names.iter().any(|name| name == "verify"),
        "there is no `verify` job in CI: the gate has no home. Jobs found: {names:?}"
    );

    let gate = job_body(&workflow, "verify").expect("the verify job has no body");
    // Setting up the machine is not checking the code: `apt-get` may install libraries, and the gate
    // may be the only thing that runs a tool afterwards.
    let checks: Vec<String> = commands_in(gate)
        .into_iter()
        .filter(|command| {
            ["cargo", "clippy", "rustfmt", "rustup", "scripts/"]
                .iter()
                .any(|part| command.contains(part))
        })
        .collect();
    assert_eq!(
        checks,
        vec!["scripts/verify.sh".to_string()],
        "the `verify` job must run `scripts/verify.sh` verbatim and no part of it separately, got \
         {checks:#?}"
    );

    for name in &names {
        if name == "verify" {
            continue;
        }
        let body = job_body(&workflow, name).expect("a job without a body");
        // Commands, not prose: a comment may name the gate to explain why a job is not the gate.
        assert!(
            !commands_in(body)
                .iter()
                .any(|command| command.contains("scripts/verify.sh")),
            "job {name:?} runs scripts/verify.sh: the gate has one home, the job that also uploads \
             the captures. A platform that cannot run the gate proves something cheaper (#31)."
        );
    }
}

/// The portability matrix (#31) claims macOS and Windows are checked by CI on every push. A matrix in a
/// document that CI no longer runs is the drift this file exists to catch — so the two lists must match.
#[test]
fn the_portability_matrix_is_the_matrix_ci_runs() {
    let workflow = read(".github/workflows/ci.yml");
    let check =
        job_body(&workflow, "check").expect("no `check` job: the build matrix of #31 is gone");

    let mut platforms: Vec<&str> = Vec::new();
    for line in check.lines() {
        if let Some(rest) = line.trim().strip_prefix("os: [") {
            for platform in rest.trim_end_matches([']']).split(',') {
                platforms.push(platform.trim());
            }
        }
    }
    platforms.sort_unstable();
    assert_eq!(
        platforms,
        ["macos-latest", "ubuntu-latest", "windows-latest"],
        "the check job must cover Linux, macOS and Windows (#31)"
    );

    let doc = read("docs/research/portability.md");
    for platform in &platforms {
        assert!(
            doc.contains(&format!("check ({platform})")),
            "docs/research/portability.md does not mention the CI job {platform:?} that proves it"
        );
    }
    for word in ["wasm", "Nix", "Windows", "macOS"] {
        assert!(doc.contains(word), "the matrix must answer for {word}");
    }
}

/// The document quotes the engine's own refusal for Windows. If the vendored build script changes its
/// words, the quote is wrong and the "blocked, with the error" row stops being evidence.
#[test]
fn the_portability_document_quotes_the_engine_where_it_refuses_a_platform() {
    let build = read("vendor/maplibre_native/build.rs");
    let quoted =
        "unsupported target: only linux and macos are currently supported by maplibre-native";
    assert!(
        build.contains(quoted),
        "vendor/maplibre_native/build.rs no longer refuses an unknown target with the sentence \
         docs/research/portability.md quotes. Either the engine learned a platform (good: update the \
         document and the matrix) or the message moved (update the quote)."
    );
    let doc = read("docs/research/portability.md");
    assert!(
        doc.contains(quoted),
        "docs/research/portability.md must quote the engine's refusal verbatim: a blocked row names \
         the error, not an impression (#31)"
    );
    assert!(
        doc.contains("Verdict: no web wall for v0.1"),
        "the wasm question must have a one-line answer (#31)"
    );
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

/// **The interaction table is a claim about code, so the document names every state it covers** (#101).
///
/// `tests/transitions.rs` makes the matrix exhaustive over `Interaction` and `Begin`; this makes the *prose*
/// exhaustive over the same enums. Adding a state to the machine therefore fails until the ADR has a row for
/// it — a table that quietly stops describing the code is worse than no table.
#[test]
fn the_interaction_table_names_every_state() {
    let adr = read("docs/adr/0004-interaction-state.md");
    // Each name appears in the table's own row, which is why they are checked as "\n| **Name** |".
    for state in ["Idle", "Lifted", "Resizing", "Menu", "Renaming", "Settings"] {
        let row = format!("| **{state}** |");
        assert!(
            adr.contains(&row),
            "docs/adr/0004-interaction-state.md has no table row for {state} — the machine has one more \
             state than the table describes (#101)"
        );
    }
    assert!(
        adr.contains("Maximised") && adr.contains("layout"),
        "the ADR must say that a maximised pane is a layout fact and not an interaction (#101): the \
         decision is the reason the table is six rows and not twelve"
    );
    // And the code points at the document, so a reader of `Begin::allowed_from` finds the prose.
    let source = read("src/ui/mod.rs");
    assert!(
        source.contains("docs/adr/0004-interaction-state.md"),
        "`Begin::allowed_from` must name the ADR that is its prose (#101)"
    );
}
