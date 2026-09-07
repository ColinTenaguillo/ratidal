# ratidal

A terminal client for TIDAL, in Rust. Browse your collection and play hi-res
audio without leaving the shell.

> **Unofficial.** This is a third-party client with no affiliation with, or
> endorsement from, TIDAL. It talks to TIDAL's undocumented internal API, which
> can change or stop working at any time. You need your own paid TIDAL
> subscription — this does not provide access to music.

## What works

- Device-flow login — a code and a link, confirmed in your browser
- The home page, with the same rows and tabs the web client shows
- Explore: genres, moods and decades, and the pages behind them
- Search across tracks, albums, artists and playlists
- Artist pages — top tracks, albums, singles, similar artists, biography, radio
- Your playlists, albums, artists and favourite tracks, each in its own view
- Mixes & Radio, split into your own mixes and TIDAL's stations
- A queue with shuffle and repeat, and favouriting from anywhere
- Playback up to **24-bit hi-res** (FLAC in fragmented MP4, over DASH)
- Cover art through the terminal's image protocol, or half blocks where there
  is none

## Install

Needs Rust (stable) and a paid TIDAL subscription — HiFi Plus for hi-res. On
Linux, the ALSA headers as well:

```sh
sudo apt install pkg-config libasound2-dev     # Debian, Ubuntu
sudo dnf install pkg-config alsa-lib-devel     # Fedora
sudo pacman -S pkgconf alsa-lib                # Arch
```

```sh
git clone https://github.com/ColinTenaguillo/ratidal
cd ratidal
cargo build --release
./target/release/ratidal
```

First run shows a code and a link. Confirm in your browser and the session is
saved until it expires.

## Keys

Press `?` in the app for the full list.

| Key | Action |
|---|---|
| `j` / `k`, `↓` / `↑` | Down / up in the list or grid |
| `h` / `l`, `←` / `→` | Left / right along a row |
| `J` / `K` | Move through the sidebar |
| `1` – `9` | Straight to a nav entry — `1` is Music, `9` is Settings |
| `Enter` | Open a playlist, album or artist, or play a track |
| `Esc`, `[` | Back one view, or out of the filter |
| `]` | Forward again, through views and sections |
| `/` | Filter the current view |
| `s` | Search the catalogue |
| `o` | See all of a row |
| `b` | Open an artist's biography |
| `R` | Play an artist's radio |
| `t`, `Tab` | Next tab, on the home page or in search |
| `Space` | Pause / resume |
| `A` | Favourite the track, or take it out again |
| `n` / `p` | Next / previous in the queue |
| `z` | Shuffle the queue |
| `r` | Repeat: off, all, one |
| `?` | The key list |
| `q` | Quit |

`Esc` steps back rather than quitting: only `q` leaves the app.

## Where things live

| What | Linux | macOS |
|---|---|---|
| Config | `~/.config/ratidal/config.toml` | `~/Library/Application Support/ratidal/config.toml` |
| Session token | alongside the config, mode `0600` | same |
| Log | `~/.cache/ratidal/ratidal.log` | `~/Library/Caches/ratidal/ratidal.log` |

## When playback stops working

If tracks fail with **"this client_id cannot stream"** or a `4005` status, the
credentials this app ships have been rate-capped by TIDAL. Put working ones in
your config:

```toml
[auth]
client_id     = "..."
client_secret = "..."
```

## Known limits

- **No gapless playback** on hi-res, which arrives as segmented MP4.
- **Not bit-perfect.** The device's physical format is left alone, so content
  is resampled to whatever rate it is already at. The quality badge reports
  what TIDAL delivered, not what reached the speakers.
- **Read-only.** No playlist editing.
- **No videos.** This plays audio, so the video rows on genre pages are left
  out.
- **10,000 items per request**, as a backstop against an endpoint that pages
  forever.

## Building on it

The crate is organised by capability rather than by layer — `auth`, `library`,
`playback`, `tidal`, `shell` — each exposing a facade with its internals kept
crate-private. `domain` is the dependency-free core. `tests/architecture.rs`
enforces that: it fails the build if a component imports the UI, if `playback`
reaches for ratatui, or if `domain` depends on anything of ours.

```sh
cargo test          # 573 tests, offline
cargo clippy --all-targets
```

`tests/keyboard.rs` drives the app by keys and reads the rendered buffer.
`tests/live_api.rs` checks the DTOs against the real API; it is `#[ignore]`d and
needs you signed in:

```sh
cargo test --test live_api -- --ignored --nocapture
```

## Acknowledgements

Read while building this, and worth reading:

- **[ratatui](https://github.com/ratatui/ratatui)** and
  **[ratatui-image](https://github.com/ratatui/ratatui-image)** — the terminal
  UI and the cover art.
- **[spotify-player](https://github.com/aome510/spotify-player)** — how a
  network-backed ratatui app is put together.
- **[rmpc](https://github.com/mierak/rmpc)** — the single-flight image-encoding
  pattern.
- **[python-tidal](https://github.com/EbbLabs/python-tidal)**,
  **[tidlers](https://codeberg.org/tomkoid/tidlers)** and
  **[tiddl](https://github.com/oskvr37/tiddl)** — references for which
  endpoints exist, the DASH manifest shape, and the device flow.

## Licence

MIT — see [LICENSE](LICENSE).
