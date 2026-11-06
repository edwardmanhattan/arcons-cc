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

## `maplibre_native` — planned, not yet copied (#5)

TFG carries a copy that fixes two bugs in how the crate manages its own cached MapLibre checkout:
upstream resolves `MLN_COMMIT` as a ref after a `--depth 1` fetch (no local ref, so every build
re-clones 2 GB), and it judges a cached checkout stale by looking for `CMakeLists.txt`, then deletes
the tree a concurrent build may be mid-clone into. The scaffold declares `maplibre_native` as an
**optional** dependency and does not copy it; #5 does, with the reason recorded here and in
ADR-0001, because that is the ticket that pays the C++ build.

## `rune` — planned before release (#9 or the release ticket)

Rune is a git dependency pinned by rev today. Vendoring it makes an offline build possible and is
recorded on `THIRD-PARTY-NOTICES.md` as MIT © 2026 Anomaly; the trigger is "before v0.1 ships", not
"when it is inconvenient".
