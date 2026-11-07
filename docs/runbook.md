# Runbook: putting the wall up

What an operator does from a cold machine to a wall that stays up, and what to check when it does
not. Everything here was run on this machine; where a compositor needed something, it says so.

## 1. Configuration, in the order it is read

Flags win over the environment, and the environment wins over the file. Nothing is read from the
source tree: a credential in a checkout is a credential in a backup.

| setting | flag | environment | `config.toml` |
| --- | --- | --- | --- |
| where the config is | `--config PATH` | — | `$XDG_CONFIG_HOME/arcons-cc/config.toml` |
| Minos REST base | — | `MINOS_BASE_URL` | `minos_url` |
| Centrifugo endpoint | — | `MINOS_SOCKET_URL` | `centrifugo_url` |
| the credential | `--credentials PATH` | `MINOS_REFRESH_TOKEN` or `MINOS_IDENTIFIER`+`MINOS_PASSWORD` | `credentials` (a path) |
| start camera | — | — | `map_center`, `map_zoom` |
| stale threshold | — | — | `stale_after_s` |
| the log | `--log-file PATH` | `RUST_LOG` (level) | — |

```toml
# ~/.config/arcons-cc/config.toml — no secret in this file, so mode 0644 is fine
minos_url      = "https://api.example.internal/api/v1"
centrifugo_url = "wss://api.example.internal/connection/websocket"
credentials    = "credentials.toml"     # relative to THIS file
map_center     = [-6.10, 106.88]
map_zoom       = 11.0
stale_after_s  = 300
```

An unknown key is refused with exit `1` and the key's name. A typo that silently defaulted is a wall
showing the wrong ocean with a configuration that looks right.

## 2. The credential

```toml
# ~/.config/arcons-cc/credentials.toml
refresh_token = "…"        # preferred: nothing to type at boot, and it rotates
```

or, where a refresh token cannot be issued:

```toml
identifier = "wall@example.internal"
password   = "…"
```

```sh
chmod 600 ~/.config/arcons-cc/credentials.toml
```

**A file with any group or other permission bit is refused** — `0644`, `0640`, `0604` all exit `2`
with the mode it found and the `chmod` that fixes it. The check happens before the contents are read:
a file we may not trust is a file whose contents should not be in memory either.

The wall account needs `read` on **`/system/live-feed`** *and* **`/system/units`** (`GET
/units/{id}`, `/commander`, `/specifications`). Feed-only draws a map and fails every click.

### Rotating it

1. Issue a new refresh token for the wall account.
2. Write it: `install -m 600 /dev/stdin ~/.config/arcons-cc/credentials.toml` with the new TOML.
3. `systemctl --user restart arcons-cc` — the wall re-reads the credential at startup, once.
4. `arcons-cc --self-check; echo $?` → `0`. If it says `3`, the token was not accepted; the old one is
   gone, so put the previous value back before trying again.

## 3. Before a session: the self-check

```sh
arcons-cc --self-check; echo $?
```

```
0 ok · 1 config unusable · 2 credentials refused · 3 login failed
4 REST read failed · 5 socket refused · 6 map seed invalid
```

One line per check; the exit code is the **first** failure in that order. A healthy wall:

```
arcons-cc 0.1.0 self-check
  config      ok  /home/wall/.config/arcons-cc/config.toml (file · minos from env)
  credentials ok  /home/wall/.config/arcons-cc/credentials.toml mode 0600 · refresh token (from file)
  login       ok  refresh token accepted, access token valid for 900 s
  picture     ok  127 hulls, 126 never reported
  socket      ok  subscribed to live-feed
  map seed    ok  /opt/arcons-cc/assets/tiles-cache.seed.sqlite
ok
```

It never prints a token, a password, or a URL containing either. The token line states a *lifetime*
and nothing else; the credential line states the file's *mode* and nothing else.

## 4. Running it

```sh
arcons-cc --fullscreen --no-decorations --log-file ~/.local/state/arcons-cc/wall.log
```

| flag | what it does | notes |
| --- | --- | --- |
| `--fullscreen` | borderless, positioned on the primary output's own geometry | verified on Hyprland/Wayland and on X11 |
| `--no-decorations` | borderless at `--window-size` | for a tiled wall slot: the compositor gives it a size, and a title bar would be something to click |
| `--window-size WxH` | logical points | ignored when fullscreen |

