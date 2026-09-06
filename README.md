# ratidal

A terminal client for TIDAL, in Rust. Browse your collection and play hi-res
audio without leaving the shell.

> **Unofficial.** This is a third-party client with no affiliation with, or
> endorsement from, TIDAL. It talks to TIDAL's undocumented internal API, which
> can change or stop working at any time. You need your own paid TIDAL
> subscription — this does not provide access to music.

## Status

Early, but usable. Login, browsing your collection, and hi-res playback work.
Search and queue controls are not built yet.

## What works

- Device-flow login — a code and a link, confirmed in your browser
- The home page, with the same rows the web client shows
- Your playlists, albums, artists and favourite tracks, each in its own view
- Cover art, drawn with the terminal's image protocol where there is one and
  half blocks where there is not — so covers appear on Alacritty and the VTE
  terminals too, coarse but recognisable
- Playback up to **24-bit hi-res** (FLAC in fragmented MP4, over DASH)
- A now-playing bar with position, duration, and the quality actually delivered

## Requirements

- Rust (stable) and a working audio output
- A paid TIDAL subscription. HiFi Plus if you want hi-res; otherwise you get
  whatever your plan allows.

## Install

```sh
git clone https://github.com/ColinTenaguillo/ratidal
cd ratidal
cargo build --release
./target/release/ratidal
```

On first run it writes a config file, then shows a code and a link. Confirm in
your browser and the session is saved — you will not be asked again until it
expires.

## Keys

Press `?` in the app for the full list.

| Key | Action |
|---|---|
| `j` / `k`, `↓` / `↑` | Down / up in whichever pane has focus |
| `h` / `l`, `←` / `→` | Left / right, and between the sidebar and the content |
| `J` / `K` | The sidebar, without moving focus to it |
| `Tab` | Switch focus between the sidebar and the content |
| `Enter` | Sign in, open a playlist or album, or play a track |
| `/` | Filter the current view |
| `t` | Next tab on the home page |
| `Space` | Pause / resume |
| `?` | The key list |
| `q`, `Esc` | Quit |

## Where things live

| What | Where |
|---|---|
| Config | `~/.config/ratidal/config.toml` (Linux), `~/Library/Application Support/…` (macOS) |
| Session token | alongside the config, mode `0600` |
| Log | the platform cache directory, `ratidal.log` |

The token is a plain file, not an OS keyring: an unsigned binary re-prompts for
the macOS keychain on every rebuild, and Linux needs D-Bus, which is absent on
headless machines. Every comparable tool makes the same choice.

## When playback stops working

If tracks fail with **"this client_id cannot stream"** or a `4005` status, the
client credentials this app ships have been rate-capped by TIDAL. It happens to
every project in this space, and it is not something a release can fix ahead of
time.

The fix is to put working credentials in your config:

```toml
[auth]
client_id     = "..."
client_secret = "..."
```

These are extracted from TIDAL's own first-party applications — the same
approach every third-party TIDAL client takes. Shipping defaults means the app
works out of the box; making them overridable means you are not stuck waiting
for a release when they stop working.

## Known limits

- **Very large libraries are capped** at 10,000 items per view. Paging stops
  there and logs it, rather than fetching without end.
- **No gapless playback** on hi-res. Hi-res arrives as segmented MP4, and the
  decoder does not support gapless for that container.
- **Not bit-perfect.** The audio backend does not set the CoreAudio physical
  format, so 44.1 kHz content is resampled. The quality badge reports the
  source, not the output.
- **Read-only.** No playlist editing.

## Building on it

The crate is organised by capability rather than by layer — `auth`, `library`,
`playback`, `tidal`, `shell` — each exposing a facade with its internals kept
crate-private. `domain` is the dependency-free core. `tests/architecture.rs`
enforces that: it fails the build if a component imports the UI, if `playback`
reaches for ratatui, or if `domain` depends on anything of ours.

```sh
cargo test          # 85 tests, no network needed
cargo clippy --all-targets
```

The DASH fixtures under `tests/fixtures/` are generated audio, not real TIDAL
content, so the suite runs offline.

## Acknowledgements

Built while reading, and learning from, these projects:

- **[spotify-player](https://github.com/aome510/spotify-player)** (MIT) — the
  architecture of a network-backed ratatui app, and the terminal-image handling
  that a later version of this will need.
- **[ratatui](https://github.com/ratatui/ratatui)** (MIT) and
  **[ratatui-image](https://github.com/ratatui/ratatui-image)** (MIT).
- **[rmpc](https://github.com/mierak/rmpc)** (BSD-3-Clause) — the single-flight
  image-encoding pattern. Reimplemented, not copied.
- **[python-tidal](https://github.com/EbbLabs/python-tidal)** (LGPL-3.0) — read
  as a reference for which endpoints exist and what they return. No code was
  copied or translated; the API surface here was written from notes and
  verified against live responses.
- **[tidlers](https://codeberg.org/tomkoid/tidlers)** and
  **[tiddl](https://github.com/oskvr37/tiddl)** — read as references for the
  DASH manifest shape and the device-flow behaviour.

## Licence

MIT — see [LICENSE](LICENSE).
