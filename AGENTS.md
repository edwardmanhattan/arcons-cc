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
diverge. **Before reporting done, paste the gate's last lines.** A claim is not evidence. If a check
is slow, ticket the fix; never delete or weaken it.

Every ticket carries a `## Proof` line. If it cannot name a command, it is not ready.

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
- **Fixtures come from the wire**, pinned to `docs/spec/` or recorded traffic — never generated from
  our own types. Goldens change only via `--update-goldens`, by a human.
- **Never edit `docs/spec/`.** Re-vendor it with a new commit hash and say what changed.
- **No secret, host, username or token in the tree, in a log line, or in a panic** (`INV-08`).
  Configuration is read from the environment or `$XDG_CONFIG_HOME/arcons-cc/`, never from source.
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
src/config.rs    paths, --self-check, the command line
src/ui/          the view + tokens.rs (the only file that names a colour or a size)
src/units.rs     the vocabulary: Unit, Picture
src/data.rs      the Minos REST client            (#3)
src/live.rs      the Centrifugo socket actor      (#4)
src/map.rs       MapLibre on its own thread       (#5)
vendor/          dependencies patched for reasons recorded in vendor/README.md
docs/spec/       the pinned Minos contract — read-only
docs/            verification contract, invariants, ADRs
```
