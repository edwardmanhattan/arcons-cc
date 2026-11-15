#!/usr/bin/env bash
# Replay a recorded control corpus through this wall's own session loop.
#
#   scripts/conformance.sh fixtures/control/session.jsonl
#
# One request per line goes to a wall built from `fixtures/intents/control-corpus.json`, and what it
# answers is printed beside what the file expects. The first line that differs is the failure and the exit
# code is non-zero, so a client in another language can run the same file and know it agrees with the wall.
#
# No display, no socket, no network: the carrier is an in-memory `Loopback` pair (the door the unix socket
# uses, #195), and the wall is the binary's own. `cargo run -- --record` writes the corpus; this reads it.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [ $# -ne 1 ] || [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
  [ $# -eq 1 ] && exit 0
  exit 64
fi

CORPUS="$1"
if [ ! -f "$CORPUS" ]; then
  echo "conformance: no corpus at $CORPUS" >&2
  exit 66
fi

# Built every time, like `capture.sh`: CI and a human run the same command, and a stale binary is a
# different wall. `cargo build` is a no-op when the binary is current.
cargo build -q || exit 1
./target/debug/arcons-cc --conformance "$CORPUS"
