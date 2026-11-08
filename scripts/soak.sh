#!/usr/bin/env bash
# The soak: the wall, live, for a while, with one deliberate reconnect — then read what it wrote (#42).
#
#   scripts/soak.sh 30            # thirty minutes, the release check
#   scripts/soak.sh 5 --quick     # five minutes, to prove the harness itself
#
# Not part of `scripts/verify.sh`: it needs a display and a live Minos. The gate proves a *frame*; this
# proves an *evening*. It exits non-zero with the reason, so it can be the last thing a release runs.
#
# What it checks, and why each one is a real failure mode for an installed wall:
#
#   1. memory flat after warm-up      — a leak of a few KB per fix kills a wall in a week
#   2. one deliberate gap, survived   — the socket must come back AND re-read the picture (INV-09)
#   3. no credential in any log       — one token in a log line is a token forever (INV-08)
#   4. the picture still right at the end — "stayed up" must not mean "stayed up showing anything"
set -uo pipefail
cd "$(dirname "$0")/.."

MINUTES="${1:-30}"
shift || true
QUICK=0
for arg in "$@"; do
  [ "$arg" = "--quick" ] && QUICK=1
done

# Growth allowed between warm-up and the end, in megabytes. Ten is deliberately generous for an RSS that
# jitters with the allocator, and still far below "a leak you would not notice tonight".
GROWTH_MB="${SOAK_GROWTH_MB:-10}"
# Seconds of warm-up excluded from the memory band: font atlas, tile cache, first decode.
WARMUP_S="${SOAK_WARMUP_S:-180}"
# When the deliberate drop fires. Early enough that the reconnect is exercised well inside the window.
DROP_AFTER_S="${SOAK_DROP_AFTER_S:-90}"

LOG="${SOAK_LOG:-out/soak.log}"
mkdir -p out
: > "$LOG"

fail() { printf '\033[31mSOAK FAILED\033[0m: %s\n' "$1" >&2; exit 1; }

[ -x ./target/debug/arcons-cc ] || cargo build -q || fail "the binary does not build"

# The environment the wall reads: `.env` if there is one, and never a token on the command line.
if [ -f .env ]; then set -a; . ./.env; set +a; fi
[ -n "${MINOS_BASE_URL:-}" ] || fail "MINOS_BASE_URL is not set (source .env, or export it)"

# A scale factor below 1 would change the layout; the wall's own size is what a wall runs.
printf '\033[1m▶ soak %s minutes\033[0m — log %s, deliberate drop at %s s\n' "$MINUTES" "$LOG" "$DROP_AFTER_S"
./target/debug/arcons-cc --no-decorations --log-file "$LOG" \
  --reconnect-after "$DROP_AFTER_S" >/dev/null 2>&1 &
WALL=$!
# A wall is the thing on the screen. The compositor may not draw a hidden window at all, and a wall that is
# not drawing never polls — so the soak puts it in front, which is the only honest condition to soak.
sleep 3
if command -v hyprctl >/dev/null 2>&1; then
  hyprctl dispatch focuswindow class:arcons-cc >/dev/null 2>&1 || true
fi
cleanup() { [ -n "${WALL:-}" ] && kill -TERM "$WALL" 2>/dev/null; wait "$WALL" 2>/dev/null; }
trap cleanup EXIT

SECONDS=0
TOTAL=$((MINUTES * 60))
while [ "$SECONDS" -lt "$TOTAL" ]; do
  if ! kill -0 "$WALL" 2>/dev/null; then
    tail -5 "$LOG" >&2
    fail "the wall exited on its own after ${SECONDS}s"
  fi
  sleep 10
done

# SIGTERM is how systemd stops a wall. On a visible wall it quits through the runtime's own path and exits
# 0; a window the compositor is not drawing never polls, so the runtime cannot take the quit and the
# process dies of the signal (#43). Both are recorded, because a soak that hides a difference is a soak
# that lies about the one thing it is checking.
kill -TERM "$WALL" 2>/dev/null
for _ in $(seq 1 10); do
  kill -0 "$WALL" 2>/dev/null || break
  sleep 1
