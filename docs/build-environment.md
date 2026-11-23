# Build environment: this machine, not the defaults

Read this before you change a profile, a job count, a target directory, or the vendored
`maplibre_native`. It describes what the build machine now does and why, so a build that behaves
unexpectedly is a known shape rather than a mystery. Everything here was run on this machine.

Source of truth for the settings is `~/.cargo/config.toml`. Read that file; this document carries
only the reason behind each line and the traps the file cannot confess.

---

## 1. Build as usual

```sh
cargo build
cargo test --all-targets
scripts/verify.sh --fast          # unchanged
VERIFY_WITH_MAP=1 scripts/verify.sh
```

A `map` build now prints these two lines and nothing from cmake:

```
warning: maplibre_native@0.9.3: Using precompiled maplibre-native static library from /home/edward/Work/vendor/maplibre-native-prebuilt/core-a33d3f0/libmaplibre-native-core-amalgam-linux-x64-vulkan.a
warning: maplibre_native@0.9.3: Extracting headers for maplibre-native core library from .../maplibre-native-headers.tar.gz into <OUT_DIR>/maplibre-native/headers
```

**Done means**: `Finished` appears, those two warnings name the prebuilt archive, and there is no
`-- Configuring Vulkan renderer backend` line. That last line means the environment is not reaching
your build; stop and check `~/.cargo/config.toml` before debugging anything else.

