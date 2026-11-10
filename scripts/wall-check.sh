#!/usr/bin/env bash
# The wall, checked against the **wire** — a stub that serves the recorded fixtures — for a short while.
#
#   scripts/wall-check.sh 60        # a minute: two socket sessions, a fix every two seconds
#   scripts/wall-check.sh 1800      # the same checks over v0.1's thirty-minute soak
#
# This is the check v0.1's definition of done asks for and the gate cannot run: the gate proves a *frame*
# from a fixture, this proves a *wall* — signing in, reading the picture, subscribing, applying fixes,
# dropping the socket on purpose and coming back. It needs a display and a GPU, because the map is the thing
# under test, and **no network**: `scripts/fake-minos.py` serves the recorded wire shapes.
#
# What it checks, each one a promise in the epic (#1):
#
#   1. the map draws from the seed      — "cold start with no network: the map draws from the seed"
#   2. sign-in, picture read, subscribe — the initial picture over HTTPS, then movement over WSS
#   3. a fix moves its marker < 1 s    — from the `fix_ms=` the wall logs (its own measurement)
#   4. one deliberate gap, survived     — two sessions and two picture reads, the socket re-read (INV-09)
#   5. no secret in the log             — one token in a log line is a token forever (INV-08)
#   6. a clean stop                     — SIGTERM, exit 0, in seconds rather than the ten it gets
#
# Exits non-zero with the reason. Not part of `scripts/verify.sh`: the gate must run without a display.
set -uo pipefail
cd "$(dirname "$0")/.."

SECONDS_TO_RUN="${1:-60}"
LOG="${WALL_CHECK_LOG:-out/wall-check.log}"
STUB_LOG="${WALL_CHECK_STUB_LOG:-out/wall-check.stub.log}"
DROP_AFTER_S="${WALL_CHECK_DROP_AFTER_S:-$(( SECONDS_TO_RUN / 2 ))}"
FIX_EVERY_S="${WALL_CHECK_FIX_EVERY:-2}"
FIX_PROMISE_MS="${WALL_CHECK_FIX_MS:-1000}"
# **The wall build, unless told otherwise** (#93). This check is about the binary a wall runs, and a wall runs
# `scripts/build-wall.sh`'s release build — taken from upstream's precompiled map core. A plain
# `cargo build --release --features map` now refuses to build for the reason in `build.rs`; a debug binary
# renders but is 1.5× the frame cost, so passing one here is a deliberate choice, named on the command line.
BINARY="${WALL_CHECK_BINARY:-./target/release/arcons-cc}"

mkdir -p out
: > "$LOG"
: > "$STUB_LOG"

fail() { printf '\033[31mWALL CHECK FAILED\033[0m: %s\n' "$1" >&2; exit 1; }
say() { printf '  %s\n' "$1"; }

command -v python3 >/dev/null 2>&1 || fail "python3 is needed for the stub (it is a test tool, not a dependency)"
if [ -z "${WALL_CHECK_BINARY:-}" ]; then
  scripts/build-wall.sh -q || fail "the wall build failed (see build.rs for why it must be this script)"
fi
[ -x "$BINARY" ] || fail "$BINARY does not exist — scripts/build-wall.sh first"
# The map is the thing under test, so a build without it fails the first check rather than passing silently.
"$BINARY" --version >/dev/null 2>&1 || fail "$BINARY does not run"

printf '\033[1m▶ wall check %ss\033[0m — stub fixtures, deliberate drop at %ss, log %s\n' \
  "$SECONDS_TO_RUN" "$DROP_AFTER_S" "$LOG"

python3 scripts/fake-minos.py --fix-every "$FIX_EVERY_S" ${WALL_CHECK_TRACE:+--trace} >"$STUB_LOG" 2>&1 &
STUB=$!
cleanup() {
  [ -n "${WALL:-}" ] && kill -TERM "$WALL" 2>/dev/null && wait "$WALL" 2>/dev/null
  kill -TERM "$STUB" 2>/dev/null
  wait "$STUB" 2>/dev/null
}
trap cleanup EXIT

for _ in $(seq 1 50); do
  PORT=$(grep -oE 'PORT=[0-9]+' "$STUB_LOG" 2>/dev/null | head -1 | cut -d= -f2)
  [ -n "${PORT:-}" ] && break
  sleep 0.1
