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

# **Built every time, and without features.** The goldens are rendered from the default build; a binary
# left behind by `cargo run --features map` draws the map-feature placeholder in every scene and turns
# every capture into a false diff (found while adding `pane-menu`, #77). `cargo build` is a no-op when the
# binary is current, so this costs a check rather than a build.
cargo build -q || exit 1
# **The scene list comes from the binary**, not from a list written here: the gate renders what
# `arcons-cc --scenes` prints (#54), and a golden pass with its own list silently skips a new scene —
# which is what happened to `panes` the first time, in #68.
mapfile -t SCENES_ALL < <(./target/debug/arcons-cc --scenes)
[ ${#SCENES_ALL[@]} -gt 0 ] || { echo "capture: the binary listed no scenes" >&2; exit 1; }

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
    # Both runs are kept, and named. A mismatch is the one failure that cannot be diagnosed from the
    # log alone, so the next thing anyone does is look at the two images — which needs them to exist
    # and to be told apart. CI uploads `out/` when the gate fails.
    printf '  \033[31mTWO RUNS DIFFER\033[0m — nondeterminism in the render, which no golden can fix\n' >&2
    printf '    kept %s (%s bytes, %s) and %s (%s bytes, %s)\n' \
      "out/$scene.first.png" "$(stat -c %s "out/$scene.first.png")" "$(sha256sum "out/$scene.first.png" | cut -c1-12)" \
      "out/$scene.png" "$(stat -c %s "out/$scene.png")" "$(sha256sum "out/$scene.png" | cut -c1-12)" >&2
    status=1
    continue
  fi

  # The golden is compared on **every** machine, within a stated tolerance: two rasterisers (this
  # laptop's iGPU and CI's lavapipe) were measured to differ by at most 1/255 on at most 23 pixels of
  # 921,600, so a byte compare can only ever be the reference machine's privilege — and CI skipping the
  # goldens is how a consistent change (a moved card, a shifted token colour) passes CI. The numbers
  # are printed on every run; a tolerance nobody can see the margin of is a tolerance that widens.
  golden="fixtures/golden/$scene.png"
  tolerance="${ARCONS_TOLERANCE:-2}"
  budget="${ARCONS_BUDGET:-200}"
  # `ARCONS_GOLDENS=exact` demands bytes: for the reference machine, when a human wants to know that the
  # pixels are the reviewed ones and not merely the same picture.
  if [ "${ARCONS_GOLDENS:-on}" = "exact" ]; then
    tolerance=0
    budget=0
  fi
  if [ ! -f "$golden" ]; then
    echo "  no golden at $golden: run scripts/capture.sh --update-goldens $scene" >&2
    status=1
  elif ./target/debug/arcons-cc --compare "$golden" --image "out/$scene.png" \
      --tolerance "$tolerance" --budget "$budget"; then
    :
  else
    status=1
  fi
done

exit $status
