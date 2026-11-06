## What and why

<!-- One paragraph. Link the ticket: "Closes #N". -->

## Proof

<!-- The command from the ticket's `## Proof` line, and its last lines of output.
     A claim is not proof. Paste the tail. -->

```
$ scripts/verify.sh
```

## Invariants touched

<!-- INV-nn … — the ones this change could break, and why it does not. -->

## Tests, fixtures or goldens changed?

<!-- "No" is the usual answer. If yes: which, and why, in one sentence each.
     Goldens only change by a human running --update-goldens. -->

## Checklist

- [ ] `scripts/verify.sh` passed locally, and its tail is pasted above
- [ ] No test was weakened to make this pass (or the reason is stated above)
- [ ] No secret, token, password or host in the diff
- [ ] New dependency? `THIRD-PARTY-NOTICES.md` updated
- [ ] Wire-shaped data came from `docs/spec/` or a recorded fixture, not from our own types