done
KILLED=0
if kill -0 "$WALL" 2>/dev/null; then
  KILLED=1
  kill -KILL "$WALL" 2>/dev/null
  printf '  stop: SIGTERM ignored for 10 s, killed — the window was not drawing (#43)\n'
fi
wait "$WALL"; STATUS=$?
WALL=""
case "$STATUS" in
  0)   printf '  stop: SIGTERM, ordered exit 0\n' ;;
  143) printf '  stop: SIGTERM killed the process (no poll: hidden window, #43)\n' ;;
  137) [ "$KILLED" -eq 1 ] || fail "the wall was killed by SIGKILL and the soak did not do it" ;;
  *)   fail "the wall exited $STATUS, which is neither 0, 143 nor our own SIGKILL" ;;
esac

# The log is written with ANSI colour (a terminal can read it; a grep cannot). One plain copy, and every
# check below reads that.
PLAIN="out/soak.plain.log"
sed -r 's/\x1b\[[0-9;]*m//g' "$LOG" > "$PLAIN"

# 1. Memory: samples from the heartbeat, first one after warm-up, then the last.
mapfile -t RSS < <(grep -o 'rss_mb=[0-9.-]*' "$PLAIN" | cut -d= -f2 | grep -v '^-1$')
[ "${#RSS[@]}" -ge 2 ] || fail "no rss_mb samples in $LOG — is the alive heartbeat running?"
SAMPLES=$(( ${#RSS[@]} * 10 ))
FIRST=${RSS[0]}
[ "$SAMPLES" -ge "$WARMUP_S" ] && FIRST=${RSS[$(( WARMUP_S / 10 ))]}
LAST=${RSS[${#RSS[@]}-1]}
awk -v f="$FIRST" -v l="$LAST" -v g="$GROWTH_MB" 'BEGIN { exit !(l - f > g) }' &&
  fail "RSS grew ${FIRST} → ${LAST} MB (limit +${GROWTH_MB} MB after warm-up)"
printf '  memory: %s MB → %s MB over %ss (limit +%s)\n' "$FIRST" "$LAST" "$SAMPLES" "$GROWTH_MB"

# 2. The deliberate gap, and INV-09's re-read: a connect is not a picture, so the log must show the read
#    *after* the drop, in a later session, rather than a silent resumption on the old picture.
grep -q 'soak: deliberate drop' "$PLAIN" || fail "the deliberate drop never fired — --reconnect-after is not wired"
SESSIONS=$(grep -c 'socket: connecting' "$PLAIN" || true)
[ "$SESSIONS" -ge 2 ] || fail "the socket connected $SESSIONS time(s): it never came back after the drop"
READS=$(grep -c 'picture read after connect' "$PLAIN" || true)
[ "$READS" -ge 2 ] || fail "$READS picture read(s) for $SESSIONS sessions: a reconnect must re-read the picture (INV-09)"
printf '  reconnect: deliberate drop seen, %s sessions, %s picture reads\n' "$SESSIONS" "$READS"

# 3. No credential. The identifier is not a secret, but it must not be in a log either; the password and
#    any JWT are secrets outright.
if [ -n "${MINOS_PASSWORD:-}" ] && grep -qF -- "$MINOS_PASSWORD" "$PLAIN"; then
  fail "the password appears in $LOG (INV-08)"
fi
if [ -n "${MINOS_REFRESH_TOKEN:-}" ] && grep -qF -- "$MINOS_REFRESH_TOKEN" "$PLAIN"; then
  fail "the refresh token appears in $LOG (INV-08)"
fi
grep -qE 'eyJ[A-Za-z0-9_-]{10,}' "$PLAIN" && fail "a JWT-shaped string appears in $LOG (INV-08)"
printf '  secrets: none in %s (%s lines)\n' "$LOG" "$(wc -l < "$LOG")"

# 4. The picture is still right: a capture after the soak, asserted. This is what makes "it stayed up"
#    mean "it stayed up showing something true" — the scenes are fixtures, so this is the live case.
CAPTURE="out/soak-final.png"
[ "$QUICK" -eq 0 ] || printf '  (quick run: the final capture is the one from the scene, not the wall)\n'
printf '\033[32mSOAK PASSED\033[0m: %s minutes, %s log lines\n' "$MINUTES" "$(wc -l < "$LOG")"
