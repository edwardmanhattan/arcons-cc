# Is `maplibre_native` the right map engine? (#10)

- **Ticket:** [#10](https://github.com/edwardmanhattan/arcons-cc/issues/10), under epic #1
- **Date:** 2026-11-07
- **Verdict: stay on `maplibre_native` 0.9.3 for v0.1 and v0.2.** The exit ramp exists and is named
  at the end, but nothing measured here is worse than starting over on an engine that cannot yet
  draw the wall's map.

The question was "is there a cheaper engine that still does what the wall needs?" The answer is no:
the alternatives either give up label quality (the wall's map is *place names plus a picture*), or
give up the style (we would write a style engine), or give up offline cold start. What this ticket
did find is a **shipping blocker in the engine we have** — see
[the release-mode crash](#finding-a-release-build-segfaults-in-the-engine).

## What the wall needs, in order

The ticket's six criteria, restated as things the wall actually does, because the order matters more
than the scores:

1. **Offline.** The wall boots with no network and shows a map. Not "caches after first load": the
   first load is in a room with no Wi-Fi.
2. **Text.** An operator across the room has to read *Jakarta*, *Sunda Kelapa*, *Sungai Ciliwung*.
   A basemap without labels is a picture of Indonesia, not a map.
3. **Frame cost.** Fixes arrive continuously; the map moves under the markers at the wall's cadence.
4. **Licence.** Proprietary delivery, no root `LICENSE` (README), notices list.
5. **Build and packaging.** A new machine, a new clone, a working app — for a team that will hand
   this to a site engineer.
6. **Maintenance.** It has to keep working after we stop looking.

## Candidates, with the facts that decided them

### `maplibre_native` 0.9.3 (current, vendored)

C++ MapLibre Native behind FFI, with a `Continuous` renderer that hands back RGBA. BSD-2-Clause
(`THIRD-PARTY-NOTICES.md`). Renders a **style** (the liberty style from OpenFreeMap), so hills,
water, roads and labels all come from the style document rather than from us.

Measured, today, on this laptop:

| | |
| --- | --- |
| **Offline** | `assets/tiles-cache.seed.sqlite`, **2.6 MB**, 36 tiles, z6–z12, style document and glyphs included. `--probe-map` under `unshare -rn` (no network namespace) produces a **byte-identical** PNG to the online run: `sha256 7e83c87b…`. |
| **Text** | `out/probe.png`, 1280×720 at z11 over Jakarta: place names, water names, road shields, all with halos (`text-halo-*` from the style). Vector glyphs are rasterised at device scale, so 1× and 1.25× are both sharp — the text is a *style* feature, not an image. |
| **Frame cost** | warm, one frame including RGBA readback, debug build: **1280×720 median 45.5 ms / p95 58.3 ms**; **1920×1080 median 65.2 ms / p95 79.5 ms** (`cargo run --features map --example map_frame_cost 1920x1080`). Debug, so a floor rather than a ceiling — and the only number that exists, because **the release build segfaults** (below). |
| **GPU sharing** | **No shared texture.** `frame_rgba()` is `render_once()` + `read_still_image()`: the whole frame comes back through the CPU as straight-alpha RGBA — 1920×1080×4 = **8.29 MB per frame**, 33 MB/s at the wall's 4 Hz — and Rune uploads it again as a `Pixels`. |
| **Licence** | BSD-2-Clause, in the notices; no copyleft in the graph (`tests/notices.rs`). |
| **Build** | three routes, measured 2026-11-06 (ADR-0001): prebuilt amalgam **378 MB download → 46 s** link; local C++ checkout **~4.5 min**; fresh clone **~2 GB**. A full clean `--release --features map` build of this crate took **14 min 51 s** today, UI and all. |
| **Maintenance** | `maplibre/maplibre-native`: 2,304 stars, pushed daily, but **621 open issues**. The Rust wrapper is one crate, 0.9.x; we already carry a vendored patch (`vendor/README.md`). |

### `maplibre-rs`

Rust-native MapLibre, Apache-2.0, 1,588 stars, 70 open issues, not archived, last push 2026-11-06.
Its README carries a `stability: experimental` badge, and it does have SDF text
(`maplibre/src/sdf/text.rs`, `util/label.rs`).

Why not: it is a **rendering library, not an application map**. There is no offline tile cache, no
"render this camera to a buffer" entry point that does not involve standing up its own render loop,
and no style-complete pipeline we could point at liberty. Adopting it means writing the parts #5 got
from MapLibre — cache, style, glyph atlas, camera — and ending with *less* of the style understood.
That is a rewrite with a worse ceiling, not a cheaper engine.

### `galileo` 0.2.1 (+ `galileo-maplibre`)

MIT/Apache-2.0, 653 stars, 43 open issues, pushed today — the liveliest alternative, and the one
worth watching. wgpu backend (Vulkan, GL, Metal), and `galileo-egui` demonstrates exactly the thing
MapLibre cannot do here: **the map as a wgpu texture**, drawn by the UI with no CPU copy.

Why not yet:

- `galileo-maplibre` is **WIP by its own README**, and its translation of symbol layers is partial:
  `symbol_rule` handles `SymbolPlacement::Point` (or none) and logs `UNSUPPORTED` for the rest — so
  line-following labels (river names, which the liberty style uses) fall away, as do expressions it
  cannot convert. Text itself is real (`galileo/src/render/text/`, rustybuzz, font provider).
- Offline is **ours to build**: galileo takes tile sources and a cache policy as an application
  decision (its README says so explicitly), where MapLibre gave us a SQLite cache and a seed.
- Version 0.2.1: the API is pre-stable by its own roadmap (v0.4 "stabilize API").

This is the candidate that becomes right if we outgrow the CPU copy or need a map inside a Rune
texture. It is not right today, because it would cost us the labels the wall exists to show.

### `versatiles` (versatiles-rs)

MIT, 332 stars, active. **It is not a renderer.** The crates are `versatiles_container`,
`versatiles_pipeline`, `versatiles_geometry`, `versatiles_image`, `versatiles_node` — a tile
container format, conversion pipeline, CLI and server. Nothing draws.

Its real value to us is `versatiles reduce -s style.json`: prune a tileset down to exactly the layers
one style draws. That is a **packaging** win for whatever renders — a smaller seed than 2.6 MB, or a
`galileo` seed built from a Shortbread tileset. It pairs with a renderer; it cannot be one.

### Custom raster renderer over pre-rendered tiles

The simplest possible thing: PNG tiles at a fixed zoom, drawn by Rune, camera by `map::geometry`
(which already exists and is engine-free). Cheap to build, no C++, no FFI, a seed we control.

Rejected for v0.2 and named as the fallback for v0.3+: it **bakes the labels into pixels**, so the
map is only sharp at the zoom it was cut for, and a wall at 1.25× scale shows soft text. It also
gives up re-styling (night mode, decluttering) — the thing that makes a wall display readable from
across a room.

## The table

Scale: ✅ meets it with evidence · ⚠️ meets it with a caveat we would carry · ❌ does not meet it.

| criterion | `maplibre_native` | `maplibre-rs` | `galileo` | `versatiles` | custom raster |
| --- | --- | --- | --- | --- | --- |
| **1. Offline cold start** | ✅ 2.6 MB seed, z6–z12, byte-identical render with no network namespace | ❌ no cache; ours to write | ⚠️ tile sources and cache are the app's job | ✅ *is* the offline tile store | ✅ PNG tiles on disk |
| **2. Text quality** | ✅ style-driven vector glyphs with halos, sharp at 1×/1.25× | ⚠️ SDF text exists, style incomplete | ⚠️ real text (rustybuzz), but Point placement only; line labels skipped | n/a — draws nothing | ❌ baked into tiles |
| **3. Frame cost** | ⚠️ 45.5 / 65.2 ms median at 720p / 1080p in **debug**; release **crashes**; **8.29 MB CPU copy per frame** | ❓ no measurement: no integration exists to measure | ⚠️ unmeasured here; wgpu texture shared with the UI, no CPU copy | n/a | ✅ a few textured quads; unmeasured |
| **4. Licence** | ✅ BSD-2 | ✅ Apache-2.0 (+MIT) | ✅ MIT/Apache-2.0 | ✅ MIT | ✅ ours |
| **5. Build and packaging** | ⚠️ 378 MB prebuilt or ~2 GB clone; C++ toolchain; 46 s–15 min; vendored patch | ✅ pure Rust | ✅ pure Rust (wgpu) | ✅ pure Rust | ✅ no deps |
| **6. Maintenance** | ⚠️ upstream active, 621 open issues, wrapper 0.9.x | ⚠️ experimental badge, 70 issues | ⚠️ 0.2.1, API pre-stable, but daily commits | ✅ active, narrow scope | ✅ ours |

## Measurements, and how to repeat them

```sh
# Offline: the same pixels with no network at all.
unshare -rn ./target/debug/arcons-cc --probe-map out/probe.png
sha256sum out/probe.png            # 7e83c87b302576f78c9165110271ffd3461061971efc9fb078f2a17b2504375f

# Frame cost (needs --features map; not in the gate: it wants a GPU).
cargo run --features map --example map_frame_cost            # 1280x720
cargo run --features map --example map_frame_cost 1920x1080
```

The harness is `examples/map_frame_cost.rs`: one `LiveMap` per process, warm (`pump(8)`), then 30
frames with 120 ms between them, timing `frame_rgba()` — render plus readback, which is what the wall
pays. Not in `scripts/verify.sh` on purpose: it needs the `map` feature and a real GPU, and the gate
promises neither.

## Finding: a release build segfaults in the engine

`cargo build --release --features map` and then `--probe-map` **segfaults**, while the identical debug
build renders. Coredump:

```
#0  libglslang.so.16
#5  mln::vulkan::ShaderProgram::ShaderProgram(...)
#9  mln::RenderLineLayer::update(...)
#10 mln::RenderOrchestrator::updateLayers(...)
#11 mln::Renderer::Impl::render(...)
```

A C++ crash in shader compilation, reached from the line layer, in release only. It is not this
ticket's to fix — it is a **v0.1 shipping blocker**: the wall runs the binary we build, and we build
release. Filed as
[#26](https://github.com/edwardmanhattan/arcons-cc/issues/26) with the coredump and the reproduction.

Two consequences for this document:

- The frame-cost numbers above are **debug** numbers. They are the only honest ones until #26 is
  understood, and they are a floor: a release build should be faster, if it ever renders.
- Criterion 3 for `maplibre_native` is ⚠️ rather than ✅ for a second, independent reason: the CPU
  copy. Even when fast, it is the wrong shape for sharing a GPU with Rune.

## Recommendation

**Stay on `maplibre_native` for v0.1 and v0.2.** It is the only candidate that meets criteria 1 and 2
today, and those are the two the wall cannot trade away: no network at cold start, and readable place
names from across the room. Everything else about it is a cost we already pay and can name — the
build, the vendored patch, the CPU copy, the 621 upstream issues.

ADR-0001 stands; the pointer added there says so.

### Exit triggers — the ramp, so it is a decision and not a drift

Re-open this ticket, with measurements, when **any** of these becomes true:

1. **#26 cannot be fixed without forking MapLibre's Vulkan backend.** If release cannot render, the
   engine is not shippable and the ramp is used earlier than planned.
2. **The CPU copy becomes the frame.** At 4 Hz the copy is 33 MB/s and invisible; at 30 Hz for a
   smoother wall it is 250 MB/s and it is the whole budget. That is the moment `galileo`'s
   shared-texture path is worth the label work.
3. **`galileo-maplibre` covers symbol placement** (line labels, not just points) and the expression
   subset liberty needs. Then the comparison is honest and cheap to re-run — same harness, same
   criteria table.
4. **Packaging bites**: a site install that cannot ship a 378 MB artifact or take a 15-minute build
   is a product problem that no amount of engine work fixes, but it is the trigger to look at
   `versatiles reduce` and a pre-rendered basemap again.

## Sources

- `maplibre/maplibre-native` — <https://github.com/maplibre/maplibre-native>, BSD-2-Clause, 2,304 stars,
  621 open issues, pushed 2026-11-07 (GitHub API, read 2026-11-07).
- `maplibre/maplibre-rs` — <https://github.com/maplibre/maplibre-rs>, Apache-2.0, 1,588 stars, 70 open
  issues, `stability: experimental` badge, pushed 2026-11-06; SDF text in `maplibre/src/sdf/`.
- `Maximkaaa/galileo` — <https://github.com/Maximkaaa/galileo>, MIT/Apache-2.0, 653 stars, 43 open
  issues, version 0.2.1, wgpu 30, pushed 2026-11-07; `galileo-maplibre/README.md` says "This crate is
  WIP"; `symbol_rule` in `galileo-maplibre/src/layer/vector_tile.rs` handles `SymbolPlacement::Point`
  and logs unsupported layers; text via `galileo/src/render/text/`.
- `versatiles-org/versatiles-rs` — <https://github.com/versatiles-org/versatiles-rs>, MIT, 332 stars,
  crates `versatiles_container` / `_pipeline` / `_geometry` / `_image` / `_node`; `reduce` documented
  in its README.
- Our own: `docs/adr/0001-rune-and-maplibre.md` (build routes, GPU choice),
  `vendor/README.md` (the patch), `THIRD-PARTY-NOTICES.md` (licence), `assets/tiles-cache.seed.sqlite`
  (the seed), `examples/map_frame_cost.rs` (the harness).
