#!/usr/bin/env bash
# Re-make `vendor/winit`: **Rune's** copy of winit, taken from Rune at the rev this workspace pins.
#
# The copy is not ours to edit — every byte of it is upstream's, which is why the script takes it from the
# rev rather than from a patch we maintain (`vendor/README.md` says why the copy has to exist at all, #185).
# Run it when the pin moves, and build afterwards: the copy is what `[patch.crates-io]` in `Cargo.toml`
# points at, so a rev that has stopped patching winit fails here rather than somewhere in a red PR.
#
#   scripts/vendor-winit.sh              # the rev `Cargo.toml` pins
#   scripts/vendor-winit.sh <rev>        # or an explicit one, for a bump under test
#
# If upstream `winit` publishes the macOS input-method extensions and Rune stops patching winit, delete
# `vendor/winit`, `vendor/winit.patch` and the `[patch.crates-io]` line together.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

rev="${1:-}"
if [ -z "$rev" ]; then
  rev="$(sed -n 's/.*anomalyco\/rune", rev = "\([0-9a-f]\{40\}\)".*/\1/p' Cargo.toml | head -1)"
fi
[ -n "$rev" ] || {
  echo "vendor-winit: Cargo.toml pins no Rune rev, and none was given" >&2
  exit 64
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# A checkout of one rev and two paths out of it: no history, no LFS, no submodules.
git init -q "$tmp/rune"
git -C "$tmp/rune" remote add origin https://github.com/anomalyco/rune
git -C "$tmp/rune" fetch -q --depth 1 origin "$rev"
git -C "$tmp/rune" checkout -q FETCH_HEAD -- vendor/winit vendor/winit.patch 2>/dev/null || {
  echo "vendor-winit: rune $rev has no vendor/winit (or no vendor/winit.patch)." >&2
  echo "  If upstream stopped patching winit, drop ours instead: vendor/README.md." >&2
  exit 1
}

rm -rf vendor/winit vendor/winit.patch
mv "$tmp/rune/vendor/winit" vendor/winit
mv "$tmp/rune/vendor/winit.patch" vendor/winit.patch

version="$(sed -n 's/^version = "\(.*\)"/\1/p' vendor/winit/Cargo.toml | head -1)"
echo "vendor/winit: winit $version, rune $rev, + vendor/winit.patch"
