# ratidal

[![CI](https://github.com/ColinTenaguillo/ratidal/actions/workflows/ci.yml/badge.svg)](https://github.com/ColinTenaguillo/ratidal/actions/workflows/ci.yml)

A terminal client for TIDAL, in Rust. Browse your collection and play hi-res
audio without leaving the shell.

> **Unofficial.** This is a third-party client with no affiliation with, or
> endorsement from, TIDAL. It talks to TIDAL's undocumented internal API, which
> can change or stop working at any time. You need your own paid TIDAL
> subscription — this does not provide access to music.
>
> **It identifies itself to TIDAL as one of their own applications.** Full
> playback is only granted to TIDAL's first-party clients, so this sends the
> `client_id` of one of them, as every third-party TIDAL client does. There is
> no honest alternative today: see
> [Credentials](#credentials) for why, and decide for yourself whether you want
> to run it.

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
- The keyboard's own play/pause and next keys, and the track in the desktop's
  player widget — MPRIS on Linux, Now Playing on macOS
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

The keyboard's media keys work too — play/pause, next, previous — and they
reach the app whether or not the terminal has focus, since the desktop
delivers them rather than the terminal.

## Themes

Seven are built in, each taken from the values its own project publishes:

| `theme = ` | |
|---|---|
| `default` | TIDAL's own colours |
| `catppuccin` | Catppuccin Mocha |
| `gruvbox` | Gruvbox dark |
| `tokyonight` | Tokyo Night Storm |
| `nord` | Nord |
| `dracula` | Dracula |
| `solarized` | Solarized dark |

```toml
[ui]
theme = "catppuccin"
```

Any colour can be set by hand, and wins over whichever theme is named:

```toml
[ui]
theme = "catppuccin"

[ui.colors]
accent = "#f5c2e7"
selection = "#585b70"
```

The eleven names are `accent`, `text`, `dim`, `heading`, `surface`,
`selection`, `quality`, `track`, `placeholder`, `border` and `on_accent`.
Values are `#rrggbb`, with the `#` optional.

A line that cannot be read is skipped, not fatal. A misspelt colour name or a
value that is not `#rrggbb` leaves that one colour as the theme had it, keeps
the rest, and says what went wrong in the status bar and the log. An unknown
theme name falls back to `default` the same way. The app always starts.

Named themes are drawn in truecolor; `default` falls back to 256 colours where
the terminal cannot do better.

## Where things live

| What | Linux | macOS |
|---|---|---|
| Config | `~/.config/ratidal/config.toml` | `~/Library/Application Support/ratidal/config.toml` |
| Session token | alongside the config, mode `0600` | same |
| Log | `~/.cache/ratidal/ratidal.log` | `~/Library/Caches/ratidal/ratidal.log` |

## Credentials

ratidal ships a `client_id` and secret belonging to one of TIDAL's own
applications, and sends them on every request. TIDAL therefore believes it is
talking to that application rather than to this one. That is a
misrepresentation, and it is worth knowing before you run it.

It is not a leak of anything private to you: your own login happens in your
browser, and your session token stays on your machine, mode `0600`. The shipped
credentials identify the *application*, not you.

There is no legitimate way to replace them today, which is the only reason they
are here:

- TIDAL's own developer portal issues credentials, but they are refused for the
  device flow — the code-and-a-link login a terminal needs. The API answers
  `Client is not a Limited Input Device client`.
- Those credentials also cap playback at 30-second previews, and only through
  TIDAL's Player SDK, which has no build for a terminal.
- Device-flow credentials are
  [documented as internal to TIDAL](https://github.com/hmelder/TIDAL/wiki/Authentication),
  and public requests for third-party access have gone
  [unanswered](https://github.com/orgs/tidal-music/discussions/321).

Every third-party TIDAL client is in the same position. That explains the
choice; it does not make it right, and if TIDAL opens a path for third-party
device clients this should change.

If playback fails with **"this client_id cannot stream"** or a `4005` status,
the shipped credentials have been rate-capped. Put working ones in your config:

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

## Contributing

How the crate is laid out, what the test suites cover, and the rules the build
enforces: [CONTRIBUTING.md](CONTRIBUTING.md).

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
