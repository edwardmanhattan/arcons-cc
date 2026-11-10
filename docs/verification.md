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

`photo` carries one more rule, because it is the first scene with an image in it: **the picture is
decoded before the first frame**. A capture that raced the decoder would be a capture of a symbol on
some runs and of a ship on others — the same class of bug as a wall-clock redraw (#35) — so the scene
waits for `Image::is_ready()` with a bound, and a picture that cannot decode draws the symbol rather
than hanging the capture.

`photo` also goes through the **reply path** (`ui::apply_reply` with a `Reply::Image`), not a shortcut
into the cache: a capture then covers bytes → cache → marker, which is the route a wall takes (#54).

**The gate renders every scene the binary lists**, and the list comes from the binary:

```sh
cargo run -- --scenes            # empty · picture · stale · card-open · photo · panes · panes-drag · panes-pan · pane-menu
scripts/verify.sh                # renders and asserts each one
```

`panes` is the Multi View witness (#68): three panes at three cameras, each pane's frame its own
rectangle, and the focused pane marked by its edge. Its assertion projects each hull against the pane
it is drawn in, using the same `Layout::rects` the view lays the panes out with.

A tiled wall also says so in its header (`40 units · panes=3`) and in its log (`pane_frames=[12, 3, 3]`): a
wall quietly drawing three of four panes is a line in a file, not a report (#71).

`pane-menu` is a wall with **one** pane and its right-click menu open (#77) — the one shape the menu was
never drawn on, and the only wall that needs it, because "Focus on new pane" is the only route to a second
pane. The assertion checks the menu's box against `ui::menu_origin` and that it stays inside the picture.

`panes-pan` is the only capture **driven by a pointer** (#80). `fixtures/scripts/panes-pan.json` presses
the border at 50% of the panel and drags it to 58% — the share `panes` sets in code — and the two captures
are then the same picture to within a hair of a point. That matters because a whole class of bug is
invisible to every other kind of test: a gesture that reaches no handler, a press landing on the wrong
element, a point measured in the element's coordinates when the layout asks in the viewport's (the bug: a
press inside pane 2 was read as 2 pt from the *window's* left edge). With the drag dropped, the border
returns to half and three assertions go red — measured, not assumed.

`panes-drag` is the same wall with a pane **lifted** (#70, #84): one pane dimmed in its slot, and the pane in
the air **drawn on the slot it will land in** — `Layout::preview`'s rectangle, the pane's own camera projected
against that rectangle's size, on the slot's plate. Two assertions: the slot's plate is drawn where the
preview puts it, and the lifted pane's hull is where that projection puts it. Both call
`ui::view::landing_rect` — the same function the picture draws from — so a preview that opened somewhere the
pane does not land, or a ghost drawn anywhere but on the slot, fails here.

Why the slot and not the pointer: a copy hanging from the hand covers the slot it is about to land in, which
is the one thing an operator is looking at — and it is a second coordinate space in a ticket whose subject
is a coordinate space got wrong (`Drag::x`/`y` are element-local; the handle is its own 28 × 4 element inside
a pane that is itself at an offset, and `lift` had asked `drop_for` about the handle's local point, which is
why a pane could only ever be dropped sideways).

That is not a convenience. Until #54 the list was written by hand in `scripts/verify.sh`, and it
omitted `photo` — so a scene added in #41 was never captured, its golden was regenerated blind in #42
(a NATO symbol where the ship belonged), and the gate stayed green for ten days while the scene drew the
wrong thing. A scene that exists is a scene the gate renders; a new one without a golden is a red gate,
which is the right moment to make a human look at it.

The two checks do different jobs, and neither substitutes for the other:

- **The golden** answers *"did anything change?"* — a tone, a gap, a moved label. It is bytes, so it
  catches what nobody thought to assert.
- **`assert.sh`** answers *"is what changed wrong?"* — "the marker for hull 12 is 5.1 px from where
  the projection puts it", "the stale marker is 146 luma and the live one 170". It reads colours from
  `ui::tokens` and geometry from `map::geometry::project`, so it cannot outlive the palette or the
  layout it describes.

### The clock a capture runs on

A scene has exactly one clock: `ui::State::now()`, which is `origin + logical`, and `logical` comes from
Rune's **virtual** frame clock (`--at 2 --fps 60` ends at exactly 2.0 s). Nothing in a drawing or
change-detection path may read the wall clock, and `tests/scaffold.rs` fails the build if one does.

The reason is subtler than "a machine's speed must not show": Rune rebuilds the element tree **only when
`poll` says something changed**, so a wall-clock comparison inside `poll` decides *when* a new value is
drawn. `card-open` did exactly that with the card's age, and the result was a capture whose text said
`4 s old` on a fast machine and `5 s old` on a slow one — intermittently, on CI's software rasteriser
and under load, which read as a flaky test rather than as a clock (#35). The fix was one line, and the
tripwire is the rule.

### A golden is compared within a stated tolerance, everywhere

`scripts/capture.sh` renders every scene **twice** and compares the bytes: on one machine, twice, a
capture must be identical (`INV-11`'s machine-independent half).

Against the committed golden, the compare is **bounded**, not byte-exact:

| bound | value | why that number |
| --- | --- | --- |
| no channel differs by more than | **2 / 255** | two rasterisers (this laptop's iGPU and CI's lavapipe) were measured at **1/255**, on at most 23 pixels of 921,600 |
| pixels differing at all | **200** | ~10× the worst measured case, and small enough that a moved label or a torn panel cannot hide |

Both bounds are needed and neither is enough: a budget alone would let 200 pixels of *text* change, and a
tolerance alone would let a whole panel shift by a rounding step. The measured numbers are printed on
every run — a tolerance nobody can see the margin of is a tolerance that quietly widens.

`ARCONS_GOLDENS=exact` demands bytes (`tolerance 0`, `budget 0`): for the reference machine, or when a
human wants to know the pixels are the reviewed ones rather than merely the same picture. There is no
`off`: skipping the goldens in CI is how a *consistent* change — a card moved, a token's colour shifted
— passes CI and fails only on one laptop.

A golden changes only through `--update-goldens`, run by a human who has looked at the capture, and the
PR says which machine made it.

