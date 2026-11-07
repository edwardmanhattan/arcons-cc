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
rune = "7ead7d63fcf6f9dd9669385a8b90e2ae80015d7b"
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
| true fullscreen | #8 | Rune at this rev exposes no fullscreen API; `--fullscreen` is borderless at the primary output's size |
| the exercise plot, orders, participants | #11 | out of scope for v0.1 on purpose |

## Consequences

- Verification works without a display, which is what makes `scripts/verify.sh` the gate.
- A Rune rev bump is a `Cargo.toml` + ADR change in one commit, not a `cargo update`.
- The map's build cost arrives in one prepared ticket instead of on every clone.
