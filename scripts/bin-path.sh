#!/usr/bin/env bash
# Print the path to a built binary, wherever this machine's cargo puts it.
#
#   scripts/bin-path.sh arcons-cc arcons-cc     # -> <target-dir>/debug/arcons-cc
#   scripts/bin-path.sh desk desk
#
# `[build] target-dir` in `~/.cargo/config.toml` moves the store off `./target` — one shared artifact
# directory for every checkout, which is why a worktree builds in seconds (docs/build-environment.md).
# A script that spells `./target/debug/<bin>` is a script that breaks the day the store moves, and it
# breaks in the least helpful way: "No such file or directory" from a capture, after a build that
# printed `Finished`.
#
# It asks cargo rather than guessing, so `CARGO_TARGET_DIR` and the config both apply. The second
# argument is the binary's name, which is not always the package's.
set -uo pipefail

package="${1:?usage: bin-path.sh <package> <binary>}"
binary="${2:?usage: bin-path.sh <package> <binary>}"

target_dir="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
  | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')" || exit 1
[ -n "$target_dir" ] || exit 1
printf '%s/debug/%s\n' "$target_dir" "$binary"
