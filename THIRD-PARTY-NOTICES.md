# Third-party notices

**This application is proprietary and is not licensed for redistribution.** There is deliberately no
root `LICENSE` file: adding one would relicense code that is not meant to be relicensed. The
components below keep their own licences, and their texts ship with them.

Everything here is either a direct dependency in `Cargo.toml` or a crate in `Cargo.lock` under a
licence that asks to be named. `tests/notices.rs` (#12) fails when a direct dependency is missing
from this file, or when a copyleft crate appears in the lockfile unnamed.

## Bundled at build time

| component | licence | what it is |
| --- | --- | --- |
| **Rune** (`rune`, `rune-kit`, and the crates it is built from: `rune-core`, `rune-text`, `rune-edit`, `rune-gpu`, `rune-image`, `rune-media`, `rune-spell`) | MIT, © 2026 Anomaly | The UI framework. <https://github.com/anomalyco/rune>, pinned to rev `d0eaf332c9c1ff8f3ab599be8e57012a72921c9a`. Vendored before release; the full licence text then lives at `vendor/rune/LICENSE`. The seven crate names arrived with the `#247` rev bump: Rune's sources are split into them, all still MIT and all still part of Rune. |
| **winit** 0.30.13 (vendored) | Apache-2.0 OR MIT | Window, input and the event loop. Not the wall's dependency: it is Rune's, copied so Rune's macOS input-method path compiles — `vendor/README.md`. Licence text at `vendor/winit/LICENSE`. |
| **wgpu-hal** 30.0.1 (vendored) | Apache-2.0 OR MIT | The GPU backend under `wgpu`. Not the wall's dependency either: Rune's DX12 partial presents live in its patched copy, and Rune does not compile on Windows without it — `vendor/README.md`. |
| **sctk-adwaita** 0.10.1 (vendored) | MIT | Wayland's adwaita client-side window frame. Rune's copy rounds a decorated window's bottom corners under it, and Rune does not compile on Linux without the additions — `vendor/README.md`. |
| **parley_engine** 0.12.0 (vendored) | Apache-2.0 OR MIT | The shaping engine under `parley`, which shapes the wall's text. Rune's copy carries `line-dictionaries` (which exists in no published version) and two shaping fixes; the pin is only resolvable because the wall patches it — `vendor/README.md`. |
| **accesskit_windows** 0.35.2 (vendored) | MIT OR Apache-2.0 | Windows' UI Automation adapter. Rune's copy reports headings and landmarks; Windows only — `vendor/README.md`. |
| **accesskit_macos** 0.27.2 (vendored) | MIT OR Apache-2.0 | macOS' NSAccessibility adapter. Rune's copy reports headings VoiceOver can find; macOS only — `vendor/README.md`. |
| **MapLibre Native** (`maplibre_native` 0.9.3, vendored) | BSD-2-Clause | The map engine, vendored for two checkout-cache fixes — see `vendor/README.md`. <https://github.com/maplibre/maplibre-native>. Behind the `map` feature, which the gate does not enable. |
| **Inter**, **IBM Plex Mono** | SIL Open Font License 1.1 | Typefaces bundled with Rune. |
| **milsymbol** (`spatialillusions/milsymbol`) | MIT — © 2017 Mans Beckman | NATO/APP-6 icon geometry, pinned by the sister project at `f5134380…` v3.0.5. **Not linked or vendored yet**: `docs/research/sheets/wall-symbol-icons.svg` is derived from it (#33), and lifting the module needs this row to stay and the licence text to travel with the copy. |

## The phone's package (`clients/phone/`, Dart)

Not part of the wall and not built into it: the phone is its own program (ADR-0008), and its dependencies are resolved
by `dart pub` from `clients/phone/pubspec.lock`. The direct ones, all development-only:

| crate | licence | role |
| --- | --- | --- |
| `test` | BSD-3-Clause | the test runner (`dart test`) |
| `lints` | BSD-3-Clause | the analysis rules `dart analyze` enforces |

The screen (`clients/phone/app/`) adds more, and the Flutter SDK itself is a **toolchain**, not a package linked
into either program:

| package | licence | role |
| --- | --- | --- |
| `material_ui` | BSD-3-Clause | the widget set and the icon catalogue the screen draws with (`mobile-ui.md`) |
| `mobile_scanner` 7.5.0 | BSD-3-Clause — © 2022 Julian Steenbakker | **the pairing QR's camera and decoder** (#271). Android: CameraX plus the **bundled** ML Kit barcode model (`com.google.mlkit:barcode-scanning`), roughly 3–10 MB in the APK; iOS: AVFoundation and Apple's Vision framework. The plugin's own Android/iOS halves are linked, and `mobile_scanner` is the only package that touches a camera |
| `shared_preferences` 2.5.6 | BSD-3-Clause | **the relay field's memory** (#271): a URL is deployment configuration, not a secret, so it is kept in platform preferences beside the app's other settings; the device key is not here (ADR-0008 decision 4) |
| `flutter_test` | BSD-3-Clause | widget tests and the golden harness (`flutter test`) |

The transitive set is `pubspec.lock`'s, and none of it ships in the wall. The two runtime plugins above ship in the
phone app only; the wall links neither. `shared_preferences` brings its per-platform implementations and `mobile_scanner`
brings CameraX/ML Kit on Android and AVFoundation/Vision on iOS — all in the app's APK or bundle, never in the wall.

## The wall's own dependencies

| crate | licence | role |
| --- | --- | --- |
| `tokio-centrifuge` 0.2.6 (vendored) | MIT | Centrifugo client. Patched for connect-frame `data` and handshake refusal codes — see `vendor/README.md`. |
| `reqwest` 0.13 | MIT OR Apache-2.0 | HTTPS reads. Never on the UI thread. |
| `tokio` 1 | MIT | Runtimes for the reads and socket threads. |
| `serde`, `serde_json` 1 | MIT OR Apache-2.0 | Wire parsing. |
| `thiserror` 2 | MIT OR Apache-2.0 | Typed errors. |
| `chrono` 0.4 | MIT OR Apache-2.0 | Timestamps from the wire. |
| `sha2` 0.10 | MIT OR Apache-2.0 | **Pairing** (#194): `devices.toml` keeps the SHA-256 of a device key, never the key. |
| `qrcodegen` 1.8 | MIT | **The pairing QR** (#270): modules only — no renderer, no image, no codec. The wall draws the grid as its own squares (`ui::qr`), so nothing on the capture path depends on a PNG. |
| `getrandom` 0.3 | MIT OR Apache-2.0 | **Pairing** (#194): the 32 bytes of a fresh device key and the six digits of a pairing code, from the OS's own source of chance. |
| `tracing`, `tracing-subscriber` 0.3 / 0.1 | MIT | Logging. **Redaction is a hand-written `Debug`, not a filter** (`INV-08`). |
| `rusqlite` 0.32 (bundled SQLite) | MIT | The offline map seed is a SQLite cache; its expiry rows are pinned before MapLibre opens it. Bundled so no system library is needed. SQLite itself is public domain. |
| `toml` 0.8 | MIT OR Apache-2.0 | `config.toml`, `credentials.toml` and `settings.toml`: the operator's format, refused on an unknown key rather than silently defaulted. |
| `libc` 0.2 | MIT OR Apache-2.0 | One `signal(2)` call, so `SIGTERM` from systemd quits through the runtime and drops the GL context in order. |
| `png` 0.18 | MIT OR Apache-2.0 | `--probe-map` writes one frame to a PNG, headless, for CI. |

## Crates with a licence that asks to be named

Pulled in through Rune's audio features (Ogg/WebM), not used by this app directly:

| crate | licence |
| --- | --- |
| `symphonia`, `symphonia-core`, `symphonia-codec-vorbis`, `symphonia-format-ogg`, `symphonia-format-mkv`, `symphonia-metadata`, `symphonia-utils-xiph` 0.5.5 | MPL-2.0 |
| `opuscule` 0.2.2 | MPL-2.0 |
| `spellbook` 0.4.2 | MPL-2.0 |
| `downloader` 0.2.8 | LGPL-3.0-or-later |

MPL-2.0 is file-level copyleft: these crates stay under MPL-2.0, unmodified. If a future build does
not need them, dropping Rune's `ogg-audio` feature removes them from the lockfile — and this table
must then shrink in the same commit.

`spellbook` arrived with the Rune rev bump of #185 and is **web only**: it sits under Rune's
`cfg(target_arch = "wasm32")` dependencies, so it is in `--target all` and in no native build. Named
so that a lockfile diff does not have to be read to find it.

`r-efi` is dual-licensed `MIT OR Apache-2.0 OR LGPL-2.1-or-later`; this project takes it under
MIT/Apache-2.0 and is not subject to the LGPL.

`downloader` is a **build-time** dependency of the vendored map engine: `maplibre_native`'s build script
uses it to fetch the precompiled core, and none of it is linked into either binary. It entered the
lockfile when #276 made this repository a workspace, because cargo makes a path dependency under the
workspace root a workspace member, and a member's build graph is resolved even when the `map` feature is
off. It is named rather than hidden: it is in the lockfile, so it is in the audit.


Two transitive crates arrived with the Rune rev bump of #159 and are neither copyleft nor used by this app
directly: `icu_normalizer` (Unicode licence; `rune-kit`'s search folding) and `objc2-core-text` (MIT/Apache-2.0;
CoreText's fallback fonts, Apple-only, so it is in the lockfile and never in a Linux build). They are named so
that the next auditor does not have to read a lockfile diff to find them.

Four more arrived with the Rune rev bump of #247, none copyleft and none used by this app directly: `stacker`
(MIT OR Apache-2.0) and its `psm` (MIT OR Apache-2.0), which `rune-taffy` uses to run its layout recursion on a
larger stack, and `psm`'s build-time `ar_archive_writer` (Apache-2.0 WITH LLVM-exception) and `object`
(Apache-2.0 OR MIT). The
same bump **removed** `zbus` 6, `zbus_macros` 6, `zbus_utils` and `zruntime`: Rune's Linux path moved to `zbus` 5,
which was already in the lockfile.
