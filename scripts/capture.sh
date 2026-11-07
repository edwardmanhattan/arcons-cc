#!/usr/bin/env bash
# Render one scene headlessly: out/<scene>.png and out/<scene>.tree.
#
#   scripts/capture.sh picture                # render, compare with the golden
#   scripts/capture.sh --update-goldens       # every scene, and rewrite fixtures/golden/
#   scripts/capture.sh --update-goldens stale # one scene
#
# The flags are spelled out **here and nowhere else**: CI and a human run this same command, so they
# cannot diverge. A scene is deterministic by construction (see `src/scene.rs`): a fixture feed, no
# network, and Rune's virtual clock, so two runs produce identical bytes (INV-11).
#
# No display and no GPU beyond a software Vulkan driver: Rune's headless renderer is the CPU.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SCENES_ALL=(empty picture stale card-open)
UPDATE=0
SCENES=()

for arg in "$@"; do
  case "$arg" in
    --update-goldens) UPDATE=1 ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "capture: unknown flag $arg" >&2; exit 64 ;;
    *) SCENES+=("$arg") ;;
  esac
done
[ ${#SCENES[@]} -eq 0 ] && SCENES=("${SCENES_ALL[@]}")

mkdir -p out fixtures/golden

# The binary the captures run. `cargo build -q` when it is absent, so a fresh checkout works and a
# stale binary is never what a golden was made from without someone noticing the build line.
[ -x ./target/debug/arcons-cc ] || cargo build -q || exit 1

status=0
for scene in "${SCENES[@]}"; do
  printf '\n\033[1m▶ capture %s\033[0m\n' "$scene"
  # `--at 2`: two seconds of the virtual clock, so a scene with a travel animation has settled and a
  # `--at 0` capture cannot accidentally be right for the wrong reason.
  if ! ./target/debug/arcons-cc \
      --scene "$scene" \
      --at 2 \
      --fps 60 \
      --size 1280x720 \
      --scale 1 \
      --snap "out/$scene.png" > "out/$scene.log" 2>&1; then
    echo "  FAILED to render $scene — see out/$scene.log" >&2
    tail -3 "out/$scene.log" >&2
    status=1
    continue
  fi
  printf '  rendered out/%s.png (%s bytes)\n' \
    "$scene" "$(stat -c %s "out/$scene.png" 2>/dev/null || echo '?')"

  if [ "$UPDATE" -eq 1 ]; then
    cp "out/$scene.png" "fixtures/golden/$scene.png"
    printf '  \033[33mupdated\033[0m fixtures/golden/%s.png — this is the line a human asked for\n' "$scene"
    continue
  fi

  # Determinism on ANY machine: a second render of the same scene must be the same bytes as the
  # first. This is the part of INV-11 that is true everywhere, and it is the part CI can hold.
  cp "out/$scene.png" "out/$scene.first.png"
  ./target/debug/arcons-cc --scene "$scene" --at 2 --fps 60 --size 1280x720 --scale 1 \
    --snap "out/$scene.png" > "out/$scene.log" 2>&1
  if cmp -s "out/$scene.first.png" "out/$scene.png"; then
    echo "  two runs identical (INV-11)"
  else
    echo "  [31mTWO RUNS DIFFER[0m — nondeterminism in the render, which no golden can fix" >&2
    status=1
    continue
  fi

  # The committed golden was made by one renderer. Another renderer (CI's software path) antialiases
  # a glyph edge differently, so comparing across machines would fail for a reason that is not about
  # the wall. `ARCONS_GOLDENS=off` keeps CI to determinism + assertions; the reference machine
  # compares bytes.
  if [ "${ARCONS_GOLDENS:-on}" = "off" ]; then
    echo "  golden comparison skipped (ARCONS_GOLDENS=off): assertions still run"
    continue
  fi
  golden="fixtures/golden/$scene.png"
  if [ ! -f "$golden" ]; then
    echo "  no golden at $golden: run scripts/capture.sh --update-goldens $scene" >&2
    status=1
  elif ! cmp -s "out/$scene.png" "$golden"; then
    # Bytes, not "looks similar": a capture that differs by one byte is a change nobody reviewed.
    echo "  \033[31mDIFFERS\033[0m from $golden — a human must look, then --update-goldens" >&2
    status=1
  else
    echo "  identical to $golden (INV-11)"
  fi
done

exit $status
