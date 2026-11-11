# Context — the words this code is written in

Arcons Command Center draws the **live maritime picture** on a wall. The domain is small and the
words are load-bearing: two people using "position" for two different things is how a marker ends
up in the wrong ocean. This file defines the vocabulary; the code uses it exactly.

## The words

| word | means | not |
| --- | --- | --- |
| **Unit** | A hull. Identified by `id_unit`, named by `name`, optionally numbered by `hull_number`. | "vessel", "ship", "target" |
| **Fix** | One accepted position report: latitude, longitude, and whatever was measured with it. | "ping", "update", "location" |
| **Position** | A Fix as the map draws it, after client-side judgement (staleness, interpolation). | the API's `position` object, verbatim |
| **Feed** | The standing picture: every live unit, whether or not it has ever reported. | "the fleet list", "the positions" |
| **Picture** | What the wall shows: the Feed, plus the judgements drawn on it. | "the dashboard", "the view" |
| **Stale** | A client-side judgement from `age_seconds`. The server does not tell us; we decide, once, and say what the threshold is (**120 s**, `units::STALE_AFTER`). | an error, a failure to connect |
| **Backfilled** | Reconstructed from an offline backlog. Real, the newest thing known, and **it did not just happen**. | "current", "live", "late" |
| **Measured** | A Fix a beacon reported. `speed_kn` and `course_deg` exist or they do not. | "actual", "true" |
| **Derived** | A position computed along an order rather than reported by a beacon. Never mixed with Measured silently. | "estimated" as a synonym for both |
| **Interaction** | What the wall is *holding* right now: nothing, a lifted pane, a border, a menu, a filter panel, a rename, the settings sheet. One value (`Interaction`), so two holds at once is unrepresentable (#101). | "mode", "dialog state", a set of booleans |
| **Pane** | One rectangle of the wall. It is a **map** (its own camera, map thread and frame) or a **tree** (the chain of command, which has none and projects nothing). | "window", "viewport", "split" |
| **Pane view** | What *that* pane is looking at: its selection, its card, its symbol-or-picture range, and (soon) its filter. `PaneView`. | the wall's selection — there is no such thing |
| **Post** | One box of a standing structure: what the structure calls it (`name`), the position that runs it (`position_title`), and the person appointed (`commander`, absent when vacant). A post with an `id_unit` **is** a hull. | "node" in the domain, "unit" when it has no hull |

## The distinctions that decide code

- **Absence is not zero.** `speed_kn`, `speed_ms`, `course_deg` and `accuracy_m` are omitted (or
  `null`) when unmeasured. A `0.0` is a *measured zero*. Types are `Option`; nothing defaults
  (`INV-01`).
- **A Unit without a Fix is still a Unit.** Half the fleet may never have reported; those hulls
  belong in the Feed and not on the map. Latitude 0, longitude 0 is a real place in the Gulf of
  Guinea (`INV-02`).
- **Backfilled is history.** Drawing it as "here now" teleports a hull across the map (`INV-03`).
- **Age only goes up** between fixes. A clock that jumps is not allowed to make a fix younger
  (`INV-06`).
- **No history exists** on `live-feed`. After a reconnect, re-read before trusting a marker
  (`INV-09`).
- **A menu holds nothing; a filter panel holds the pointer; a sheet holds the keyboard or the pointer.**
  That is the difference the interaction table turns on (`docs/adr/0004-interaction-state.md`): a menu is
  dismissed by the press that starts the next thing, a filter panel is dismissed by a press outside it
  (the picture stays visible — that is why it is not a sheet), and a sheet must be dismissed before
  anything else can start.
- **A maximised pane is layout, not interaction.** It survives every menu, sheet and gesture coming
  and going, which is why it lives on the `Layout` and the table stays seven rows — a filter panel is a
  hold, but a full-size pane is not.
- **The data is shared; the question is not.** One Feed, one socket, one Picture — but which hull a
  pane is asking about is that pane's, so a card in one pane is not a card in another (`INV-10`).
  A wall-wide selection was the first version and it drew the same card in every pane.

## Where the contract lives

`docs/spec/` is a pinned snapshot of the Minos API and realtime protocol, at a named commit. It is
not ours to edit; re-vendoring is a deliberate commit. `docs/invariants.md` holds the numbered
promises, `docs/verification.md` holds how they are proved, and `AGENTS.md` says how to work here.
