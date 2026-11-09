#!/usr/bin/env bash
# The gate. A ticket is done when this exits 0.
#
#   scripts/verify.sh              everything: fmt, clippy, tests, captures, budgets
#   scripts/verify.sh --fast       no captures, no budgets (the inner loop)
#   scripts/verify.sh --scenes a,b only these capture scenes
#
# No network, no display, no GPU beyond a software Vulkan driver. If a check needs
# any of those, it does not belong here — it belongs in a ticket that says so.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

FAST=0
SCENES=""

while [ $# -gt 0 ]; do
  case "$1" in
    --fast) FAST=1; shift ;;
    --scenes) SCENES="${2:-}"; shift 2 ;;
    -h|--help) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "verify: unknown flag $1" >&2; exit 64 ;;
  esac
done

FAILED=()
RAN=()
declare -a NAMES STATUS MS
START_ALL=$(date +%s%N)

step() { # name, then the command
  local name="$1"; shift
  local start end ms
  printf '\n\033[1m▶ %s\033[0m\n' "$name"
  start=$(date +%s%N)
  if "$@"; then
    end=$(date +%s%N); ms=$(( (end - start) / 1000000 ))
    NAMES+=("$name"); STATUS+=("ok");   MS+=("$ms")
    printf '\033[32m  ok\033[0m (%s ms)\n' "$ms"
  else
    end=$(date +%s%N); ms=$(( (end - start) / 1000000 ))
    NAMES+=("$name"); STATUS+=("FAIL"); MS+=("$ms")
    printf '\033[31m  FAILED\033[0m (%s ms)\n' "$ms"
    FAILED+=("$name")
  fi
}

skip() {
  NAMES+=("$1"); STATUS+=("skip"); MS+=("0")
  printf '\n\033[2m▶ %s — skipped: %s\033[0m\n' "$1" "$2"
}

have_crate() { [ -f Cargo.toml ]; }
have() { [ -x "$1" ]; }

# ---------------------------------------------------------------------------
# 1. The crate: format, lints, tests. Cheap, so always first.
# ---------------------------------------------------------------------------
if have_crate; then
  step "cargo fmt --check"           cargo fmt --all -- --check
  step "cargo clippy -D warnings"    cargo clippy --all-targets -- -D warnings
  step "cargo test"                  cargo test --all-targets
else
  skip "rust checks" "no Cargo.toml yet (ticket #2 creates the crate)"
fi

# ---------------------------------------------------------------------------
# 2. Spec drift: the vendored contract still covers what we model.
#    Runs inside cargo test (tests/spec_drift.rs); guarded so a scaffold-era
#    repo does not fail for not having it yet.
# ---------------------------------------------------------------------------
if [ -f docs/spec/minos-api.yaml ]; then
  NAMES+=("spec snapshot present"); STATUS+=("ok"); MS+=("0")
else
  skip "spec snapshot" "docs/spec/minos-api.yaml missing"
fi

# ---------------------------------------------------------------------------
# 3. Captures and pixel assertions. A capture is a witness; the assertion is
#    the verification. Slow enough to be opt-out, cheap enough to run always.
# ---------------------------------------------------------------------------
if [ "$FAST" -eq 1 ]; then
  skip "captures" "--fast"
elif have scripts/capture.sh; then
  run_scenes() {
    local ok=0
    # The captures run the binary, not a test harness: build it here so `capture.sh` in CI and on a
    # laptop runs the same file.
    cargo build -q || return 1
    # **The scenes come from the binary, not from a list in this script.** `photo` was added in #41 and
    # never rendered by the gate, because this line named four scenes by hand — and it spent ten days
    # drawing the NATO symbol over a golden that had been regenerated blind (`#54`). A scene that exists is
    # a scene the gate renders; adding one without a golden is a red gate, which is the correct moment to
    # make a human look at it.
    local scenes="${SCENES:-$(./target/debug/arcons-cc --scenes | paste -sd, -)}"
    [ -n "$scenes" ] || { echo "  the binary listed no scenes"; return 1; }
    for scene in ${scenes//,/ }; do
      scripts/capture.sh "$scene" || ok=1
      if have scripts/assert.sh; then
        scripts/assert.sh "out/$scene.png" || ok=1
      else
        echo "  (no scripts/assert.sh yet: $scene rendered, nothing asserted)"
      fi
    done
    return $ok
  }
  step "captures + assertions" run_scenes
else
  skip "captures" "scripts/capture.sh missing (ticket #9)"
fi

# ---------------------------------------------------------------------------
# 4. Budgets. Numbers, not vibes. Named as tests that print the measured value — and `--nocapture` puts
# that number in CI's log, because a budget whose measurement is only visible on failure cannot be audited
# (`#48`).
# ---------------------------------------------------------------------------
if [ "$FAST" -eq 1 ]; then
  skip "budgets" "--fast"
elif have_crate; then
  step "budgets (frame CPU, memory)" cargo test --all-targets -- --include-ignored --nocapture budget
else
  skip "budgets" "no crate yet"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
END_ALL=$(date +%s%N)
TOTAL_MS=$(( (END_ALL - START_ALL) / 1000000 ))

printf '\n%s\n' "────────────────────────────────────────────────────────"
for i in "${!NAMES[@]}"; do
  printf '  %-8s %-32s %6s ms\n' "${STATUS[$i]}" "${NAMES[$i]}" "${MS[$i]}"
done
printf '  %-8s %-32s %6s ms\n' "" "total" "$TOTAL_MS"
printf '%s\n' "────────────────────────────────────────────────────────"

if [ ${#FAILED[@]} -gt 0 ]; then
  printf '\033[31mverify FAILED\033[0m: %s\n' "${FAILED[*]}"
  exit 1
fi
printf '\033[32mverify PASSED\033[0m\n'
