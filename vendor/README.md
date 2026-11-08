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
