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

**The map feature is the one thing the gate does not compile**, because compiling it means MapLibre's C++
and this machine's toolchain. That is a real hole and it has cost us: #101 renamed a `State` field and left
one `#[cfg(feature = "map")]` `drag_pane` assigning the old one; the default gate was green and CI's macOS
`map` job was the first thing to notice. So:

```sh
VERIFY_WITH_MAP=1 scripts/verify.sh   # the same gate, plus `cargo check --features map`
```

Run it before pushing anything that touches code behind `cfg(feature = "map")`. CI's `map` jobs run the same
command, which is why a change to those lines is decided there even when a local gate says PASSED.

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
cargo run -- --scenes            # empty · picture · stale · card-open · photo · panes · panes-chrome
                                 # panes-drag · panes-pan · panes-menu · panes-gaps · panes-gap-resize
                                 # panes-named · panes-max · panes-cards · panes-range · objects
                                 # settings · settings-dismiss · pane-menu · pane-tree
scripts/verify.sh                # renders and asserts each one
```

`panes` is the Multi View witness (#68): three panes at three cameras, each pane's frame its own
rectangle, and the focused pane marked by its edge. Its assertion projects each hull against the pane
it is drawn in, using the same `Layout::rects` the view lays the panes out with.

`objects` is the **operational layer** (#110): a buoy, a cone and one unlabelled light, on two panes at two
zooms on the same water. The manifest and the placements arrive through the replies a wall takes, and the
stand-in artwork through `Reply::TacticalAsset` — so the capture covers wire data → decoded cache → mark,
not a shortcut. The assertion projects each placement against **its pane's** frame, which is the point:
a layer drawn from the wall's camera agrees with the focused pane and lands tens of pixels away in its
neighbour. It also probes the label plate's left padding for the plate's fill: the two labelled marks have
one, the unlabelled one has none — an empty label is not a label (`INV-01`), and defaulting `""` to a drawn
plate is red. The three PNGs under `fixtures/scenes/` are stand-ins for the presigned SVG artwork, because a
scene may not open a socket (`INV-11`).

`panes-range` is the same wall stripped to **two panes at two zooms on the same hull** (#115): the focused
pane at `PHOTO_ZOOM`, where a ship is its photograph, and its neighbour at a chart zoom, where it is a
symbol. Both panes hold the decoded bytes — the cache is the wall's — so the capture is about the *latch*:
the assertion requires picture ink in one pane and symbol ink in the other, and a wall-wide range draws one
representation in both.

`pane-tree` is the **chain of command** (#116): a tree pane beside a map pane, one structure's forest under
the supreme command. The data is #107's — `GET /unit-organisations/{id}` returns `nodes` as the roots of a
**forest** (more than one root is normal, and each root nests its own subtree), and `GET /tni-commander` is
one post shared by every branch, so it is drawn as a **single root** with a connector to each branch root:
TNI AL, TNI AD and TNI AU are siblings under it, never children of one another. The scene reads the recorded
structure through the same replies a wall takes (`ui::apply_reply`), so the capture covers wire data → pane →
plate. The assertion counts the plates from the wire — one node per post, so a dropped root is a red line —
samples each connector's **elbow** for the connector's own ink (a diagonal, or a run that never turned, has
nothing there), and counts accent pixels inside the tree pane: **none**, because a tree pane has no frame and
so draws nothing that projects (`INV-04` holds literally) — while the map pane beside it still draws its
markers, so the tree did not take the wall. `cargo test ui::tree` holds the shape claims a capture cannot: a
forest renders as a forest (two roots, not one, and the flattened tree is the falsifier), a connector leaves
the parent's **bottom edge** rather than its centre, a post with an `id_unit` selects that hull **in that
pane only**, and a layout pass gives a tree pane no frame while the map beside it gets one.

A tiled wall also says so in its header (`40 units · panes=3`) and in its log (`pane_frames=[12, 3, 3]`): a
wall quietly drawing three of four panes is a line in a file, not a report (#71).

`panes-chrome` is the same wall with **both corner controls in every pane** (#111): a filter toggle and a
maximise toggle at `view::chrome_buttons`, drawn after the markers and after the name plate. The capture
opens the filter panel because `fixtures/scripts/panes-chrome.json` presses the focused pane's toggle, so
the scene also proves the control reaches a handler rather than being decoration. The assertion samples
each button's own rectangle for ink and measures the popover's fill against `tokens::FILTER_SIZE`; removing
either toggle leaves a button-shaped hole and the line names the pane.

`pane-tree-narrow` and `pane-tree-zoom` are the **tree pane as a canvas** (#127). The first is the
operator's report made a capture: the same forest in a **320 pt** pane, where 1:1 would cut it, with the
camera untouched — so the fit runs and the assertion requires **every post and every connector inside the
pane's own rectangle** (8 posts, 7 connectors; the recorded forest is 784 pt wide, so the fit is ≈0.37).
The second is a hand's camera: zoomed to 2.0 with the recorded fleet command's centre put at the known
pane point (300, 280), and the assertion requires that node's plate at the point `view::tree_point` maps
it to, and **fewer posts on screen than the tree has** — a fit would show all of them. The third is the
only gesture the gate can drive that `cargo test` cannot: `fixtures/scripts/pane-tree-pan.json` presses a
post and drags it, and the assertion requires that post's plate to have moved by exactly the travel —
absolute panning, not a sum of per-event deltas, and a drag that reached no canvas would leave the tree at
the fit. The transform is one
`El::scale`/`El::offset` over the laid-out tree, never a per-node re-layout, so the drawing, the text and
the hit test cannot disagree; `cargo test ui::tree` holds the arithmetic (a fit puts 784 pt inside a 320 pt
pane; a wheel zoom keeps the point under the pointer still, clamp included). Below
`tokens::TREE_SECONDARY_MIN` the secondary lines are dropped and only names draw, which is what keeps an
overview a shape rather than a grey block.

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

`panes-max` is the same wall with one pane **full size over it** (#85), and since #111 the maximise is a press
on the pane's own **toggle** (`fixtures/scripts/panes-max.json`), not a call to the function behind it. The
assertion projects that pane's hull against a frame the size of the **panel** — not its slot's — which is the
whole claim: a full-size pane is the same pane with a bigger rectangle. Rendered with the slot's frame instead,
it is red: `no marker ink at (640, 372)`. A toggle that is missing, or whose press reaches no handler, leaves
the pane in its slot and the assertion finds no full-size picture. The tiling behind is asserted structurally
(`rects.len()` is still the number of panes), and the engine's half of it — the map thread actually rendering at
the window's size and back — is an `#[ignore]`d test, `a_pane_resizes_to_full_size_and_back`, run with
`cargo test --features map --lib -- --ignored`.

