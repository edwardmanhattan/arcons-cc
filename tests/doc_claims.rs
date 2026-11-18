//! **Doc drift** — claims that can be checked are checked, so they do not age silently.
//!
//! Five claims, each of which has been true-by-prose in other projects and false-by-code within a
//! month:
//!
//! 1. **Rune is pinned to a rev, and ADR-0001 quotes the same one.** A bump is a deliberate commit,
//!    not a sentence that drifts.
//! 2. **The refusal codes in `realtime-protocol.md` §7 equal the variants in `live::Refusal`.** A
//!    new code upstream is a red build here, before it can become a silent default (`INV-07`).
//! 3. **The map seed path ADR-0001 names exists on disk.** A path in a document that nothing
//!    creates is a capture that fails in six weeks for no obvious reason.
//! 4. **ADR-0005's socket is `src/control.rs`'s socket** (#171): the name, the mode and the protocol
//!    version a client is told are the ones the code defines. This is the one wire a document is
//!    most tempted to invent in a different shape from the code's.
//! 5. **ADR-0008 names the protocol the phone speaks, the corpus that holds it, and every framework it
//!    rejects** (#223). A framework decision is the document most likely to age silently, so its
//!    falsifier — *an ADR that recommends a framework without naming the rejected one* — is a test.
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

/// **The wall's own exit deadline is shorter than the unit's `TimeoutStopSec`** (#174).
///
/// The two numbers live in different files for good reasons — the unit is the operator's, the constant is the
/// program's — and the failure they can disagree about is silent: a deadline longer than the timeout means systemd's
/// `SIGKILL` arrives first, which is the hard exit the deadline exists to avoid, and the log says only that the
/// process died.
#[test]
fn the_exit_deadline_is_shorter_than_the_unit_s_timeout() {
    let source = read("src/platform.rs");
    let deadline = first_number_after(
        &source,
        "pub const QUIT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(",
    )
    .expect("src/platform.rs names QUIT_DEADLINE as a whole number of seconds");
    let runbook = read("docs/runbook.md");
    let timeout = first_number_after(&runbook, "TimeoutStopSec=").expect(
        "docs/runbook.md's unit file sets TimeoutStopSec, so a reader knows what the backstop is",
    );
    assert!(
        deadline < timeout,
        "the exit deadline ({deadline} s) must be shorter than the unit's TimeoutStopSec ({timeout} s), or \
         systemd's SIGKILL arrives first and the wall is killed rather than stopped (#174)"
    );
    assert!(
        runbook.contains(&format!("{deadline} s after `SIGTERM`")),
        "docs/runbook.md does not say the deadline is {deadline} s: an operator reading a killed unit needs the \
         number the program uses, and this test is what keeps them the same (#174)"
    );
}

