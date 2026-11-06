# arcons-cc

Arcons **Command Center**: a wall display for the live maritime picture. Rune (GPU UI) on top of
MapLibre, fed by Minos.

**Proprietary. Not licensed for redistribution. No root `LICENSE` on purpose** — third-party
components are listed in `THIRD-PARTY-NOTICES.md`.

## What v0.1 is

A map of every live vessel, moving as fixes arrive, where clicking a unit opens a small floating card
with what is known about it.

**The plan lives in the issues, not in this file.** Start at
[#1 EPIC v0.1 — the live maritime picture](https://github.com/edwardmanhattan/arcons-cc/issues/1), then
take the first ticket whose blockers are closed (#2, the scaffold).

## Where the data comes from

Minos, directly. REST is `:8080`; the socket is Centrifugo on `:8000` (`:9090` is gRPC between them,
no client touches it).

| need | transport | where |
| --- | --- | --- |
| initial picture | HTTPS GET | `/live-feed/positions` — not paginated; includes vessels that never reported (`position` absent) |
| movement | **WSS** | `ws://<host>:8000/connection/websocket`, subscribe `live-feed` |
| one vessel | HTTPS GET | `/live-feed/positions/{id}` |
| identity, spec | HTTPS GET | `/units/{id}`, `/units/{id}/specifications` |
| commander | HTTPS GET | `/units/{id}/commander` — current is the entry with no `to_at` |
| sign in | HTTPS POST | `/auth/login` with `return_refresh_token: true`, then `/auth/refresh` |
| the exercise plot (v0.2) | HTTPS poll | `/games/{id}/positions?at=` — **scenario** clock |

Source documents: `minos/docs/realtime-protocol.md` and `minos/docs/openapi/minos-api.yaml`
(read-only copies live in the TFG repo at `minos-docs/`).

## Rules that are traps if forgotten

- An **absent or null** optional field is **not zero**. `speed_kn`/`course_deg`/`accuracy_m` are
  omitted when unmeasured; render nothing.
- `backfilled: true` is real, is the newest thing known, and **did not just happen**. Never draw it as
  "here now".
- The token rides in the **connect frame's `data`** as `{"token": "…"}` — never a header.
- **Subscribe before, or with,** the initial read, and reconcile by `id_unit`.
- **No history exists**: after a reconnect, re-read, then resubscribe.
- **Never build a channel name** (`107`).
- Refusals: `101`/`103`/`107` stop; `109` refresh then retry; `100` retry with backoff; `105` ignore.
- **The UI thread never blocks.** Reads, socket and map each own a thread.

## Stack

Rune (pinned by git rev, vendored before release) · `maplibre_native` (offline tile seed) ·
`tokio-centrifuge` (vendored: upstream cannot set connect data) · `reqwest` · `tokio`.

## Build and run

```sh
cargo run -- --self-check       # config, credentials, REST, socket, map seed
cargo run                       # the display
scripts/capture.sh picture      # headless scene → PNG + tree
```

CI runs `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, and the headless scenes.