`VERIFY_WITH_MAP=1` is now cheap — the C++ core is a download, not a 2003-file compile — so the
local map hole described in `AGENTS.md` (#152) can be closed on every ticket, not only in CI.
---

## 2. What the config makes true

| setting | you observe |
| --- | --- |
| `[build] target-dir = ~/.cache/cargo/target` | every checkout of this repo shares one artifact store. A second checkout building at the same time prints `Blocking waiting for file lock on build directory` and then proceeds. That message is serialization, not a hang. |
| `[profile.dev] debug = 1` | line tables and variables, no full type info. Test binaries drop from ~600 MB to ~10x smaller. Enough for `gdb` frames; add `debug = 2` in the project's `Cargo.toml` for a session that needs types. |
| `[build] jobs = 4` | four concurrent rustc. The limit here is the disk, not the 12 threads. |
| `rustflags = ["-Clink-arg=-Wl,--threads=2"]` | each `rust-lld` uses 2 threads instead of 12. |
| `MLN_PRECOMPILE = "1"` + the two `MLN_CORE_LIBRARY_*` paths | the MapLibre core comes from one shared archive. cmake never runs. |
| `SCCACHE_*` | `sccache` is installed and would cache C/C++ objects, but the C++ core is prebuilt, so nothing in this repo feeds it. It matters only for a source build (see §4). |

`tfg` keeps its own `target/` (its `.cargo/config.toml` sets `target-dir = "target"`), so the two
projects do not share a store.

### Space, and the one command that is global

The shared store is one directory for all checkouts. `cargo clean` with no arguments deletes it —
`cargo clean` removes the directory itself, confirmed on this machine — and every checkout pays a
full rebuild afterwards.

Use `cargo clean -p <crate>` to drop one crate. Whole-store cleans are a deliberate act, and
`rm -rf ~/.cache/cargo/target` is the same act.

---

## 3. Reading memory on this machine

`btop`'s `MemB` column is RSS, which counts shared pages once per process. Nine linkers each
showing 2.0 GB summed to 24 GiB on a 23.1 GiB machine while real usage was 11.5 GiB.

PSS is the honest per-process number:

```sh
smem -tk | tail -3            # system PSS total ≈ btop's "Used"
grep -m1 ^Pss: /proc/<pid>/smaps_rollup
```

Shared memory is normal here: every process maps the same `libLLVM.so`, the same `.rlib` inputs, and
the same page cache. Only PSS money adds up to physical RAM.

---

## 4. The MapLibre core

| item | value |
| --- | --- |
| archive | `/home/edward/Work/vendor/maplibre-native-prebuilt/core-a33d3f0/libmaplibre-native-core-amalgam-linux-x64-vulkan.a` (378 MiB) |
| headers | `.../core-a33d3f0/maplibre-native-headers.tar.gz` (498 KB) |
| pinned by | `[package.metadata.mln] release` in `vendor/maplibre_native/Cargo.toml` |
| source checkout | `/home/edward/Work/vendor/maplibre-native` (`MLN_LOCAL_REPOSITORY`) — needed only for source builds |
| backend | **Vulkan**, because the dependency enables no backend feature and Linux defaults to Vulkan |

Both `MLN_CORE_LIBRARY_*` values must point at **files**, not directories. The build script derives
the `-l` name from the archive filename and extracts the tarball into `OUT_DIR`.

### Backend changes

`opengl`, `vulkan`, and `wgpu` each need a matching archive from the same release. `glx` together
with `MLN_PRECOMPILE` is rejected outright by `build.rs` ("the OpenGL context is fixed by the
prebuilt artifact"). When you change a backend feature: fetch that variant, then update the two
paths.

### Core bump

1. Read the new tag from `[package.metadata.mln] release`, for example `core-<sha>`.
2. `mkdir ~/Work/vendor/maplibre-native-prebuilt/core-<sha8>` and download, from that release tag,
   `libmaplibre-native-core-amalgam-linux-x64-vulkan.a` and `maplibre-native-headers.tar.gz`.
   GitHub throttles a *resumed* connection to ~40 KB/s; a fresh one runs ~700 KB/s and the 378 MiB
   takes ~11 minutes. Restart the download rather than resuming it.
3. Point the two `MLN_CORE_LIBRARY_*` paths at the new directory.
4. `git -C ~/Work/vendor/maplibre-native fetch && git -C ~/Work/vendor/maplibre-native checkout <sha>`
   to keep the checkout on the same revision for future source builds.

**Done means**: the next `cargo build` prints the *new* archive path and still reports `Finished`.

### Source build, when you need to touch the C++

```sh
export PATH=/home/edward/.cargo/bin:$PATH    # armerge lives here and is NOT on the default PATH
MLN_PRECOMPILE=0 cargo build                  # a shell variable wins over [env]
```

This compiles the core in `OUT_DIR` for that checkout alone, behind a checkout lock shared by all
builds. `armerge` 2.2.2 is installed; `llvm-objcopy` is present; `ninja` is not (CMake uses Makefiles,
which is what `build.rs` expects on Linux).

---

## 5. Triage: "the build is wedged"

Signature: load average in the tens while the CPU sits near idle, the desktop freezes, `btop` shows
several GB per process, used RAM stays low. That is the filesystem, not the build, and not memory.

```sh
ps -eo stat --no-headers | grep -c '^D'          # blocked threads; single digits is normal
cat /proc/pressure/io                            # first two lines: "some"/"full"
for i in 1 2 3; do dd if=/dev/zero of=/home/edward/.fslat bs=4k count=1 conv=fsync status=none; \
  /usr/bin/time -f "%es" true; done; rm -f /home/edward/.fslat
```

A 4 KB `fsync` on `/home` answers in **milliseconds** when the machine is healthy and in **seconds**
when a build has flooded btrfs. Measured on the incident: 4.3–5.2 s during the build, **17–24 ms**
with the nine linkers frozen (`kill -STOP`), back to 5–10 s on `kill -CONT`.

Immediate relief without losing work:

```sh
kill -STOP $(pgrep -x rust-lld)     # pause the writers; fsync drops to ms within a second
kill -CONT $(pgrep -x rust-lld)     # resume
```

Then attack volume, not parallelism: smaller debug info, fewer jobs. Adding threads to a full
disk queue lengthens the queue.

**What the incident was**: nine `rust-lld` processes each writing a ~600 MB debug test binary through
CoW + `zstd:3` + dm-crypt on `/home`. 108 threads sat in btrfs `handle_reserve_ticket`, each `fsync`
waited for a whole-filesystem commit that had to flush the linkers' 5.4 GB first, and every
Chromium-based app (WhatsApp runs as a Chromium web app on `~/.config/chromium`) blocked for seconds
on its own IndexedDB commits. CPU 6%, RAM 11.5 GiB used: the bottleneck was commit latency.

---

## 6. Scheduler

The NVMe runs **kyber**, chosen by omarchy and left in place deliberately: kyber holds a 2 ms read
latency target, which is why cached reads stayed at 2 ms throughout the incident. bfq would not have
shortened the btrfs commit, which was the actual cost.

`/etc/udev/rules.d/60-iosched.rules` — a bfq rule written and then removed during the
investigation — never took effect anyway: `60-omarchy-io-scheduler.rules` sorts later and wins.
Only omarchy's rule remains, and it is the one to keep.

---

## 7. Changelog of machine changes

| what | where |
| --- | --- |
| `debug = 1`, `jobs = 4`, shared `target-dir`, linker threads, sccache env, prebuilt core paths | `~/.cargo/config.toml` (backups: `config.toml.bak.*`) |
| nodatacow (`+C`) on the artifact store and the sccache dir | `~/.cache/cargo` — the flag sits on the **parent** so a recreated `target/` inherits it |
| MapLibre checkout moved | `~/Work/vendor/maplibre-native` (was `~/Work/Crossnet/tfg/vendor/maplibre_native/target/maplibre-native`) |
| `MLN_LOCAL_REPOSITORY` updated to the new path | `~/.cargo/config.toml` and `tfg/.cargo/config.toml` — **the `tfg` change is still uncommitted** |
| stale cmake/fingerprint dirs for the old checkout path removed | `tfg/target` (~2.9 GB) |
| dead per-checkout target dirs removed | 86 GB of files, 53 GB reclaimed on the volume |
| sccache installed | `/usr/bin/sccache` (pacman 0.18.0) |
| armerge installed | `~/.cargo/bin/armerge` 2.2.2 |

---

## 8. Open items

- Commit `tfg/.cargo/config.toml` (`M` in that repo's `git status`).
- Add `~/.cargo/bin` to the shell PATH if you plan source builds of the core.
