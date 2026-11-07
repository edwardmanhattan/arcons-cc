# Verification

**A ticket is done when a command says so.** Not when the prose reads well, not when the code compiles,
not when a screenshot looks plausible. If nothing can fail, nothing was verified.

This file is the contract. `AGENTS.md` links here; every ticket carries a `## Proof` line; CI runs
`scripts/verify.sh`, which is the same gate a human or an agent runs before saying "done".

## The ladder

Cheapest rung that can answer the question. Never climb two rungs for one question.

| rung | what it catches | what it cannot | cost |
| --- | --- | --- | --- |
| **types** | absence, units, nullability, wrong shape | anything about behaviour | free |
| **unit / property / contract tests** | logic, parsing, arithmetic, state machines | layout, colour, overlap | seconds |
| **headless scene + pixel assertion** | geometry, presence, colour, clipping | motion feel, latency | seconds |
| **budget test** | frame CPU, memory, reconnect storms | everything visual | minutes |
| **human eye on a capture** | taste, "does this look like a warship console" | nothing repeatable | the expensive one |

A capture is a **witness**, not an oracle. It is worth taking because it is the evidence a human reads.
It is not a verification unless something counts its pixels (`scripts/assert.sh`) and fails with a number.

## The one gate

```sh
scripts/verify.sh          # everything: fmt, clippy, tests, captures, budgets
scripts/verify.sh --fast   # no captures, no budgets: the inner loop
```

It exits non-zero on the first failure with the command that failed, and it needs **no network, no
display, and no GPU beyond a software Vulkan driver**. If a check needs any of those, it belongs in a
ticket that says so, not in the gate.

## Every ticket carries a `## Proof` line

One line: the command, and what a failure looks like. Not "tests pass" — the invocation.

```md
## Proof

`cargo test units::` — fails if any marker is projected without a fix (INV-02), if an absent
`speed_kn` renders as `0.0` (INV-01), or if interpolation appends to the track (INV-05).
```

If a ticket cannot name a command, it is not ready. Write the command first; it is the specification.

## Invariants

`docs/invariants.md` holds numbered claims (`INV-01` … `INV-12`). A test that enforces one **cites it in
its failure message**, so a red build says which promise broke rather than which function returned what.
Adding an invariant is cheap and expected; renumbering one is not allowed, because tickets quote them.

## Rules

1. **Never weaken a test to make a change pass.** If an assertion must change, the change names the
   invariant it moves and says why, in one sentence.
2. **Fixtures come from the wire**, pinned to `docs/spec/` — never generated from our own types. A test
   that round-trips our struct through our own serializer proves only that we agree with ourselves.
3. **Assert against tokens and constants, not literals.** A colour assertion with `#22D3EE` in it
   outlives the palette and then lies quietly.
4. **Goldens change only with `--update-goldens`, run by a human.** CI compares; it never rewrites.
5. **Budgets are numbers in tests.** "Feels smooth" is not a result. `≤ 8 ms frame CPU at 1080p` is.
6. **No catch-all match arm over a wire enum.** An unknown refusal code or event type must be a visible
   branch (log + ignore), never a silent default that swallows the next protocol addition.
7. **A test that cannot fail is deleted.** Assert on an outcome, not on the call happening.
8. **Unknown JSON fields must not break parsing.** The server adds fields without ceremony; `deny_unknown_fields`
   is how a client breaks on a release that changed nothing for it.

## Drift detectors

The failure mode a long agent run hits is not a wrong function — it is a *stale assumption*, still green.

- **Spec drift** — `docs/spec/` holds a pinned snapshot of `minos-api.yaml` and `realtime-protocol.md`,
  with the commit it came from. A test parses the snapshot's `required:` lists for the schemas we consume
  and fails when a field is required there and unmodelled here. Re-vendoring is a deliberate commit.
- **Doc drift** — claims that can be checked are checked: "Rune is pinned to rev X" is a test over
  `Cargo.toml`, not a sentence in an ADR that ages silently.
- **Notices drift** — a dependency in `Cargo.lock` without an entry in `THIRD-PARTY-NOTICES.md` fails.

## What is not verification

- A summary written by the agent that made the change.
- A screenshot nobody counted pixels on.
- "Acceptance criteria" prose with no command behind it.
- A test that asserts the implementation's own intermediate value.
- A green build from before the change.

## For agents: the loop

1. Run `scripts/verify.sh --fast` before you start, so you know the baseline is green.
2. Write the failing check first when the ticket allows it; the failure is the proof it can fail.
3. Run `scripts/verify.sh` before you report done, and paste its **last lines**, not a claim.
4. If you changed a test, a fixture or a golden: say which, and why, in one sentence.
5. If a check is slow, ticket the fix. Do not delete the check.

## Captures: what the wall looks like, as a command

```sh
scripts/capture.sh picture          # out/picture.png, compared with fixtures/golden/picture.png
scripts/assert.sh out/picture.png   # pixels and structure, with numbers
scripts/capture.sh --update-goldens <scene>   # a human decided the new picture is right
```

A scene is deterministic by construction: `fixtures/scenes/feed.json` instead of a server, Rune's
virtual clock instead of `Instant::now()`, no socket and no map surface. Two runs are identical
bytes (`INV-11`), which is what makes a golden a check rather than a suggestion.

The two checks do different jobs, and neither substitutes for the other:

- **The golden** answers *"did anything change?"* — a tone, a gap, a moved label. It is bytes, so it
  catches what nobody thought to assert.
- **`assert.sh`** answers *"is what changed wrong?"* — "the marker for hull 12 is 5.1 px from where
  the projection puts it", "the stale marker is 146 luma and the live one 170". It reads colours from
  `ui::tokens` and geometry from `map::geometry::project`, so it cannot outlive the palette or the
  layout it describes.
