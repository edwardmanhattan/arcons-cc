# ADR-0004 — One interaction value, and the table it obeys

- Status: accepted
- Date: 2026-11-10
- Ticket: [#101](https://github.com/edwardmanhattan/arcons-cc/issues/101)

## The problem, in the operator's words

> I think you need to find every implicit state machine, and make them explicit state machines.

They said it because the two reports before it were the same defect in different costumes:

- **#98** — the settings sheet had no legal way out. `settings_open: bool` was set in two places, cleared
  in three, and the modal dismissal that was supposed to clear it could never fire.
- **#100** — a dragged pane landed beside the one it was dropped on, whichever edge it was dropped against.
  The layout was fine; the *drop* was computed from one number too few.

Neither was a missing `if`. Both were states the code allowed and the design did not: a sheet whose dismissal
is unreachable, a gesture whose result is not the gesture's meaning.

## What the wall was holding, before

| field | shape | what it meant | combinations it allowed |
| --- | --- | --- | --- |
| `settings_open` | `bool` | a sheet over the wall | × open while renaming × open while a pane is lifted |
| `rename` | `Option<Rename>` | a text sheet with a pane | × two renames × a rename of a closed pane |
| `maximised` | `Option<ViewId>` | one pane over the tiling | × maximised while dragging × maximised while renaming elsewhere |
| `dragging` | `Option<PaneDrag>` | a pane in the air with a preview drop | × a lift while resizing × a lift while a menu is open |
| `resizing` | `Option<Border>` | a border being held | × resizing and lifting at once |
| `menu` | `Option<(ViewId, (f32, f32))>` | one pane's menu, one place | × two menus × a menu for a pane that is gone |
| `selected` + `card` | two fields | this hull's card is showing | × a card with no selection × a selection with no card |

Seven fields, `2^7` bit patterns, of which perhaps fifteen were meant. Every new feature added a fourth
boolean and a fifth `is_none()` in a row of them. The socket actor and `FixLatency` were already written the
other way — an enum with named variants and a handful of transitions, each with a test — which is what made
the difference visible.

## The decision

**One value for the whole wall**, `State::interaction: Interaction`:

```rust
pub enum Interaction {
    Idle,                                   // nothing is held
    Lifted(PaneDrag),                       // a pane is in the air (#70, #84)
    Resizing(view::Border),                 // a border is held (#69)
    Menu { pane: ViewId, at: (f32, f32) },  // a pane's menu (#77, #81, #83)
    Renaming(Rename),                       // a name sheet (#83)
    Settings,                               // the settings sheet (#82, #98)
}
```

with one **request** vocabulary, `Begin`, and one place that decides:

```rust
impl Begin {
    pub fn allowed_from(&self, now: &Interaction) -> bool { … }
}
impl State {
    pub fn begin(&mut self, begin: Begin) -> bool;   // enters, or refuses and changes nothing
    pub fn end(&mut self) -> bool;                   // the single way back to Idle
}
```

The per-pane questions — *this* pane's selected hull, *this* pane's card, *this* pane's filter — are not
here: they live on the pane (`PaneView`, #99), because a wall has one interaction and many views.

## The table

`+` is taken, `·` is refused. Read a row as "the wall is in this state"; a column as "the operator asks for
this".

| from \ begin | Lift | Resize | Menu | Rename | Settings |
| --- | --- | --- | --- | --- | --- |
| **Idle** | + | + | + | + | + |
| **Lifted** | + (re-aim) | · | · | · | · |
| **Resizing** | · | + (re-grip) | · | · | · |
| **Menu** | + | + | + (move) | + | + |
| **Renaming** | · | · | · | + | · |
| **Settings** | · | · | · | · | + |

The rules behind the cells:

- **One hold at a time.** A lift and a border both hold the pointer, so neither can start over the other.
  Rune hands a drag to one element anyway; this says so where the state is, not where the pointer is.
- **A menu holds nothing**, so a press elsewhere closes it and whatever the press means then happens: a
  lift, a border, a rename seeded from the row the operator pressed, the settings sheet from the header.
- **A sheet holds the keyboard** (renaming) or the pointer (settings), and is modal, so nothing starts behind
  it. A sheet cannot open behind another sheet.
- **Re-entering is a move, not a second copy.** A menu re-asked on another pane moves; a lift re-aimed
  updates its drop; the settings sheet asked for twice is still one sheet.

**What is deliberately *not* in this machine: `Maximised`.** A pane drawn full size over the tiling is a
**layout** fact — `Layout::maximised` — and not a thing the wall is holding:

- it survives every one of these states coming and going (a menu closes over it, a rename happens inside it,
  Escape from a card leaves it up);
- it has its own way back (`Back to the wall`, the `max-close` plate, and Escape after the holds are released);
- the tiling behind it is untouched, so "maximised while renaming" is two independent facts rather than a
  state pair to enumerate — which halves the table.

## What this buys

1. **"A menu and a rename at once" is unrepresentable.** Not refused by a test; not expressible by the type.
2. **Refusals change nothing.** `begin` returns whether it happened, and a refusal leaves the value exactly
   as it was — the half a boolean-returning API always forgets (see `tests/transitions.rs`).
3. **Escape is one match, not five `if`s.** The old handler's *order* was the policy, readable only by reading
   it; now the order is the enum's and the code says so.
4. **A new state costs a row.** `tests/transitions.rs` enumerates both enums exhaustively, so adding a
   variant fails the test until it is listed — and `tests/doc_claims.rs` fails until this document names it.

## The cost

The states carry their payloads, so reading one is a `match` rather than a field access. That is the point —
`state.menu()` and `state.dragging()` are the two accessors that exist for callers that only want to ask, and
the rest of the codebase asks the machine rather than the fields.
