# ratidal

A terminal client for TIDAL, in Rust. Browse your collection and play hi-res
audio without leaving the shell.

> **Unofficial.** This is a third-party client with no affiliation with, or
> endorsement from, TIDAL. It talks to TIDAL's undocumented internal API, which
> can change or stop working at any time. You need your own paid TIDAL
> subscription — this does not provide access to music.

## Status

Usable. Login, browsing, search, playback and queue controls all work. It is
still a young project: the API it talks to is undocumented, and the rough
edges are listed under [Known limits](#known-limits).

## What works

- Device-flow login — a code and a link, confirmed in your browser
- The home page, with the same rows and tabs the web client shows
- Explore: genres, moods and decades, and the pages behind them
- Search across tracks, albums, artists and playlists
- Artist pages — top tracks, albums, singles, similar artists, biography, radio
- Your playlists, albums, artists and favourite tracks, each in its own view
- Mixes & Radio, split into your own mixes and TIDAL's stations
- A queue with shuffle and repeat, and favouriting from anywhere
- Cover art, drawn with the terminal's image protocol where there is one and
  half blocks where there is not — so covers appear on Alacritty and the VTE
  terminals too, coarse but recognisable
- Playback up to **24-bit hi-res** (FLAC in fragmented MP4, over DASH)
- A now-playing bar with position, duration, and the quality actually delivered

## Requirements

- Rust (stable) and a working audio output
- A paid TIDAL subscription. HiFi Plus if you want hi-res; otherwise you get
  whatever your plan allows.

On **Linux** the audio backend links against ALSA, so its headers have to be
present at build time:

```sh
sudo apt install pkg-config libasound2-dev     # Debian, Ubuntu
sudo dnf install pkg-config alsa-lib-devel     # Fedora
sudo pacman -S pkgconf alsa-lib                # Arch
```

Nothing else differs between platforms — there is no OS-specific code in the
crate.

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

- **Very large libraries are capped** at 10,000 items per request. Paging stops
  there and logs it — a backstop against an endpoint that ignores `offset` and
  hands back the same page forever, rather than a limit anyone should hit.
- **No gapless playback** on hi-res. Hi-res arrives as segmented MP4, and the
  decoder does not support gapless for that container.
- **Not bit-perfect.** The audio backend does not set the device's physical
  format — CoreAudio on macOS, ALSA on Linux — so content is resampled to
  whatever rate the device is already at. The quality badge reports what TIDAL
  delivered, not what reached the speakers.
- **Read-only.** No playlist editing.
- **Videos are dropped.** Genre pages carry rows of music videos; this plays
  audio, so those rows are left out rather than drawn as things that cannot be
  opened.

## Building on it

The crate is organised by capability rather than by layer — `auth`, `library`,
`playback`, `tidal`, `shell` — each exposing a facade with its internals kept
crate-private. `domain` is the dependency-free core. `tests/architecture.rs`
enforces that: it fails the build if a component imports the UI, if `playback`
reaches for ratatui, or if `domain` depends on anything of ours.

```sh
cargo test          # 573 tests, no network needed
cargo clippy --all-targets
```

The DASH fixtures under `tests/fixtures/` are generated audio, not real TIDAL
content, so the suite runs offline.

`tests/keyboard.rs` drives the whole app the way a person does — keys in,
rendered buffer out — because a test that calls the handlers directly passes
just as well when the key is bound to nothing.

There is a second suite, `tests/live_api.rs`, that runs against the real API
with your own session. It is `#[ignore]`d by default and needs you signed in:

```sh
cargo test --test live_api -- --ignored --nocapture
```

It exists because every DTO field is `#[serde(default)]`, which makes a *wrong*
field name silent — the parse succeeds and the view comes back empty. Only a
real response catches that.

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
