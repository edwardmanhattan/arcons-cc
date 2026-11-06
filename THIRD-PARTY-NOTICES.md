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
| **Rune** (`rune`, `rune-kit`) | MIT, © 2026 Anomaly | The UI framework. <https://github.com/anomalyco/rune>, pinned to rev `7ead7d63fcf6f9dd9669385a8b90e2ae80015d7b`. Vendored before release; the full licence text then lives at `vendor/rune/LICENSE`. |
| **MapLibre Native** (`maplibre_native` 0.9.3, vendored) | BSD-2-Clause | The map engine, vendored for two checkout-cache fixes — see `vendor/README.md`. <https://github.com/maplibre/maplibre-native>. Behind the `map` feature, which the gate does not enable. |
| **Inter**, **IBM Plex Mono** | SIL Open Font License 1.1 | Typefaces bundled with Rune. |

## The wall's own dependencies

| crate | licence | role |
| --- | --- | --- |
| `tokio-centrifuge` 0.2.6 (vendored) | MIT | Centrifugo client. Patched for connect-frame `data` and handshake refusal codes — see `vendor/README.md`. |
| `reqwest` 0.13 | MIT OR Apache-2.0 | HTTPS reads. Never on the UI thread. |
| `tokio` 1 | MIT | Runtimes for the reads and socket threads. |
| `serde`, `serde_json` 1 | MIT OR Apache-2.0 | Wire parsing. |
| `thiserror` 2 | MIT OR Apache-2.0 | Typed errors. |
| `chrono` 0.4 | MIT OR Apache-2.0 | Timestamps from the wire. |
| `tracing`, `tracing-subscriber` 0.3 / 0.1 | MIT | Logging. **Redaction is a hand-written `Debug`, not a filter** (`INV-08`). |
| `rusqlite` 0.32 (bundled SQLite) | MIT | The offline map seed is a SQLite cache; its expiry rows are pinned before MapLibre opens it. Bundled so no system library is needed. SQLite itself is public domain. |
| `png` 0.18 | MIT OR Apache-2.0 | `--probe-map` writes one frame to a PNG, headless, for CI. |

## Crates with a licence that asks to be named

Pulled in through Rune's audio features (Ogg/WebM), not used by this app directly:

| crate | licence |
| --- | --- |
| `symphonia`, `symphonia-core`, `symphonia-codec-vorbis`, `symphonia-format-ogg`, `symphonia-format-mkv`, `symphonia-metadata`, `symphonia-utils-xiph` 0.5.5 | MPL-2.0 |
| `opuscule` 0.2.2 | MPL-2.0 |

MPL-2.0 is file-level copyleft: these crates stay under MPL-2.0, unmodified. If a future build does
not need them, dropping Rune's `ogg-audio` feature removes them from the lockfile — and this table
must then shrink in the same commit.

`r-efi` is dual-licensed `MIT OR Apache-2.0 OR LGPL-2.1-or-later`; this project takes it under
MIT/Apache-2.0 and is not subject to the LGPL.
