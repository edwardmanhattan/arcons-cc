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
