# The phone's screen — `docs/mobile-ui.md`

**What the phone draws, and what it refuses to draw.** [`mobile-remote.md`](mobile-remote.md) is the ground: the
carrier, the pairing, and a client's rules — read it first. It deliberately says nothing about the screen. This note
is the screen.

It exists because the panes control is unlike anything else in this repository: **a diagram of the arrangement that
is also a control**. A list of panes would be a form; the wall's shape is the thing an operator actually wants to
change, and a picture of it can be pressed. Written down, its states stop being discovered by a phone that has to
guess, and its refusals stop being found by an operator in another room.

The note is [issue #225](https://github.com/edwardmanhattan/arcons-cc/issues/225), part of epic
[#217](https://github.com/edwardmanhattan/arcons-cc/issues/217). The decisions it stands on are `ADR-0005`'s
(decision 1: **a client sends intents; the wall renders**) and `ADR-0006`'s (decision 4: **no viewer changes the wall
optimistically**; decision 5: **focus is shared**).

## The screen

```
┌───────────────────────────────────────┐
│ Narrow seas   ①  ②  ③                 │   the workspaces, as chips; ① is active
├───────────────────────────────────────┤
│ ┌───────────────┬───────────────┐     │
│ │ ▣ map         │ ▤ chain of    │     │   each card: a glyph, the title, the kind in words
│ │   “North”     │   command     │     │
│ │  ◉ focused    │               │     │   the ring is the pane that has the keys
│ │               ├───────────────┤     │
│ │               │ ▦ newsroom    │     │
│ │               │   “Straits”   │     │
│ └───────────────┴───────────────┘     │
│  not connected · last read 41 s ago   │   #206's honesty, at phone size
└───────────────────────────────────────┘
```

Three bands, and each is a value the phone has read rather than a thing it remembers:

| band | drawn from | what it is |
| --- | --- | --- |
| the chips | `active`, `workspaces[].name` | which workspace is on the wall. A chip is a press; the wall switches |
| the canvas | `workspaces[].tiling`, `panes[]` | **the arrangement**, as cards. One card per pane, tiles like the wall |
| the state line | the last read's outcome, and a refusal's `kind` | what the phone actually knows: live, connecting, not connected, update required |

The phone's read is `{"v": 1, "read": "wall"}` — the **projection**. Not `read: "scene"`: a scene carries the water,
the facilities, the structures and the cards, which is everything a renderer needs and nothing this screen draws.
The projection is small on purpose (`ADR-0005` decision 7), and this screen is why its fields are the ones they are
(#181, #218).

## The overview canvas

**The phone lays the tiling into its own rectangle** (#220). That is the whole claim of the canvas: the arrangement
is a tree of **shares**, so the shape is the same in a 360 pt phone and in a 1280 pt panel — a `ratio` is `a`'s
share of its cell, not a number of points. Nothing is scaled from the wall's pixels, and no camera, viewport or
window size is ever asked for.

**A leaf is a pane id** (#218), not an index into `panes`: a phone holds a projection across changes, and after a
`pane.close` an index would name a different pane — the right arrangement of the wrong panes. Every leaf is a card,
and the `panes[]` row for that id says what the card says:

| on the card | from |
| --- | --- |
| its title | `title` — the operator's name, or the pane's own number. **Every pane has one** (#83) |
| its kind, as a glyph **and a word** | `kind` — `map`, `chain of command`, `newsroom` |
| a ring | `focused`: the pane with the keys. Drawn in the wall's **`focus_edge`** role — the one colour that means "this one has the keys" (`src/ui/tokens.rs`), named once in the phone's palette rather than repeated per screen |
| a mark | `maximised`: the pane the wall is drawing over the others |

The active workspace's name is the **title of the screen**, as it is the title of the wall (#139), and the chips are
the switch. Naming one is the wall's sheet; the phone shows the name and does not edit it.

**The glyph and the word are one rule, and the word is the load-bearing half.** A glyph nobody can name is a button
nobody presses (#111): the canvas draws the kind in words, and the glyph is the framework's own icon set rather than
a path drawn for this screen — the phone's UI is not Rune and not the wall's ([`ADR-0008`](adr/0008-the-phones-framework.md)),
so Rune's catalogue is not what it draws from, and a symbol invented here would be a picture nobody else can name. A tree pane says what the wall's own header control
says (`chain of command`, `src/ui/mod.rs`), because two words for one kind is how a reader learns to distrust both.

**A maximised pane is marked, not expanded.** `ADR-0004` is the rule: a maximised pane is *layout*, not an
interaction — it survives every surface coming and going. The wall draws it over the others and the tiling is still
the tiling, so the canvas marks that card and keeps drawing the arrangement underneath. A phone that grew one card
to full size would be showing an arrangement the wall does not have.

**A ring is the wall's, not the phone's.** `focused` is one value for the whole wall (`ADR-0006` decision 5), so the
ring the phone draws is the ring the glass draws. A press on a card sends `pane.focus`; the ring moves when the next
projection says it moved.

**An empty workspace is a row, not an error.** `tiling` is `null` for a workspace the wall cannot tile — #141's legal
empty one — and the canvas draws the wall's own words where the cards would be (`No panes yet.`). The chip stays: a
workspace emptied by closing its last pane is still on the wall, by name. An empty *tree* is a different thing (a
wall claiming to tile nothing), and drawing the two alike is how a client invents a pane.

### The cells are not the cards

The phone mirrors one function, `view::Layout::cells` (`src/ui/view.rs`): the cells cover the canvas exactly, and a
point — a finger — is hit-tested against a **cell**, never against the drawn card. The wall's own rule, and for the
same reason: a press in the gutter still belongs to a pane's cell. The gutter is the phone's own look; how wide it is
is a design choice and no number the phone has to match. The wall's `GAP` insets what a *wall* draws.

Then the three numbers that decide where a drag **lands** are not on the wire, so they are recorded rather than
guessed: `fixtures/control/tiling/` (#220) carries `PANE_EDGE_BAND` = 0.3, `MIN_PANE_PT` = 160 and
`RATIO_RANGE` = 0.05…0.95, with the cells it produces and with what a release at a recorded point does. A client
that guesses 20% for the band sends `centre` where the wall's own hand sends `left`. The band is a share of a card's
own span and so it transfers to the phone as it stands; `MIN_PANE_PT` does not — see *the gutter* below, where the
difference is.

## The drag

A drag is a held gesture, and a held gesture is the client's: the wall sees **one intent, on release**. That is
`docs/control.md`'s rule and this screen's whole design — the phone draws every intermediate state itself, and sends
nothing until it knows what it means.

```
idle ──long-press a card──▶ held ──move over another card──▶ preview
  ▲                            │                                │
  │                            │                                ├── release on a card: one pane.drop ──▶ moving…
  │                            └── back where it started ───────┤   (nothing sent: a release over the lifted
  │                                                             │    pane is no drop, exactly as on the wall)
  └──── the projection says so ──────────── moving… ────────────┘
```

**Long-press lifts.** The card rises out of its cell, follows the finger, and the canvas keeps drawing the hole it
left. The hold is the phone's; nothing is on the wire yet.

**The landing is drawn before release, and it is computed the wall's way.** The card under the middle of the lifted
card is the target; each of that target's four edges is `|point − edge| / span` deep; an edge counts only below
`PANE_EDGE_BAND`; the nearest edge that counts wins; and **no edge in band is `centre`**. That rule is
`view::Layout::drop_for`'s (`src/ui/view.rs`), stated once in `fixtures/README.md` and answered for every recorded
case in `fixtures/control/tiling/`'s `drops`. A preview computed another way is **two layouts that disagree**, which
is the bug `pane.drop` exists to prevent.

**The preview is the slot, not the finger.** The lifted card is drawn above the thumb so the operator can see the
slot it will land in, and the slot is shaded: a copy hanging from the hand covers the one thing you are looking at.
The wall settled this in #84 for a pointer; a thumb covers more, not less.

**The preview's shape is `Layout::apply`'s**, so that the picture before release is the picture after:

- a **re-tile** (`left`, `right`, `top`, `bottom`): the lifted pane is closed out of its slot (its sibling takes the
  space) and the target is split at one half on the named side. The pane **keeps its id** — its camera, its thread
  and its frame live on that id, and a re-tile that minted a new one would be a new pane wearing the old one's title.
- a **swap** (`centre`): the two leaves trade places, and every ratio above them stays where it was.

**Release sends one intent**: `pane.drop` with `pane`, `onto` and `edge`. One. The forty points of the drag are the
phone's own drawing, and `docs/control.md`'s "a drop is not a drag: no hold, no preview on the wall, no sixty intents
a second" is the rule that keeps a phone's radio out of a gesture.

**Cancel is a place, not a button.** Dragging back over the lifted card's own cell is no drop — `drop_for` returns
nothing over the dragged pane, which is what makes "released where it started" a no-op on the wall too — so the card
settles back and **nothing is sent**. The system's back gesture does the same thing, and both are silent, because
nothing happened.

**A refusal returns the card and shows the wall's words.** The sentence is the wall's `why`, verbatim — the rules are
the wall's, and a paraphrase is a second copy of a rule (the same reason `arcons-cc ctl` prints `why`). What the
phone says around it depends on the refusal's `kind`, which is the next section but one.

### The gutter

A gutter between two cards is a handle, and dragging it is the other settled gesture:

- **one `pane.resize`** on release, with the two panes the gutter separates and the share the first side takes. The
  pair names the border — `pane` and `other`, in either order — because an id names one pane and a pane has a border
  on each side.
- **the share is measured against the cells**, never the drawn cards, because the wall's ratio is a share of the
  cell. A share measured inside the phone's gutter is off by the gap.
- **the preview is the share, and the wall's clamp is the wall's.** `pane.resize` is clamped to `RATIO_RANGE` and the
  wall's cells stop a pane at `MIN_PANE_PT` — 160 points *of the wall's viewport*, a rectangle the phone is not told
  and does not need (the recorded `narrowest-share.json` is exactly this: a 0.05 share drawn as 64 pt and lifted to
  160). A phone that recomputed that clamp would be guessing a window size, so it draws the share it read and says
  `moving…` until the projection lands. The projection is the truth; the phone's arithmetic stops at the value it
  sends.

## Nothing is optimistic

`ADR-0006` decision 4: a press sends an intent, the wall applies it, and **the viewer that pressed sees it in the
next projection like everybody else**. A phone that moved its own card first would either be right — and have drawn
a wall it could not know — or wrong, and visibly argue with its authority. So the interesting state is **designed**:

| moment | what the canvas draws | what it says |
| --- | --- | --- |
| before release | the card in the slot it would land in | nothing: the operator is still deciding |
| after release, before the answer | the card in that slot | `moving…` |
| the answer is `ok`, the next projection lands | the card where the projection puts it | nothing |
| a refusal arrives | the card back in the cell the last projection gave it | the wall's words |

`moving…` is not a spinner and not a claim: it is the phone saying *I have asked, and this is where it will go if the
wall agrees*. It spans two round trips — the intent's answer, then a read — which is why it has to exist at all.

**A snap-back with no sentence is the failure this rule must not produce.** Every return of a card is accompanied by
one of two things: the wall's words, or `moving…` until the wall's answer arrives. A card that jumps back while the
screen says nothing is the operator losing a gesture they made and being told nothing about it.

One gesture at a time: while a card is `moving…` the canvas takes no second lift, because two drops in flight are two
answers to "where is that pane" and the projection will only settle one of them.

## The four states

The phone knows exactly four things about its connection, and each one is *derived*, never felt:

```
connecting ──a read lands──▶ live ──a read fails──▶ not connected
     │                         ▲                         │
     │                         └───── a read lands ──────┘
     └──────────── kind: "version" ────────────────────▶ update required
```

| state | how the phone knows | what it draws |
| --- | --- | --- |
| **connecting** | it has never had a successful read | the chips are absent, the canvas is empty, and the state line says `connecting…`. **No diagram from a previous life**: a phone that has read nothing has no shape to draw |
| **live** | the last read landed | the canvas, and the state line says nothing at all. Silence is the phone saying *this is the wall* |
| **not connected** | a read did not land | the diagram it last read, **dimmed**, under `not connected · last read 41 s ago` |
| **update required** | a refusal with `kind: "version"` | one screen, in the next section |

**`connecting` is not `not connected`.** A phone that has never heard from the wall is not showing a stale
arrangement; it is showing none. `ADR-0006` decision 6 made the same distinction for a kiosk, and it matters more
here: the two states draw different screens, and a phone that confused them would either hide a usable diagram or
draw one that has never existed.

**`not connected` labels the diagram and takes its verbs away.** The diagram stays, because it is the arrangement the
operator was looking at a moment ago and it is the only thing on the screen that says which pane is which. It is
drawn dimmed, the state line carries the age of the last read, and **the canvas sends nothing** until a read lands.
That is not caution: every intent on this screen names a pane by id, and a stale id is `no such pane`, a `rule` —
*do not retry until something else changes*. The something else is a read, so the retry is a read and never a replay
(`mobile-remote.md` decision 1).

**The number in `last read 41 s ago` is an age, not a sequence.** The wall's own stale banner for a viewer names a
sequence (`last held #41`, #206) because a scene carries `seq`, and since #221 a projection carries one too, with a
watch to go with it. This screen still draws the age: it is the number a person can act on, and an age says exactly
what it says, where a sequence is a number the phone has to be told means "current". The `seq` is what the phone
compares when a publish arrives — one version after the one it holds, or it reads (`docs/control.md`).

**Nothing is retried through a `version` refusal.** The screen that replaces the phone's whole UI carries the wall's
words (`unsupported protocol version`), one sentence in the app's own words, and one button that opens the store
page. No dialog to dismiss, no second button, and no request sent in the meantime: a client that retries a version it
cannot speak is a client that loops, and `docs/control.md` already says a version refusal means *send nothing more,
update it*.

```
┌───────────────────────────────────────┐
│                                       │
│   unsupported protocol version        │   the wall's words, verbatim
│                                       │
│   This app is older than this wall.   │   the app's own sentence
│   Update it to control the wall.      │
│                                       │
│   ┌───────────────────────────────┐   │
│   │           Update              │   │   one button; back does nothing
│   └───────────────────────────────┘   │
└───────────────────────────────────────┘
```

## The verbs it uses

The vocabulary is `docs/control.md`'s, and a note is not a second reference. This screen sends four verbs and refuses
six, and the six are named because a note that is silent about what it will not do leaves the next author to
discover it from a refusal.

| the verb | the gesture on the canvas | why this one |
| --- | --- | --- |
| `workspace.focus` | a chip is pressed | the canvas is one workspace at a time, so a chip is a switch and not a target. `index` is the chip's place in the row |
| `pane.focus` | a card is pressed | the ring is shared, and a press is how a client says *this one has the keys*. `pane` is the card's id |
| `pane.drop` | a lifted card is released over another | the wall's own landing: `Layout::apply` through `State::apply_drop`. `edge` is the slot the card was over, and `centre` is a swap — one verb, because a swap is a slot like any other. The accessible **Move to…** sends this same verb, so the hand and the list cannot disagree |
| `pane.resize` | a gutter is dragged and released | the share the wall already stores, clamped by the wall. `pane` and `other` are the two the gutter separates, in either order |

| the verb this screen deliberately does not send | why |
| --- | --- |
| `pane.split_at` | it takes `at`, a point in **that pane's frame** pixels. The phone has no frame and is told none: the field would be a guess, and `docs/control.md` already says a client that has not been told the frame cannot honestly send one |
| `cursor.move` | a cursor lives in the **picture panel's** points. The phone draws a diagram, not the picture, so it has no space to put a point in |
| `cursor.press` | a press means something where it lands — a hull, a pane's water — and this screen draws neither. The phone's press names a **pane**, which is a value, not a point |
| `cursor.release` | the same, and a cursor with no press has nothing to release |
| `pane.close_all` | the wall's pane menu offers `Close all panes` where it changes more than the rows beside it (#241), and this screen does not: a **clear** on a diagram is one press from a workspace with nothing where the operator's panes were, and a reset is asked for deliberately. The verb exists on the wire for a client that means it |
| `workspace.close` | a delete on an arrangement is not a press for a card, and a phone cannot show what it destroyed (#242). The wall's chip menu offers `Close workspace` only where the workspace is empty and not the wall's last; the phone draws the arrangement and leaves forgetting it to the glass. The verb exists on the wire for a client that means it |

The plane has more verbs than this screen uses — `pane.close`, `pane.open`, `pane.split`, `pane.move`,
`workspace.new`, `pane.close_others`, `workspace.close_others`, `workspace.close`, `menu.focus`, `menu.submenu`, `menu.choose`,
`surface.settings`, `surface.close`, `map.camera`, `object.follow`, `pane.select`, `rename.begin`, `rename.set` —
and the note does not name them here because this screen does not send them. Closing a pane is a menu's row on the
wall, not a long-press on a diagram, and moving a pane **across** workspaces is `pane.move` and not a drop:
`pane.drop` is within one workspace by rule, so a phone carrying a card to another workspace is two settled changes
and a gesture nobody has drawn yet.

The phone's roadmap verbs are `pair.begin`, `pair.claim` and `pair.forget` ([#194](https://github.com/edwardmanhattan/arcons-cc/issues/194)).
They are not on the wire today, and this table is about what the wall answers **now**: the pairing screen below is
specified against `mobile-remote.md` decision 3, and the three verbs join the table when #194 lands.

## Refusals → what the phone does

Every answer is `{"ok": true, "changed": …}`, a refusal, or a projection. A refusal carries the wall's words in `why`
and its category in `kind` (#198), and the category is what decides the screen — never the prose, because a sentence
can be edited and a category cannot.

| `kind` | the wall's words (examples, verbatim) | the phone's sentence | what it does next |
| --- | --- | --- | --- |
| `rule` | `no such pane`, `the last pane on the wall`, `those panes are not neighbours`, `the pane cannot be swapped with itself`, `the tiling refused the pane` | the wall's words, as they are | returns the card to the cell the last projection gave it, then **re-reads**, because a rule usually means the premise moved — and never retries the same intent |
| `input` | `unknown verb`, `"edge" is left, right, top, bottom or centre`, `"ratio" is a share from 0.05 to 0.95` | none of the wall's words: *This app asked the wall for something it does not understand.* | this is the phone's own bug. It re-reads, restores the cell, and writes one log line — no secret, no host, no token (`INV-08`). An operator is not asked to fix an `unknown verb` |
| `version` | `unsupported protocol version` | the update screen, with the wall's words on it | sends **nothing more** in this session. One button, which is not a retry |
| `busy` | `the wall did not answer`, `the wall has stopped` | *The wall is not answering.* | keeps the diagram and the pending card, waits, and retries the **read** first: a read is how the phone learns whether the wall is there at all. It never treats `busy` as a refusal — the change may have landed |

Two rules under the table:

- **A refusal is not an error state.** The connection is still up and the next line may be a fresh request
  (`docs/control.md`), so a `rule` or an `input` refusal leaves the phone `live`: the wall answered. `busy` is the
  other thing — the wall did not answer — so the phone retries the read, and a read that keeps failing is what turns
  the state to `not connected`. Only `version` ends a session.
- **A stale premise is a rule, not a busy** (#222's rule, and it is already true at one client): a `pane.drop` naming
  a pane another client closed is `no such pane`. The phone's answer to that is a re-read, and the canvas then shows
  the arrangement the other client made.

## Accessibility, and why the UI is not in Rust

A drag is not a reachable control. No screen reader can long-press a card and hover it over another card's edge
band; no switch control can hold a gutter halfway. So the drag is a **shortcut**, and the screen has a path that is
made of words:

**Every control here is drawn, so every control carries its own semantics node** (`ADR-0008` decision 2): the
framework renders the chips, the cards and the rings, and it will describe none of them unless this screen says so.
The path below is therefore not a fallback for a screen reader — it is the same walk that draws the canvas, spoken.

**Move to…** — on any card, a list of places, and then the same intent the hand sends:

```
Move “North”…
  ├ left half of “Feed”            →  pane.drop { pane: 3, onto: 2, edge: "left" }
  ├ right half of “Feed”           →  pane.drop { …, edge: "right" }
  ├ top half of “Feed”             →  pane.drop { …, edge: "top" }
  ├ bottom half of “Feed”          →  pane.drop { …, edge: "bottom" }
  ├ swap with “Feed”               →  pane.drop { …, edge: "centre" }
  └ cancel
```

Five rows for every other pane on the canvas, in the wall's own order of edges. The words are the phone's; the
values are the wall's (`left`, `right`, `top`, `bottom`, `centre`). **The list is a pane's slots and not the canvas's
regions**: `pane.drop` names a slot *of a pane*, so "top right" of the canvas is not a name this vocabulary has, and a
list that offered it would be offering two intents with one press. The phone's accessibility path and its drag end
at the same request, so a voice user and a hand user cannot land in different places.

**The canvas has one spoken description, from the same walk that draws it.** The cards are laid out by walking the
tiling tree; the description is that walk in words, so a pane the canvas draws is a pane the reader hears:

> workspace 2, three panes: map “North” left; chain of command “Fleet” top right; newsroom “Straits” bottom right.

Each pane is **its title and its kind**, because a description that says `map` twice is one the operator cannot act
on — the picture has a ring to tell them apart and the words need the title to do the same job. The focused card is
announced as focused, the maximised one as full size, and the empty workspace as `No panes yet.`, which is what the
screen says. The chips are a tab list; the cards are buttons in the tiling's reading order; the ring is drawn and
described, never colour alone (#111's rule, one surface further out).

**What this note does not decide** is the gutter's accessible path. A share is a number, and no recorded vocabulary
names one — `RATIO_RANGE` is #220's, and a picker of shares (or of `narrower`/`wider`) is a decision nobody has
taken. It is recorded here rather than invented, so the next ticket starts from the question.

## Pairing

One screen, and it is the only screen before the overview:

```
┌───────────────────────────────────────┐
│  Pair with a wall                     │
│                                       │
│  ┌───┐┌───┐┌───┐  ┌───┐┌───┐┌───┐     │   six digits, drawn on the wall, typed here
│  │ 4 ││ 8 ││ 1 │  │ 9 ││ 0 ││ 2 │     │
│  └───┘└───┘└───┘  └───┘└───┘└───┘     │
│                                       │
│  Name this phone   [ operations phone ]│
│                                       │
│  ┌───────────────────────────────┐    │
│  │            Pair               │    │
│  └───────────────────────────────┘    │
└───────────────────────────────────────┘
```

Someone at the wall opens pairing — an intent drawn on the wall, or `arcons-cc ctl` — and the wall draws six digits
for **two minutes** (`mobile-remote.md` decision 3). The code is a picture in the room and never a message: the phone
does not ask for it, so it cannot pair itself.

The phone sends the code and a name, and gets back **a key, once**. The key goes into the platform's store —
Keystore or Keychain, never a file and never anything this repo can read (#223's decision 4) — and from then on the
phone's identity is that key and its name. The code is not kept, not logged, and not shown again; a six-digit number
that survives the claim is a code that outlives its two minutes (`INV-08`).

**Forget this wall** is a row on the overview's settings: it deletes the key from the platform store and the wall's
name with it. That is a local act, and the wall's side — the device's row — is revoked where the rows live: the wall's
own surface, with `pair.forget` (#194). A phone that has forgotten its key cannot send anything, so there is nothing
for it to ask.

Pairing's refusals are `rule`s, all three of them, and each has its own sentence and its own next step:

| the wall's words | the phone says | what it does |
| --- | --- | --- |
| `no pairing is open` | the code expired | returns to the code field, emptied: the wall has to be asked again |
| `that code is not the code` | that code is not the wall's | keeps the field, selects the digits, no retry |
| `that device is already paired` | this name is taken | keeps the name field focused: a second phone is a second name |

A wrong code and an expired code are refused **by name**, not by a "no": the two need different actions, and a
client that could not tell them apart would retry the wrong one for ever.

**A code that has been guessed at is a code that gives up** (#262): the wall burns it after five wrong ones, and the
answer to the fifth is `no pairing is open` — the wall's own words for a code that is gone, which is the first row
above and its step with it (the field empties and the wall has to be asked again, `pair.begin`). The phone adds no
sentence of its own for a burnt code, and it cannot tell the fifth wrong guess from an expired code, which is the point:
nothing on the wire says how many tries are left.

## What it refuses to draw

`ADR-0005` decision 7, restated where a UI author will read it:

- **The water.** No tile, no chart, no coastline. There is no codec, no frame clock and no bandwidth budget in this
  client, and `INV-04`/`INV-12` stay claims about one machine.
- **The fleet.** No hull, no marker, no track, no fix's age, no `stale` judgement. The projection does not carry
  them, and a client that could draw them would have the picture.
- **The cards.** No hull card, no facility card, no photograph: `panes[].selected` is the *id* of the hull a pane's
  card is about, and naming it is not drawing it.
- **The story text.** No headline, no country, no source count, no page. A newsroom pane is a card that says
  `newsroom`, and the phone's business with it is where it is, not what it reads.
- **A cursor it did not place** — and the phone places none. A cursor's points are the picture panel's; the canvas
  is a diagram of the arrangement, and the three cursor verbs are in the table above as verbs it does not send.
- **A stale diagram presented as current.** The label, the dimming and the read are the whole of what separates
  `not connected` from a lie.

A pane's `camera` is in the projection, and this screen does not draw it: a latitude and longitude on a diagram
would be a map's subject on a card, and a camera is a value a map client sends, not a fact about an arrangement.
The same for `panes[].title`'s ellipsis: the phone cuts a long title the way it cuts any label, with the
framework's ellipsis (Flutter's, `ADR-0008`), because the wall's drawn `…` is a rasteriser's answer and not a value
on the wire.

## What this note leaves open, deliberately

- **The refresh cadence, now decided**: #221 added the `seq` the projection carries and the watch that publishes it at
  4 Hz when it changed (`docs/control.md`). The rule this screen keeps is unchanged: the phone reads first on every
  connect and every return from sleep, and takes the watch's publishes in between — it reads and never replays.
- **The gutter's accessible path** — see the accessibility section. A share is a number and no recorded vocabulary
  names one.
- **The phone's framework and design system**, which is `ADR-0008`'s
  ([#223](https://github.com/edwardmanhattan/arcons-cc/issues/223)) and lands with it, and **the corpus**, which is
  #224: a client in another language obeys this wire by replaying `fixtures/control/` through
  `scripts/conformance.sh`. The phone's own test suite runs the same files, and this note is the specification those
  tests are about.
- **Two remotes at one wall** (#222). This note describes one phone; when there are two, the ring is still shared,
  a refusal is still by arrival order, and a stale premise is still a `rule`.
