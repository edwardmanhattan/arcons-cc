# vendor/ — dependencies we patched, and why

A vendored copy is a debt with a reason attached. Every directory here records **what upstream does
wrong, what we changed, and when to drop it**. If the reason stops being true, delete the copy.

Nothing in this directory is re-exported as our API: the code is upstream's, the patch is ours.

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

Copied from Rune's own `vendor/winit` at the rev `Cargo.toml` pins (`4af66a06`): winit 0.30.13 as published, plus
Rune's `vendor/winit.patch` — ten files, 364 lines. It is applied through `[patch.crates-io]` in **our** root
`Cargo.toml`, and re-made by `scripts/vendor-winit.sh`, which takes the copy and the patch from the pinned rev.

**Why.** From `#791` on, `rune`'s macOS path calls `winit::platform::macos::{set_ime_text, take_ime_clauses,
take_ime_replacement}` — Rune's additions to winit's `NSTextInputClient`, so a composition's clauses and a
reconversion reach the app. Those functions exist only in Rune's patched winit, and a dependency's `[patch.crates-io]`
applies to the workspace root alone: depending on Rune by rev got published winit, and `check (macos-latest)` failed
with three `cannot find function`s. The wall is a workspace root, so the wall carries the patch.

**What we changed.** Nothing. The copy is Rune's, byte for byte, and `vendor/winit.patch` is Rune's file, committed so
the change is readable here rather than only in their history. Pinned to rune's rev, not to winit's: when the section
below says "drop it", it goes with the pin it came from.

**What it costs.** 3.1 MB, 218 files, and a copy re-made on **every** winit bump. The patch is applied when the copy is
made, so a patch that no longer applies fails at `scripts/vendor-winit.sh` rather than rotting into a build nobody can
reproduce — which is the whole reason the script exists instead of a copied directory.

**Drop it when** upstream `winit` publishes those three functions in a release (Rune stops patching winit, and its
`vendor/winit` disappears with one `scripts/vendor-winit.sh` run failing loudly), or Rune gates the IME extensions
behind a feature this wall can leave off. Delete `vendor/winit`, `vendor/winit.patch` and the `[patch.crates-io]` line
together.
