#!/usr/bin/env bash
# Re-make the crates **Rune** patches in its own workspace: upstream crates it depends on by `[patch.crates-io]`
# in *its* manifest. Cargo applies patches from the workspace root alone, so a workspace that depends on Rune by
# rev gets the published crate — and Rune does not compile against it. This wall is a workspace root, so the
# copies have to live here; `vendor/README.md` says, per crate, what upstream does wrong and when to drop it (#185).
#
#   scripts/vendor-patches.sh              # the rev `Cargo.toml` pins
#   scripts/vendor-patches.sh <rev>        # or an explicit one, for a bump under test
#
# **The list is the rev's, not ours.** It is read from `[patch.crates-io]` in the manifest at the rev, so a bump
# that starts patching a seventh crate copies it here instead of leaving it to be discovered as a compile error on
# one platform (the #185 shape: Rune's patch does not reach the workspace that depends on it). A crate we still
# patch that the rev has stopped patching is a red run here, with the three things to delete named.
#
# The copies are Rune's, byte for byte, and so are the `.patch` files beside them: the script takes them from the
# rev rather than keeping a patch we would have to carry forward. Run it when the pin moves.
#
# When upstream takes a patch back (the crate publishes the change, or Rune stops patching it), delete that crate,
# its `.patch` file and its `[patch.crates-io]` line together — this script names all three when it happens.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# The `[patch.crates-io]` entries that are ours, not Rune's: a copy we made for our own reason, which must not be
# mistaken for one the rev has dropped. Every other `vendor/…` patch line is Rune's and moves with the rev.
OURS=(tokio-centrifuge)

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

# A checkout of one rev, and the two paths out of it: no history, no submodules. The size filter keeps the fetch to
# the files the copy needs (the rev's tree and its `vendor/` trees, every one of them well under 2 MB) instead of
# the repository's — about 45 MB rather than a few hundred, and one request rather than one per file. A vendored
# file that is ever larger still arrives, lazily, at the checkout below.
git init -q "$tmp/rune"
git -C "$tmp/rune" remote add origin https://github.com/anomalyco/rune
git -C "$tmp/rune" fetch -q --depth 1 --filter=blob:limit=2m origin "$rev"
git -C "$tmp/rune" checkout -q FETCH_HEAD -- Cargo.toml vendor

# `[patch.crates-io]` at the rev: every `name = { path = "vendor/<name>" }` in that section alone, ending at the
# next table. Comments above an entry are why upstream carries it; they are not parsed and do not need to be.
patched="$(awk '
  /^\[/ { in_patch = ($0 ~ /^\[patch\.crates-io\]/) ; next }
  in_patch && match($0, /^[A-Za-z0-9_-]+[[:space:]]*=[[:space:]]*\{[[:space:]]*path[[:space:]]*=[[:space:]]*"vendor\/[^"]+"/) {
    name = $0
    sub(/[[:space:]]*=.*/, "", name)
    print name
  }
' "$tmp/rune/Cargo.toml")"

[ -n "$patched" ] || {
  echo "vendor-patches: rune $rev lists no [patch.crates-io], which cannot be right." >&2
  echo "  Read $tmp/rune/Cargo.toml — or upstream may have moved its patches." >&2
  exit 1
}

echo "vendor-patches: rune $rev patches $(echo "$patched" | tr '\n' ' ')"

for crate in $patched; do
  [ -d "$tmp/rune/vendor/$crate" ] || {
    echo "vendor-patches: rune $rev lists $crate in [patch.crates-io] but has no vendor/$crate." >&2
    echo "  Upstream is inconsistent; read its manifest before taking the rev." >&2
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

# Our root `[patch.crates-io]` entries that point at `vendor/`, which is the set we carry.
ours="$(awk '
  /^\[/ { in_patch = ($0 ~ /^\[patch\.crates-io\]/) ; next }
  in_patch && match($0, /^[A-Za-z0-9_-]+[[:space:]]*=[[:space:]]*\{[[:space:]]*path[[:space:]]*=[[:space:]]*"vendor\//) {
    name = $0
    sub(/[[:space:]]*=.*/, "", name)
    print name
  }
' Cargo.toml)"

is_ours() {
  local needle="$1" own
  for own in "${OURS[@]}"; do [ "$own" = "$needle" ] && return 0; done
  return 1
}

# A crate the rev patches that our manifest does not: Rune will not compile on the platform that needs it, and the
# failure arrives as a missing method in a dependency's source. Loud here, with the lines to add.
missing=""
for crate in $patched; do
  echo "$ours" | grep -qx "$crate" || missing="$missing$crate = { path = \"vendor/$crate\" }\n"
done
# A crate we patch that the rev has stopped patching: our copy is now a debt with no reason, and a stale one.
stale=""
for crate in $ours; do
  is_ours "$crate" && continue
  echo "$patched" | grep -qx "$crate" || stale="$stale$crate "
done

status=0
if [ -n "$missing" ]; then
  echo >&2
  echo "vendor-patches: rune $rev patches crates this workspace does not. Add to [patch.crates-io] in Cargo.toml:" >&2
  printf "%b" "$missing" | sed 's/^/  /' >&2
  echo "  …then record each in vendor/README.md (what upstream does, why we carry it, when to drop it)." >&2
  status=1
fi
if [ -n "$stale" ]; then
  echo >&2
  echo "vendor-patches: rune $rev no longer patches:$stale" >&2
  for crate in $stale; do
    echo "  drop vendor/$crate, vendor/$crate.patch and its [patch.crates-io] line together." >&2
  done
  [ -n "$missing" ] || echo "  (the copy was left alone: delete it by hand, then re-run)" >&2
  status=1
fi
exit "$status"
