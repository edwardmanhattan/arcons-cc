# Real-time protocol — for the Rust command centre

**Status: BUILT and verified end to end.** A beacon batch posted to `POST /live-feed/fixes` reaches a
subscribed socket client, through the full deployed chain. Everything below was checked against the
running system rather than derived from the code.

This document is the contract for the command centre. It is the mirror of `beacon-protocol.md`, which
is the contract for the Flutter handset: one describes how positions go IN, this describes how they
come OUT.

---

## 0. Read this first — there is no official Rust SDK

Centrifugal Labs ships official client SDKs for the **browser** (JavaScript, Dart), **mobile**
(Swift, Java, Dart, C#) and **backend** (Go, Python). **Rust is not among them.** The Centrifugo client
protocol is open and documented, so this is not a blocker, but it does mean you are not calling a
blessed library and this is worth knowing before you plan the work.

What exists:

- **[`tokio-centrifuge`](https://crates.io/crates/tokio-centrifuge)** — an *unofficial* Rust client
  SDK. As of September 2026 it is at **0.2.6**, which is pre-1.0: expect the API to move. **Pin the
  exact version** and read its changelog before upgrading. It is the realistic path.
- **`tauri-plugin-centrifugo`** — if the command centre is a Tauri app, this wraps the above and
  saves you the integration.

**SSE / HTTP-streaming were looked at as a way to avoid the SDK question, and they are NOT a
shortcut.** Recorded here so nobody has to re-open it:

- SSE is **DISABLED** in this deployment. `/connection/sse` returns 404 while `/connection/websocket`
  responds, so the endpoint is not registered at all. It is off by default and would need
  `sse: { "enabled": true }` in the Centrifugo configuration.
- SSE is **unidirectional by nature**, so Centrifugo pairs it with a **second endpoint** —
  `/emulation` — that the client POSTs its commands to. Connecting and subscribing therefore becomes
  two moving parts instead of one. It is not a plain "HTTP GET that never ends".
- Centrifugo's documentation states this transport **"is only implemented by our JavaScript SDK at
  this point"**. It does not remove the need for a Rust implementation; it adds one on top.

So **`tokio-centrifuge` over WebSocket is the shortest path**, unofficial as it is. We are happy to
enable SSE if you find a use for it, but it is not a way out of §0.

---

## 1. What this is

Centrifugo is a standalone pub/sub server that sits in front of our API. Your client opens **one**
connection to it and multiplexes every subscription over that connection. Our API never handles the
socket; it only *publishes* into Centrifugo, and Centrifugo *asks* our API whether a connection or a
subscription is allowed.

```
   your client  ──WebSocket──►  Centrifugo  ──gRPC──►  Minos API
                                                    (who are you? may you watch this?)
        ▲                             │
        └─────── publications ────────┘
                                      ▲
                              Minos API (on ingest)
```

Two consequences that are easy to get wrong:

- The socket is **not on the API's port**. Centrifugo listens on **`:8000`**; the API is on `:8080`.
  `:9090` is a gRPC port between Centrifugo and the API — client code never touches it.
- **REST and real-time are separate paths to the same data.** Polling `GET /live-feed/positions`
  still works and is not deprecated. Use it for the initial picture and the socket for updates.

---

## 2. Connecting — the token goes in `data`, not a header

```
ws://<host>:8000/connection/websocket
```

The connection token is sent in the **connect frame's `data`** field, as JSON:

```json
{ "token": "<access_token>" }
```

**Not** an `Authorization` header, and this is not a style preference — a browser WebSocket cannot set
headers at all, so Centrifugo forwards whatever the SDK puts in the connect frame. Use the same field
regardless of your transport.

`<access_token>` is the **same access token** `POST /auth/login` already returns. There is no separate
real-time token and nothing extra to fetch.

### What the server answers

On success Centrifugo replies with:

| field | meaning |
|---|---|
| `user` | your account id, as a string. This is your identity for the life of the connection. |
| `expire_at` | when this connection ends — **the token's own expiry**, see §8. |
| `channels` | **`["personal:<your id>"]`** — already subscribed for you, server-side. |

> **Do not subscribe to `personal:<your id>` yourself.** The server has already done it, so an explicit
> subscribe returns **105 already subscribed**. That is not an error; it is the feature working.

---

## 3. Signing in first

`POST /auth/login` (see the OpenAPI spec). Note the field is **`identifier`**, not `username`.

Two cases are refused at connect, both deliberately:

- An account with **`must_change_password`** set. The HTTP layer refuses such an account everywhere, and
  the socket would otherwise be a side door around that gate. Change the password first.
- A token with **no expiry**. A token with no stated lifetime would authorise a connection that never
  ends, so it is refused rather than treated as eternal.

---

## 4. The channels

| channel | who may subscribe | what it carries |
|---|---|---|
| `live-feed` | **every authenticated caller** | every vessel's newest position — see §5 |
| `personal:<user id>` | that account only | messages addressed to one user ("you now command this hull") |
| `game:<id>` | only if `GET /games/{id}` would be readable to you | exercise lifecycle, roster, readiness |
| `game:<id>:positions` | same check as `game:<id>` | per-exercise position stream |

> **WHAT ACTUALLY EMITS, as of 2026-09-23** — this note said "nothing emits yet" and was stale in both
> directions, which is how it came to contradict §4b two screens below it:
>
> | channel | emitting? |
> |---|---|
> | `live-feed` | **yes** — one message per beacon batch |
> | `personal:<id>` | **yes** — `personal.message_sent` for addressed messages |
> | `game:<id>` | **partly** — `game.message_sent` for a message with no recipients, `game.clock_changed` for a pause, resume or rate change, `game.order_issued` for an applied order, and `game.hull_jumped` for a position set by hand. The other five types in §4a are SPECIFIED AND UNBUILT |
> | `game:<id>:positions` | **yes** — see §4b |
>
> So a client can rely on positions, the live feed and messages today. Tell us which of the remaining
> §4a events your command centre actually needs and we will implement them in that order rather than all
> at once.

### The transport envelope: every payload arrives at `pub.data` — and it is NOT the §4a envelope

**Two different things are called an envelope, and they nest.** Read this before §4a, because getting it
wrong produces no error at all.

A push frame carries the publication, and the payload sits **one level further down**:

```json
{"push":{"channel":"game:2850:positions","pub":{"data":{"id_game":2850,"positions":[]}}}}
```

**`pub` has exactly one key and it is `data`.** Publishing `{"channel": c, "data": X}` makes a subscriber
receive `pub = {"data": X}`.

**This is Centrifugo's envelope, not something the API adds.** Confirmed by publishing a fixed payload
straight to Centrifugo over HTTP with no application code in the path at all — the subscriber still saw it
under `data`. So it applies to every channel here, uniformly, and no amount of changing the Go code would
remove it.

Every payload shown in this document is the object at **`pub.data`**. None of these fields is ever at the
top level of `pub`. A client that reads `pub.id_unit` gets `undefined` **and no error at all** — the
subscription succeeds, the messages arrive, and one field lookup quietly returns nothing. That is not
hypothetical: it is precisely how our own probe reported a correct plot as empty on its first run.

**The §4a envelope is a SECOND wrapper inside that one, and only some channels have it.** Two of the four
channels publish a bare payload; two publish it inside the `GameEvent` of §4a. So the number of `.data`
levels is **not the same on every channel**, and two channels that both begin `game:<id>` differ from each
other:

| channel | what is published | where the client reads it |
|---|---|---|
| `live-feed` | a bare `FeedPositionEvent` | `pub.data.id_unit`, `pub.data.latitude`, … |
| `game:<id>` | a `GameEvent` | `pub.data.type`, then **`pub.data.data.…`** — two levels |
| `personal:<id>` | the same `GameEvent` | `pub.data.type`, then **`pub.data.data.…`** — two levels |
| `game:<id>:positions` | a bare `GamePositionEvent`, **with no `type` at all** | `pub.data.positions`, `pub.data.final` |

The last line is the one to be careful with. `game:<id>:positions` is **not** a `GameEvent`: it has no
`type` discriminator, so a client cannot switch on the type there — the channel name already says what it
is holding. And because it is bare, it has **one** `.data` level while its neighbour `game:<id>` has two.

> **Open question for the client teams**, to settle before anyone builds against the shape above: should the
> position stream publish a `GameEvent` like
> `game:<id>` does, so that every `game:` channel carries a `type` and the nesting is uniform? It is a
> one-line change on the server and it is breaking to make later, so it is worth deciding **before** any
> client is built against the current shape. The verified shape is the one documented above.

### What the server keeps: presence and a one-message history window

The `game` namespace sets `presence: true` and a short history (`history_size: 2`, `history_ttl: 60s`,
`history_meta_ttl: 10m`). Both are properties of the EXERCISE channels, and both are about recovery rather
than about the protocol:

- **presence** reports who is subscribed to a channel right now, so a board can show who is watching. Every
  participant can already see the roster, so this discloses nothing they did not have.
- **history** lets a client that reconnects within the window recover the last message on a channel instead
  of waiting up to a second for the next snapshot. **Two messages, not a backlog** — it is a recovery
  window, not a replacement for the REST reads. `history_meta_ttl` must stay well ABOVE `history_ttl` or
  recovery stops working before the window it advertises has passed.

**CLIENT PUBLISH IS OFF, AND IN v6 THAT IS NOT SPELLED `publish`.** The permission is three options —
`allow_publish_for_client`, `allow_publish_for_subscriber`, `allow_publish_for_anonymous` — and all three
are absent from this deployment. That absence is load-bearing: it is what makes every event, and every drag
frame, pass through the API's gates (the authority, the exercise state, the pause, the mode). **Do not add
one of them to "enable publishing".** On `game:<id>:positions` a client that could publish would be able to
draw any hull, an opponent's included, wherever it liked.

### 4a. `game:<id>` — the exercise event stream

**Status: PARTLY BUILT.** `game.message_sent`, `game.clock_changed` and `game.order_issued` emit today. The
other five types in the table below are specified and unbuilt — the shapes are agreed so that neither side
has to guess, and `dto.NewGameEvent` is the envelope every one of them uses.

**IT IS SAFE TO BUILD THEM ONE AT A TIME.** Each mirrors an existing HTTP operation, so each is a publish
call in a service that is already writing the row, and none of them needs the others to exist first.

#### The rule that decides every payload here: a publication is a BROADCAST

`game:<id>` is subscribable by **participants**, not only staff — the authorisation is the same check
`GET /games/{id}` performs, and participants pass it. And a Centrifugo publication is delivered to
**every** subscriber of the channel, with no way to address one of them.

Therefore **a payload on `game:<id>` must be safe for the least privileged subscriber**, because there
is no mechanism that could make it otherwise. That is the constraint the whole section hangs on.

Concretely: `area` and `map_tag` are withheld from participants until the exercise is in execution
(concept [2.2] — a Commando does not know the ground until it is Executed). So:

- **No `game:<id>` event carries `area` or `map_tag` while `GameHasStarted()` is false.**
- `game.state_changed` is the **one** event that carries them, and only when the new state is execution
  or closure — which is exactly the moment the withholding ends. The event that reveals the ground is
  the event that starts play.

Anything a Game Master must see but a Commando must not **cannot go on this channel at all**. It needs
a staff-only channel, which we have not built.

**Messaging — the first feature to hit this — solved it the other way round.** An addressed message is
never published here: it goes to each recipient's `personal:<id>` channel instead, so a `RAHASIA`
message never reaches a channel every participant subscribes to. That is why `personal.message_sent`
exists as a type of its own rather than as a second audience for `game.message_sent`. A staff-only
channel is still unbuilt, and messaging did not need one.

#### The envelope

Every event on this channel, whatever its type:

```json
{
  "type": "game.state_changed",
  "id_game": 12,
  "occurred_at": "2026-09-15T10:20:00Z",
  "data": { }
}
```

| field | notes |
|---|---|
| `type` | the discriminator. **Switch on it and ignore types you do not know** — new ones will be added without ceremony, and a client that fails on an unknown type breaks on every server release. |
| `id_game` | matches the channel, so a client multiplexing several exercises can route without tracking which subscription fired. |
| `occurred_at` | server clock, when the change committed. |
| `data` | type-specific, below. |

#### The events

| `type` | emitted when | `data` |
|---|---|---|
| `game.state_changed` | a transition commits | `{ "from": "preparation", "to": "execution" }`, plus `"area"` and `"map_tag"` **only when the new state has started** |
| `game.roster_changed` | a participant is added, removed, or has their role changed | `{ "action": "added"\|"removed"\|"role_changed", "id_user": 9, "name": "…", "role": "…" }` |
| `game.readiness_changed` | a participant declares or withdraws readiness | `{ "id_user": 9, "name": "…", "ready": true, "declared_at": "…" }` |
| `game.units_changed` | a hull is assigned to, released from, or re-commanded within the exercise | `{ "action": "assigned"\|"released"\|"commander_set", "id_unit": 13, "name": "KRI Ahmad Yani", "hull_number": "KRI-AH-YN" }` |
| `game.organisation_changed` | the task organisation moves — a hierarchy node is created, edited, deleted, reparented, or a unit is placed or unplaced | `{ "action": "…", "id_node": 7, "id_unit": 13 }` |
| `game.placement_changed` | a hull's **starting position on the map** is set or cleared — before play begins only | `{ "action": "set"\|"cleared", "id_unit": 13, "latitude": -6.0, "longitude": 106.0 }` |
| `game.clock_changed` | the scenario clock is paused, resumed, or its rate changed | `{ "action": "paused"\|"resumed"\|"rate_changed", "running": false, "time_factor": 6, "assumed_now": "…" }` |
| `game.order_issued` | a Commando's order is applied: the current leg closes and the next opens | **the `GameFixRecord` that the 201 from `POST /games/{id}/units/{id_unit}/order` returns** — `id_unit`, `assumed_time`, `latitude`, `longitude`, `heading_deg`, `speed_kn`, `speed_ms`, `requested_speed_kn`, `requested_speed_ms`, `clamped`, plus that record's two audit fields `created_at` and `created_by`. The server reuses that single struct rather than declaring a near-copy, so the event cannot drift from the read it mirrors |
| `game.hull_jumped` | a hull's position is **SET by hand** in a `static` exercise: a drag is committed | **the same `GameFixRecord`**, whose `kind` is `jump` — the field that tells it from an order, because a jump's `heading_deg` and `speed_kn` are both `0`. It mirrors `PUT /games/{id}/units/{unit_id}/position`. A consumer drawing a course line MUST check `kind` first, or it draws a hull holding due north at nought knots |

Every one of these mirrors an existing HTTP operation, so there is no new authority here: **an event
carries exactly what the corresponding read would have shown the same caller.**

#### `game.message_sent` is a BROADCAST and nothing else

Published when a message is sent with **no `to` and no `cc`** — which A2 defines as visible to every
game participant, making this channel exactly its audience. `data` is the message, shaped exactly like
`GET /games/{id}/messages/{message_id}` returns it:

| `type` | emitted when | `data` |
|---|---|---|
| `game.message_sent` | a message is sent to the whole exercise | the `GameMessage` of the API contract, with `broadcast: true` |

**AN ADDRESSED MESSAGE NEVER APPEARS HERE**, not even a `TERBUKA` one: "addressed" is what the
recipient list means, and publishing it here would disclose it to everyone. It goes to
`personal.message_sent` on each recipient's personal channel instead.

#### `personal.message_sent` — the one event that is not on a game channel

An addressed message is published to each recipient's `personal:<id>` channel, once per recipient.
`personal:<id>` is granted by the connect proxy to exactly the user it names, so the channel **is** the
recipient list and no filter is needed on the client.

| `type` | emitted when | `data` |
|---|---|---|
| `personal.message_sent` | a message names this user in `to` or `cc` | the same `GameMessage` shape |

**A DIFFERENT TYPE RATHER THAN THE SAME ONE ON A DIFFERENT CHANNEL.** A notification banner needs to
tell "this is for me" from "this is for the exercise", and the channel alone cannot say it once a
client is multiplexing both.

**THE EVENT NEVER CARRIES THE REAL AUTHOR WHEN AN IDENTITY WAS ASSUMED.** `sender.assumed_role` is
present and `sender.author` is absent — **for every subscriber, the Game Master included**. A published
event has no caller to tailor it to: the HTTP read can show a Judge who really sent a message because
it knows who is asking, but an event is written once and read by whoever is subscribed. Staff read the
author over HTTP when they need it, which is what F5 asks for anyway.

When **no** identity was assumed there is nothing to conceal, so `sender.author` is present exactly as
it is over HTTP — otherwise a client could not attribute a broadcast at all.

#### There is no judgement event, and its absence is deliberate

**JUDGEMENTS NEVER TOUCH THIS CHANNEL.** The client settled (B8) that a judgement is visible to the
Game Master and the Judges and to **nobody else** — not to the judged person, not to their node
commander, not to any unit Commando. So there is no `game.judgement_recorded`, no `personal.*`
counterpart, and no event type reserved for one.

**STOP LOOKING RATHER THAN WAITING FOR IT.** This is a permission and not a filter: `read` and `judge`
on `/games/judgements` are held by the judge side and the Game Master, so a subscriber who lacks them
is refused the HTTP route too — the socket and the read agree, which is the rule §4a states for every
event. Publishing a judgement would either disclose it to the whole exercise or need the staff-only
channel that does not exist (see the note above `game.message_sent`).

**A STAFF CLIENT LEARNS ABOUT JUDGEMENTS BY READING, NOT BY SUBSCRIBING.**
`GET /games/{id}/judgements` is the only way to see one, and a command centre that wants scores as they
are given should poll it: judgements are recorded by hand at human pace, so there is nothing a socket
could accelerate. If that ever changes, the event is added here first.

#### `game.order_issued` is the one that draws a moving plot

Worth stating because it is not obvious, and it is the reason this channel does not need a position
event per second: **a position is a closed-form function of the leg in force and the elapsed assumed
time.** So an order event carrying the new leg is *sufficient* for a client to advance that hull
itself, forever, with no further events — **`movement-formula.md` writes that function out in full,
with worked examples verified against the running server**, and `README.md` §2 states the model it
belongs to.

That is what makes `game:<id>:positions` a **convenience rather than a requirement**: it exists so a
client can be dumb about it, not because the data is otherwise unavailable. Two consequences for you:

- **`clamped` and `requested_speed_kn` are on the event on purpose.** An order for 35 kn applied at
  30 is a thing the Commando must be told, and the payload is deliberately the same shape the ORDER
  response returns so the two can be reconciled without a lookup.
- **EVERY SPEED IS CARRIED TWICE, IN KNOTS AND IN METRES PER SECOND.** A ship's speed is published and
  ordered in knots, and a client that works in SI should not have to know the conversion. `speed_ms` is
  the SAME FACT as `speed_kn` — derived on the way out, never stored — and `requested_speed_ms` is the
  same fact as `requested_speed_kn`, **absent under exactly the same condition.** The SI values are the
  exact conversion and are deliberately NOT rounded: 30 kn is 15.433333333333334 m/s, because 1852/3600
  is a repeating decimal. Round on display, or not at all.
- **`assumed_time` is on the SCENARIO clock**, like every other instant here. It is the stamp of the
  leg, not the wall clock time the order arrived.

#### `game:<id>:positions`

Same payload as `live-feed` (§5), filtered to the hulls assigned to that exercise. Separate from
`game:<id>` because the two have different rates and different consumers: a Game Master watching the
roster should not have a fix per second per hull down the same socket, and a client that wants only the
map should not have to receive roster events to get it.

`FeedPositionEvent` carries no `area` or `map_tag`, so it is inherently participant-safe and the §4a
rule does not bite here.

**BUILT AND EMITTING — 2026-09-23.** The channel is now live, and its payload deliberately DOES NOT
reuse the live feed's shape. See §4b.

---

## 4b. `game:<id>:positions` — the plot

**Status: BUILT and publishing.** One message per game per second, while the scenario clock is running.

**This is the only thing in the system that publishes WITHOUT AN INPUT.** Every other event here fires
inside a request — a beacon posts, an order is applied, a message is sent. A plot changes with TIME, so
there is no caller to hang the publish on and the server runs a ticker. That is why it has a rate and a
lifecycle where nothing else does.

**IT IS MOSTLY DERIVED AND PARTLY MEASURED, and "derived" left this heading when A10 landed.** A hull
whose own device authors its movement (`movement_mode: auto`) has no leg to derive from, so it is served the
newest report its device made **in this exercise** — see the report route in §5's sibling section — and
every row says which it is, through `source`. The two are built by ONE function, which is what keeps this
event and the HTTP read from describing the same instant two ways.

### The payload is a SNAPSHOT, and it is not `FeedPositionEvent`

**This object arrives at `pub.data`** — see the envelope note in §4. `final` is shown here because it is
always present, `false` on every ordinary tick:

```json
{
  "id_game": 2850,
  "assumed_time": "2026-11-23T06:10:55.977584Z",
  "positions": [
    { "id_unit": 3902, "latitude": -90.0, "longitude": 106.0000973475,
      "heading_deg": 180, "speed_kn": 20,
      "assumed_time": "2026-09-23T10:00:39.276671Z", "clamped": true,
      "movement_mode": "manual", "source": "derived" }
  ],
  "final": false
}
```

**`movement_mode` AND `source` ARE THE TWO FIELDS THAT KEEP THE EXCEPTION HONEST**, and neither is
redundant:

| row | meaning |
|---|---|
| `manual` + `derived` | an ordinary hull, advanced along its own leg |
| `auto` + `measured` | a device reporting for itself — the position, course and speed are all its own |
| `auto` + `derived` | **a device that has said nothing yet**, drawn at its origin |

The third is what every `auto` hull passes through when its mode is set, and it is the reason both fields
are needed: the mode alone cannot tell a client whether the coordinates are measured, and the source alone
cannot tell it from an ordinary hull.

**THE DEPARTURE FROM §4a IS DELIBERATE.** That section said this channel carries "the same payload as
`live-feed` (§5)". It does not, for two reasons:

- **A DERIVED position has no `received_at` and no `backfilled`.** Nothing received it and it cannot be
  backfilled — it is computed from the leg in force at the moment it is asked for. Serving those fields
  would invite a client to read meaning into them. **A MEASURED row does have a real `received_at` in the
  feed, and this payload still does not carry it**: the two kinds of row have to stay one shape, or a
  client would have to know what it was looking at before it knew which fields to read. `source` is what
  tells them apart.
- **One message per hull would be a thousand publications a second.** The agreed scale is ~100 units
  across ~10 concurrent exercises. Batched per game, it is ten.

`positions` is **the same value `GET /games/{id}/positions` returns**, built by the same function, so the
socket and the read cannot describe one instant two different ways.

### The fields

| field | notes |
|---|---|
| `type` | **`game.position_snapshot`, or `game.hull_drag_preview`.** Switch on this and IGNORE a value you do not know — see "Drag previews" below |
| `id_game` | repeated from the channel name, so a client multiplexing subscriptions can tell what it is holding |
| `assumed_time` | **the SCENARIO instant this snapshot describes**, not the wall clock the tick fired on |
| `final` | **always present.** `true` on the LAST snapshot of the stream — see below. `false` on every ordinary tick |
| `positions[].assumed_time` | the stamp of the LEG in force, which is a different thing — see §5 |
| `positions[].clamped` | **always present.** `true` means the leg crossed a pole and the path was **folded** there: the hull kept going and came back up the far side, so the position is a real place and should be drawn — but a straight line from the leg's origin to it is not the route, because the route went over a pole. The latitude is exact and the longitude is a convention; see `movement-formula.md` §5 |

A hull with no leg at or before `assumed_time` is **absent** rather than present with a zeroed position.

**AND SO IS A PIECE INSIDE ANOTHER THAT ITS HOST IS HOLDING** (U4, 2026-10-06). A contained unit "has no
position of its own — it follows its host — and it is not its own contact on the plot": the operator sees it
by drilling into the host's details, not as a second mark sitting on the ship. The exclusion is applied in the
**one query both consumers draw from**, so this stream carries it without any code of its own — which is the
general shape of the rule this document states above, and the reason it is worth saying out loud here: there
is no second place for a client to notice the difference between the socket and the read. **The clearance is
what puts the piece back**: the moment the host lets it go it is a hull like any other, drawn at the launch
U3 writes at the host's position and pace, and carrying `source: derived` (or `measured`, if its own device
authors its movement) like anything else. Being aboard is not the rule; being aboard and **held** is.

A client that needs to say *why* a hull it commands is missing should read
`GET /games/{id}/units/{unit_id}/embarkations`: it reports what the host carries and whether each piece has
been cleared, which is the same flag this stream filters on.

### The rate, and what to do about it

**One second of WALL time.** The scenario step therefore depends on the time factor: at 1x a tick moves
the scenario clock by a second, and at 240x it moves it by four minutes.

**THAT IS WHY `assumed_time` IS ON THE EVENT.** Two snapshots give you the interval and both end
positions, so you can **interpolate** and draw smooth movement without knowing the movement formula at
all. The tick is a DRIFT CORRECTOR, not the animation source. A client that would rather compute the path
itself has `movement-formula.md`.

**Interpolating between snapshots is not the same as computing a position.** Straight-line interpolation
between two nearby snapshots is fine; linearly interpolating in LATITUDE between two ORDER POINTS is not,
and §9 of that document explains why.

### Interpolation breaks across a clock jump

**THE SCENARIO CLOCK IS NO LONGER STRICTLY INCREASING, AND TWO SNAPSHOTS ARE NOT ALWAYS ONE SECOND APART
ON IT.** A Game Master can move the clock without waiting for it — a **skip** to the end of a step's window,
or a **rewind** back into an earlier attempt — and either is recorded as an instantaneous signed jump. The
server can already record one (B0b); the endpoints that perform one arrive with B5 and B6, and a client
should implement the rule below now rather than discover it on the day.

**THE RULE: interpolate only when the interval is the one you expect.** Two consecutive snapshots are
interpolable when their `assumed_time` differ by the scenario step the tick is supposed to be worth at the
current factor. On anything else — a larger interval, or a NEGATIVE one — **snap to the new frame instead of
interpolating**. You do not have to know why the clock jumped; you only have to refuse to draw a straight
line across it.

**A BACKWARD `assumed_time` IS NOT AN ERROR, AND IT IS NOT OUT-OF-ORDER DATA.** Do not sort by
`assumed_time`, and do not drop a snapshot whose stamp is older than the one before it: after a rewind that
is exactly what an honest frame looks like. Apply frames **in the order they arrive** — the channel is
ordered — and let `assumed_time` decide only whether a pair is interpolable. The rule above is the whole
compatibility story, and a client that sorts will not break loudly; it will show the fleet two hours into
the future for as long as the older frames keep arriving.

**A REWIND ALSO REDRAWS THE FLEET AT AN EARLIER ATTEMPT**, so a hull may jump a long way in the plot at the
moment of the jump — backwards along its own track, or to a position from an attempt that was abandoned. That
is what `epoch` on the timeline records, and it is the same fix chain the REST reads resolve.

### Drag previews — ephemeral frames of a hull being moved by hand

`game.hull_drag_preview` rides this channel beside the snapshots (A11). It is **one frame of somebody
moving a hull** in a `static` exercise, published as they drag rather than when they release:

```json
{"type":"game.hull_drag_preview","id_game":5312,"id_unit":3902,
 "latitude":-6.1,"longitude":106.1,"assumed_time":"2027-04-13T03:36:53+07:00","dragged_by":2}
```

**IT IS ADVISORY, NOT AUTHORITATIVE.** The frame says where a pointer is. The hull is where the committed
drag put it, and that arrives as `game.hull_jumped` on `game:<id>` a moment later. Draw the frame if you
like; never store it as the hull's position — the two are different kinds of fact, which is why the payload
is a type of its own rather than a `GameHullPosition` with different values.

**IT IS EPHEMERAL, AND THAT IS WHY NOBODY HAS TO CLEAN IT UP.** The snapshot republishes the truth every
second, so an abandoned drag — the pointer left the map, the browser closed — is overwritten within one
tick. There is no cancel message and no server-side drag state, because there is nothing to undo: the next
authority on where the hull is arrives on its own.

**FRAMES COME THROUGH THE API, NOT FROM THE CLIENT THAT IS DRAGGING.** Centrifugo's `game` namespace does
not enable client publish, so a frame exists only if
`POST /games/{id}/units/{unit_id}/position/preview` accepted it — and that route asks the same questions
the commit does: the authority, the exercise's state, the pause and the mode. Enabling client publish would
let any participant draw any hull, an opponent's included, past every one of those gates, on a channel that
every participant subscribes to.

**DO NOT EXPECT SIXTY FRAMES A SECOND.** A client throttles to roughly ten while a pointer is down, and the
commit carries the exact final position. This channel is a correction stream: interpolate between snapshots
and let a frame shorten the worst case from a second to a tenth — **subject to the interval rule above**, so a
pair separated by a clock jump stays a snap rather than becoming a straight line.

### Silence, and the three ways the stream stops

**A running exercise publishes every second. Any other state publishes ONCE and then goes quiet**, and
the message that does it carries `final: true`:

| what happened | what you receive |
|---|---|
| the clock was paused | one snapshot with `final: true`, then silence until it resumes |
| the beat in play reached the end of its window | one snapshot with `final: true`, then silence until the next Play, Skip or End |
| the exercise left `execution` | one snapshot with `final: true`, then nothing more |

**THE FINAL MESSAGE IS THE WHOLE POINT.** Without it a client cannot tell a stopped clock from a dead
socket, and would either poll an HTTP read forever or reconnect a connection that was never broken. With
it, silence has exactly one meaning: *nothing more is coming until something changes.*

**AND A CLOSED WINDOW IS NOT THE END OF THE EXERCISE.** In a step-bounded exercise the clock stops by itself
when the beat in play reaches the end of its window, and the plot stops with it — but the exercise is still
open and the next Play, Skip or End starts the clock again. **`final: true` never means "this exercise is
over."** It means *nothing more is coming until something changes*, and only the exercise's own state — the
`game:<id>` channel — can tell a client that nothing more will ever come. **Do not unsubscribe on it.**

**The payload does not say WHICH of the three happened, deliberately.** A client's reaction is the same in
both cases — stop expecting movement, keep drawing what you have — and the difference is a fact about the
EXERCISE rather than about the plot, which is what the `game:<id>` stream is for. The `final` snapshot is
also, for a departure, the last thing the server knew: the game is no longer in the sweep's page, so there
is nothing to recompute from and nothing to recompute — the plot stopped moving.

**`game.clock_changed` NOW EXISTS** (§4a) and is published to `game:<id>` on pause, resume and rate change.
It says which movement happened and what the clock now stands at. A client subscribed to both channels can
distinguish a pause from a closure by watching for it; a client subscribed only to positions cannot, and
does not need to.


> ⚠️ **The interim answer is `GET /games/{id}/positions`, and it is NOT `/live-feed/positions`.**
>
> An earlier version of this note said "the command centre polls the HTTP read for positions", which
> was ambiguous when it was written and is now wrong in a way that matters, because there are two
> reads and they are different things:
>
> | read | what it returns | who may call it |
> |---|---|---|
> | `GET /live-feed/positions` | the live feed — positions **measured** by reporting devices aboard real vessels, in no game | authentication |
> | `GET /games/{id}/positions` | an exercise's plot: **derived** from the fix chain to an assumed instant, and **measured** for the hulls whose own device authors their movement | a participant of that game, or staff |
>
> The command centre's plot wants the second, and the first will not draw a game at all: a hull
> ordered to steam along a rhumb line is not reporting from a device. The two are kept apart
> deliberately, because a measurement is never clamped — nothing derived it — and a derived position is the
> only kind that can be, carrying the flag when the geometry had to fold it at a pole.
>
> **A10 IS NOT A CROSSING OF THAT BOUNDARY — a correction to what this note said first.** An `auto` hull is
> served a REPORT rather than a derivation, and it comes from the exercise's OWN records
> (`game_unit_reports`), written through `POST /games/{id}/units/{unit_id}/fixes`. The live feed is not
> read, not written and not joined: a report inside an exercise may be simulated or made by somebody
> playing a part, and the real-asset picture must not be written from fiction. The boundary stands, and
> every plotted row says whether its position is `derived` or `measured`, so the distinction above survives
> as something a client can READ rather than something the payload assumes.

`GET /games/{id}/positions` is **built** (2026-09-21). Two parameters to know before building
against the poll:

- **`?at=<RFC3339>`** — an instant on the **scenario** clock, not the wall clock. Omitted, it means
  the scenario `now`. A client that sent its own clock would be asking about a different day, which
  is the easiest mistake to make here and the only one this read can be wrong about.
- **`?from_unit=<id>`** — turns the read into a measure. Every position then also carries
  `distance_m`, `bearing_deg` (great-circle, and **not symmetric** — the bearing back is not this
  plus 180) and `relative_bearing_deg` from the observer's own live heading, in `[-180, 180)`.
  **A hull inside another that has not been cleared is refused here with a 409 naming the ship
  holding it** (U4): it has no position to measure from, and the useful answer is which hull to
  range from instead, or which hull to ask to let it go.

**A participant sees the whole plot** — every hull, including the other side's and the judge side's.
Fog of war is a future release. That is deliberately the opposite of `GET /games/{id}/units`, which
stays staff-facing so a Commando does not learn the order of battle *before* the exercise; the two do
not contradict each other, because the plot does not exist until play begins. See `README.md` §1.1.

**A piece inside another is not drawn while its host holds it** (U4) — the client's rule, and the
payload above carries it for free, because the socket and the read draw from one query. See §4b's
field notes.

Polling this at exercise scale is fine, and it is a decision rather than an oversight. When the
channel lands, the payload above is the agreed shape.

#### Reconnecting — you will have missed events

There is **no message history** configured on these channels, so a reconnect does not replay what was
missed. Events are low-frequency, so the simple answer is the right one: **re-read `GET /games/{id}`
(and the roster) after every reconnect, then resubscribe.** Treat the socket as an accelerator over the
read, never as the only source of state. That is the same discipline as §9.

If you would rather have true gap-free recovery, Centrifugo supports channel history with offset-based
recovery, and we would enable it on the `game` namespace. Ask — it costs us config and gives you
replay, but the re-read above is enough for events this slow.

The authorisation rule for a game channel is the **same check the HTTP read uses** — not a parallel
one. A subscriber list even slightly wider than the reader list would leak through the socket exactly
what `GET /games/{id}` withholds.

Channel names are built by the server and handed to you. **Do not construct them yourself**: a
malformed name is refused (107) precisely because it means a client is inventing names rather than
being told them.

---

## 5. `live-feed` — the position event

One JSON object per publication, delivered at **`pub.data`** — see the envelope note in §4. **One message per beacon batch, per vessel** — not one per fix.

```json
{
  "id_unit": 13,
  "name": "KRI Ahmad Yani",
  "hull_number": "KRI-AH-YN",
  "latitude": -6.0888,
  "longitude": 106.9111,
  "speed_kn": 14.2,
  "speed_ms": 7.307333333333334,
  "course_deg": 87.5,
  "accuracy_m": 4.5,
  "recorded_at": "2026-09-15T09:56:28.923518653Z",
  "received_at": "2026-09-15T09:56:28.923518653Z",
  "backfilled": false
}
```

### The fields

| field | notes |
|---|---|
| `id_unit` | join key against `GET /live-feed/positions` |
| `name`, `hull_number` | the unit's label, so a marker is drawable without a prior lookup |
| `latitude`, `longitude` | always present, always finite |
| `speed_kn`, `course_deg`, `accuracy_m` | **optional — the key is ABSENT when unmeasured** |
| `speed_ms` | `speed_kn` in metres per second — the same fact, and absent exactly when it is. **The exact conversion, deliberately unrounded**: 14.2 kn is 7.307333333333334 m/s because 1852/3600 does not terminate |
| `recorded_at` | when the fix was TAKEN |
| `received_at` | when the server ACCEPTED it |
| `backfilled` | **read §6 before you draw anything** |

**The optional three are omitted, never zero.** A receiver that has not acquired a fix reports none of
them, and a course below roughly 0.1 kn is jitter rather than a measurement. A `0` sent here would be
indistinguishable from a measured zero and drawn with exactly the same confidence — so if the key is
missing, render nothing. Do not default it to 0.

### There is no `age_seconds`

The HTTP read carries one; this event deliberately does not. Age is measured against the moment of
**rendering**, so a value frozen at publish time is wrong the instant it arrives and grows more wrong
the longer you stay connected. Compute it yourself from `received_at` — a server that sent it would be
sending an under-estimate dressed as a measurement.

---

## 6. `backfilled` — do not skip this one

A handset that loses signal buffers its fixes and flushes them when it reconnects. That batch can be
**days** old. When it lands, the vessel's newest fix is a point from hours or days ago.

`backfilled: true` means: *this is real, it is the newest thing we know, and it did NOT just happen.*

- `backfilled: false` — the fix is live. `recorded_at` and `received_at` will be effectively identical,
  because the server re-stamps live fixes with its own clock. Move the marker.
- `backfilled: true` — reconstructed from the offline buffer. The vessel **was** there; it may not be
  there now. Draw it on the track, and **do not** treat it as the vessel's current position.

Verified both ways against the running system:

| fix age | `recorded_at` vs `received_at` | `backfilled` |
|---|---|---|
| live | identical | `false` |
| ten minutes late | 10 minutes apart | `true` |

**If you ignore this field, every vessel that drives through a dead zone will teleport across your map
when it comes back.** That is the single most likely way to get this wrong.

---

## 7. Refusals, and what to do about each

These are Centrifugo's client-protocol codes, not HTTP ones, and the number tells your client whether
to retry. Handle them explicitly rather than treating every failure as "reconnect".

| code | meaning | what to do |
|---|---|---|
| `101` | unauthorized — bad, absent or unverifiable token | **Do not retry.** Sign in again. |
| `103` | permission denied | **Do not retry.** You may not watch that channel. |
| `105` | already subscribed | Not an error. Ignore, or do not double-subscribe. |
| `107` | invalid channel name | **Do not retry.** A name was built client-side; use the server's. |
| `109` | token expired | **Retry after refreshing the token** — see §8. |
| `100` | internal error | **Retry**, with backoff. Our side failed, not yours; it is marked temporary. |

The `101` / `109` split matters: **101** means do not come back, **109** means refresh and come back. A
client that treats them the same either retries forever with a token that will never work, or signs its
officers out every hour.

`100` is worth honouring too. If our database blips, you get `100` rather than a refusal, precisely so
that a blip does not look permanent — a client that gives up on `100` never sees the data after the
blip ends.

---

## 8. Reconnection and token expiry

The connection **cannot outlive the token that opened it**. `expire_at` in the connect reply is the
token's own expiry, and when it lapses Centrifugo disconnects you.

So the lifecycle is: **refresh the access token, reconnect, resubscribe.** This costs one reconnect per
token lifetime and needs no refresh proxy on our side. Plan for it rather than being surprised by a
periodic disconnect — it is the design, not a fault.

**Reconnect on:** transport failure, `109`, and `100`.
**Do not reconnect on:** `101`, `103`, `107` — these will fail identically forever.

A reconnect must be **exponential backoff with jitter**. Do not spin: a client that hot-loops on a
refused connection is a retry storm, and it will look like our outage.

---

## 9. The socket and the read must agree

Initial state comes from `GET /live-feed/positions`; updates come from the socket. Use **both**:

1. `GET /live-feed/positions` for the whole picture, including vessels that have **never** reported
   (they appear with no `position` — that is deliberate, a vessel silently missing from a list is
   indistinguishable from one that does not exist).
2. Subscribe to `live-feed` and apply events on top.

Subscribe **before** or concurrently with the initial read, and reconcile by `id_unit` — otherwise a
fix published between the read and the subscribe is lost with no way to notice.

`backfilled` is derived by the same code on both paths, so the read and the socket cannot disagree
about whether the same fix is live. If you ever see them disagree, that is a bug on our side; tell us.

---

## 10. How to check it

Ask us for the dev command centre address and a test account, and:

1. `POST /auth/login` → take `data.access_token`.
2. Connect to `ws://<host>:8000/connection/websocket` with `{"token": "<token>"}` in `data`.
3. Subscribe to `live-feed`.
4. **We will POST a fix for a test vessel**, or you can post one yourself with a commander account via
   `POST /live-feed/fixes`.
5. You should receive a publication matching §5, with `backfilled: false`.

Nobody has to guess: the API logs every publish it makes, and the system has already been verified end
to end with a real beacon payload.
