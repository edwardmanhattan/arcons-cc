# Multi View: N panes in one window (#64)

- **Ticket:** [#64](https://github.com/edwardmanhattan/arcons-cc/issues/64), under epic #1 (v0.2)
- **Date:** 2026-11-09
- **Machine:** this laptop, Intel iGPU, Wayland under Hyprland — the reference machine. Rune
  `46cfcd64` (after [#65](https://github.com/edwardmanhattan/arcons-cc/issues/65)), `maplibre_native`
  0.9.3 vendored, **debug** builds (the wall's release route and its numbers are in
  [`map-engines.md`](map-engines.md)).
- **Verdict: one `LiveMap` per pane, one thread each, with a per-pane render cadence — and the map
  engine cannot do it any other way.** The engine has one camera and one viewport *per renderer*:
  there is no viewport rectangle in the API to render N views in one pass. So the question the
  ticket asked ("can one `LiveMap` render N viewports?") is **no at the API level**, and what is
  left is a memory/independence trade, measured below: **13 % of RSS** against serialising every
  pane onto one render loop.

## 1. The feature

Right-click the map → **Focus on new pane** → a pane at that spot and that zoom. Panes are unlimited,
tiled dynamically (a binary tree, the shape a tiling WM uses), moved by dragging a handle or a border,
all inside **one OS window** — the wall is a kiosk surface and a pane must not be losable behind a
terminal.

The constraint that decides the design is not the layout; it is the map engine's frame cost. One map
frame at 1080p is **42.5 ms** in release (30.9 ms at 720p, [`map-engines.md`](map-engines.md)), and a
60 fps frame clock is 16.6 ms. N panes are N map renders. That is why the design below is about
*cadence* as much as about tiling.

## 2. What Rune gives us, checked at the pin we are on

Every symbol in this table was read in the checkout at `46cfcd64`, not from memory or from the ticket:

| need | at `46cfcd64` | where |
| --- | --- | --- |
| right-click at a point **in the map's own coordinates** | ✅ `El::on_secondary_press_local(fn(&mut S, PointerEvent, &mut Tx))` | `crates/rune/src/el.rs:945` |
| a context menu we draw | ✅ `Kit::anchored(..)` / `El::anchored(key, Side, Align, gap)` | `crates/rune-kit/src/popover.rs:19` |
| a **native** popup menu | ⚠️ macOS and Windows only — **not Linux**, so the wall's menu is a drawn overlay | `popup_menu/*` |
| tiled layout primitives | ✅ `row()`/`col()`, `.grow()`, `.absolute()`, `.size()` | `crates/rune/src/el.rs` |
| generic N-pane tiling | ❌ still nothing upstream: `SplitView` is a fixed sidebar/main/inspector shell | `crates/rune-kit/src/shell/split.rs` |
| a resizable pane with a grip | ✅ `PaneState` (app-owned width, min/max, `resizing`) and `SplitView`'s conventions — the leaf we want | `crates/rune-kit/src/shell/pane.rs:14` |
| drag a thing to a new place, gap opens where it lands | ✅ `TabDragState` (`set_incoming`, `incoming_slot`), `TabReorder`, `Reorder` | `crates/rune-kit/src/shell/tab_strip/drag.rs:16` |
| a matched element that flies to its new place | ✅ `El::matched_crop` | `crates/rune/src/el/motion.rs` |
| focus for "the active pane" | ✅ `Tx::focus`, `El::autofocus`, `ModalFocus`, `El::focus_fill` | `crates/rune/src/runtime/input/modal_focus.rs`, `el/focus.rs` |
| timers as events | ✅ `Tx::clock`, `Tx::wake_at(due)` | `crates/rune/src/el/tx.rs:212,220` |
| hit test at a pointer (drop targets) | ✅ `Tx::element_at{,_in,_with}` → `Picked` | `crates/rune/src/el/tx.rs` |

So: **we still write the tiling tree and the pane menu, and we copy `TabDragState`'s incoming-gap
model rather than inventing a drag.** Everything that made this feature expensive in the ticket's
original survey is in the pin now, because #65 took it.

One correction to note in passing: `examples/map_frame_cost.rs` says "a second `LiveMap` in the same
process crashes glslang". **Spike A below loaded four and rendered eleven frames each without a
crash.** That sentence described the from-source `-O2` build of #26, not the debug route the local
tree uses; the comment there now points at this document.

## 3. Spike A — N `LiveMap`s in one process

`cargo run --features map --example map_panes -- a <n> <w>x<h> shared|separate`, one process per `N`
so a crash cannot take the smaller numbers with it. Each pane gets its own camera over the same
archipelago, so every pane draws real tiles. Frames are one per pane per round, sequentially, with the
120 ms gap the single-pane harness needs.

| panes | size | cache | load (each) | RSS after load | per-pane frame (median) | round |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 1920×1080 | shared | 1335 ms | **+133 MiB** | 63.5 ms | — |
| 2 | 1920×1080 | shared | 1272 / 1386 ms | +152 MiB | 64.7 / 66.3 ms | 369 ms |
| 1 | 960×540 | shared | 1044–1104 ms | +124 MiB | 22.5–31.6 ms | — |
| 2 | 960×540 | shared | 1049 / 1077 ms | +136 MiB | 34.0 / 37.0 ms | 311 ms |
| 3 | 960×540 | shared | 1061–1130 ms | +139 MiB | 31.4–38.3 ms | 462 ms |
| 4 | 960×540 | shared | 1050–1099 ms | **+154 MiB** | 31.3–36.0 ms | 605 ms |
| 4 | 960×540 | **separate** | 1022–1113 ms | +166 MiB | 24.6–34.6 ms | 602 ms |
| 1 | 640×360 | shared | 1042 ms | +124 MiB | 32.1 ms | — |
| 4 | 640×360 | shared | 989–1080 ms | +151 MiB | 27.6–32.4 ms | 599 ms |

What the table says, in the order it matters:

1. **N `LiveMap`s in one process work.** 4/4 loaded and rendered at every size; no crash, no abort at
   teardown (`map_panes` leaves through `process::exit`, because the engine *does* assert in
   `~RunLoop` — `Failed to close loop` — when a process with a live map walks out of `main`).
2. **The first pane costs 124–133 MiB and every further pane costs 12–17 MiB** with a shared cache
   (+16–20 MiB with one cache file each). The bulk of the first pane is the Vulkan device, wgpu and
   the first style, and it is paid once however many panes follow.
3. **Sharing one SQLite cache file is fine.** The ticket's unknown — "can N `LiveMap`s share the one
   cache without locking each other (`map::cache_path()` is a single file)" — is **yes at N = 4**:
   four renderers, one cache file, all four styles loaded, no cache error in any run. The separated
   caches bought nothing: +12 MiB total, no measurable frame difference, and four copies of a file
   apt to be 2.6 MB–256 MiB. **Keep one cache.**
4. **Frame cost is per pane and does not fall with the pane.** 1080p ≈ 64 ms, 960×540 ≈ 31–38 ms,
   640×360 ≈ 28–32 ms, all debug. The 960→640 step barely helps, which says the per-frame cost at
   these sizes is render-and-readback overhead (pipeline, tile meshes, an 8.29 MB-then-2.07 MB CPU
   copy), not fill rate. **A small pane is not a cheap pane**, so the cadence, not the resolution, is
   where a pane budget comes from.
5. **Every pane pays its own ~1 s style load.** Loading is serial in the harness; it need not be in
   the wall (each pane's load runs on its own thread, as `MapThread::spawn` does today), but the first
   frame of a 4-pane layout is *at least* one second of a pane not being there yet.

## 4. Spike B — one renderer, N cameras

The ticket asked whether one `LiveMap` can render N viewports. **Read the API first, because the
answer is in the types:** `ImageRenderer` has `set_map_size(Size)` (`image_renderer.rs:375`),
`update_camera` (558) and `read_still_image` (597) — **one camera, one viewport, one readback, no
viewport rectangle anywhere**, and mode `Static` is a whole-frame still, not a sub-rect. So "N
viewports in one pass" is not a call that exists in 0.9.3, on this pin or upstream. What *is* possible
is one renderer cycling N cameras, which is what the spike measured
(`cargo run --features map --example map_panes -- b <n> <w>x<h>`):

| panes | size | pass | per-pane median | RSS |
| --- | --- | --- | --- | --- |
| 4 | 960×540 | same size, N cameras | 29.3–61.4 ms | **+119 MiB** |
| 4 | 960×540 | tiled sizes (2× 960×540, 2× 480×270), `set_map_size` between panes | 26.0–53.4 ms | +151 MiB → 151 MiB peak |

Verdict on B:

- **It is not faster.** N sequential full renders either way; the numbers are the same band as spike A.
  The engine cannot overlap them, so all N panes share one render loop and one thread.
- **It saves ~23–35 MiB at N = 4** (151 vs 174 MiB peak): one style, one cache, one glyph atlas.
- **It costs a resize between panes of unequal size** (≈+7–10 ms on the 960×540 pane), and it makes
  every pane's update wait behind every other pane's render. A focused pane cannot be smooth while
  three unfocused ones are mid-cycle.

**Decision: spike A.** One `LiveMap` + one `MapThread` per pane, as the wall has one today. The
independence is the feature — the focused pane is the one the operator is working, and it must not
queue behind three panes somebody else's tiling is redrawing — and 13 % of RSS is a cheaper price than
serialising the wall's map. One cache file for all panes (finding 3 above), which is also what keeps
the disk cost flat.

## 5. The render budget, with the arithmetic

The wall redraws what moved, not what exists. Today that is `map::thread`: a camera change renders
once (coalesced), and an idle map re-renders at 4 Hz so labels and tiles that settle still land
(`IDLE_INTERVAL`, `#28`). Multi View keeps that shape and adds a **per-pane cadence**:

| pane | renders when | at |
| --- | --- | --- |
| **focused** | the camera changes, a fix inside it arrives, its tiling changes; idle re-render otherwise | 4 Hz idle (as today) |
| **unfocused** | its tiling changed, the camera changed programmatically, a fix inside it arrives and it is not moving | 1 Hz idle — a quarter of the focused pane's, because it is smaller and read at a distance |

Release arithmetic at 1080p (42.5 ms per pane-frame, `map-engines.md`), for the wall-sized case of
four 960×540 panes on a 1080p surface (half-resolution per pane, which is what tiling a 1080p screen
four ways gives you; 960×540 is 30.9 ms release at 720p's band — take 30 ms to be safe):

```
4 panes × 1 Hz idle × 30 ms                     = 0.12 s/s   (each on its own thread)
+ the focused pane moving at 4 Hz × 30 ms        = 0.12 s/s   (idle)
+ a drag on the focused pane, every coalesced frame = 30 ms per 60 Hz frame → 1.8 s/s, one core
```

The focused pane's *drag* is the only expensive mode, it is what the wall already does with one pane,
and it lands on that pane's own thread. **The budget claim to test is: four panes idle inside 0.25 s of
CPU per second, and a drag inside one core** — one thread per pane is the reason that is arithmetic
rather than hope.

At 200 markers, `INV-12`'s existing 8 ms applies **per pane**, and only the pane that changed
reprojecting: markers are Rune elements over a `Pixels`, and a pane's marker layer is rebuilt when
that pane's frame or picture changes.

## 6. What we write

```rust
pub struct View {                  // one pane
    pub id: ViewId,
    pub camera: Camera,
    pub map: MapThread,            // spike A: one LiveMap + one thread per pane
    pub frame: Option<Rendered>,   // INV-04 becomes per pane: markers project against *this* frame
}

pub struct Layout {                // a binary tree, the shape a tiling WM uses
    root: Node,                    // Split { dir, ratio, a, b } | Leaf(ViewId)
    focused: ViewId,
}
```

- **A new pane splits the pane that was right-clicked**, along its longer edge, unlimited depth. The
  geographic point is the pane's camera plus the click in element points, which needs a new
  `map::geometry::unproject` — the inverse of the `project` everything already draws with. `INV-04`
  and the menu are only as good as that inverse, so it is tested against `project` across
  `MIN_ZOOM..=MAX_ZOOM` before anything else is built.
- **Ratio drag** on the border: a 6 pt grip (the width `SplitView` already uses), clamped at a minimum
  pane size — the "move the pane" request in its cheapest honest form. The pane state follows
  `PaneState`'s conventions: app-owned numbers, `min`/`max`, persisted, not a widget's internal.
- **Drag-to-swap** copies `TabDragState`: press a pane's handle, drag, the hovered target **opens a
  gap** where the pane will land, release to swap. Hit testing with `Tx::element_at`. `El::matched_crop`
  gives the pane that flies to its new place.
- **The context menu** is `Kit::anchored(..)` at the click point, drawn by us (Linux has no native
  popup): `Focus on new pane`, later `Close pane`, `Reset layout`.
- **Focus** uses the engine's own: `Tx::focus` + `El::focus_fill` for the focused pane's border,
  rather than a key-routing table of ours.
- **The header reports it**: `panes=3` beside `units=127`, and the log line grows `pane_frames=` per
  pane, so a wall quietly dropping to 4 fps is a line in a file (the lesson of `#28`, `#51`).

Two invariants change meaning and must change text and tests with the first ticket:

- **`INV-04`** — a marker's pixel agrees with **its pane's** projection, within 1 px. One `Rendered`
  per pane, and a marker inside pane B must never be projected against pane A's camera. The test
  sweeps two panes with different cameras and asserts a marker in each agrees with its own.
- **`INV-12`** — the budget is per pane and names the pane count it allows (4 at 960×540), with the
  idle cadence in the table above as the workload it measures.

## 7. Risks, named

- **The engine is one flat renderer per instance, so panes are the unit of everything.** No shared
  tile meshes, no shared glyph atlas: four panes parse the same tiles four times. That is why the
  per-pane delta is 12–17 MiB and why a fifth pane would be the point to re-run spike A.
- **`~RunLoop` asserts on process exit with a live map.** The harness works around it with
  `process::exit`; the wall joins its map thread today (`map::thread`), and **N maps need N joins
  before teardown** — that is a real, shipped-shaped risk in the first implementation ticket, not a
  harness quirk.
- **Panning a pane while another pane is loading a style**: the second pane's ~1 s load is on its own
  thread, so it should not stall the first — but that is untested and belongs in the first ticket's
  proof (two panes, one created while the other pans).
- **8.29 MB per 1080p frame is copied through the CPU** (`frame_rgba` → `Pixels`); four panes at
  960×540 is 2.07 MB each. The API does expose `take_texture()`, which is the route off the CPU
  copy — out of scope here, and named because it is the next thing that would make panes cheaper.

## 8. Tickets this files

Filed as separate issues, each with its own `## Proof`, in this order:

1. **`unproject` + the layout tree + split by right-click** — geometry inverse, tree (split/close/
   ratios/clamping), the pure layout function covering the viewport exactly, `panes` scene and golden.
2. **Ratio drag on the border** — the 6 pt grip and the clamp, property-tested on the tree.
3. **Drag-to-swap**, copying `TabDragState`'s incoming gap; `El::matched_crop` for the landing.
4. **The render budget, the header and the log** — per-pane cadence, `panes=` and `pane_frames=`,
   `INV-12` restated with the pane count, plus the N-join teardown.

## 9. Reproducing the numbers

```sh
cargo build --features map --example map_panes
# Spike A: N instances, one process per N (a crash must not take the others' numbers)
for n in 1 2 3 4; do ./target/debug/examples/map_panes a $n 960x540 shared; done
./target/debug/examples/map_panes a 1 1920x1080 shared
./target/debug/examples/map_panes a 4 640x360 shared
./target/debug/examples/map_panes a 4 960x540 separate   # the cache question
# Spike B: one renderer, N cameras, then N panes of unequal size
./target/debug/examples/map_panes b 4 960x540
```

Debug only. The wall's release numbers (30.9 ms at 720p, 42.5 ms at 1080p) come from
`scripts/build-wall.sh`'s amalgam route — the local-checkout release build segfaults inside glslang
(`map-engines.md`), so a release run of this harness here would measure the crash, not the map.