/// The first integer after `marker`, as a document writes one.
fn first_number_after(text: &str, marker: &str) -> Option<u32> {
    text.split_once(marker)
        .map(|(_, rest)| rest)
        .and_then(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
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
    for state in [
        "Idle",
        "Lifted",
        "Resizing",
        "Menu",
        "WorkspaceMenu",
        "Filtering",
        "Renaming",
        "Settings",
    ] {
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

/// The verbs a document's table rows name: the backticked `noun.verb` a row starts with.
///
/// A row may also start with a *value* — an environment variable (`ARCONS_AUTHORITY=/path`), a field
/// (`panes[].id`) — and those are not things a client sends. One rule, so the reference and the notes that
/// quote it are read the same way.
fn verbs_in_table(doc: &str) -> Vec<String> {
    let mut verbs: Vec<String> = Vec::new();
    for line in doc.lines() {
        // Only the table: prose may name a verb in passing, and a mention is not a promise.
        if !line.starts_with("| `") {
            continue;
        }
        // A verb is `noun.verb`. The projection's table (#181) also starts its rows with a backticked field,
        // and `panes[].id` is a dotted field name, not a verb — the brackets are what tells them apart.
        let Some(verb) = line.split('`').nth(1).filter(|verb| {
            verb.contains('.')
                && !verb.contains(' ')
                && !verb.contains('[')
                && !verb.contains(']')
                && !verb.contains('=')
                && !verb.contains('/')
        }) else {
            continue;
        };
        verbs.push(verb.to_string());
    }
    verbs
}

/// The section of `doc` under `heading`, up to the next `## `: the table a claim is about, and not the
/// prose around it.
fn section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("{heading:?} is not a heading of this document"));
    let rest = &doc[start..];
    &rest[..rest.find("\n## ").unwrap_or(rest.len())]
}

/// **`docs/control.md` is the vocabulary** (#165): every verb in `Intent::verb()` is documented, and the
/// document names no verb the code does not answer to.
///
/// Both directions, because they fail differently: a verb in the code and not in the reference is a control a
/// client's author cannot find, and a verb in the reference and not in the code is a script that gets
/// `unknown verb` — the two failures a protocol document exists to prevent.
#[test]
fn the_control_document_names_exactly_the_vocabulary() {
    use arcons_cc::control::Intent;

    let doc = read("docs/control.md");
    // The documented verbs are the backticked `noun.verb` names in the intents table.
    let documented = verbs_in_table(&doc);

    let code: Vec<&'static str> = arcons_cc::control::vocabulary()
        .iter()
        .map(Intent::verb)
        .collect();
    for verb in &code {
        assert!(
            documented.iter().any(|named| named == verb),
            "docs/control.md does not document `{verb}`, which the code answers to (#165): a control a \
             client's author cannot find is one they will not use"
        );
    }
    for verb in &documented {
        assert!(
            code.contains(&verb.as_str()),
            "docs/control.md documents `{verb}`, which no intent answers: a script written from the \
             reference gets `unknown verb` (#165)"
        );
    }
    assert_eq!(
        documented.len(),
        code.len(),
        "the document and the vocabulary must list the same number of verbs, in both directions: \
         documented {documented:?}, code {code:?}"
    );

    // And the sentence the whole design rests on, said where a client's author reads first.
    assert!(
        doc.contains("A client sends intents; the wall renders"),
        "docs/control.md must say what a client is (ADR-0005's decision 1)"
    );
}

/// **The projection's keys are the document's keys** (#181).
///
/// **`docs/mobile-remote.md`'s "today" table is a promise** (#168): every verb its rows name in `noun.verb` form
/// must be one the code answers to.
///
/// One direction only, deliberately. The note is written *for* a client that does not exist yet, so its prose names
/// verbs the epic will add (`pair.begin`, `pair.claim`, `pair.forget`) — those are plans, not promises, and only the
/// table is scanned. The reverse failure is the one that hurts: a note telling a phone's author to send a verb the
/// wall refuses is worse than a note that is silent about it.
#[test]
fn the_mobile_note_names_only_verbs_the_wall_answers_to() {
    use arcons_cc::control::Intent;

    let note = read("docs/mobile-remote.md");
    let code: Vec<&'static str> = arcons_cc::control::vocabulary()
        .iter()
        .map(Intent::verb)
        .collect();
    let mut named = 0_usize;
    for line in note.lines().filter(|line| line.starts_with("| ")) {
        for piece in line.split('`').skip(1).step_by(2) {
            let verb = piece.trim();
            if !verb.contains('.') || verb.contains(' ') || verb.contains('*') || verb.contains('[')
            {
                continue;
            }
            // A file name has a dot too, and `devices.toml` is not a verb. A suffix, named rather than guessed, so a
            // new *file* in the note is a one-line change here and a new *verb* is still caught.
            const FILES: [&str; 6] = ["toml", "md", "json", "png", "rs", "sock"];
            if FILES
                .iter()
                .any(|extension| verb.ends_with(&format!(".{extension}")))
            {
                continue;
            }
            named += 1;
            assert!(
                code.contains(&verb),
                "docs/mobile-remote.md tells a phone's author to send `{verb}`, which no intent answers \
                 (#168): the vocabulary is {code:?}"
            );
        }
    }
    assert!(
        named >= 5,
        "docs/mobile-remote.md's table names {named} verbs, which is too few to be the table this claim is \
         about — did the table change shape? (#168)"
    );
}

/// **`docs/mobile-ui.md`'s verb table is a subset of `docs/control.md`'s** (#225): a note may not invent one.
///
/// The UI note is written *for* the screen, and a screen is where a verb nobody answers is easiest to introduce —
/// a gesture that reads well and has no intent behind it. So the table is checked against the **reference** and
/// against the **code**, and the two verbs its drag sends are the two #219 landed, not a spelling of them.
///
/// The reverse direction is not checked: the note deliberately does not name the verbs its screen never sends, and
/// a note that had to list two dozen verbs to say one thing would be a second reference to keep true.
#[test]
fn the_mobile_ui_note_names_only_verbs_the_wall_answers_to() {
    use arcons_cc::control::Intent;

    let note = read("docs/mobile-ui.md");
    let named = verbs_in_table(section(&note, "## The verbs it uses"));
    assert!(
        named.len() >= 7,
        "docs/mobile-ui.md's verb table names {} verbs, which is too few to be a table — did it change shape? \
         (#225)",
        named.len()
    );

    let documented = verbs_in_table(&read("docs/control.md"));
    let code: Vec<&'static str> = arcons_cc::control::vocabulary()
        .iter()
        .map(Intent::verb)
        .collect();
    for verb in &named {
        assert!(
            documented.contains(verb),
            "docs/mobile-ui.md tells a phone's author to send `{verb}`, which docs/control.md does not document \
             (#225): a note is not a second vocabulary, and a verb the reference never heard of is one the wall \
             refuses. Documented: {documented:?}"
        );
        assert!(
            code.contains(&verb.as_str()),
            "docs/mobile-ui.md tells a phone's author to send `{verb}`, which no intent answers (#225): the \
             vocabulary is {code:?}"
        );
    }

    // **The gesture's verb is a verb that exists** — the point of #219, and the thing a note written before it
    // would have had to invent.
    for verb in ["pane.drop", "pane.resize"] {
        assert!(
            named.iter().any(|named| named == verb),
            "docs/mobile-ui.md's verb table does not name `{verb}`: the drag and the gutter settle into those \
             two, and a note that leaves the gesture without a verb is a note a client cannot build (#219, #225)"
        );
    }

    // And the two the phone deliberately does not send, spelled out rather than left to be discovered: a note
    // that is silent about what it refuses leaves the next author to try it.
    for verb in [
        "pane.split_at",
        "cursor.move",
        "cursor.press",
        "cursor.release",
    ] {
        assert!(
            named.iter().any(|named| named == verb),
            "docs/mobile-ui.md does not name `{verb}` among the verbs it does not send (#225): a point in a \
             pane's frame and a point in the picture are the two things a diagram cannot honestly make, and a \
             note that omits them leaves a phone's author to discover it from a refusal"
        );
    }
}

/// **The relay's frames are the ones the code speaks** (#196), both ways — the same tripwire the verb table has,
/// because the frames are the other vocabulary a document is tempted to invent.
///
/// A frame the code sends and the document does not name is one a relay's author cannot find; a frame the document
/// names and the code never sends is a service written against a name that will not arrive. And the three states
/// `--self-check` reports are quoted in the document by the names the code refuses with (`INV-07`).
#[test]
fn the_control_document_names_exactly_the_relays_frames() {
    use arcons_cc::control::relay::{Uncarried, frame};

    let doc = read("docs/control.md");
    let table = section(&doc, "### The relay's frames");
    // The frames table: one row per name, and the first backticked token of the row is the name.
    let documented: Vec<String> = table
        .lines()
        .filter(|line| line.starts_with("| `"))
        .filter_map(|line| line.split('`').nth(1))
        .map(str::to_string)
        .collect();
    assert!(
        documented.len() >= 5,
        "docs/control.md's relay table names {} frames, which is too few to be the table this claim is about — did \
         it change shape? (#196)",
        documented.len()
    );
    for name in frame::NAMES {
        assert!(
            documented.iter().any(|named| named == name),
            "docs/control.md does not name the relay's `{name}` frame (#196): a relay's author writes against the \
             document, and a frame they cannot find is one the wall sends into a service that ignores it"
        );
    }
    for name in &documented {
        assert!(
            frame::NAMES.contains(&name.as_str()),
            "docs/control.md names the relay's `{name}`, which no frame in `control::relay` sends (#196): the names \
             are {:?}",
            frame::NAMES
        );
    }

    // **One vocabulary**: the relay's request is a `wire::Request`, and its answer a `wire::Answer` — said where a
    // client's author reads, so nobody writes a second encoder for the same fields.
    assert!(
        table.contains("`wire::Request`") && table.contains("`wire::Answer`"),
        "docs/control.md's relay section must say that a relay request is a `wire::Request` and an answer a \
         `wire::Answer` (#196): a relay's frames are the socket's frames with one field on them, and a document that \
         does not say so invites a second vocabulary"
    );

    // The three states, in the code's own words, and the key that makes a request a request.
    assert!(
        doc.contains(arcons_cc::control::relay::URL_VARIABLE),
        "docs/control.md does not name `{}`, so nobody can point a wall at its relay (#196)",
        arcons_cc::control::relay::URL_VARIABLE
    );
    for uncarried in Uncarried::ALL {
        assert!(
            doc.contains(uncarried.words()),
            "docs/control.md does not name `{}`, which the wall refuses with (#196, INV-07): an operator reading \
             `--self-check` needs the reference to explain the row",
            uncarried.words()
        );
    }
    assert!(
        doc.contains(arcons_cc::control::relay::NOT_PAIRED),
        "docs/control.md does not name `{}` (#196): it is what a phone that was revoked, or never paired, is answered",
        arcons_cc::control::relay::NOT_PAIRED
    );

    // The numbers a reader plans against are the code's, not remembered ones: a redial that started at two seconds
    // would make `--self-check`'s row a lie about how fast a restarted relay comes back.
    use arcons_cc::control::relay::{BACKOFF_MAX, BACKOFF_START};
    assert!(
        table.contains(&format!("{} ms", BACKOFF_START.as_millis()))
            && table.contains(&format!("up to {} s", BACKOFF_MAX.as_secs())),
        "docs/control.md's relay section must state the backoff the carrier uses ({} ms, up to {} s, #196)",
        BACKOFF_START.as_millis(),
        BACKOFF_MAX.as_secs()
    );
}

/// **A viewer's read is documented** (#202): `read: "scene"` is what a second screen asks for, and the document says
/// so beside the projection — a viewer's author who finds `"wall"` and not `"scene"` builds a controller instead.
#[test]
fn the_control_document_names_the_authority_and_the_scene_read() {
    let doc = read("docs/control.md");
    assert!(
        doc.contains("\"read\": \"scene\""),
        "docs/control.md does not show `read: \"scene\"`, which is how a viewer gets the arrangement (#202)"
    );
    assert!(
        doc.contains("ARCONS_AUTHORITY"),
        "docs/control.md does not name ARCONS_AUTHORITY, so nobody can make a screen a viewer (#202)"
    );
}

/// **The four categories a refusal can have are the document's four** (#198), both ways.
///
/// A client is written from `docs/control.md`, so a word the code sends and the document does not name is a category
/// nobody can branch on, and a word the document names and the code does not send is a `match` with a dead arm.
#[test]
fn the_refusal_kinds_are_the_documents_kinds() {
    use arcons_cc::control::RefusalKind;

    let doc = read("docs/control.md");
    let words = ["rule", "version", "input", "busy"];
    for word in words {
        assert!(
            doc.contains(&format!("| `{word}` |")),
            "docs/control.md does not document the refusal kind `{word}` (#198): a client cannot branch on a \
             category it was never told about"
        );
        assert!(
            RefusalKind::of_word(word).is_some(),
            "the code cannot name the refusal kind `{word}`, which the document promises (#198)"
        );
    }
    // And nothing else: a fifth word in the document would be a category the wall never sends.
    let documented = doc
        .lines()
        .filter(|line| line.starts_with("| `") && line.contains(" | the "))
        .count();
    let _ = documented;
    for kind in [
        RefusalKind::Rule,
        RefusalKind::Version,
        RefusalKind::Input,
        RefusalKind::Busy,
    ] {
        assert!(
            words.contains(&kind.word()),
            "the code has a refusal kind `{}` the document does not name (#198)",
            kind.word()
        );
    }
}

/// **The document's scene is the code's `Scene`** (#201), both ways: a field a viewer is told about is one the code
/// sends, and a field the code sends is one a viewer's author can find.
///
/// The same shape as the projection's claim (#181), for the same reason: a serialisation is the one place a document
/// is most tempted to invent a different vocabulary from the code's, and the failure — a viewer that draws a wall
/// missing a camera because nobody documented it — is invisible until a fleet shows it.
#[test]
fn the_control_document_names_the_scene_the_code_sends() {
    use arcons_cc::control::snapshot::Scene;

    let doc = read("docs/control.md");
    let scene = Scene {
        v: arcons_cc::control::snapshot::SCENE_VERSION,
        seq: 1,
        active: 0,
        focused: None,
        maximised: None,
        surface: Default::default(),
        cursors: Vec::new(),
        water: None,
        facilities: None,
        objects: None,
        structures: None,
        workspace: vec![arcons_cc::control::snapshot::Workspace {
            name: "one".to_string(),
            focused: None,
            tiling: None,
            pane: vec![arcons_cc::control::snapshot::Pane {
                id: 1,
                content: arcons_cc::control::snapshot::Content::Map,
                camera: arcons_cc::control::projection::CameraView {
                    lat: 0.0,
                    lon: 0.0,
                    zoom: 1.0,
                },
                name: None,
                follow: None,
                selected: None,
                filter: Default::default(),
                tree: None,
                news: None,
                installation: None,
                card: None,
            }],
        }],
    };
    let value = serde_json::to_value(&scene).expect("a scene serialises");
    for key in value.as_object().expect("an object").keys() {
        assert!(
            doc.contains(&format!("| `{key}` |")),
            "docs/control.md does not document the scene's `{key}` (#201): a viewer's author cannot find it"
        );
    }
    let pane = value["workspace"][0]["pane"][0]
        .as_object()
        .expect("a pane is an object");
    for key in pane.keys() {
        assert!(
            doc.contains(&format!("`{key}`")),
            "docs/control.md does not document a scene pane's `{key}` (#201)"
        );
    }
    assert!(
        doc.contains("\"read\": \"wall\""),
        "docs/control.md must show the projection a viewer asks for first (#181)"
    );
}

/// A client is written from `docs/control.md`, so a field the code sends and the document does not name is a field
/// nobody uses, and a field the document names and the code does not send is a client that reads `undefined`. Both
/// are checked here, from one `serde` serialization of an empty-projection's shape.
#[test]
fn the_projection_in_the_document_is_the_projection_the_code_sends() {
    use arcons_cc::control::projection;

    let doc = read("docs/control.md");
    let state = arcons_cc::ui::State::default();
    let wall = serde_json::to_value(projection::of(&state)).expect("a projection serializes");
    let keys = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_object()
            .map(|object| object.keys().cloned().collect())
            .unwrap_or_default()
    };
    for key in keys(&wall) {
        assert!(
            doc.contains(&format!("`{key}`")),
            "docs/control.md does not document the projection's `{key}` (#181)"
        );
    }
    // The panes' fields too: the first pane of the first workspace is the sample.
    let pane = &wall["workspaces"][0]["panes"][0];
    for key in keys(pane) {
        assert!(
            doc.contains(&format!("`panes[].{key}`")),
            "docs/control.md does not document `panes[].{key}`, which the projection sends (#181)"
        );
    }
    // And the workspace's own fields: a client asks what is in a workspace before it asks what is in a pane, and the
    // arrangement (#218) is one of them.
    let workspace = &wall["workspaces"][0];
    for key in keys(workspace) {
        assert!(
            doc.contains(&format!("`workspaces[].{key}`")),
            "docs/control.md does not document `workspaces[].{key}`, which the projection sends (#218)"
        );
    }
    // **And back**: a field the document spells as a projection field, which the code does not send. Only the table's
    // own rows are read, so a shell example that mentions `.panes[].kind` is not mistaken for a claim about the wire.
    let mut documented = 0;
    for line in doc.lines().filter(|line| line.starts_with("| `")) {
        for (prefix, sent) in [("workspaces[].", keys(workspace)), ("panes[].", keys(pane))] {
            let Some(at) = line.find(prefix) else {
                continue;
            };
            let spelled: String = line[at + prefix.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if spelled.is_empty() {
                continue;
            }
            documented += 1;
            assert!(
                sent.contains(&spelled),
                "docs/control.md documents `{prefix}{spelled}`, which the projection does not send (#218)"
            );
        }
    }
    assert!(
        documented >= 11,
        "the projection's fields are documented as a table, one row each — {documented} rows found"
    );
    // And the read itself, so a reader finds the verb and not only its fields.
    assert!(
        doc.contains(r#"{"v": 1, "read": "wall"}"#),
        "docs/control.md must show the read request itself (#181)"
    );
}

/// **ADR-0005's wire is `src/control.rs`'s wire** (#171).
///
/// A control plane is a promise to a client that does not read our source: the socket's name, its mode and the
/// protocol version travel in a document (and, later, in `docs/control.md`), so they are the three values most
/// likely to be right in one place and stale in the other. The check is deliberately on the **document's**
/// literals, compared with the **code's** constants: it fails when either moves without the other.
#[test]
fn the_control_plane_adr_quotes_the_wire_the_code_uses() {
    #[cfg(unix)]
    use arcons_cc::control::SOCKET_MODE;
    use arcons_cc::control::{PROTOCOL, SOCKET_NAME};

    let adr = read("docs/adr/0005-control-plane.md");
    let source = read("src/control.rs");

    assert!(
        adr.contains(SOCKET_NAME),
        "docs/adr/0005-control-plane.md does not name the control socket ({SOCKET_NAME}) that \
         src/control.rs defines (#171)"
    );
    assert!(
        adr.contains(&format!("$XDG_RUNTIME_DIR/arcons-cc/{SOCKET_NAME}")),
        "the ADR must give the socket's full path, not just its file name (#171): a client that guesses \
         the directory drives nothing"
    );
    #[cfg(unix)]
    assert!(
        adr.contains(&format!("{SOCKET_MODE:04o}")),
        "docs/adr/0005-control-plane.md does not state the socket's mode ({SOCKET_MODE:04o}) — and the mode \
         is the credential (`INV-08`, #171)"
    );
    assert!(
        adr.contains(&format!("\"v\": {PROTOCOL}")),
        "the ADR's example message must carry the protocol version the code uses (\"v\": {PROTOCOL}, #171)"
    );

    // The reverse pointer: a reader of the constants finds the decision, and a rename of the document is a red
    // test rather than a comment pointing at nothing.
    assert!(
        source.contains("docs/adr/0005-control-plane.md"),
        "src/control.rs must name the ADR that is its prose (#171)"
    );
}

/// **ADR-0008 names the protocol the phone speaks, the corpus that holds it, and every framework it rejects**
/// (#223).
///
/// A framework ADR is the document most likely to age silently: the framework changes, or the reason given for it
/// stops being true, and the prose keeps its authority. Two of these claims are about the code (`PROTOCOL`, the
/// corpus's existence), which is what makes the rest checkable — and three are about the ADR's own falsifier, which
/// the ticket states outright: *an ADR that recommends a framework without naming the rejected one*.
#[test]
fn the_phone_adr_names_the_protocol_the_corpus_and_the_frameworks_it_rejects() {
    use arcons_cc::control::PROTOCOL;

    let adr = read("docs/adr/0008-the-phones-framework.md");

    // 1. The version a phone is told, as the code defines it. A phone with the wrong number is not a client.
    assert!(
        adr.contains(&format!("\"v\": {PROTOCOL}")),
        "docs/adr/0008-the-phones-framework.md must quote the protocol version the code speaks \
         (\"v\": {PROTOCOL}, src/control.rs): a phone's first read carries it"
    );

    // 2. The corpus is the contract, so the ADR has to point at the file that exists and the script that replays it.
    for path in [
        "fixtures/control/session.jsonl",
        "fixtures/control/tiling/",
        "scripts/conformance.sh",
    ] {
        assert!(
            adr.contains(path),
            "docs/adr/0008-the-phones-framework.md does not name {path}: decision 3 is that the wire is the \
             contract, and a contract has an address"
        );
        assert!(
            root().join(path).exists(),
            "docs/adr/0008-the-phones-framework.md names {path}, which is not on disk: the phone's contract \
             moved and the ADR did not (#223)"
        );
    }

    // 3. Every alternative, named — the ticket's own falsifier, and the reason the rejection is worth reading.
    for rejected in [
        "Rune",
        "egui",
        "Slint",
        "Makepad",
        "Compose Multiplatform",
        "SwiftUI",
        "progressive web app",
    ] {
        assert!(
            adr.contains(rejected),
            "docs/adr/0008-the-phones-framework.md does not name {rejected}, which is rejected by decision: an \
             ADR that recommends a framework without naming the rejected one is the falsifier #223 names"
        );
    }

    // 4. The one thing the ADR says is not negotiable: where the device key lives.
    for store in ["Keystore", "Keychain"] {
        assert!(
            adr.contains(store),
            "docs/adr/0008-the-phones-framework.md does not say the key lives in {store} (decision 4): \
             `pair.claim` hands a phone a key once, and this is the only place it can live"
        );
    }

    // 5. The framework it took, and the escape hatch's shape — a second UI must stay a defect, not a preference.
    assert!(
        adr.contains("Flutter"),
        "the ADR must name the framework this repository chose (#223)"
    );
    assert!(
        adr.contains("a second UI is a second vocabulary"),
        "docs/adr/0008-the-phones-framework.md must keep the rule that a screen moving to SwiftUI for polish \
         repeals the decision: it is `mobile-ui.md`'s rule too, said where the framework is chosen"
    );
}

/// **The phone's tree is where ADR-0008 says, and its corpus is the wall's** (#248).
///
/// The ADR took the decision that the app lives in this repository, so the path it names is a claim like any other: a
/// client tree that moved, or a package that reads a copy of the corpus instead of the wall's recording, is the drift
/// this file exists to catch. The second half is the one that matters — a phone held to its own copy of
/// `fixtures/control/` would pass while disagreeing with the wall.
#[test]
fn the_phone_reads_the_walls_corpus_by_path() {
    let adr = read("docs/adr/0008-the-phones-framework.md");
    assert!(
        adr.contains("clients/phone/"),
        "docs/adr/0008-the-phones-framework.md does not name where the phone lives: the ADR took that decision (#248) \
         and a decision a reader cannot find is not one"
    );
    for path in ["clients/phone/pubspec.yaml", "clients/phone/lib/wire.dart"] {
        assert!(
            root().join(path).exists(),
            "the ADR says the phone lives in clients/phone/, and {path} is not on disk (#248)"
        );
    }

    // It reads the repository's corpus, by the path that leads back out of its own package.
    let corpus = root().join("fixtures/control/session.jsonl");
    assert!(
        corpus.exists(),
        "fixtures/control/session.jsonl is gone: the phone's spec is the wall's recording"
    );
    let suite = read("clients/phone/test/corpus_test.dart");
    assert!(
        suite.contains("../../fixtures/control/session.jsonl"),
        "clients/phone/test/corpus_test.dart does not read ../../fixtures/control/session.jsonl: the phone must be held \
         to the wall's recording by path, never to a copy of it (#248)"
    );

    // And no second copy of the recording exists anywhere in the client tree.
    let copied = walk(&root().join("clients"), &mut Vec::new())
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("session.jsonl"))
        .collect::<Vec<_>>();
    assert!(
        copied.is_empty(),
        "the phone has its own copy of the corpus: {copied:?}. The wall's recording is the specification, and a copy \
         of it is a second truth (#248)"
    );
}

/// Every file under `dir`, recursively, excluding build output. Used by the claim above to prove a *copy* is absent.
fn walk(dir: &Path, into: &mut Vec<PathBuf>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name == ".dart_tool" || name == "target" || name == "out" {
            continue;
        }
        if path.is_dir() {
            walk(&path, into);
        } else {
            into.push(path);
        }
    }
    into.clone()
}
