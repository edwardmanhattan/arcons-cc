# How much NATO symbology can this wall draw honestly? (#33)

- **Ticket:** [#33](https://github.com/edwardmanhattan/arcons-cc/issues/33), under epic #1
- **Date:** 2026-11-07
- **Feeds:** [#32](https://github.com/edwardmanhattan/arcons-cc/issues/32) — the symbol↔picture switch
- **Verdict: one frame, one hull icon, and the label does the rest.** The wall draws a *surface vessel*
  symbol whose affiliation is **unknown**, whose status is drawn as ours already is (live / stale /
  history), and whose icon is one of **eleven** sea shapes plus submarine. Nothing else the standard
  offers can be sourced from Minos, and at the wall's marker size most of it could not be seen anyway.

This is the reasoning, the mapping table, and the two pictures behind it. It is a document because the
ticket is research: no code changes here except the sheet generator.

## 1. What Minos knows, and what the standard wants

The standard (MIL-STD-2525, APP-6) composes a symbol from six things — affiliation, battle dimension,
status, an icon, a set of modifiers, and an echelon. Read against the pinned contract
(`docs/spec/minos-api.yaml`) and our model (`src/data/types.rs`):

| 2525 / APP-6 field | what Minos has | what this wall draws, and why |
| --- | --- | --- |
| **Affiliation** (friend / hostile / neutral / unknown) | **nothing.** No field on a unit, no lookup, no default | **Unknown, always.** The wall shows one navy's own fleet, so "friend" would usually be true — and it is still a claim the data cannot make. The quatrefoil says *nobody has said*, which is exactly the state. If a source appears, this table gains a row, not a guess. |
| **Battle dimension** (land/sea surface, air, subsurface) | `movement_domain` on the hull (a `Helper`), plus the feed's own nature | **LandAndSeaSurface, closed frame**, for the live feed — every hull in `live-feed` is a surface vessel reporting a beacon. `movement_domain` is *not* dimension: the spec warns it "answers how a thing MOVES: a submarine moves Submerged and is an Aset Laut". It could inform the choice (Submerged → subsurface, Air → air) and that is a mapping to decide with the operator, not to infer. |
| **Status** (present / planned / …) | derived: fix age, `backfilled` | **Map ours**: `Glyph::Live` → present, `Glyph::Stale` → present-but-not-current, `Glyph::History` → the past. Note this is *ours*, not 2525's palette: the standard's planned/anticipated is about intention, and a hull that stopped reporting is a different fact from one that was never there. `INV-01`/`INV-03` stay the authority. |
| **Icon** (what kind of thing) | the operator's breadcrumb: domain > **category** > **unit_type** > **class** > hull | **A curated sea vocabulary, chosen by the operator's categories.** The taxonomy is *their* vocabulary; 2525 is someone else's. Mapping category→icon is a judgement per category, and the honest default for an unmapped category is `Unspecified` (an empty frame interior), not a plausible warship. |
| **Modifiers** (airborne, amphibious, motorized, …) | nothing | **None.** They qualify ground and air formations; a single hull has none, and inventing one would be a claim about capability. |
| **Echelon** (team … army group) | nothing — a hull is a platform, not a formation | **None.** TFG reached the same conclusion for the same reason ("three ticks over KRI Cakra would be a falsehood rather than a default"). |
| **Course / speed leader** | our `course_deg`, `speed_kn` | Drawn as **ours**: the heading tick already exists (`markers::place` draws it only when measured, `INV-01`). A 2525 speed leader is a different notation with its own meaning; do not borrow the name. |
| **Name / hull number** | `name`, `hull_number` on the feed | Drawn as **text beside the symbol**, which is what enables the rest of this table to stay blank. TFG moved hull class out of the icon into the label for exactly this reason. |

Two consequences: **the wall's symbol carries one frame and one icon** — everything else the standard
composes is either ours or empty — and **affiliation is the one field we must state in the negative**,
because a wall that silently draws "friendly" is making a claim nobody entered.

## 2. What the standard costs at the size the wall draws

This is where the ported standard actually breaks, and it is arithmetic, not taste.

The frame takes the outside of the symbol box. TFG derives the icon's reach rather than tuning it
(`frame_icon_radius = ICON_BOX_HALF · box / canonical_height − box · STROKE_RATIO`, with
`ICON_BOX_HALF = 50` and `STROKE_RATIO = 0.072`):

| frame | canonical height | icon diameter, as a fraction of the box | at 12 pt | at 24 pt | at 32 pt |
| --- | --- | --- | --- | --- | --- |
| rectangle (friendly) | 100 | `2·(50/100 − 0.072)` = **0.856** | 10.3 pt | 20.5 pt | 27.4 pt |
| square / diamond (neutral / hostile) | 110 / 144 | 0.765 / 0.550 | 9.2 / 6.6 pt | 18.4 / 13.2 pt | 24.5 / 17.6 pt |
| quatrefoil (unknown) | 138.5 | `2·(50/138.5 − 0.072)` = **0.578** | **6.9 pt** | 13.9 pt | 18.5 pt |

And the stroke, at `0.072 · box`: **0.86 pt at 12**, 1.73 pt at 24, 2.30 pt at 32.

So today's marker — `tokens::MARKER = 12.0`, with `MARKER_TARGET = 24.0` already reserved as the touch
target — would draw an **unknown-affiliation** symbol's icon **6.9 pt across, with a 0.86 pt stroke**.
That is a dot with a halo, and it is the strongest argument in this document:

> **A symbol at this wall's current marker size is not a symbol.** The frame is 12 pt and the icon
> inside it is under 7 pt. Drawing 2525 at 12 pt is worse than drawing the current 12 pt glyph,
> because it costs four more vertices to say the same "something is here".

The wall should draw symbols at **24 pt** (the existing target size, so no layout changes) or larger;
`#32` already needs a `PHOTO_ZOOM` and a size for the picture, and this is the number the symbol end
of that switch should use.

## 3. Which icons survive

`docs/research/sheets/wall-symbol-icons.svg` is the contact sheet: the 24 icons TFG's manifest keeps,
at 12, 16, 24, 32 and 40 pt, fitted on the longer axis as the map fits them. Read it at 12 pt and then
at 24 pt; the difference is the whole argument above.

What it shows, at our sizes:

- **Surface combatant, patrol craft, amphibious warfare ship are one icon at 12–16 pt** — a downward
  arrow, differing only in the tail's step count. At 24 pt the tail reads; at 32 pt the three are
  clearly three.
- **Tanker / auxiliary / hospital ship are one trapezoid** at every size here; the hospital's cross is
  the only difference, and it lands at about one stroke wide at 24 pt.
- **Mine warfare** (three bumps) and **carrier** (a filled arrow) do read at 24 pt — they are the two
  shapes in the surface set that survive without help.
- **Submarine** is a distinct filled shape at 12 pt already.

TFG measured the same thing at 22 pt and moved its box to 32 pt in response; their
`assets/symbology/FINDINGS.md` §2 records the contact sheet that forced it, and §3 records two icons
dropped because their features were finer than one stroke. Neither finding is about TFG's drawing:
they are properties of the standard at small sizes.

**The recommendation for the sea set** is therefore the ten TFG already curated, plus these rules:

1. Draw at 24 pt or more; below that, draw *one* generic surface icon for every hull rather than
   pretending the class is readable.
2. Do not add a 2525 icon the operator's taxonomy cannot supply — an `Unspecified` interior is a true
   statement and every invented icon is a false one.
3. The hull's **name** is the identity channel, as it is today in the card; the symbol is the
   *category* channel.

## 4. The text-code question

807 of milsymbol's 1,496 icon keys are **letter codes**, not geometry (`text("DD")`), and only 59 sea
icons have geometry at all (TFG's `FINDINGS.md` §1). The tempting answer is "draw the letters".

It does not survive the wall. A 24 pt symbol box can hold a 3-letter code at about **7 pt** — and this
wall's own body text token is `tokens::LABEL`, **12 pt**, because the display is read from across a
room. A 7 pt code inside a stroke is less legible than no code, and it competes with the hull's name
label, which is the one piece of text on the map that is *exactly* right (an operator typed it).

**Decision: geometry only, no text inside symbols.** Text stays where it is already true: the marker's
name (when labels land) and the card's rows.

## 5. Reuse: lift TFG's module, with the licence paid

TFG's `src/symbology/` (4,045 lines) is the right starting point and its design is unusually compatible
with ours: it depends on **nothing** — no toolkit, no database, no map — so it compiles into a crate
whose only job is this. `frame.rs` derives the frames, `icons.rs` states the readability thresholds,
`generate.rs` emits the table from the manifest, and `svgpath.rs` flattens curves to polylines, which
is the only form a Rune element can draw.

Licence, stated now so it cannot be discovered later in review:

- The geometry originates in **`spatialillusions/milsymbol`, MIT, © 2017 Mans Beckman**, pinned by TFG
  at `f5134380157f475cbf5a9bdec69b6c33cf66e0e7` (v3.0.5). MIT is compatible with proprietary delivery;
  it requires attribution and the licence text to travel with the copy.
- Therefore **`THIRD-PARTY-NOTICES.md` gains a row for milsymbol** when the module is lifted, and the
  generated file carries its provenance in its header (TFG's already does). This document's sheet is
  derived from that geometry, so the row exists **now**, with the sheet as the reason.
- `vendor/README.md` records a vendored copy when one exists; a *lifted module* is not a vendored
  dependency, so this belongs in an ADR (or an ADR note) rather than `vendor/`.

**Recommended shape for #32**: a new `src/symbology/` in this crate, holding `mod`, `frame`, `icons`,
`icons_generated` and `svgpath` lifted from TFG, with the manifest and generator kept so the table can
be regenerated rather than hand-edited. A Rune painter on top of `IconMark` polylines lives in
`src/ui/symbol.rs`.

## 6. The invariants travel with the data, not after it

TFG's `generate.rs` enforces, at generation time, the three things that keep a symbol vocabulary from
rotting. Port them with the data or the data will drift:

1. **Fit**: every icon's ink fills the em box on its longer axis to a floor (their tightest is 0.24,
   loosest 0.84), so no icon is a dot and none overflows.
2. **Gap**: no feature finer than one stroke — this is what dropped two upstream icons, and no box size
   rescues them.
3. **Fill from geometry**: upstream's default is "fill everything", which *deletes* line-only icons
   (the infantry saltire has zero area). The fill decision must be made from the geometry, not from
   the `fill:` flag.

And one invariant of ours, which TFG does not need: **the symbol vocabulary is a `match` with no
catch-all over the wire enum** (`INV-07`'s spirit) — a taxonomy category the crate does not know must
draw `Unspecified` **visibly**, not `_ => SurfaceCombatant`.

## 7. When the wall knows nothing

The empty cases, each with its drawn answer, because "nothing is drawn" is a lie a map tells:

| state | drawn |
| --- | --- |
| no icon for the class | the frame + an **empty interior** (`Unspecified`) — a real state, not a hole |
| no course | symbol **upright**, no heading cue (`INV-01`) |
| no fix | **no symbol at all** (`INV-02`: absence is not zero, coast is not a hull) |
| stale | the same symbol, dimmed — status is ours, not the standard's |
| backfilled | history styling, never `present` (`INV-03`) |

The table is about **symbols**. A **picture** is different in one way that matters: its orientation is a
measurement, so it cannot be left to a default. The backend stores every hull photograph **bow-east**
(`GET /units/8` is 2167 × 280, the flight deck and the bow ramp at the right edge; `/units/5` matches),
so the drawn angle is `course − IMAGE_BOW_DEGREES` (`units::mark`, 90° — [#63](https://github.com/edwardmanhattan/arcons-cc/issues/63)).
A hull steaming east draws the image exactly as stored, a hull steaming north is turned a quarter to
port, and **no course** lands at −90° — bow north, the way a ship icon is drawn, which is not a claim
that the hull is steaming north. Before #63 the constant did not exist and a hull steaming north was
drawn broadside.

## 8. Recommendation

1. **Draw one frame, one icon.** Affiliation unknown; dimension land/sea surface; status ours; icon
   from eleven curated sea shapes plus submarine.
2. **Draw it at ≥ 24 pt.** At 12 pt the icon inside the frame is 6.9 pt and the standard cannot be
   seen; `#32` should take 24 pt as the symbol size at the switch.
3. **No text codes**, no modifiers, no echelon: nothing sources them, and the label carries identity.
4. **Lift TFG's module** (with its generator and invariants) into `src/symbology/`, and pay the
   milsymbol notice the same commit.
5. **Map the operator's categories to icons with them, not at them** — a table in an ADR, one row per
   category, with `Unspecified` as the explicit default for any category not yet decided.

## 9. Reproducing the picture

```sh
scripts/symbol-sheet.py            # reads TFG's generated table, writes the SVG
rsvg-convert -w 1400 docs/research/sheets/wall-symbol-icons.svg -o /tmp/sheet.png
```

The script reads `~/Work/Crossnet/tfg` by default (`--tfg` to point elsewhere) so the sheet cannot drift
from what the sister project would draw. It renders **polylines only** — one fill per closed mark, one
stroke per open mark — which is why it is a judgement aid and not a verifier: once #32 lands, the
verification is a golden capture of the wall itself (`INV-11`).

## Sources

- `docs/spec/minos-api.yaml` (pinned): `UnitClassSummary` (the breadcrumb), `movement_domain`,
  `UnitSummary.image_url`, and the "movement_domains answers how a thing MOVES" warning.
- `~/Work/Crossnet/tfg/src/symbology/` — `mod.rs` (composition model), `frame.rs`
  (`frame_icon_radius`, `canonical_extent`, `STROKE_RATIO`), `icons.rs` (`BOX_PX = 32`), `generate.rs`
  (the invariants), and `assets/symbology/FINDINGS.md` (the port's measured findings).
- `spatialillusions/milsymbol` — MIT, © 2017 Mans Beckman, pinned by TFG at `f5134380…`, v3.0.5.
- Our own: `src/ui/tokens.rs` (`MARKER = 12`, `MARKER_TARGET = 24`, `LABEL = 12 pt`),
  `src/ui/markers.rs` (`INV-01`, `INV-02`, `INV-03` in the drawing rules), `docs/invariants.md`.