**Verified on Wayland (Hyprland, 2026-11-07):** `--no-decorations --window-size 1100x700` opens a
borderless window at that geometry (screenshot in the #8 PR); `--fullscreen` covers the focused
output. The compositor owns placement: a fullscreen window goes on the focused monitor unless a rule
moves it (`windowrulev2 = monitor 1, class:^(arcons-cc)$`), and a tiled `--no-decorations` window is
sized by the layout, not by `--window-size`.

**Not verified here:** X11, which this machine has no session for. The flag maps to winit's
`with_decorations(false)` for both backends, and the request is documented rather than measured — say
so rather than implying a test that did not run.
WSLg (WSL2's Wayland) has no compositor-side placement of its own and opens at the requested size,
windowed; use `--window-size`, not `--fullscreen`.

Exit codes are the self-check's: a unit file that reports `status=2` says "the credential is
refused" without anyone reading a log line.

## 5. A systemd user unit

```ini
# ~/.config/systemd/user/arcons-cc.service
[Unit]
Description=Arcons Command Center
After=graphical-session.target network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStartPre=/usr/local/bin/arcons-cc --self-check
ExecStart=/usr/local/bin/arcons-cc --fullscreen --no-decorations --log-file %h/.local/state/arcons-cc/wall.log
Restart=on-failure
RestartSec=10
# `--self-check` in ExecStartPre means a wall with a refused credential never opens a window at all.
# The GPU variables are the app's own: `ARCONS_MAP_VK_DRIVER` selects the integrated GPU (`#16`).

[Install]
WantedBy=graphical-session.target
```

```sh
systemctl --user daemon-reload
systemctl --user enable --now arcons-cc
journalctl --user -u arcons-cc -f
```

`ExecStartPre=` failing is a service that will not start, which is the right answer: a wall showing a
stale picture from three days ago is worse than a wall showing nothing at all.

`SIGTERM` (what `systemctl --user stop` sends) sets a flag the view loop reads; it quits through the
runtime's own path, so the GL context and the socket are dropped in order and the exit status is `0`.
There is no `TimeoutStopSec` dance and no leaked thread.

## 6. What the startup should say

```
arcons-cc: the map renders through /usr/share/vulkan/icd.d/intel_icd.json — the integrated GPU, where the panel is
INFO minos https://api.example.internal/api/v1 (refresh token from file) · socket wss://… · stale after 300 s
INFO first map frame in 1645 ms
```

- The GPU line is the one to read first on a hybrid laptop: MapLibre sorts devices itself and will
  pick the discrete GPU, which is not where the panel is wired (`#16`). `ARCONS_MAP_VK_DRIVER=off`
  turns the pinning off; `ARCONS_MAP_VK_DRIVER=<path>` names one ICD file.
- `first map frame` is measured from process start. The ticket's target is under 3 s, and on this
  machine (debug build, Hyprland/Wayland, Intel RPL-P, `--features map`) the measured range is
  **1.6–2.3 s** — 1645 ms and 2281 ms across two runs. It is printed on every run, so a regression is a
  number in the log rather than a feeling.
- `RUNE_GPU_LOG=1` adds Rune's own line (`a surface draws with wgpu Vulkan on …`). Keep it in mind
  when frames are slow: the two lines together say which GPU each half is using.

## 7. When something is wrong

| symptom | read this |
| --- | --- |
| no window, exit `1` | the config: the message names the key and the file |
| no window, exit `2` | the credential's mode; `chmod 600` is in the message |
| a window that says `no socket` | the wall is read-only: no credential was found. `--self-check` |
| `socket down: refused 101` | the token was refused; rotate it (§2) |
| `socket down: refused 103` | the account may not subscribe to `live-feed`: a permissions change, not a code bug |
| `refused 109` | the token expired mid-session; the actor refreshes once and comes back by itself |
| a picture that stops updating, header says `reconnecting` | the socket, not the REST client: the last read is still on screen, and the banner says so |
| every hull dim | the feed's own timestamps are older than `stale_after_s`; check `age_seconds` on the wire before blaming the wall |
| the map is blank but markers are placed | the seed: `--self-check` proves it is present, `--probe-map` proves it draws |
