#!/usr/bin/env bash
# The binary a wall runs: release, the map included, and the map engine's core taken from upstream's
# prebuilt amalgam rather than compiled here. Why is written down in #26 and in `docs/runbook.md` §6.
#
#   scripts/build-wall.sh            # → target/release/arcons-cc
#   scripts/build-wall.sh --features map,x
#
# The local-repository route (`MLN_LOCAL_REPOSITORY`, what `~/.cargo/config.toml` points at) is the
# development route: it compiles MapLibre's C++ with this machine's compiler, which segfaults on the
# first map frame at -O2 and above (#26). Debug builds are unaffected, which is why the gate and every
# capture so far were taken on one.
set -euo pipefail
cd "$(dirname "$0")/.."

# `MLN_LOCAL_REPOSITORY` wins over `MLN_PRECOMPILE` in the vendored build script, so it is cleared
# here explicitly rather than relying on it being unset in the environment.
MLN_LOCAL_REPOSITORY= MLN_PRECOMPILE=1 exec cargo build --release --features map "$@"
