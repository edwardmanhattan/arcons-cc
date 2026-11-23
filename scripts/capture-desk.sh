#!/usr/bin/env bash
# Render one desk scene headlessly: out/desk-<scene>.png.
#
#   scripts/capture-desk.sh time-field        # render, prove two runs match, and compare the golden
#   scripts/capture-desk.sh --update-goldens  # every scene, and rewrite fixtures/golden/desk/
#
# The desk's scenes are its own: `scripts/capture.sh` drives the wall's binary, and a scene list
# written in this file would rot the way the wall's did (#54). It comes from `desk --scenes`.
#
# Three things differ from the wall's harness, each for a reason:
#
#   * **`--scale 1`**, so a desk golden is 1440×900 on every machine rather than 2× on this laptop.
#   * **The scenes are the desk's own, and so are their bounds.** `desk --compare` calls the same
#     `arcons_cc::verify::compare` the wall does — the counting and the printed numbers have one
#     implementation — but the bounds come from `desk::scene::bounds`, which today hands every desk
#     scene the defaults because no run has measured one yet. A byte compare is not the test: two
#     rasterisers round glyph edges differently, and it would be red for reasons nothing in this
#     repository changed.
#   * **A missing golden is red, with the command that writes one.** `--update-goldens` rewrites it;
#     the wall's equivalent is reviewed by a human before it lands, and so is this.
#
# No display and no GPU beyond a software Vulkan driver: Rune's headless renderer is the CPU.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo build -q -p desk || exit 1
# The binary's path is cargo's answer: this machine's `[build] target-dir` is a shared store off
# `./target` (docs/build-environment.md), so `./target/debug/desk` is a path that stops existing.
DESK="$(scripts/bin-path.sh desk desk)" || exit 1
mapfile -t SCENES_ALL < <("$DESK" --scenes)
[ ${#SCENES_ALL[@]} -gt 0 ] || { echo "capture-desk: the desk listed no scenes" >&2; exit 1; }

UPDATE=0
SCENES=()

for arg in "$@"; do
  case "$arg" in
    --update-goldens) UPDATE=1 ;;
    -h|--help) sed -n '2,22p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
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
  if ! "$DESK" \
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
  "$DESK" --scene "$scene" --at 2 --fps 60 --size 1440x900 --scale 1 \
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
    continue
  fi

  # The golden is compared **after** the determinism check, because a golden compared against a
  # render that is not itself stable is noise. The counting, the three bounds and the numbers printed
  # on every run are `arcons_cc::verify::compare`'s — the same function the wall's `--compare` calls —
  # so "the same picture" cannot mean two things in this repository (#276).
  golden="fixtures/golden/desk/$scene.png"
  compare_args=(--compare "$golden" --image "$png")
  # The wall's two human overrides, spelled the same way: `ARCONS_GOLDENS=exact` demands bytes for the
  # reference machine, and the three numbers can be widened or tightened for one run only.
  if [ "${ARCONS_GOLDENS:-on}" = "exact" ]; then
    compare_args+=(--tolerance 0 --budget 0 --rounding 0)
  else
    [ -n "${ARCONS_TOLERANCE:-}" ] && compare_args+=(--tolerance "$ARCONS_TOLERANCE")
    [ -n "${ARCONS_BUDGET:-}" ] && compare_args+=(--budget "$ARCONS_BUDGET")
    [ -n "${ARCONS_ROUNDING:-}" ] && compare_args+=(--rounding "$ARCONS_ROUNDING")
  fi
  if [ ! -f "$golden" ]; then
    echo "  no golden at $golden: run scripts/capture-desk.sh --update-goldens $scene" >&2
    status=1
  elif "$DESK" "${compare_args[@]}"; then
    :
  else
    status=1
  fi
done

exit $status
