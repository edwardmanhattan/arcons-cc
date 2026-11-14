# ADR-0001 — Rune for the interface, MapLibre Native for the map

- **Status:** accepted (scaffold, #2)
- **Date:** 2026-11-06
- **Deciders:** Edward Manhattan

## Context

A wall display has three jobs, and they pull in different directions: draw a map that moves, put
readable text on it from across a room, and stay up for hours unattended. v0.1 has to do all three
with no network at cold start and no display at verification time — a capture nobody can take in CI
is a claim nobody can check.

That last constraint decides most of this. We need a UI stack that renders **headlessly to pixels**,
from a fixed clock, so a capture is reproducible and a test can count what is in it.

## Decision 1 — Rune

`rune` (github.com/anomalyco/rune), **pinned by rev**:

```
rune = "04389ed930177f1f475ff3de5249304545b3d5b4"
```

Why:

- **Headless is a first-class mode, not a testing afterthought.** `--snap` renders a scene to PNG,
  `--tree` dumps the element tree, `--script` drives input on a virtual clock. That is exactly the
  "headless scene + pixel assertion" rung of `docs/verification.md`, and it is why `INV-11`
  (same fixtures + same clock ⇒ byte-identical PNG) is reachable at all.
- **The renderer, text and springs are in-process.** No windowing toolkit's theme, no webview, no
  system font, so a capture does not vary with the machine's settings.
- Same code path native and headless: what CI proves is what the wall draws.

Costs we accept: it is pre-release, so a rev bump is a deliberate act; `rune` on crates.io is a
different project (the Rune scripting language), so every dependency line carries `git =`; and it is
large — the build pulls a few hundred crates and wants Vulkan.

The pin is by **rev, never `branch = "dev"`**: `dev` moves, and a wall display that silently changes
renderer is not verifiable. `tests/doc_claims.rs` (#12) will fail if this ADR and `Cargo.toml`
disagree about the rev.

### The rev, and what we now take from upstream (#65)

Was `7ead7d63fcf6f9dd9669385a8b90e2ae80015d7b` (2026-11-06); now
`46cfcd64cf348ea9391c5db01f0d488c548d74d9` (2026-11-09), 159 commits on. Nothing in `src/` changed for
the bump: these are things we **intend to use**, not things this commit switches on. Each is a
sentence, because an ADR that lists an API without saying what it buys is a changelog.

- **`Tx::clock` / `Tx::wake_at`** — timers are events, so the 1 Hz keep-alive (#43) becomes a wake
  rather than a thread, and the Multi View render budget (#64) can be scheduled per pane without a
  thread per pane. Not adopted yet: today's threads are the working version, and a rewrite belongs in
  those tickets.
- **Focus as an API** (`Tx::focus`, `El::autofocus`, `ModalFocus`, `El::focus_fill`) — "the active
  pane" with a focus ring is upstream's model instead of our own key routing, which is what #64's
  panes need and what `card-open` fakes today.
- **`rune-kit::shell::TabStrip` / `TabDragState`**, with `Reorder` and `El::matched_crop` — drag to
  reorder with the incoming gap drawn where the dragged thing will land. That is #64's drag-to-swap,
  already written and public upstream; we write the adoption, not the interaction.
- Smaller, and the reason `El` is worth re-measuring upstream: `El` is 192 bytes with in-place
  builders (#337), popup tooltips, scroll modes and RTL. None of it is load-bearing for v0.1.

### Bumped again: `46cfcd64` → `b73010b` (#79)

54 commits on, 2026-11-09, and the reason is the v0.2 pane work rather than the renderer: `rune-kit` grew
**`image_zoom`** (`Stage`, `Preview`, `ImageZoom` — a full-window layer over a live element with zoom, a
close affordance and Escape, which is the shape of "full-size this pane for a moment", #85), and the
60-ish handlers between the pins are fixes we would otherwise work around. Nothing in `src/` changed for
the bump; the only visible difference is **text rasterisation** — the wordmark and the card's glyphs, at
the same positions, which is what the eight goldens' diff was before they were regenerated. What the pane
backlog needs *already existed* at `46cfcd64` and needs no bump: `rune-kit`'s `menu` (including
`context_menu` at a `PointerPress`), `dialog`, `overlay::scrim`, `text_input`, `segmented`, and
`shell::{SplitView, PaneState, PaneMotion}` — so #80–#84 are API work we have, and #85 is the one the bump
buys.

Two things about *our* code that the bump did not fix, and #84 records: `Drag::x`/`y` are **element-local**
(`runtime/input/gesture.rs`), and a drag reaches the element that was **pressed**, not its ancestors — so
the pane drags were computing drops from the wrong point and the map's drag never fired once panes existed.
Neither is a Rune bug; both were ours.

The bump is not free, and the frame budget is where it shows. **`INV-12`'s raw CPU went up ~7–10%**
(4.09–4.12 → 4.38–4.76 ms per frame with 200 markers, this laptop, Intel iGPU), while the
**reference-machine number went down** (4.23–4.32 → 3.77–4.12 ms) because the fixed calibration
workload upstream got ~20% more expensive in the same bump (machine factor ×0.95–0.97 → ×1.14–1.18).
So the normalised budget — the one the gate enforces — improved, and the honest reading is "our
per-marker work is now a smaller share of a Rune frame", not "Rune frames got cheaper". The ratio
assumes the reference workload is a constant; a renderer bump is exactly when it is not. #48's design
still leaves 3.8–4.1 ms against a limit of 8, so the budget stays where it is; the numbers in this
paragraph are the new measurement.

### Bumped again: `b73010b` → `14e3c5d` (#106)

169 commits on, 2026-11-11. **The new surface was read in full, and one addition replaced a
hand-rolled assumption**: the card's name is the one multi-paragraph, feed-supplied text on the
wall, and it now takes `El::dir_by_paragraph` so each paragraph lays out in the direction of its own
words, as a browser's `unicode-bidi: plaintext` does. A Latin name is one text and lays out exactly
as before — which is why the goldens' diff is rasterisation and not layout — while a right-to-left
name no longer leans on the wall's direction. The other eight replace nothing this tree hand-rolls
yet: it has no tooltip, no scroll view, no web page and no keyboard focus to hand back; the wall's
"focus" is a pane (`Layout::focused`), not an element. Each is listed here with the ticket that will
adopt it, so the next wave cites this shape instead of reinventing one:

- **`El::help`, `help_row`, `help_keys`, `help_side`/`help_above`, `help_when_truncated`** — a hint
  attached to the control it describes, with rows and keycaps, and shown only when a text is cut.
  The wall explains itself in visible rows today ("Enter names it, Escape leaves it as it was"),
  which is the right shape for a keyboard-first wall; a control that needs a hint on hover is the
  pane menu's and settings sheet's chrome, when they grow one.
- **`Tx::return_focus` and `El::press_focuses_below`** — focus handed back to the trigger when a
  menu or sheet closes, and the region under a menu's scrim focused as a press there would. The
  pane menu opens at a right-click point, which is not an element and does not survive the close;
  the settings trigger does, so it is the first candidate when the wall grows a keyboard path to
  the sheet.
- **`El::dir_by_paragraph`** — adopted on the card's name, above. Pane names are operator-typed and
  may be Arabic or Hebrew too, but #83's plate is one line; it takes this when a name can hold a
  newline.
- **`El::unselectable`**, **`El::set_content`** — a web page's text selection and a styled text
  reused with new words. The wall is native and rebuilds its tree per frame, so neither has a call
  site; the first web build and the first templated row will.
- **`Tx::relaunch`** — restart in place with the same arguments, which is what a setting that
  cannot be applied live needs. Nothing in `settings.toml` is in that class yet (#82 is one value,
  applied and written).
- **`El::on_far_from_bottom_under`** — "jump to latest" for a viewport under a frame. The wall has
  no scroll view; a scrollable feed (#11's plot) is where it lands.

What the bump buys beyond the nine, and why it is taken now rather than with those tickets:

- **The window groups its own modules**: `rune::desktop` → `rune::window`, `cursor` →
  `input/cursor`, `pixels` → `image/pixels`, `titlebar` → `window/titlebar`, `windows.rs` →
  `window/`. The public re-exports (`rune::Pixels`, `rune::WindowOptions`, `rune::Display`) are
  where they were, so this is a naming fact: a path written from an old screenshot of the tree is
  stale, and the names above are the current ones.
- **A wall that stays up**: `gpu/health.rs` and `render/gpu_loss.rs` replace a lost device or an
  unconfigured surface instead of panicking the process (#492); a window covered whole by other
  windows rests (#570); `clipboard/secret.rs` clears a secret on a timer. A display that is up for
  hours is the product, and a device loss used to be a coredump.
- **Resizing is layout, not a view run**: a commit whose only change is the viewport re-solves
  layout and re-breaks text, skipping the view. Panes resize on every gutter drag and every full-size
  toggle (#85), so this is `INV-12`'s cadence paid less often.
- **More newest surface for the wave**: `App::on_visibility` (#497) for a wall that is off screen,
  `rune::marks` (#569) for a launch trace, `El::{core_animation, icon_rotation, native_surface,
  surface_anchor, surface_unscaled, video}`, and the new modules later tickets name —
  `el/{help,input,paragraphs}.rs`, `overlay/{presence,lifetime}.rs`,
  `runtime/input/{hover_delay,press_run,key_releases}.rs`, `text/{inline_fit,synthetic_bold}.rs`,
  `render/breaker.rs`.

**Text rasterisation moved again**, exactly as at the last bump: all sixteen goldens differ, and
every difference is glyph pixels on a text row — the wordmark and header status, the card and menu
labels, the name plates, the sheet — with no geometry, colour or marker change. Measured before the
goldens were rewritten: `empty` differed on 1692 pixels of 921,600, confined to the header's 13 text
rows; `picture`, `stale`, `photo` and `settings-dismiss` on 1510, also header text only; the
menu-bearing scenes up to 5,482 where their labels are. `scripts/assert.sh` passed every structural
assertion against the new renderer before `scripts/capture.sh --update-goldens` was run, so the
pictures moved where text is drawn and nowhere else.

The frame budget is unchanged and still clear: two post-bump gate runs measured **2.9–3.8 ms** on
the reference machine for one 1280×720 pane with 200 markers panning (limit 8) and **3.8–10.7 ms**
for four 960×540 panes (limit 14), normalised by the same-run calibration workload. The spread is
the calibration workload's, not the wall's — #79 recorded the same effect — and both ends are inside
the limits, so the bump costs no budget and no budget decision.

### Bumped again: `14e3c5d` → `04389ed`

164 commits on, 2026-11-12. **The changes that move the wall's pixels are named, so the goldens'
diff is attributable rather than assumed** — and this time the direction is the one #130 asked for:

- **A stroke no wider than a device pixel draws as a hairline** (#616), as Chromium draws it. Every
  marker and facility symbol is a stroked glyph, so the antialiased edge of each symbol moves. The
  ink mask at the old threshold is *identical* and its best overlap is at (0,0): positions did not
  move, coverage did.
- **Run cells rasterize at their text's raster factor** (#599), so a count under a scaled tree is as
  crisp as the words beside it. The tree canvas (#127) is the call site, and this **removed the
  8/255 tolerance exception** those two scenes needed in `verify::golden_bounds` (#130): measured on
  lavapipe across two runs, the tree scenes now differ by **1/255**, so every scene keeps the
  two-step colour contract. The rounding counts did not fall (296 and 289 one-step pixels); the
  colour difference did, which is the difference between glyph coverage and glyph shape.
- **A scaled or moving canvas stays centred in its snapped box** (r267), again the tree canvas.
- **One mask fill for the CPU and the GPU** (#622, #630), and masks kept across launches (#684):
  the same bytes on both paths, which is the direction `INV-11` and the golden bounds want.
- `ImageZoom::gliding` (#685) is the predicate a glide-driven camera can poll; the tree camera does
  not glide (`TreeCamera::touched` fits once and stays), so there is no call site yet.

Two hours later the head moved two commits further, `205b8d4` → `04389ed` (the web accessibility
mirror's rebuild cadence and a macOS precompiled-pipeline path); neither touches Linux rendering, so the
pin is the newer rev and the goldens and measurements above stand.

**The goldens were regenerated after the diff was read**: every change is glyph or icon coverage on
a text/symbol row — the header text, name plates, card labels, marker and facility symbols — with no
geometry, colour or marker-count change. `scripts/assert.sh` passed every structural assertion
against the new renderer (`empty` has no marker ink; `panes` has 3 panes and 39 markers each) before
`scripts/capture.sh --update-goldens` was run. The diff is 1,300–13,600 pixels per scene, all at
symbol and glyph edges; the tree scenes are the largest because their text is scaled.

### Bumped again: `04389ed` → `25db9cd`

`04389ed930177f1f475ff3de5249304545b3d5b4` → `25db9cd122a6972816342fdb24db3c4ad0c72e0f`, in both
`rune` and `rune-kit` and in `THIRD-PARTY-NOTICES.md`, in one commit.

**Two commits, 2026-11-12**, and neither draws anything the wall can see:

- **`93fa605c` — *Exercise the sibling cache budget in the layer walk (r298)*.** **Tests only**: it
  tightens the walk in `crates/rune/src/render/layers/plan_tests.rs` so one frame provably keeps one
  sibling cache while borrowing the other (`Disposition::Cached { cache: None }`), and fixes the second
  spec's box to be the 32×16 its label claims. No production file is touched. A test that can now fail
  is a reason to take the rev, not a reason to fear it.
- **`25db9cd1` — *Windows: a launch trace splits the first frame's show (#721)*.** Five
  `crate::marks::mark` calls (`loop.built`, `window.frame_installed`, `frame.first.done`,
  `window.cloaked`, `window.uncloaked`), one of them inside `platform/win32/cloak.rs` (Windows-only
  module), and a changelog entry. `marks::mark` writes a tracing instant; it paints no pixels, and the
  hunk beside the custom title bar is under `#[cfg(target_os = "windows")]`.

So the pixel surfaces this ADR watches — `Text`, `Icons`, `Render`, `canvas`, `image` — are untouched,
and the honest evidence is **zero**: `scripts/capture.sh` reported every scene *identical to its golden,
byte for byte* (no rounding, no tolerance needed) before this section was written. That is the case a rev
bump is not allowed to invent: if a renderer change does not move the wall, the goldens must not move
either, and #134's rule that a bump is a reviewed commit rather than an automation is what makes the
difference sayable.

The rev is taken now because the pin should hold a state we have read, not the state we happened to have
read once: two commits behind is already a diff nobody reviewed, and the two we are behind are the two
easiest to review there will ever be.

### Bumped again: `25db9cd` → `8219692` — 83 commits, and the wall does not move

`25db9cd122a6972816342fdb24db3c4ad0c72e0f` → `82196923fcb43d493353455edab0113cb4493c38`, in both `rune`
and `rune-kit`, in `THIRD-PARTY-NOTICES.md`, and in `Cargo.lock`, in one commit. This is not the two-commit
bump of #134; it is **83 commits, 352 files, +26,638 / −1,583**, filed by the upstream tracker as #159 — the
largest range this pin has ever taken, and the first where the honest evidence is expected to be *something*
rather than *nothing*.

**What it is made of.** Twenty commit subjects are Windows platform work (occlusion and covered draws, CPU-first
hand-over, `ITaskbarList3`, jump lists, D3D12 idle threads, vendored winit), two are macOS (CoreText fallback
weights, the Dock's badge), and of the rest, eight are text or fonts, six are accessibility and focus, and the
tail is media locking, video layers, notifications, Luau, and three r281 layer-cache fixes. Sixteen of the 83
touch the pixel surfaces this ADR watches (`text/`, `render/`, `edit.rs`, `rune-kit`'s search) — 23 files, +1,469
— and 7,849 more lines arrive in `vendor/parley_engine`, the shaping engine the text commits are built on.

**Every one of those sixteen is either a cache behind a claim of identical output, or a script this wall does
not write.** Grouped, because that is the review:

- **Shaping and glyph coverage.** `#725` gives the vendored parley engine mixed-direction text (itemisation, an
  isolate-aware shaper, an LRU of shaped atoms); `#745` CJK forms and language; `#788` a line-break strictness
  switch; `#789` Hiragino's weights; `#764` COLR v1 gradients; `#779` a COLR painter 15–40% faster. The wall's
  text is left-to-right Latin in Inter and IBM Plex Mono, and four of those six draw no glyph it has. The two
  that could move a Latin glyph claim the opposite: `#782` states *"every layout is identical (glyphs,
  positions, clusters, line boxes)"* for a 14% faster shaping path, and `#779` states *"every byte of the images
  identical"*.
- **Font resolution, which is the one that could really have moved something.** `#766` replaces Linux's folder
  walk and its `fc-list`/`fc-match` subprocesses with an in-process fontconfig match, and macOS's folder walk
  with CoreText's own fallback. A fallback happens when a character the app's fonts lack is drawn — on this
  wall, an arrow or a symbol inside an operator's own name, never in the fixtures. It also means a cold
  fallback no longer forks a process, which is the kind of change a wall notices at start-up and not in a
  picture. `objc2-core-text` entering `Cargo.lock` is this commit, and it is Apple-only.
- **Caches that only write.** `#746` re-uploads only the atlas rows that hold glyphs when the GPU comes back
  (*"snapshots identical"*), `#784` lets a resting window write its mask cache, and the three r281 layer
  commits rebind parked targets once, bind them against the new outline texture, and composite a cache hit
  with the layer's turn. The wall has no turned instances and no layer cache of its own; the modal scrim is a
  plain draw.

So the picture is the easy half. **The range's real content for us is behaviour a settled golden cannot see**,
and that is what the ticket's integration question was for:

- **Backspace deletes one code point, as Chromium does** (`#764`, `edit.rs` and `edit/input.rs`). Before, it
  deleted a whole grapheme cluster. This is *our* field: `begin_rename` (#83, #113, #139) and the workspace
  chip's rename (#154). A Latin name is unchanged; a name typed with a combining accent now loses the accent
  alone. Named here because it is the one commit in the range that changes what an operator can do.
- **Accessibility changed shape without a pixel moving** (`#726`, `#727`, `#732`, `#734`, `#763`, `#778`): a box
  that only takes a press is no longer a button to a screen reader, tree items and grid cells with a press are
  Tab stops under `Cx::web_focus`, every hover group lights while keyboard focus is inside it, focus falls back
  to a surviving neighbour when its element leaves, and F6 cycles landmarks. The wall's scrims and its pane
  press-boxes are exactly the *"surface that is not a control"* case, and `hover_group_focus` is now
  `hover_group` — the menu's keyboard model (#164) gets its affordance without us building one.
- **Motion has a cause** (`#750`): a commit no input in its window caused mounts and unmounts at once, with no
  entrance transition. The wall animates nothing — `src/units/motion.rs` is motion *semantics* over fixes, not
  a transition — so this is not ours to feel. It is recorded because the day we add a transition, the escape
  hatch is `Tx::animated`, and the difference between a data-driven arrival and an input-driven one becomes a
  decision rather than an accident.
- **`rune::exit_deadline(after)`** (`#787`): a quit that runs long now ends, and says where it waited. The
  wall's `SIGTERM` path already quits **through Rune's own quit path** so the map thread's GL context is
  dropped in order (`src/platform.rs`), which means a map thread that will not let go can hold the process past
  systemd's `TimeoutStopSec` and earn a `SIGKILL`. Arming a deadline after `flush_layout` (#153) is now
  possible and is **not taken here**: a bump is a dependency change, and that is a behaviour change. It is its
  own ticket, [#174](https://github.com/edwardmanhattan/arcons-cc/issues/174).

**What the control-plane epic (#162) gets for free** — the half of the ticket the bump exists to answer:

- **`El::on_background_press`** (#732): a pointer's press on a surface that is not a control, without becoming
  a button or a Tab stop. That is precisely what a remote's cursor press on the water needs (#167), and what
  #154's chip right-click did by hand.
- **`El::labelled_by` / `described_by` / `busy`, `Tx::announce`, `Tx::assistive_technology`** (#732, #734, #778):
  the menu's rows can name themselves from their own titles, a rename can announce its result, and work that
  only an assistive client wants costs nothing without one. The menu's keyboard model (#164) is now
  constructible rather than aspirational.
- **F6 landmarks and focus fallback** (#727): a modal a remote opens has a focus story for the moment its
  control disappears — which is #166, closing panes under a focus ring.
- **`RUNE_REGION_MISS`** (#730): the tool the next budget regression needs, because it names what made a memo
  or region build again. Our frame budget is a gate, so this is the diagnostic that comes with it.
- **Script IME steps** (#735): `{"type": "ime", "preedit"…}` and `{"type": "ime", "commit"…}` drive an
  input method in a headless run. Our fixtures are Latin and typed with `{"type": "type"…}`; recorded because
  it is how a future capture proves typing without touching the machine's layout.
- **Bidi and search folding** (`El::isolate`, `El::dir_by_paragraph`, `text::LINE_BREAK`,
  `rune_kit::search::Fold::for_language`): the wall has no bidi text and no CJK, and its filter matches its own
  vocabulary. Noted, unused — `icu_normalizer` in the lockfile is kit's search folding, not ours.

**The evidence is zero, and this time zero is a claim worth making.** `ARCONS_GOLDENS=exact scripts/capture.sh`
rendered all **40 scenes twice each, and every scene is identical to its golden byte for byte** — no tolerance,
no rounding budget, no one-step pixels:

```
40 scenes, exit 0 — "identical to fixtures/golden/<scene>.png — byte for byte"
```

That is 83 commits, 16 of them in the renderer, and not one pixel of the wall. The gate is otherwise green
(`cargo fmt`, `clippy -D warnings`, 40 scenes plus their assertions, the frame budget), and `Cargo.lock` moved
only the six `rune*` git sources plus two transitives — `icu_normalizer` and the Apple-only
`objc2-core-text` — neither copyleft, both recorded in `THIRD-PARTY-NOTICES.md`.

If a future reader wonders why this section is longer than the bump: because 83 commits is where a renderer
change *hides*, and the only way to say it is not there is to have looked at each of the sixteen places it
could be and to have the goldens say the rest.


## Decision 2 — MapLibre Native (`maplibre_native`)

Why: it renders a style with vector tiles **from a local seed**, which is what "cold start with no
network" requires; it has a `Continuous` renderer that suits a moving camera; and it has a Vulkan
backend on Linux, the same driver stack Rune uses.

**Reviewed for #10 (2026-11-07): keep it.** `docs/research/map-engines.md` compares it against
`maplibre-rs`, `galileo`, `versatiles` and a custom raster renderer on the six criteria that decide
this (offline, text quality, frame cost, licence, packaging, maintenance). It is the only candidate
that meets the first two today — a 2.6 MB seed that renders byte-identically with no network
namespace, and the liberty style's own labels — and the costs we carry (build, vendored patch, and a
full RGBA frame copied through the CPU every frame) are measured there. That document names the
triggers that re-open the question; this ADR changes the day one of them fires.

**The `map` feature is off in the gate**, and that is the design rather than a delay: its build
script fetches and compiles C++, and `scripts/verify.sh` promises *no network, no display, no GPU
beyond software Vulkan, every run*. The projection, the camera and the scheduling are therefore in
`src/map/geometry.rs` and `src/map/thread.rs` with **no MapLibre in them**, and the engine lives
behind `--features map` (vendored at `vendor/maplibre_native`, see `vendor/README.md`).


**The wall's binary is built from the precompiled amalgam** (`scripts/build-wall.sh`, #26). The local
checkout route compiles the same C++ with this machine's compiler, and at `-O2` and above the result
segfaults on the first map frame inside glslang (`-O0` and the debug build are fine, upstream's
artifact is fine). The local route stays for development; the wall gets the artifact upstream built.

Three ways to build the engine, measured on this machine (2026-11-06):

| how | cost | when |
| --- | --- | --- |
| `MLN_PRECOMPILE=1` | downloads upstream's prebuilt amalgam: **378 MB** static lib + 0.5 MB headers, then links. Whole crate built in **46 s**. | the default for a developer, and the only route that never compiles C++ |
| `MLN_LOCAL_REPOSITORY=<checkout>` | compiles the C++ from an existing checkout — no 2 GB clone. **~4.5 min** incremental here. | this laptop, via `~/.cargo/config.toml`, pointing at the TFG checkout it already has |
| neither | clones ~2 GB of C++ and submodules, then builds it | the fallback nobody should pay for twice |

The map seed lives at **`assets/tiles-cache.seed.sqlite`** (2.6 MB, recorded from TFG's cache). It is
embedded with `include_bytes!` so a wall install cannot be missing it beside the binary, and it is
copied to a writable cache on first run because MapLibre writes tiles into the same file. Every
`resources`/`tiles` `expires` row is pinned to 2100 **before** MapLibre opens the cache: the seed was
captured with ordinary HTTP expiry, so without pinning a fresh install asks the network to revalidate
the style and a machine with no network can never report style-loaded.

**Both sides project with the same camera, by construction.** The map thread publishes the camera and
size of the frame it rendered (`map::geometry::Rendered`); markers call `Rendered::project`, which is
the same `project` the picture was drawn against. There is one lat/lon → pixel conversion in the
client, and `INV-04`'s test fails the moment a second one appears.

## Decision 3 — the UI thread never blocks

Reads, the socket and the map each own a thread and a runtime of their own; the view loop reads what
they last published and draws it. Rune's view function runs between frames with `&mut State`, so a
blocking call there stops the compositor's frame loop, not just the request. This is why `reqwest`
carries `blocking` and `tokio` exists at all, and why the socket SDK is vendored.

## Hardware note — the hybrid laptop, recorded because it is load-bearing

This machine's panel is wired to the Intel iGPU; there is also an RTX 2050 that is not driving
anything. Rune picks the adapter and `RUNE_GPU_LOG=1` says which. Recorded here from the scaffold's
own run, on Wayland under Hyprland:

```
rune: GPU "Intel(R) Graphics (RPL-P)" (wgpu; Vulkan, IntegratedGpu, 0000:00:02.0):
      the compositor's GPU; compositor on 0000:00:02.0
rune: a surface draws with wgpu Vulkan on Intel(R) Graphics (RPL-P)
```

Under XWayland (`DISPLAY=:0`, no `WAYLAND_DISPLAY`) the same adapter is chosen, for the documented
reason `lowest power`. **The map must land on that adapter too**: paying a discrete-GPU copy per
frame is a stutter on every pan, and it would show up as `INV-12`'s frame CPU budget rather than as
an error. That is why #5 has to say which adapter it used, not just that it drew.

## Hardware note 2 — the map renders on the compositor's GPU too (#16)

Rune chooses the compositor's GPU. **MapLibre does not, by itself**: it enumerates the Vulkan
physical devices, sorts them *itself*, and takes the first. Measured on this laptop, 2026-11-06,
before the fix:

```
[INFO] {arcons-map}[Render]:      Sorted order:
[INFO] {arcons-map}[Render]:            [0] NVIDIA GeForce RTX 2050
[INFO] {arcons-map}[Render]:        Using "NVIDIA GeForce RTX 2050" with driver: "/usr/lib/libGLX_nvidia.so.615.78.08"
rune: GPU "Intel(R) Graphics (RPL-P)" (wgpu; Vulkan, IntegratedGpu, 0000:00:02.0): the compositor's GPU
```

Every map frame would be shaded on a GPU nobody is looking at, then read back across the bus at 4 Hz —
a laptop that never idles, and (later) an INV-12 budget number nobody can explain.

Mesa's `MESA_VK_DEVICE_SELECT` does not move it: MapLibre picks from its own list. **Limiting the
Vulkan loader to one ICD does**, because then there is only one device to pick. `arcons-cc` sets
`VK_DRIVER_FILES`/`VK_ICD_FILENAMES` from `main`, before any thread starts, choosing:

- `ARCONS_MAP_VK_DRIVER=<path>` — that ICD manifest, verbatim;
- `ARCONS_MAP_VK_DRIVER=off` — leave it to the loader;
- unset or `auto` — **the integrated GPU**, because on a hybrid laptop that is the one the panel hangs
  off and it is what Rune picks. Ad-hoc ranking, not filtering: this machine offers
  `intel_hasvk_icd.json` (Haswell-era) beside `intel_icd.json`, and the legacy one sorts first and
  finds no device on a modern iGPU. `lvp`/`swrast` (lavapipe) is never preferred: it is what a CI
  runner has, not what a wall has.

After the fix, one log line from each half of the window, on the same device:

```
arcons-cc: the map renders through /usr/share/vulkan/icd.d/intel_icd.json — the integrated GPU, where the panel is
[INFO] {arcons-map}[Render]:        Using "Intel(R) Graphics (RPL-P)" with driver: "/usr/lib/libvulkan_intel.so"
rune: a surface draws with wgpu Vulkan on Intel(R) Graphics (RPL-P)
```

Two sharp edges, recorded because they cost coredumps to find:

- A `VK_DRIVER_FILES` path the loader accepts but which drives no usable device makes MapLibre throw
  a C++ exception that nothing on the Rust side can catch: the process aborts. `choose_driver` refuses
  a path that is not a file and ranks the legacy Intel driver away, which covers the two ways to get
  there by accident; the remaining one is a driver genuinely broken on the machine.
- The ICD list is read from the loader's own search paths (`$HOME`, `$XDG_DATA_DIRS`,
  `/usr/local/share`, `/usr/share`), so a distro that keeps manifests somewhere else falls back to
  "the loader decides" rather than guessing.

## What is deferred

| deferred | owner | why |
| --- | --- | --- |
| the map feature inside the gate | #9 | it needs a C++ toolchain or a 378 MB download; the gate stays free of both |
| a test that names the adapter the map got | #16 follow-up | the evidence is two log lines today; an assertable report needs MapLibre to expose its device |
| `vendor/rune` | before v0.1 ships | it is a git dependency today; vendoring makes the build reproducible offline |
| true fullscreen | #8 | Rune exposes it as `Tx::toggle_full_screen`; `--fullscreen` is still borderless at the primary output's size until #8 picks the output and decides when to toggle |
| the exercise plot, orders, participants | #11 | out of scope for v0.1 on purpose |

## Consequences

- Verification works without a display, which is what makes `scripts/verify.sh` the gate.
- A Rune rev bump is a `Cargo.toml` + ADR change in one commit, not a `cargo update`.
- The map's build cost arrives in one prepared ticket instead of on every clone.

### Read and **declined**: `8219692` → `09ab92a5`, because the range cannot build on macOS

The tracker filed #185: `dev` had moved 104 commits past the pin. The range was read the way a bump is read — every
commit that touches a pixel surface named, and the gate run to see what moved — and then it was **not taken**, because
the range does not compile on macOS for anything that depends on Rune rather than being Rune.

**The defect.** `#791` ("Input methods: compositions carry their clauses, and committed text can be converted again")
is the fourth commit in the range, and it makes `rune`'s macOS path call `winit::platform::macos::{set_ime_text,
take_ime_clauses, take_ime_replacement}`. Those functions are not in the published `winit` 0.30.13; they are in
**Rune's own vendored copy**, applied by `[patch.crates-io]` in *Rune's* manifest. Cargo applies patches from the
workspace root only, so an app that depends on Rune by rev gets the unpatched crate and:

```
error[E0425]: cannot find function `set_ime_text` in module `winit::platform::macos`
error[E0425]: cannot find function `take_ime_clauses` in module `winit::platform::macos`
error[E0425]: cannot find function `take_ime_replacement` in module `winit::platform::macos`
error: could not compile `rune` (lib) due to 3 previous errors
```

`check (macos-latest)` is a real gate, so the bump is a red PR. Taking it would mean vendoring `winit` ourselves — 3.1 MB,
175 files, re-made on **every** future bump — to repair a packaging defect in a dependency. That is a project decision
with a permanent maintenance cost, not a bookkeeping one, so the pin holds at
`82196923fcb43d493353455edab0113cb4493c38` and #185 stays open with this analysis.

**What the review found anyway, for the day it is takeable.** The only pixel movement would have been icon edges: with
the range built on Linux, 45 scenes differed from their goldens in `rune::Icon` antialiasing alone — two rows of each
workspace chip's glyph, the pane header's glyphs, the settings glyph (~100 px a scene, brightest channel +45) — from
`#828`, icon elements covered with the op Chromium's Ganesh picks. Nothing else moved: not text (`#862`'s weight
ranges, `#878`/`#872`'s paint caches), not markers (`#885`'s rounded-containment snap on integral rectangles), not the
wheel (`#856`'s Chromium distances — the wall's wheel intents carry their own deltas).

**What would change this decision.** Any of: the IME extensions reaching a published `winit` (or a `rune` feature that
gates them), a rev whose macOS path does not need them, or an explicit decision that this wall vendors `winit` — which
belongs in `vendor/README.md` with the re-vendor step, not in a bump commit.
