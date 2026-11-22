#!/usr/bin/env bash
# Render one desk scene headlessly: out/desk-<scene>.png.
#
#   scripts/capture-desk.sh time-field        # render, and prove two runs are the same bytes
#   scripts/capture-desk.sh --update-goldens  # every scene, and rewrite fixtures/golden/desk/
#
# The desk's scenes are its own: `scripts/capture.sh` drives the wall's binary, and a scene list
# written in this file would rot the way the wall's did (#54). It comes from `desk --scenes`.
#
# Three things differ from the wall's harness, each for a reason:
#
#   * **`--scale 1`**, so a desk golden is 1440×900 on every machine rather than 2× on this laptop.
#   * **No pixel comparison in the gate yet.** The wall compares goldens within `verify::golden_bounds`,
#     a measurement keyed by the wall's scenes; the desk's scenes need their own measurement, and a byte
#     compare between two rasterisers is red for reasons nothing in this repository changed. Until that
#     measurement exists the gate proves the scene **renders and is deterministic** — and the golden is
#     recorded here, for a human and for the ticket that adds the bounds.
#   * **A missing golden is not a red gate.** `--update-goldens` writes one; the wall's equivalent is
#     reviewed by a human before it lands, and so is this.
#
# No display and no GPU beyond a software Vulkan driver: Rune's headless renderer is the CPU.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo build -q -p desk || exit 1
mapfile -t SCENES_ALL < <(./target/debug/desk --scenes)
[ ${#SCENES_ALL[@]} -gt 0 ] || { echo "capture-desk: the desk listed no scenes" >&2; exit 1; }

UPDATE=0
SCENES=()

for arg in "$@"; do
  case "$arg" in
    --update-goldens) UPDATE=1 ;;
    -h|--help) sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "capture-desk: unknown flag $arg" >&2; exit 64 ;;
    *) SCENES+=("$arg") ;;
  esac
done
[ ${#SCENES[@]} -eq 0 ] && SCENES=("${SCENES_ALL[@]}")

mkdir -p out fixtures/golden/desk

status=0
for scene in "${SCENES[@]}"; do
  printf '\n\033[1m▶ desk capture %s\033[0m\n' "$scene"
  png="out/desk-$scene.png"
  if ! ./target/debug/desk \
      --scene "$scene" \
      --at 2 \
      --fps 60 \
      --size 1440x900 \
      --scale 1 \
      --snap "$png" > "out/desk-$scene.log" 2>&1; then
    echo "  FAILED to render $scene — see out/desk-$scene.log" >&2
    tail -3 "out/desk-$scene.log" >&2
    status=1
    continue
  fi
  printf '  rendered %s (%s bytes)\n' "$png" "$(stat -c %s "$png" 2>/dev/null || echo '?')"

  # Determinism: the same scene twice must be the same bytes, or no golden means anything.
  cp "$png" "out/desk-$scene.first.png"
  ./target/debug/desk --scene "$scene" --at 2 --fps 60 --size 1440x900 --scale 1 \
    --snap "$png" > "out/desk-$scene.log" 2>&1
  if cmp -s "out/desk-$scene.first.png" "$png"; then
    echo "  two runs identical"
  else
    printf '  \033[31mTWO RUNS DIFFER\033[0m — nondeterminism in the render, which no golden can fix\n' >&2
    status=1
    continue
  fi

  if [ "$UPDATE" -eq 1 ]; then
    cp "$png" "fixtures/golden/desk/$scene.png"
    printf '  \033[33mupdated\033[0m fixtures/golden/desk/%s.png — this is the line a human asked for\n' "$scene"
  fi
done

exit $status