`panes-named` is the same wall with **two panes named and one not** (#83, #113): a short name, a name long
enough to be cut, and a pane somebody never named. **Every pane has a title**, so the assertion probes
`ui::view::name_probe` — the rectangle the plate is placed against — for the plate's own colour on all three
panes, so a hull that happens to sit in the corner is not mistaken for a title. What the plate says keeps #83's
rule: the unnamed pane shows its own number (`pane-03`, from its id and not its position) and the named panes
show their names, because a name is something somebody typed and a title is not.

`panes-gaps` is the same wall with **space between the panes** (#82): the settings sheet's first setting. The
assertion finds the gutter the way an operator sees it — a run of the window's own background inside the
panes' union — and measures it against `tokens::GAP`, so it is about the look and not about a copy of the
number. Rendered with the gap removed, it is red: `no vertical gutter of 8 pt — measured []`.

`panes-gap-resize` is the gapped wall with the **gutter as the handle** (#114): the wall is left at half and
`fixtures/scripts/panes-gap-resize.json` presses the middle of the gap and drags it to 58%. The expected
layout is the one `panes-gaps` draws — same split, same space — so a press that reaches no element leaves the
wall at 50/50 and both the golden and the assertion say so. The assertion finds the gutter's own column and compares
its centre with `PANES_DRAGGED` of the panel (within 2 pt), then projects each pane's hull against the 58/42
layout. Drawing the gutter elements *under* the panes is the other falsifier: the press then falls through to a
pane's map drag, which pans a camera instead of moving the line.

`settings` is the wall with its **settings sheet open** (#82): the one modal surface, over a picture that is
dimmed and not reflowed (the scrim is *pinned* to the root, so opening it costs the picture nothing). The
assertion measures the sheet's box against `tokens::SHEET_SIZE` and requires ink in its rows.

`panes-menu` is the same wall with its menu **open** (#81), which is the only wall that offers `Close pane`. Its
assertion samples each row of the menu from `tokens::MENU_ROW` rather than from the surface's declared size, so
a surface shorter than its items is caught: the second label still draws, it just hangs off the plate — which
is what the ticket meant by "a menu sized for one row clips the second", and what the first attempt at the
assertion missed by asking the same function that drew it. Since #111 the menu has **two rows** on one pane and
**three** on a tiled wall: `Full size` / `Back to the wall` left it for the pane chrome's maximise toggle.

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

