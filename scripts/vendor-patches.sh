#!/usr/bin/env bash
# Re-make the crates **Rune** patches in its own workspace: upstream crates it depends on by `[patch.crates-io]`
# in *its* manifest. Cargo applies patches from the workspace root alone, so a workspace that depends on Rune by
# rev gets the published crate — and Rune does not compile against it. This wall is a workspace root, so the
# copies have to live here; `vendor/README.md` says, per crate, what upstream does wrong and when to drop it (#185).
#
#   scripts/vendor-patches.sh              # the rev `Cargo.toml` pins
#   scripts/vendor-patches.sh <rev>        # or an explicit one, for a bump under test
#
# The copies are Rune's, byte for byte, and so are the `.patch` files beside them: the script takes them from the
# rev rather than keeping a patch we would have to carry forward. Run it when the pin moves — a rev that has stopped
# patching one of them fails here, which is cheaper than discovering it in a red PR.
#
# When upstream takes a patch back (the crate publishes the change, or Rune stops patching it), delete that crate,
# its `.patch` file and its `[patch.crates-io]` line together, and drop it from `CRATES` below.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CRATES=(winit wgpu-hal)

rev="${1:-}"
if [ -z "$rev" ]; then
  rev="$(sed -n 's/.*anomalyco\/rune", rev = "\([0-9a-f]\{40\}\)".*/\1/p' Cargo.toml | head -1)"
fi
[ -n "$rev" ] || {
  echo "vendor-patches: Cargo.toml pins no Rune rev, and none was given" >&2
  exit 64
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# A checkout of one rev, and one path out of it: no history, no submodules.
git init -q "$tmp/rune"
git -C "$tmp/rune" remote add origin https://github.com/anomalyco/rune
git -C "$tmp/rune" fetch -q --depth 1 origin "$rev"
git -C "$tmp/rune" checkout -q FETCH_HEAD -- vendor

for crate in "${CRATES[@]}"; do
  [ -d "$tmp/rune/vendor/$crate" ] || {
    echo "vendor-patches: rune $rev has no vendor/$crate." >&2
    echo "  If upstream stopped patching it, drop ours instead: vendor/README.md." >&2
    exit 1
  }
  rm -rf "vendor/$crate" "vendor/$crate.patch"
  mv "$tmp/rune/vendor/$crate" "vendor/$crate"
  if [ -f "$tmp/rune/vendor/$crate.patch" ]; then
    mv "$tmp/rune/vendor/$crate.patch" "vendor/$crate.patch"
  fi
  version="$(sed -n 's/^version = "\(.*\)"/\1/p' "vendor/$crate/Cargo.toml" | head -1)"
  echo "vendor/$crate: $crate $version, rune $rev"
done
