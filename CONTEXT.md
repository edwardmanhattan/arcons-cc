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
| **Pane** | One rectangle of the wall. It is a **map** (its own camera, map thread and frame) or a **tree** (the chain of command, which has its own camera but no frame and projects nothing). | "window", "viewport", "split" |
| **Pane view** | What *that* pane is looking at: its selection, its card, its symbol-or-picture range, and its filter. `PaneView`. | the wall's selection — there is no such thing |
| **Tree camera** | Where a tree pane's own points land in the pane: `pane = tree * zoom + pan`, and whether a hand has taken it (until it has, a resize re-fits). | the map's camera |
| **Installation** | A facility — a posko, a base, a dock, a radar station — at one coordinate. Not a Unit: no hull number, no class, no fix chain, no staleness judgement. | "unit", "vessel", "ship" |
| **Post** | One box of a standing structure: what the structure calls it (`name`), the position that runs it (`position_title`), and the person appointed (`commander`, absent when vacant). A post with an `id_unit` **is** a hull. | "node" in the domain, "unit" when it has no hull |
| **Workspace** | A named, independent tiling of panes: its own cameras, its own focus ring, its own panes. **One is active at a time**, and **empty is valid** — "No panes yet." is a workspace, not an error. The header's chips choose between them, and the active one's name is the wall's title — **pressed to rename it**, like a pane's plate. The name is in memory only, like a pane's. | "tab", "page", "layout" (the layout is the workspace's, not the wall's) |
| **Newsroom** | The strategic-environment panel: events from Minos' `banglinstra` feed, read one page at a time. The API's word for the route is `banglinstra`; this code says **newsroom** (the client's mockup spells it *Banglingstra*). | "the news", "the feed" — the Feed is the hull picture |
| **Story** | One event as the corpus grouped it: a headline, the country it happened in, and how many outlets carried it (`source_count`). What the list shows. | "article" |
| **Article** | One document behind a Story: one domain, one URL, one `seen_at`. **A Story is not an Article**: the same wire story under seven mastheads is one Story and seven Articles. | "story" |
| **Scope** | Which tab a Story is filed under — `international`, `regional`, `national` — decided by the **event's** country, not the outlet's. The API also has `unknown`, a real scope and not a null; the panel has no fourth tab, and `min_confidence=0.5` drops those stories anyway. | "category"; the publisher's country |
| **Window** | `latest` — the last 24 hours **of the data held**, anchored by the server to the newest article it has, never to the wall clock — or `archive`, which has no lower bound. | "today", "the last day" |
| **Confidence** | The share of a Story's location rows behind its country, 0–1. A fact to show (below 0.5 the country is not settled), not a filter to re-apply: the server already applied `min_confidence`. | "accuracy", "score" |

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
- **Absence is not zero, in the newsroom too.** A Story with no `country`, a country with no
  `place`, an Article with no `tone`: each draws a dash. A `tone` of `0.0` is a measured neutral and
  still draws; `event_code` is absent for most documents and those are still Articles (`INV-01`).
- **`latest` is anchored to the data, not to the clock.** The client sends the word `latest` and
  computes no 24-hour cutoff: a three-day-old corpus on an air-gapped box must show those three days,
  not an empty tab.
- **A paged reply carries the question that asked for it.** The newsroom's `Reply::Stories` carries
  its `NewsQuery`, so a page arriving after a tab change or a page press is compared with what is on
  screen and discarded. "Newest arrival wins" would draw page 2 of the regional tab under the
  national heading.
- **The data is shared; the question is not.** One Feed, one socket, one Picture — but which hull a
  pane is asking about is that pane's, so a card in one pane is not a card in another (`INV-10`).
  A wall-wide selection was the first version and it drew the same card in every pane.

## Where the contract lives

`docs/spec/` is a pinned snapshot of the Minos API and realtime protocol, at a named commit. It is
not ours to edit; re-vendoring is a deliberate commit. `docs/invariants.md` holds the numbered
promises, `docs/verification.md` holds how they are proved, and `AGENTS.md` says how to work here.
