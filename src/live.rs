//! The Centrifugo socket actor: movement, on its own thread, with its own runtime.
//!
//! #4 owns this module. Nothing here touches the UI thread; the actor publishes what arrived and
//! the view loop reads it on the next frame.
//!
//! The five things that decide whether this module is right:
//!
//! 1. **The token rides in the connect frame's `data`** as `{"token": "…"}` — never an
//!    `Authorization` header, never the frame's `token` field. Upstream `tokio-centrifuge` 0.2.6
//!    cannot set it, which is why `vendor/tokio-centrifuge` exists.
//! 2. **The envelope is not uniform.** Centrifugo adds one `data` level, and the channels differ
//!    beneath it: `live-feed` is a bare event at `pub.data`; `game:<id>` puts a `GameEvent` at
//!    `pub.data.type` with its payload at `pub.data.data.*` (two levels); `game:<id>:positions` is
//!    bare, has **no** `type`, and lives at `pub.data.positions`. The wrong depth is `undefined`
//!    with no error.
//! 3. **Never build a channel name.** The server grants them; an invented one is refused with `107`.
//! 4. **Refusals are total** (`INV-07`): `101`/`103`/`107` stop · `109` refresh then retry · `100`
//!    retry with backoff · `105` ignore. Every code gets its own arm; a new code upstream is a
//!    compile-visible decision, never a silent default.
//! 5. **Subscribe before, or with, the initial read**, and reconcile by `id_unit`; a fix published
//!    in the gap is otherwise lost with no way to notice. And there is no `live-feed` replay, so a
//!    reconnect re-reads before any marker is trusted (`INV-09`).
//!
//! No item exists yet on purpose — #4 lands the actor, and its state machine is the ticket.
