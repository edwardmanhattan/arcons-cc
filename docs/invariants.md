# Invariants

Numbered claims this display makes about itself. Every one is enforced by a test that **names it in the
failure message**. A red build should tell you which promise broke.

Adding an invariant: take the next free number, add a row, write the test, cite it. **Never renumber** —
tickets and commits quote these. Retiring one keeps its number and says `retired` with the reason.

| id | claim | owner | proof |
| --- | --- | --- | --- |
| **INV-01** | An absent or `null` optional feed field is never rendered as a number. | #3, #6, #7 | types are `Option`; fixture with all three absent → view model `None`, tree has no digits |
| **INV-02** | Nothing is projected without a fix; no marker is drawn at (0,0) unless a fix says so. | #6 | property test over the picture + fixture with a never-reported vessel |
| **INV-03** | A `backfilled` fix never sets the "here now" glyph. | #6 | table test over both flag values |
| **INV-04** | A marker's pixel agrees with the map's projection of the same lat/lon under the rendered camera, within 1 px. | #5, #6 | property test over random lat/lon and camera |
| **INV-05** | Display interpolation never creates or mutates a Fix. | #6 | test asserts track length and fix identity across frames |
| **INV-06** | Displayed age never decreases except when a new fix arrives. | #3, #6, #7 | property test over event sequences |
| **INV-07** | Every refusal code maps to exactly one action, and there is no catch-all. | #4 | exhaustive test over the code enum |
| **INV-08** | No log line, error message or panic contains a token, password or refresh token. | #3, #8 | property test over formatted events on failure paths |
| **INV-09** | After a reconnect, no marker is trusted until a picture read completes. | #4 | model-based actor test over disconnect sequences |
| **INV-10** | One card, one selection; the card stays inside the window. | #7 | headless scene + assertion |
| **INV-11** | Same fixtures + same scene + same virtual clock ⇒ byte-identical PNG. | #9 | render twice, compare bytes |
| **INV-12** | Camera pan/zoom stays within the frame-CPU budget at 1080p with 200 units. | #6, #9 | budget test, fails with the measured number |

## The detail behind each

### INV-01 — absence is not zero

`speed_kn`, `course_deg` and `accuracy_m` are omitted when unmeasured, and the API also encodes them as
`nullable`. A `0` in the payload is a *measured zero* and must render as one. There is no default, no
`unwrap_or(0.0)`, no `"—"` masking a value that exists.

```
units::absent_speed_is_not_zero
  INV-01: speed_kn absent must render no value, got "0.0"
```

### INV-02 — no phantom position

Half the fleet may never have reported. Those vessels belong in the picture and not on the map. Latitude 0,
longitude 0 is a real place in the Gulf of Guinea, so a missing fix drawn at the origin is a wrong fix,
not an empty one.

### INV-03 — backfilled is history

A batch flushed after a dead zone is real and may be days old. Drawing it as "here now" teleports a vessel
across the map — the single most likely way to get this wrong.

### INV-04 — markers and tiles agree

Both sides must call the same `project_mercator` with the camera the map last rendered. Parity is a
property of construction; the test exists to notice when someone re-derives it "just here".

### INV-05 — interpolation is display only

Motion between two fixes is a drawing decision. It never appends to a track, never re-stamps
`received_at`, and never becomes the input to a later calculation.

### INV-06 — age only goes up

Age comes from the server at read time and from `received_at` plus monotonic local elapsed time between
reads. A clock that can jump is not allowed to make a fix younger.

### INV-07 — refusals are total

`101`/`103`/`107` stop · `109` refresh then retry · `100` retry with backoff · `105` ignore. Every code
has an arm; a new code is a compile-visible decision, never a silent default.

### INV-08 — secrets stay out of logs

Tokens are redacted by a hand-written `Debug`, not by string filtering. The test formats every error path
and asserts the token's bytes appear nowhere.

### INV-09 — reconnect re-reads first

There is no replay on `live-feed`. After a reconnect the marker set is untrusted until a completed read
arrives, so a stale picture is never presented as current.

### INV-10 — one card

Selecting a unit replaces the selection. The card follows its vessel, flips rather than clipping, and
never leaves the window.

### INV-11 — determinism

Fixtures in, virtual clock, same scene, same bytes. Without this, goldens are noise and CI is a slot machine.

### INV-12 — budgets

The number lives in one place (`ui/tokens.rs` or `tests/budget.rs`), and the failure prints the measured
value next to the limit.

The frame-CPU number is the **reference machine's** (this laptop), and the test says so by measuring a fixed
reference workload in the same run and dividing: 8 ms is the wall's budget, and how slow the machine running
the check is cancels out. `#48` is why — CI ran 2.3× slower than the wall and the same commit landed on both
sides of the line.
