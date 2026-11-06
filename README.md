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
| the exercise stream (v0.2) | WSS | `game:<id>:positions` — **granted** to a game's readers |

**Reading the socket envelope.** Centrifugo adds one `data` level, and the channels are not uniform:
`live-feed` publishes a bare event at `pub.data`; `game:<id>` publishes a `GameEvent` at `pub.data.type`
with its payload at `pub.data.data.*`; `game:<id>:positions` is bare, has **no `type`**, and lives at
`pub.data.positions`. A client that assumes one shape gets `undefined` and no error.

`GET /live-feed/positions` needs the `read` action on `/system/live-feed`. That permission is the answer
to "what account does the wall get".

Source documents: `minos/docs/realtime-protocol.md` and `minos/docs/openapi/minos-api.yaml`
(read-only copies live in the TFG repo at `minos-docs/`).

## Rules that are traps if forgotten

- An **absent or null** optional field is **not zero**. `speed_kn`/`speed_ms`/`course_deg`/
  `accuracy_m` are omitted when unmeasured; render nothing.
- `backfilled: true` is real, is the newest thing known, and **did not just happen**. Never draw it as
  "here now".
- The token rides in the **connect frame's `data`** as `{"token": "…"}` — never a header.
- **Subscribe before, or with,** the initial read, and reconcile by `id_unit`.
- **No history exists**: after a reconnect, re-read, then resubscribe.
- **Never build a channel name** (`107`).
- Refusals: `101`/`103`/`107` stop; `109` refresh then retry; `100` retry with backoff; `105` ignore.
- **The UI thread never blocks.** Reads, socket and map each own a thread.

## Verification

**A ticket is done when a command says so.** The contract is [`docs/verification.md`](docs/verification.md),
the numbered promises are [`docs/invariants.md`](docs/invariants.md), and the gate is `scripts/verify.sh`
(the same command CI runs).

```sh
scripts/verify.sh          # fmt, clippy, tests, captures + pixel assertions, budgets
scripts/verify.sh --fast   # the inner loop
```

Every ticket carries a `## Proof` line: the command, and what a failure looks like. A ticket that cannot
name one is not ready. The upstream API contract is pinned in [`docs/spec/`](docs/spec/README.md) so a
server change is a deliberate commit here rather than a silent breakage.

## Stack

Rune (pinned by git rev, vendored before release) · `maplibre_native` (offline tile seed) ·
`tokio-centrifuge` (vendored: upstream cannot set connect data) · `reqwest` · `tokio`.

## Build and run

```sh
cargo run                       # the display (Wayland or X11)
cargo run -- --fullscreen       # borderless, at the primary output's size
cargo run -- --window-size 1920x1080
cargo run -- --version          # the version, no window
cargo run -- --self-check       # config, credentials, REST, socket, map seed (exit 2 if anything is)

scripts/verify.sh               # the gate — the same command CI runs
scripts/verify.sh --fast        # the inner loop: no captures, no budgets
```

First build is the slow one: Rune comes from git and pulls a few hundred crates. After that,
`cargo test` takes seconds and needs no display and no network.

Where the other documents sit: [`AGENTS.md`](AGENTS.md) is how to work here (one ticket per branch,
what "done" means, the rules that are not preferences); [`CONTEXT.md`](CONTEXT.md) is the domain
vocabulary; [`docs/adr/0001-rune-and-maplibre.md`](docs/adr/0001-rune-and-maplibre.md) records why
Rune and why MapLibre, and what each defers.

## There is no LICENSE, on purpose

This application is proprietary and not licensed for redistribution. A root `LICENSE` would
relicense it; the third-party components are listed in [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)
and keep their own licences.
