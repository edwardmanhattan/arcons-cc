#!/usr/bin/env bash
# Ask pixel and structure questions of a capture. Non-zero exit when the answer is wrong.
#
#   scripts/assert.sh out/picture.png            # the scene is the file name
#
# The numbers live in `src/verify.rs`; this is the command line the gate and a human share. Colours
# come from `ui::tokens`, geometry from the same projection the markers are placed with, so an
# assertion cannot outlive the palette or the layout it describes.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

png="${1:-}"
if [ -z "$png" ]; then
  echo "assert: usage: scripts/assert.sh out/<scene>.png" >&2
  exit 64
fi
if [ ! -f "$png" ]; then
  echo "assert: no capture at $png — scripts/capture.sh first" >&2
  exit 66
fi
scene="$(basename "$png" .png)"
args=(--assert "$scene" --image "$png")

printf '\n\033[1m▶ assert %s\033[0m\n' "$scene"
./target/debug/arcons-cc "${args[@]}"
