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

**Deferred, deliberately, and this is the one place the scaffold does not wire the pin up:**
`maplibre_native` is declared in `Cargo.toml` as an **optional** dependency behind the `map` feature,
off by default. Its build script checks out and compiles roughly 2 GB of C++. Making that the price
of `cargo test` would break the gate's promise — *no network, no display, no GPU beyond software
Vulkan, every run* — and would block every ticket on a C++ build. #5 owns turning `map` on, vendoring
the crate (`vendor/maplibre_native`, see `vendor/README.md`) and paying the build once, in a commit
that says so.

The map seed lives at **`assets/tiles`** (see `src/map.rs`). The directory is in the tree, empty
until #5 records a seed; `--self-check` distinguishes "no seed directory" from "a directory with no
tiles", and #12's doc-claim test fails if the path named here does not exist.

**Both sides must project with the same camera.** The map owns the camera it last rendered; markers
are drawn against that camera, not re-derived (`INV-04`).

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

## What is deferred

| deferred | owner | why |
| --- | --- | --- |
| `map` feature on, `maplibre_native` vendored | #5 | 2 GB of C++ per build; the gate must stay network-free |
| `vendor/rune` | before v0.1 ships | it is a git dependency today; vendoring makes the build reproducible offline |
| true fullscreen | #8 | Rune at this rev exposes no fullscreen API; `--fullscreen` is borderless at the primary output's size |
| the exercise plot, orders, participants | #11 | out of scope for v0.1 on purpose |

## Consequences

- Verification works without a display, which is what makes `scripts/verify.sh` the gate.
- A Rune rev bump is a `Cargo.toml` + ADR change in one commit, not a `cargo update`.
- The map's build cost arrives in one prepared ticket instead of on every clone.
