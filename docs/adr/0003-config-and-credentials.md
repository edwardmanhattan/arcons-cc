# ADR-0003: configuration, credentials, and the codes that say what is wrong

Status: accepted (#8)

## The problem

A wall display is not a program someone starts when they want to look at it. It starts with the
machine, runs unattended for weeks, and is diagnosed by whoever is on shift — often by telephone,
often from a screenshot of a terminal. Two things follow:

- **Everything the process needs must be on disk**, in a format the operator can read and a script can
  check, and never in the source tree.
- **Failure must be a short number.** "It says something about a token" is not a report an operator
  can act on; `exit 2` is.

## The decision

### Configuration: flags → environment → file

`$XDG_CONFIG_HOME/arcons-cc/config.toml`, with `--config` naming another file. The environment is the
developer's route (`MINOS_*`, the same names `.env.example` documents) and the file is the
deployment's. Flags exist where an operator needs to override one thing — `--config`, `--credentials`,
`--log-file` — not for every value: a wall's unit file should read as one command, and the rest lives
in the file.

**Unknown keys are refused.** `map_cetner = [...]` is an exit-1 error naming the key, not a default
that quietly draws the wrong ocean. The same for a `centrifugo_url` that is not `ws://`/`wss://` and a
`stale_after_s` that is zero.

`stale_after_s` is in the file because the threshold is a **deployment fact**: a feed that reports
every ten minutes and a feed that reports every ten seconds cannot share one number for "old". It
reaches the glyph through `UnitView::glyph_with`, and `units::STALE_AFTER` is only the default.

### Credentials: a file, mode `0600`, checked before it is read

`credentials.toml` holds exactly one mode: `refresh_token`, or `identifier` + `password`. Both at once
is refused — two answers where the wrong one wins silently.

The mode check runs **before** `read_to_string`, and the refusal names the fix:

```
…/credentials.toml is readable by group or others (mode 0644); it holds a secret: chmod 600 …
```

The refresh token is the deployment's path: no password at rest, and the server rotates it. A password
is exchanged at startup and never written back — the wall is a display, not an account manager.

A keyring backend is deliberately **not** in v0.1: it adds a session-dbus dependency to a process that
starts before a user session necessarily exists, and the failure mode (a locked keyring at boot) is a
wall that does not start. It remains a follow-up.

### Redaction is structural

There is no filter anywhere that greps a token out of a log line. `Secret` has a hand-written `Debug`
that prints `<redacted>`, no `Display`, and the only way to the value is `.expose()`, which reads like
what it is. `Minos`'s `Debug` prints the base URL (a hostname, not a secret) and the credential's
redacted form. The self-check prints a token's *lifetime* and a file's *mode*, never a value.

This is what `INV-08` asserts as a property: a canary credential in the file, in the environment and in
a session, run through every failure path the self-check owns, and asserted absent from the output.

### Six exit codes, the first failure wins

```
0 ok · 1 config unusable · 2 credentials refused · 3 login failed
4 REST read failed · 5 socket refused · 6 map seed invalid
```

`--self-check` runs **every** check and prints one line each; the exit code is the first failure in
that order. Stopping at the first problem hides the second one, and an operator then fixes one thing
per session. The ordering is the dependency ordering: config before credential, credential before
login, login before read, read before socket. A failed login prints `picture: skipped (no session)`
rather than a second failure caused by the first.

`data::Session` splits **sign-in** from **read** precisely so that 3 and 4 are different answers: "the
credential is refused" and "the credential works and the read fails" have different fixes, and one
function that does both cannot tell them apart.

### `SIGTERM` is a clean exit, not a kill

`libc::signal` sets an `AtomicBool` (async-signal-safe: no allocation, no lock) and the view loop calls
`Tx::quit` — Rune's own path, which drops the GL context and joins the map thread. A closed socket and
a joined map thread on exit are the difference between a journal that ends with `Stopped` and one that
ends with a driver warning.

### Logging: stderr always, a file when asked

`tracing` at `RUST_LOG` (default `info`), to stderr and, with `--log-file`, to a file created `0600`.
Both streams get the same lines: an operator watching a session and a log read afterwards must not
disagree. A log that cannot be written is not a crash — the wall keeps drawing and stderr still has the
line, because failing the picture over a full disk would be worse than a missing line.

## What this rules out

- **A credential in the environment of the unit file.** It is world-readable in `/proc` and it appears
  in `systemctl show`. The environment route stays for development, where that does not matter.
- **A default credential.** There is no "try these and see": a wall with no credential says so in the
  header and runs read-only rather than guessing.
- **A catch-all in the failure mapping.** Six codes, six outcomes, tested; a seventh is a compile-visible
  decision.
