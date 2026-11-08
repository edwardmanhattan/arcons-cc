# Where this wall builds, and what each missing platform costs (#31)

- **Ticket:** [#31](https://github.com/edwardmanhattan/arcons-cc/issues/31), under epic #1
- **Date:** 2026-11-08
- **Answer in one line:** Linux is proven and is what ships. macOS arm64 is a *build* target with an
  engine asset to match it. Windows builds the UI and **cannot build the map engine from the
  precompiled route at all**. Nix is not a platform but a constraint: its sandbox forbids the
  download the wall's map build depends on. **The web is not a port — it is a different map.**

The rule for every row below is the ticket's rule: a "blocked" row names the command and the error,
not an impression. Where the answer is "nobody has run it", it says that instead.

## The matrix

`✅` proven by a command whose output is in the repo or in CI · `🔨` builds, nobody has run it ·
`❌` blocked, with the error · `—` not attempted, with the reason

| layer | Linux (Arch) | Debian/Ubuntu | macOS arm64 | Windows x64 | Nix | wasm |
| --- | --- | --- | --- | --- | --- | --- |
| **crate, no map engine** | ✅ `scripts/verify.sh` | ✅ CI `check (ubuntu-latest)` | 🔨 CI `check (macos-latest)` | 🔨 CI `check (windows-latest)` | 🔨 same tree, same `cargo check` | — Rune's web target is experimental and the map is the product |
| **map engine** | ✅ Vulkan, offline seed | ✅ same amalgam, same job | ✅ **builds** on `macos-latest` (Metal amalgam; nobody has run it) | ❌ `panic: unsupported target` | ❌ the build downloads | ❌ no wasm backend at all |
| **socket** | ✅ live | ✅ same crates | 🔨 | 🔨 | 🔨 | ❌ `tokio-tungstenite` has no wasm path |
| **credential file mode** | ✅ refused unless `0600` | ✅ | ✅ (POSIX modes) | ❌ **refused by design** — Windows has ACLs, not modes; the wall now refuses and names the environment instead | ✅ | — |
| **stops on `SIGTERM`** | ✅ ordered exit 0 (#43) | ✅ | ✅ (same `libc` path) | ❌ no POSIX signals; needs a console-close handler | ✅ | — |
| **goldens/captures** | ✅ lavapipe or iGPU, within tolerance | ✅ CI renders the gate on lavapipe | — nothing renders on a Mac yet | — | — | — |

## Per target

### Arch Linux x86_64 — the platform it ships on

```sh
scripts/build-wall.sh        # release, precompiled amalgam, MLN_LOCAL_REPOSITORY cleared
scripts/verify.sh            # the gate: fmt, clippy, tests, captures, budgets
```

Proven: the wall runs on this laptop, the gate is green, 200 hulls cost 4.1 ms of an 8 ms frame
budget, and the map renders through the Intel iGPU (`arcons-cc: the map renders through
/usr/share/vulkan/icd.d/intel_icd.json`).

What a package must depend on, for the `PKGBUILD`: `libxkbcommon` (Rune), `vulkan-icd-loader` plus a
driver (Rune's wgpu backend and the map's Vulkan core), `ttf-liberation` or the operator's font of
choice, and the usual `glibc`. **Not** `sqlite`: `rusqlite` is built `bundled`, so the seed cache is
static. The build needs `cmake` only on the from-source route, which #26 established this wall must
not take (`-O2` in the local C++ build segfaults in `glslang`), so the package builds against the
upstream amalgam.

### Debian / Ubuntu — proven by CI, not by a hand

`ubuntu-latest` runs the whole gate, including four goldens rendered on lavapipe. That is a real run:
the same el/map/marker code drew real pixels. The tolerance in `scripts/capture.sh` (2/255 on ≤200
pixels) exists because that rasteriser and the laptop's iGPU differ by 1/255 on 3–23 pixels (#37), and
it is applied on *every* machine rather than only on CI.

Debian proper (rather than Ubuntu) is not separately tested; the crates it affects are the same
`libxkbcommon`/Vulkan pair.

### macOS arm64 — a build target with an asset behind it

The map engine's default backend there is Metal (`vendor/maplibre_native/build.rs`,
`GraphicsApi::from_selected_features`: "if none are enabled, defaults to Metal on macOS/iOS, Vulkan
elsewhere"), and the release the vendored crate pins publishes `amalgam-macos-arm64-metal.a` — the
same shape of asset as the Linux Vulkan core this wall ships with.

**It builds, on CI, with the map on.** The `map (macos-latest)` job ran

```
cargo check --features map --all-targets --locked
```

in one minute, and the build script said what it took:

```
warning: maplibre_native@0.9.3: Downloading precompiled maplibre-native core library from
  …/core-a33d3f00fb9dbf9117ce18edd799aa0ca5bcce5b/libmaplibre-native-core-amalgam-macos-arm64-metal.a
warning: maplibre_native@0.9.3: Using precompiled maplibre-native static library from …/*-metal.a
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 00s
```

That is a *compiler* result: nothing has drawn a pixel on Metal. The next step on macOS is a capture
and a golden rendered there, not a port.

**Intel Macs are blocked in the crate, not in the engine:** `download_static` matches only
`amalgam-linux-arm64`, `amalgam-linux-x64` and `amalgam-macos-arm64`, and everything else hits

```
panic: unsupported target: only linux and macos are currently supported by maplibre-native
```

so an `x86_64-apple-darwin` build of this wall fails in `maplibre_native`'s build script. The
sentence is misleading — macOS *is* supported, on arm64 — and that is worth an upstream note.

`cargo check --all-targets` on `macos-latest` (arm64) is the CI job `check (macos-latest)`; the map
attempt is `map (macos-latest)`, allowed to fail with its log in the job summary. **Nobody has run the
wall on a Mac.** Until someone does, "macOS builds" is a claim about a compiler, and this document
says so.

### Windows x64 — the map engine is the wall, and it is not there

- **The crate builds** (`check (windows-latest)`), and the first CI run found a real Windows-only defect
  that no Linux toolchain could have: `src/config/check.rs`'s test helper took a file `mode` that only
  the `cfg(unix)` branch used, so `cargo clippy -D warnings` failed with `unused variable: mode`. That is
  the matrix earning its keep on the first push.
- **The map does not build from the precompiled route**, at any feature combination. The `map
  (windows-latest)` job fails in `maplibre_native`'s build script, before it ever looks for an asset:

  ```
  thread 'main' panicked at vendor\maplibre_native\build.rs:168:9:
  unsupported target: only linux and macos are currently supported by maplibre-native
  ```

  There is no `amalgam-windows-*` release asset to point it at.
- **The from-source route** is the vendored crate's CMake path with vcpkg-provided dependencies
  (`MLN_WITH_OPENGL`/Vulkan) — the same route that #26 showed produces a segfaulting binary at `-O2`
  on this machine's toolchain, plus a report from the sister project (TFG) that the map fails to load
  on Windows. Two independent reasons not to promise it.
- **The credential is refused, and that is the fix.** `src/config/credential.rs` used to compile the
  mode check out on non-unix platforms (`let _ = (path, metadata); Ok(())`), so a wall on Windows would
  have read a credential file whose permissions nothing had examined — silently, because the code that
  would have said so was gone. `admit_mode` now splits into `mode_from` (a fact: `Some(mode)` where the
  OS states one) and `mode_verdict` (the rule, tested on every platform), and a missing mode is a
  **refusal** that names the way out: `MINOS_REFRESH_TOKEN`, or `MINOS_IDENTIFIER` and
  `MINOS_PASSWORD`, in the environment.
- `SIGTERM` does not exist there; a console-close handler is a separate ticket nobody needs yet.

So: **Windows is blocked on the map**, not on our code, and the blocker is an upstream release-asset
gap. Packaging an installer for a wall that cannot draw the sea is not packaging this product.

### Nix — a constraint, not a platform

Nix's sandbox has no network at build time, and `vendor/maplibre_native/build.rs` downloads the core
amalgam (378 MB) unless it is told where one already is:

```
MLN_CORE_LIBRARY_PATH          # libmaplibre-native-core-amalgam-linux-x64-vulkan.a
MLN_CORE_LIBRARY_HEADERS_PATH  # maplibre-native-headers.tar.gz, extracted
```

Both must be set — the crate panics if only one is. That is exactly the shape a fixed-output derivation
expresses:

```nix
mlnCore = fetchurl {
  url = "https://github.com/maplibre/maplibre-native/releases/download/${rev}/libmaplibre-native-core-amalgam-linux-x64-vulkan.a";
  hash = "sha256-…";  # pinned once, reviewed like a dependency
};
```

with `MLN_CORE_LIBRARY_PATH` and `MLN_CORE_LIBRARY_HEADERS_PATH` pointing into its store path. The
alternative — building MapLibre's C++ inside the derivation — is the #26 crash route and hours of
build time per evaluation; it is not the recommendation.

The rest of the derivation is ordinary: `libxkbcommon`, `vulkan-loader`, a Vulkan driver for the
machine (the derivation ships the loader, the machine ships the driver).

### The web — no, and the reason is the product, not the build flags

Rune has an experimental web target (`WEB.md`, one canvas through WebGPU/WebGL2), so a *shell* could
exist. Three things stop it:

1. **MapLibre Native has no wasm backend.** The vendored build script's target match ends in the panic
   quoted above, and no web asset is published. A browser wall would need a different map — MapLibre GL
   JS in a canvas, or raster tiles — which means a second renderer and a second projection, and
   `map::geometry` (which `INV-04` ties the markers to) stops describing the picture.
2. **The socket is not wasm-friendly.** `tokio-tungstenite`, which the vendored Centrifugo client uses,
   does not compile for `wasm32-unknown-unknown`; it needs `web-sys`/`gloo-net` or the browser's own
   `WebSocket`, i.e. a second client behind the same envelope rules.
3. **The offline seed is `rusqlite`.** Possible in wasm with a prebuilt `sqlite3.wasm`, not wired, and
   a browser has no business holding the wall's tile seed anyway.

**Verdict: no web wall for v0.1.** If a demo is ever wanted, say which map it shows in the ticket
first — a demo that shows a different map is a different product.

## What the CI matrix is, and what it is not

`check` (three OSes) runs `cargo check --all-targets --locked`, `cargo fmt --check` and `cargo clippy
--all-targets -- -D warnings`. It is not the gate: the gate is `scripts/verify.sh`, which is bash and
renders pixels, and it stays one Linux job so that CI and a human cannot diverge.

`map` (macOS, Windows) runs `cargo check --features map` with `MLN_PRECOMPILE=1` and
`continue-on-error`. Its log is copied into the job summary. It is a **report**, not a gate: a broken
map on Windows is a fact recorded here, not a reason nobody can land a card fix. When it passes twice
in a row, delete `continue-on-error` and make it real.

## Consequences, in the order they would bite

1. **macOS arm64 is one asset away from shipping.** What is missing is not code: nobody has run it, and
   the goldens have never been rendered on Metal. The next step there is a capture on a Mac, not a port.
2. **Windows needs an upstream release asset** (`amalgam-windows-*`) or a CMake route we trust at `-O2`,
   and the C++ crash in #26 is the evidence that we do not currently trust it. The credential gap found
   here is fixed; the map gap is filed where it belongs (below).
3. **Nix is a packaging task with a known shape** (a `fetchurl` for the amalgam), and its real cost is
   the pinning discipline, not the build.
4. **The web is a product decision** with a named price: a second map engine.

## Follow-ups filed

- **[#46](https://github.com/edwardmanhattan/arcons-cc/issues/46)** — the Windows map engine: needs an
  upstream `amalgam-windows-*` asset for the pinned revision, or a CMake route we trust at `-O2`, plus a
  note upstream that "only linux and macos" means "only arm64 macOS".
- **[#43](https://github.com/edwardmanhattan/arcons-cc/issues/43)** (closed) — the missing console
  handler is *not* filed separately: `SIGTERM` does not exist on Windows, and a console-close handler
  is worth writing when a Windows wall is possible at all, i.e. after #46.