done
[ -n "${PORT:-}" ] || fail "the stub never said which port it was given — see $STUB_LOG"
say "stub: http://127.0.0.1:$PORT/api/v1, a fix every ${FIX_EVERY_S}s"

MINOS_BASE_URL="http://127.0.0.1:$PORT/api/v1" \
MINOS_SOCKET_URL="ws://127.0.0.1:$PORT/connection/websocket?cf_protocol=json" \
  "$BINARY" --no-decorations --log-file "$LOG" --reconnect-after "$DROP_AFTER_S" >/dev/null 2>&1 &
WALL=$!

sleep 3
STARTED=$SECONDS
while [ $(( SECONDS - STARTED )) -lt "$SECONDS_TO_RUN" ]; do
  kill -0 "$WALL" 2>/dev/null || { tail -5 "$LOG" >&2; fail "the wall exited on its own after $(( SECONDS - STARTED ))s"; }
  sleep 5
done
kill -TERM "$WALL" 2>/dev/null
STOPPED=0
for _ in $(seq 1 10); do
  sleep 1
  kill -0 "$WALL" 2>/dev/null || { STOPPED=1; break; }
done
[ "$STOPPED" -eq 1 ] || { kill -KILL "$WALL" 2>/dev/null; fail "SIGTERM was ignored for 10 s"; }
wait "$WALL"; STATUS=$?
WALL=""
[ "$STATUS" -eq 0 ] || fail "the wall exited $STATUS on SIGTERM, not 0"
say "stop: SIGTERM, exit 0"

# The log is coloured; every check below reads a plain copy.
PLAIN=out/wall-check.plain.log
sed -r 's/\x1b\[[0-9;]*m//g' "$LOG" > "$PLAIN"

grep -q 'first map frame' "$PLAIN" ||
  fail "the map never drew: no 'first map frame' in $LOG (a cold start with no seed?)"
say "map: $(grep -m1 -o 'first map frame in [0-9]* ms' "$PLAIN") from the tile seed"

SESSIONS=$(grep -c 'socket: connecting' "$PLAIN")
[ "$SESSIONS" -ge 2 ] || fail "the socket connected $SESSIONS time(s): the deliberate drop did not come back"
READS=$(grep -c 'picture read after connect' "$PLAIN")
[ "$READS" -ge 2 ] || fail "$READS picture read(s) for $SESSIONS sessions: a reconnect must re-read (INV-09)"
grep -q 'deliberate drop' "$PLAIN" || fail "--reconnect-after never fired: the drop is not wired"
say "socket: $SESSIONS sessions, $READS picture reads, the deliberate drop seen"

FIXES=$(grep -oE 'fixes=[0-9]+' "$PLAIN" | cut -d= -f2 | sort -n | tail -1)
[ -n "${FIXES:-}" ] || fail "no 'fixes=' line in the log: the wall never reported its picture"
[ "$FIXES" -gt 0 ] || fail "the wall applied no fix in ${SECONDS_TO_RUN}s: movement over WSS is dead (#28)"
say "fixes: $FIXES applied"

mapfile -t LAT < <(grep -o 'fix_ms=[0-9-]*' "$PLAIN" | cut -d= -f2 | grep -v '^-1$')
if [ "${#LAT[@]}" -eq 0 ]; then
  fail "no fix latency in the log: the promise in the DoD cannot be decided"
fi
WORST=$(printf '%s\n' "${LAT[@]}" | sort -n | tail -1)
[ "$WORST" -le "$FIX_PROMISE_MS" ] ||
  fail "a fix waited ${WORST} ms for a frame, over the ${FIX_PROMISE_MS} ms promise (#1)"
say "latency: worst ${WORST} ms over ${#LAT[@]} samples (promise ${FIX_PROMISE_MS})"

grep -qE 'eyJ[A-Za-z0-9_-]{10,}' "$PLAIN" && fail "a JWT-shaped string appears in $LOG (INV-08)"
say "secrets: no JWT-shaped string in $(wc -l < "$PLAIN") lines"

printf '\033[32mWALL CHECK PASSED\033[0m: %ss, %s fixes, worst fix %s ms\n' \
  "$SECONDS_TO_RUN" "$FIXES" "$WORST"
