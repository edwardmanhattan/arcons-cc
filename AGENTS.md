# AGENTS.md — working in arcons-cc

Read this, then [`CONTEXT.md`](CONTEXT.md) for the domain words and
[`README.md`](README.md) for the scope. **The plan lives in the issues, not in these files**:
start at [#1](https://github.com/edwardmanhattan/arcons-cc/issues/1) and take the first ticket whose
blockers are closed. One ticket per branch and PR.

## A ticket is done when a command says so

The contract is [`docs/verification.md`](docs/verification.md): a ticket is not done when the prose
reads well, when it compiles, or when a screenshot looks plausible. It is done when the gate is
green. The numbered promises are [`docs/invariants.md`](docs/invariants.md) (`INV-01` … `INV-12`);
a test that enforces one cites it in its failure message.

```sh
scripts/verify.sh          # the gate: fmt, clippy -D warnings, tests, captures, budgets
scripts/verify.sh --fast   # the inner loop
```

CI runs exactly `scripts/verify.sh` — never its individual parts — so CI and a human cannot
diverge. **The two `map` jobs are the exception that proves it, and they are not advisory**: the gate
cannot build MapLibre's C++, so the `#[cfg(feature = "map")]` code is compiled by CI alone, and a red
`map (macos-latest)` or `map (windows-latest)` is a red PR. #152 is why: #138 moved `panes` from a field
to a method, four map-gated sites were left behind, and the map feature did not compile for five merges
while this gate was green. Locally, `VERIFY_WITH_MAP=1` closes the hole at the gate's own standard. **Before reporting done, paste the gate's last lines.** A claim is not evidence. If a check
is slow, ticket the fix; never delete or weaken it.

Every ticket carries a `## Proof` line. If it cannot name a command, it is not ready.

### When a tripwire goes red

The gate parses our own claims, so some failures are about a document rather than a function:

| test | it failed because | what to do |
| --- | --- | --- |
| `tests/spec_drift.rs` | the pinned Minos snapshot requires a field we do not model, or no longer has one we do | model it (or stop modelling it). Only re-vendor `docs/spec/` if the contract changed meaning — and say what changed |
| `tests/doc_claims.rs` | `Cargo.toml` and ADR-0001 disagree about the Rune rev; §7 of the protocol and `live::Refusal` disagree about the refusal codes; the ADR's map-seed path is gone; or CI started running a part of the gate instead of the gate | fix the code or the document deliberately. A 7th refusal code means a new variant **and** a new action, never a default |
| `tests/invariant_coverage.rs` | an invariant has no test and no IOU, or has an IOU it no longer needs | write the test that cites it, or delete its line from `PENDING` in the same change |
| `tests/notices.rs` | a dependency is not named in `THIRD-PARTY-NOTICES.md`, or a copyleft crate appeared in the graph | add the row. Never drop the check |

Every one of those messages names the claim, the file and the ticket that owns it. Read the message; it is the specification.

## Commands

```sh
cargo run                       # the display (Wayland or X11)
cargo run -- --version          # the version, no window
cargo run -- --self-check       # what is configured, what is missing (exit 2 if anything is)
cargo run -- --window-size 1920x1080
cargo run -- --fullscreen
cargo run -- --snap out/frame.png   # Rune's headless snapshot; #9 wraps this in scripts/capture.sh
```

## Rules that are not preferences

- **The UI thread never blocks.** Reads (`src/data`), the socket (`src/live`) and the map
  (`src/map`) each own a thread; the view loop gets messages. A blocking call in `view` freezes the
  wall forever, not for a moment.
- **Absence is not zero**, **backfilled is not now**, **the socket envelope is not uniform**, **the
  token rides in the connect frame's `data`**, and **no history exists on `live-feed`** — the five
  traps. They are spelled out in `README.md` and `CONTEXT.md`; INV-01/02/03/09 are the same claims as
  tests.
- **Colours and sizes live in `src/ui/tokens.rs`.** Nothing outside it names one; `tests/scaffold.rs`
  fails when something does. Assert against tokens, never literals.
- **An icon comes from Rune's catalogue** (`rune::Icon` / `El::icon`), and its meaning is also in words
  where there is room. A glyph nobody can name is a button nobody presses (#111).
- **Fixtures come from the wire**, pinned to `docs/spec/` or recorded traffic — never generated from
  our own types. Goldens change only via `--update-goldens`, by a human.
- **Never edit `docs/spec/`.** Re-vendor it with a new commit hash and say what changed.
- **No secret, host, username or token in the tree, in a log line, or in a panic** (`INV-08`).
  Configuration is read from the environment or `$XDG_CONFIG_HOME/arcons-cc/`, never from source.
  **`layout.toml` holds the arrangement and nothing else** — workspaces, panes, cameras, names, filters —
  and `deny_unknown_fields` is what keeps it that way. A capture never reads or writes it.
- **No dependency without an entry in `THIRD-PARTY-NOTICES.md`.** The app is proprietary; there is
  no root `LICENSE` on purpose — do not add one (`README.md` says so too).
- **Rune is pinned by rev.** Bump it deliberately, in a commit that names the old and new revs.
  Never `branch = "dev"`; never a vendored copy without a reason in `vendor/README.md`.
- **No catch-all match arm over a wire enum** (`INV-07`): a new refusal code or event type must be a
  visible branch, not a silent default.

## Asking instead of inventing

Comment on the issue rather than choosing silently, especially for: the wall's credential mode
(refresh token vs password exchange), confirmation that the wall account holds `read` on
`/system/live-feed`, and whether the display starts on the live feed or a configured game id.

## Layout

```
src/main.rs      flags, startup, exit codes
src/config.rs    the command line, the resolved Config, Secret (redacting by construction)
src/config/      file.rs (config.toml) · credential.rs (mode 0600) · settings.rs (settings.toml, written by the display) · layout.rs (layout.toml: the wall as the operator arranged it) · check.rs (--self-check, 0–6)
src/ui/          the view, view.rs (the pane tree), tree.rs (the chain of command), markers.rs, objects.rs (the operational layer), installations.rs (the places), filter.rs (per-pane visibility and the panel's vocabulary), unitcard.rs, tokens.rs (the only file that names a colour or a size)
src/units/       the vocabulary and the view model: Unit, UnitView, Glyph, Picture, motion, card
src/data.rs      the Minos REST client            (#3)
src/live.rs      the Centrifugo socket actor      (#4)
src/map/         geometry (no engine, in the gate) + the MapLibre thread (#5)
vendor/          dependencies patched for reasons recorded in vendor/README.md
fixtures/        recorded wire data the tests read (see fixtures/README.md)
docs/spec/       the pinned Minos contract — read-only
src/platform.rs  the log, SIGTERM, the start clock
src/scene.rs     the named states a capture renders: fixture feed, no network, virtual clock
src/verify.rs    --assert: pixel and structure questions with numbers
scripts/         verify.sh (the gate), capture.sh <scene>, assert.sh <png>
fixtures/scenes/ the 40-vessel feed the scenes load; golden/ the captures they must match
docs/            verification contract, invariants, ADRs, runbook.md (putting the wall up)
```
