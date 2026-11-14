# ADR-0006 — A fleet of walls: one authority, many viewers

**Status:** proposed. Supersedes the *scope* of `ADR-0005`'s "one process with two roles" — not its reasoning — and
answers the test `ADR-0005` wrote for itself.

## The problem

A wall is a display in a room. The next ask is **the same wall on many machines**: an operations room with a wall per
department, a shore office and a vessel that must see the same picture, a replica in a second building. Each of them is
**interactive** — anyone standing at any of them may pick a pane, pan a camera, open a card — and all of them must show
one wall, not five walls that look alike.

Today every process is both the plane and the display: it holds `State`, reads Minos, draws, and serves its own unix
socket. Two of those on two machines are two walls, and nothing keeps them agreeing.

`ADR-0005` made the plane one process *with* the viewer and wrote the test that would end that:

> 1. **A client must outlive the viewer**, or 2. **a wall with no display**.

A fleet is both, arrived at from the other end: the plane must answer for viewers that do not own the picture, and it
must be able to answer with no display attached. The reasoning that made one process right — *the viewer is the only
thing that can apply a change, so a second process would need a state-replication protocol to keep faithful* — is
exactly what a fleet must now take on, because the alternative is five copies of the state and no way to say which is
the wall.

## The decision

### 1. One authority owns the wall's state and the water

The **authority** is a process with no GPU and no window. It holds the state the picture is drawn from, applies every
intent (the existing vocabulary, unchanged, with the existing refusals), and performs every external read: Minos REST
and the live feed. Exactly one machine holds the Minos credential (`INV-08` is *strengthened*: one secret, not five).

It owns persistence too: `layout.toml`, `settings.toml`, `credentials.toml`. A viewer owns none of them.

### 2. Viewers draw locally from a snapshot, and never from pixels

A viewer connects, receives **a `Scene`: the whole renderable state plus a data snapshot, with a `seq`**, and draws it
with its own Rune, its own MapLibre threads, its own GL contexts, at its own frame rate.

**The picture never crosses the wire** — the ADR-0005 rejection of a codec stands, and `INV-04` and `INV-12` stay claims
about one machine's drawing surface. What crosses is what the picture is *computed from*: the pane tree, cameras,
filters, names, selection, cursors, and the fleet/newsroom data with the authority's own timestamps.

**"The same view" therefore means the same state, not the same bytes**, and that is a deliberate, stated limit: two
monitors tile the same tree into different rectangles, and two GPUs rasterise differently. The byte-identical goldens in
this repository remain a claim about *this gate*, rendering fixtures on one machine — they were never a claim a fleet
could keep, and they do not become one.

### 3. Deltas are `Scene`s, and there is no such thing as an event

Every change publishes a new numbered `Scene` (starting with whole snapshots, because a wall's state changes a few
times a second and a small object at 4 Hz is nothing on a LAN). A viewer whose `seq` skips asks for a fresh one.

This is the decision that keeps the protocol small: **no event log, no ordering rules, no replay, no per-verb
replication** — one value, monotonically numbered, and a client that has fallen behind is a client that re-reads. The
`read: "wall"` of `ADR-0005` is the same idea at a smaller size, and it is what makes a kiosk that slept for an hour
correct in one frame.

### 4. Every viewer is a controller, and the authority is the only thing that changes

Any viewer may send any intent. Settled changes are **never applied optimistically**: a press sends an intent, the
authority applies it, and every viewer — including the one that pressed — sees it in the next `Scene`. On a LAN that is
one round trip, and a wall that briefly disagrees with itself is worse than a wall that reacts in 2 ms.

**Continuous gestures stay local until they settle**: a drag moves a pane's camera, a border, or a map thread on the
viewing machine and sends **one** intent when it is released. That is already `#173`'s rule for the camera, and it is
what keeps a drag from being 60 network writes a second.

### 5. Focus is shared; a cursor is not

The focused pane, the selection, the active workspace and each pane's camera are **state**: one wall, so everyone sees
one focus. A press on a pane in one room moves the focus in all of them, which is the point of a fleet.

A **cursor** is per-client — keyed by an `Origin` — because a pointer is a hand, and two hands are not a mistake. Every
viewer draws every cursor it is told about, so a remote's press is visible where it lands; the wall's own pointer is the
`wall` origin. This replaces `docs/mobile-remote.md`'s "one cursor" decision, which was right for one remote and wrong
for a fleet.

### 6. A viewer that has lost the authority says so

No `Scene` for a while (`HEARTBEAT` × grace) and the viewer draws **a banner over a picture it stops calling
current** — the mobile note's rule, applied to a kiosk: *never a labelled picture of a wall you are no longer watching*.
When it reconnects it takes a fresh snapshot; it sends nothing it had queued.

This is the one place a fleet is worse than a single wall, and it is worse honestly: one authority is a single point of
failure, mitigated by persistence (a restart is seconds), by systemd, and by every viewer being able to say what it is
showing. A hot standby is a consistency problem this ADR does not attempt.

### 7. The single-wall case stays the default, and is the same code

With no authority configured, the viewer **is** the authority: the same `apply`, the same refusals, the same `poll`
queue, no network. A fleet differs only in where `apply` runs. That is not a courtesy to the old behaviour — it is how
the gate keeps working: a scene builds a `State`, and a capture renders it, with no authority in the loop at all.

## Rejected

- **Ship frames over the wire.** A codec, a frame clock, a bandwidth budget per viewer, and `INV-04`/`INV-12` become
  network claims. Rejected: kiosks have GPUs, and the wire should carry what changed, not what it looks like.
- **No authority — each wall its own, kept in sync by broadcasting intents.** Rejected: two walls that both apply a
  `pane.close` while disconnected arrive at different walls, and nothing can say which is right. One thing decides.
- **A database-backed plane with per-field merges.** Rejected: it buys concurrent editing of a wall nobody wants two
  people to edit at once, and it costs a schema, a migration story and a merge policy — for a document that is one
  screen.
- **Per-viewer focus and per-viewer layout.** Rejected: that is a *different product* — five walls with a shared feed.
  The ask is one wall on five screens.
- **A viewer that applies its own intents first and reconciles later.** Rejected: a reconciliation that visibly moves a
  pane back is worse than a 2 ms wait, and "which one won" becomes a rule nobody can test.
- **Reusing the unix socket's file-mode credential over the network.** Rejected: there is no filesystem between
  machines. A viewer presents a device key (paired as `docs/mobile-remote.md` describes) and the authority keeps a hash
  of it — the mobile epic's pairing, pointed at a LAN instead of a relay.

## The test that would make this wrong

- **A viewer that must run with no authority and still be the same wall** — that is today's mode, and it is kept.
- **A wall with no network at all** — still the single-wall case.
- If the authority's `Scene` grows to carry something only the viewer can produce (a frame, a texture, a layout), this
  ADR has failed and the seam is in the wrong place.

## Consequences for the gate

- **45 scenes do not change.** A capture is a `State` rendered locally; no authority is involved, and `INV-11`
  ("a capture reads no file and binds no socket") is untouched.
- A new claim is needed instead: **a `Scene` of a state, applied to a fresh state, renders the same capture** — which is
  the only way to prove the replication vocabulary is complete. That is ticket 1, and it is useful even if no fleet is
  ever built.
- The authority has no pixels, so it cannot be in the frame-CPU budget; its own budget is "applies an intent and
  publishes a `Scene` in under X ms", which is a number to measure, not to guess.
