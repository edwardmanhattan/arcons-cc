# vendor/ — dependencies we patched, and why

A vendored copy is a debt with a reason attached. Every directory here records **what upstream does
wrong, what we changed, and when to drop it**. If the reason stops being true, delete the copy.

Nothing in this directory is re-exported as our API: the code is upstream's, the patch is ours.

**The six Rune copies are the rev's list, not ours.** `scripts/vendor-patches.sh` reads `[patch.crates-io]` from the
manifest at the pinned rev and copies every crate it names — two when `#185` started this, six at `d0eaf332`, six at
`2899b3d3`. A crate
listed there that this file does not have a `[patch.crates-io]` line for is a run that fails with the lines to add; a
crate we carry that the rev has stopped patching is a run that fails with the three things to delete. The alternative,
a `CRATES` array in the script, is what let the `#185` bump carry two of that rev's four patches without anything going
red (#247).

## `tokio-centrifuge` — 0.2.6, patched

Copied from `~/Work/Crossnet/tfg/vendor/tokio-centrifuge` (byte-identical), applied through
`[patch.crates-io]` in the root `Cargo.toml`. Pinned `=0.2.6`; the pin and the copy move together.

**Why.** Minos reads the wall's credential from the **connect frame's `data`**, as
`{"token": "…"}` — not from an `Authorization` header, and not from the frame's `token` field. That
is literal in the pinned spec (`docs/spec/realtime-protocol.md`), and it is what the vendored
`CentrifugeConfig::with_connect_data` / `Client::set_connect_data` exist to do. Upstream 0.2.6
builds the connect command without a place to put it.

The same copy also surfaces **handshake refusal codes** (101/103/107/109) through `on_disconnected`.
Upstream drops them, which would leave `INV-07` — every refusal maps to exactly one action, no
catch-all — unenforceable: the actor would see "disconnected" and could not tell "wrong password"
from "the network blinked", so it could not stop where it must stop.

**Drop it when** upstream can set connect data and reports refusal codes. Until then, the copy is the
only reason #4 can be written correctly.

## `maplibre_native` — 0.9.3, patched (#5)

Copied from `~/Work/Crossnet/tfg/vendor/maplibre_native` (source only — its `target/` is a 2.9 GB
build cache, not source), and depended on **by path** rather than through `[patch.crates-io]`: a
version requirement cannot select a patched copy whose own version is older than the registry's, and
Cargo silently resolved 0.9.4 instead. A path dependency says what we mean.

**Why.** Two bugs in how upstream manages its own cached MapLibre checkout:

- it resolves `MLN_COMMIT` as a ref after a `--depth 1` fetch, which leaves no local ref, so every
  build believes the checkout is wrong and re-clones ~2 GB;
- it judges a cached checkout stale by looking for `CMakeLists.txt` and then **deletes the tree** —
  so two overlapping builds (an editor's `cargo check` and a terminal's `cargo run`, normal here)
  race, and one restarts a 39-submodule clone underneath the other.

`mln_checkout.rs` takes a lock, waits for a concurrent build instead of racing it, and reads the
expected commit from the tag's own name.

**Drop it when** upstream ships both fixes.

**Building it (measured, ADR-0001):** `MLN_PRECOMPILE=1` downloads the prebuilt 378 MB amalgam and
needs no C++ compile (~46 s for the whole crate); `MLN_LOCAL_REPOSITORY=<path>/maplibre-native`
compiles from an existing checkout (~4.5 min here, via this machine's `~/.cargo/config.toml`). The
map feature is off in the gate, so neither is on `scripts/verify.sh`'s path.

## `rune` — planned before release (#9 or the release ticket)

Rune is a git dependency pinned by rev today. Vendoring it makes an offline build possible and is
recorded on `THIRD-PARTY-NOTICES.md` as MIT © 2026 Anomaly; the trigger is "before v0.1 ships", not
"when it is inconvenient".

## `maplibre_native` — the link name of a precompiled core (#46)

`bundle_precompiled` derived the link name with `replacen("lib", "", 1).replace(".a", "")`. That is right for
the Linux and macOS cores, which are `libmaplibre-native-core-amalgam-<os>-<arch>-<api>.a`, and wrong for
upstream's Windows cores, which are `maplibre-native-core-windows-x64-<api>.lib`: the first "lib" is inside
"maplibre", so the directive named a file (`mapre-native-core-windows-x64-vulkan.lib`) that cannot exist. It
is now a prefix strip plus a suffix strip for `.a` or `.lib`, which is equivalent on Linux and macOS and
correct on Windows.

Nothing else about Windows is patched: the probe in CI (`map (windows-latest)`, manual dispatch only)
records what the build script does with that core, and `docs/research/portability.md` says why the wall does
not draw on Windows yet.

## `winit` — Rune's copy, because a dependency's patch cannot reach us (#185)

Copied from Rune's own `vendor/winit` at the rev `Cargo.toml` pins (`2899b3d3`): winit 0.30.13 as published, plus
Rune's `vendor/winit.patch` — twenty files, 1,611 lines (sixteen files, 617 lines at `d0eaf332`). It is applied through
`[patch.crates-io]` in **our** root `Cargo.toml`, and re-made by `scripts/vendor-patches.sh`, which takes the copy and
the patch from the pinned rev.

**Why.** From `#791` on, `rune`'s macOS path calls `winit::platform::macos::{set_ime_text, take_ime_clauses,
take_ime_replacement}` — Rune's additions to winit's `NSTextInputClient`, so a composition's clauses and a
reconversion reach the app. Those functions exist only in Rune's patched winit, and a dependency's `[patch.crates-io]`
applies to the workspace root alone: depending on Rune by rev got published winit, and `check (macos-latest)` failed
with three `cannot find function`s. The wall is a workspace root, so the wall carries the patch. Since `#1188` the same
copy also carries `WindowExtWayland::content_corner_radius`, which the Wayland frame's rounded bottom corners need.

**What we changed.** Nothing. The copy is Rune's, byte for byte, and `vendor/winit.patch` is Rune's file, committed so
the change is readable here rather than only in their history. Pinned to rune's rev, not to winit's: when the section
below says "drop it", it goes with the pin it came from.

**What it costs.** 3.1 MB, 218 files, and a copy re-made on **every** winit bump. The patch is applied when the copy is
made, so a patch that no longer applies fails at `scripts/vendor-patches.sh` rather than rotting into a build nobody can
reproduce — which is the whole reason the script exists instead of a copied directory.

**Drop it when** upstream `winit` publishes those three functions in a release (Rune stops patching winit, and its
`vendor/winit` disappears with one `scripts/vendor-patches.sh` run failing loudly), or Rune gates the IME extensions
behind a feature this wall can leave off. Delete `vendor/winit`, `vendor/winit.patch` and the `[patch.crates-io]` line
together.

## `wgpu-hal` — Rune's copy, for the same reason as `winit` (#185)

Copied from Rune's `vendor/wgpu-hal` at the rev `Cargo.toml` pins: wgpu-hal 30.0.1 as published, plus Rune's
`vendor/wgpu-hal.patch` — twelve files, 1,405 lines (eight files, 440 lines at `d0eaf332`). Applied through our own
`[patch.crates-io]`, re-made by `scripts/vendor-patches.sh`.

**Why.** Rune's DX12 backend calls `Surface::set_present_rects` — `Present1`'s dirty rectangles on a sequential
flip-model swap chain, which is how `render::damaged` reaches the screen on Windows — and the method is Rune's
addition. Published wgpu-hal 30.0.1 has no such method, so `check (windows-latest)` failed with

```
error[E0599]: no method named `set_present_rects` found for opaque type `impl Deref<Target = <Dx12 as Api>::Surface>`
   --> rune/src/window/draw.rs:433
```

The same patch also picks one adapter through DXGI without making a device on the others, loads FXC before the first
compile instead of with the instance (Rune's uber shader is precompiled), reads a removed D3D12 device as lost instead
of as a hang (`#1207`), and loads the Windows system DLLs it needs through `LOAD_LIBRARY_SEARCH_SYSTEM32` (`#1250`).

**What we changed.** Nothing — the copy and the patch are Rune's, and the patch file is committed so the change is
readable here.

**What it costs.** 2.4 MB, 86 files, re-made on every `wgpu` bump. Linux and macOS never execute the added code, so the
gate's pictures are unaffected (`scripts/verify.sh` proves that per bump, since the copy is what they build against).

**Drop it when** upstream `wgpu-hal` gains those entry points, or Rune stops patching it. Delete `vendor/wgpu-hal`,
`vendor/wgpu-hal.patch` and the `[patch.crates-io]` line together; `scripts/vendor-patches.sh` fails loudly, naming all
three, when the rev stops listing a crate it carries.

## `sctk-adwaita` — Rune's copy, for the Wayland frame's rounded corners (#247)

Copied from Rune's `vendor/sctk-adwaita` at the rev `Cargo.toml` pins: sctk-adwaita 0.10.1 as published, plus Rune's
`vendor/sctk-adwaita.patch` — four files, 917 lines. Applied through our own `[patch.crates-io]`, re-made by
`scripts/vendor-patches.sh`.

**Why.** From `#1188` on, Rune draws a decorated Wayland window's content rounding its bottom two corners under the
adwaita client-side frame: the frame's one-point border follows the same arcs, and its shadow reaches over the window's
foot. `AdwaitaFrame::content_corner_radius` is Rune's addition to the published crate, and `rune`'s `window::corners`
asks for it — through `winit`, which depends on `sctk-adwaita` and inherits **our** patch because our `winit` copy is a
path dependency of the workspace root. Published `sctk-adwaita` has no such method, so the alternative is a `rune` that
does not compile on Linux.

**What we changed.** Nothing — the copy and the patch are Rune's, byte for byte, and the patch file is committed so the
change is readable here. This one is a *patch of a dependency of a patch*: `vendor/winit` is ours, and it resolves
`sctk-adwaita` through this entry rather than from crates.io.

**What it costs.** 232 KB, 26 files, re-made on every `sctk-adwaita` or `winit` bump. It is in the gate's Linux build
(`cargo tree -i sctk-adwaita` → `winit` → `rune`), so a patch that stops applying is a red gate, not a red job on a
platform nobody runs. The pictures do not move: headless captures draw no window frame; what changes on screen is the two
bottom corners of a real window under a compositor that defers decorations to the client.

**Drop it when** upstream `sctk-adwaita` gains `content_corner_radius` and the arc-correct border, or Rune stops
patching it. Delete `vendor/sctk-adwaita`, `vendor/sctk-adwaita.patch` and the `[patch.crates-io]` line together; this
script will not let the line outlive the rev's list.

## `parley_engine` — Rune's copy, and the crate that made this rev resolvable at all (#247)

Copied from Rune's `vendor/parley_engine` at the rev `Cargo.toml` pins: parley_engine 0.12.0 as published, plus Rune's
`vendor/parley_engine.patch` — ten files, 708 lines. Applied through our own `[patch.crates-io]`, re-made by
`scripts/vendor-patches.sh`.

**Why.** Two reasons, and the first is why the pin could move at all. `#861` added
`parley_engine`'s `line-dictionaries` feature to Rune's `rune-text` (Windows: Thai, Lao, Khmer and Burmese line breaks
from ICU4X's baked dictionaries), and that feature exists only in Rune's patched copy: asking for it from crates.io made
the rev **unresolvable**, which is what held the pin at `4af66a06` (#939) while `dev` ran 80 commits ahead. Patching
`parley_engine` here is what puts a crate with that feature in the graph, so the bump is possible at all. The second
reason is what the patch does: UAX #9 rule L1 for a paragraph's trailing whitespace and isolates (BidiTest.txt 770,241 of
770,241, from 644,283 without it), and shaping that stops redoing work from the same inputs — an analyzer keeps its ICU
segmenters, a run's and a style's metrics are made once per font instance and size, 14% off a text's shaping with the
layouts identical.

**What we changed.** Nothing — the copy and the patch are Rune's, byte for byte.

**What it costs.** 336 KB, 24 files, re-made on every `parley` bump. It is in the gate's Linux build through
`parley 0.12` → `parley_engine`, so the wall's own text is shaped by the patched engine and the gate proves it every run.
The L1 fix could move a paragraph whose text ends in whitespace; the wall's labels do not, and the capture review for
`#247` found the text moved by sub-pixel rounding only.

**Drop it when** `parley_engine` publishes both changes (the shaping caches and L1) *and* `line-dictionaries`, or Rune
stops asking for the feature. Then delete `vendor/parley_engine`, `vendor/parley_engine.patch` and the
`[patch.crates-io]` line together.

## `accesskit_windows` — Rune's copy, because UI Automation is Windows' own (#247)

Copied from Rune's `vendor/accesskit_windows` at the rev `Cargo.toml` pins: accesskit_windows 0.35.2 as published, plus
Rune's `vendor/accesskit_windows.patch` — six files, 1,785 lines. Applied through our own `[patch.crates-io]`, re-made by
`scripts/vendor-patches.sh`.

**Why.** Rune's Windows accessibility adapter reports headings and landmarks with `HeadingLevel`, `LandmarkType` and
`LocalizedLandmarkType` — what Narrator's and NVDA's heading and landmark navigation read — and answers a field past its
64 KiB text-run limit from the field's value (`src/long_text.rs`). Both are Rune's additions; the published crate has
neither, so a consumer gets an adapter that compiles and then says nothing useful. It is **Windows only** and is not in
the gate's Linux build: `check (windows-latest)` compiles it, which is the only proof CI can give.

**What we changed.** Nothing — the copy and the patch are Rune's, byte for byte.

**What it costs.** 340 KB, 19 files, the largest patch of the six by line count (1,785 lines across six files), re-made
on every AccessKit bump. It was already in the pinned rev's `[patch.crates-io]` at `4af66a06` and this repository did
not carry it then: nothing on Linux or macOS noticed, because the crate is not in either graph. That is the whole reason
`scripts/vendor-patches.sh` now derives its list from the rev instead of naming two crates (#247).

**Drop it when** upstream `accesskit_windows` reports headings and landmarks itself, or Rune stops patching it. Delete
`vendor/accesskit_windows`, `vendor/accesskit_windows.patch` and the `[patch.crates-io]` line together.

## `accesskit_macos` — Rune's copy, for headings VoiceOver can find (#247)

Copied from Rune's `vendor/accesskit_macos` at the rev `Cargo.toml` pins: accesskit_macos 0.27.2 as published, plus
Rune's `vendor/accesskit_macos.patch` — six files, 1,176 lines. Applied through our own `[patch.crates-io]`, re-made by
`scripts/vendor-patches.sh`.

**Why.** From `#975` on, Rune reports headings to NSAccessibility as AppKit's `AXHeading` with their level as the value
(the published role, `Heading`, is none VoiceOver knows), so VoiceOver's rotor and VO-Command-H find them; its
accessibility children, focused element and hit test answer from the superclass off the main thread instead of racing
the main thread's borrow; and a field past the 64 KiB text-run limit keeps its line, string and selection ranges. All
three are Rune's additions. It is **macOS only** and is not in the gate's Linux build: `check (macos-latest)` compiles
it.

**What we changed.** Nothing — the copy and the patch are Rune's, byte for byte.

**What it costs.** 216 KB, 16 files, re-made on every AccessKit bump. Unlike its Windows twin this crate is new in the
pinned rev's patch list since `4af66a06` (it arrived with `#975`), so this bump is where the wall starts carrying it.

**Drop it when** upstream `accesskit_macos` reports headings as `AXHeading` and is re-entrant off the main thread, or
Rune stops patching it. Delete `vendor/accesskit_macos`, `vendor/accesskit_macos.patch` and the `[patch.crates-io]`
line together.
