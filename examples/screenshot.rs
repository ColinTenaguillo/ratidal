//! Render each view to a PNG so its colours and layout can actually be looked
//! at.
//!
//! `preview.rs` prints the buffer as text, which shows structure but throws
//! away every colour — so a palette mistake, an invisible selection, or a
//! widget painting over another all survive it. This paints each cell as a
//! block of its real foreground and background, which makes those visible.
//!
//! The app's own `shell::draw` does the drawing: a preview that assembles the
//! layout itself drifts from the real one, and then it checks its own copy
//! rather than the UI.
//!
//! Text is drawn as a coarse 3x5 bitmap: enough to tell headings from body and
//! to see where the text sits, without pulling in a font engine for a
//! development tool.
//!
//! Usage: cargo run --example screenshot -- [outdir] [width] [height]

use image::{Rgb, RgbImage};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;
use ratidal::domain::Track;
use ratidal::library::{Album, Artist, Playlist};
use ratidal::shell::carousel::{Card, CarouselState};
use ratidal::shell::home::{HomeState, Row, Shortcut};
use ratidal::shell::sidebar::Section;
use ratidal::shell::App;

/// Cell size in pixels. Terminal cells are about twice as tall as wide.
const CW: u32 = 7;
const CH: u32 = 14;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).cloned().unwrap_or_else(|| ".".into());
    let w: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(120);
    let h: u16 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(40);

    std::fs::create_dir_all(&dir).expect("create output directory");

    for (name, section) in [
        ("home", Section::Music),
        ("playlists", Section::Playlists),
        ("albums", Section::Albums),
        ("profiles", Section::Profiles),
        ("tracks", Section::Tracks),
        // An album opened from the grid, which is a view of its own rather
        // than the favourites list under a different name.
        ("album-open", Section::Albums),
    ] {
        let mut app = sample_app(section);
        if name == "album-open" {
            let tracks = app.tracks.clone();
            app.update(ratidal::shell::Action::ActivateSelection);
            // Opening clears the list and the fetch fills it; stand in for
            // the response so the view is shown with content in it.
            app.tracks = tracks;
        }
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
        terminal
            .draw(|f| ratidal::shell::draw(f, &mut app))
            .expect("draw");

        let out = format!("{dir}/{name}.png");
        paint(terminal.backend().buffer()).save(&out).expect("save");

        // The text alongside the picture: a 3x5 glyph is enough to see where
        // text sits, not enough to read it.
        let buf = terminal.backend().buffer();
        let text: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(format!("{dir}/{name}.txt"), text).expect("write text");
        println!("wrote {out} ({w}x{h} cells)");
    }
}

/// An app filled with data shaped like the real thing, parked on `section`.
fn sample_app(section: Section) -> App {
    let mut app = App {
        session: Some(ratidal::auth::StoredToken::default()),
        ..App::default()
    };
    while app.sidebar.section() != section {
        app.sidebar.next();
    }

    app.playlists = [
        ("PACS EVERYTHING", 6),
        ("To download", 32),
        ("Ouais ouais", 2),
        ("Join me", 52),
        ("Coco 3.0", 60),
        ("La mort avec toi", 27),
        ("Classical Focus", 118),
        ("Coco", 206),
        ("Jazzy", 12),
        ("Chill", 23),
        ("Coco 2.0", 67),
        ("Coco summer", 21),
    ]
    .iter()
    .map(|(t, n)| Playlist::sample(t, *n))
    .collect();

    app.albums = [
        ("Friday Night in San Francisco", "Al Di Meola, John McLaughlin", "1981"),
        ("Mad World (with Solar State)", "Beauz, Hard Lights", "2026"),
        ("Feet Work, Body Work", "Gonzi", "2026"),
        ("Acid & Repeat", "AREA ONE, NIOTech", "2026"),
        ("Acid Symphony", "Niotech, Wolverave", "2026"),
        ("FEFE", "Beauz, The Infamous", "2026"),
        ("Body Rave", "Niotech, Kyzwall", "2026"),
        ("Dura (Special Instrumental)", "Kar Vogue", "2018"),
        ("TECHNO DRUG (BEAUZ Remix)", "LNY TNZ, MADGRRL", "2026"),
        ("Dame Un Grr", "Beauz, Cassie Ann", "2026"),
        ("I Want You Bad", "Gonzi, Kichta", "2026"),
        ("Don't Stop!", "Gonzi", "2025"),
    ]
    .iter()
    .enumerate()
    .map(|(i, (title, artist, year))| Album {
        id: i as u64,
        title: (*title).into(),
        artist: (*artist).into(),
        year: Some((*year).into()),
        cover: None,
    track_count: 10,
    duration: None,
    })
    .collect();

    app.artists = [
        "Kendrick Lamar",
        "2Pac",
        "Camaron de la Isla",
        "Doc Gyneco",
        "Ice Cube",
        "Manitas De Plata",
        "Gipsy Kings",
        "Paco de Lucia",
        "Jose Reyes",
        "Niska",
        "Booba",
        "Tyler, The Creator",
    ]
    .iter()
    .enumerate()
    .map(|(i, name)| Artist { id: i as u64, name: (*name).into(), picture: None })
    .collect();

    app.tracks = [
        ("Beuk My Stride", "ARENCI", "Beuk Seizoen EP 1", 134, false),
        ("Dear Mama", "2Pac", "Me Against The World", 280, true),
        ("All Eyez On Me", "2Pac, Big Syke", "All Eyez On Me", 308, true),
        (
            "Mediterranean Sundance / Rio Ancho",
            "Al Di Meola, John McLaughlin, Paco de Lucia",
            "Friday Night in San Francisco",
            693,
            false,
        ),
        ("I Get Around", "2Pac, Digital Underground", "Strictly 4 My N.I.G.G.A.Z.", 259, true),
        ("Cheers (feat. Q-Tip)", "Anderson .Paak, Q-Tip", "Oxnard", 335, true),
        ("Hit 'Em Up (Single Version)", "2Pac, The Outlawz", "Greatest Hits", 313, true),
        ("California Love (Original)", "2Pac, Roger Troutman, Dr. Dre", "Greatest Hits", 285, true),
        ("Ghetto Gospel", "2Pac, Elton John", "Loyal To The Game", 238, true),
        ("Hate It Or Love It (G-Unit)", "50 Cent, The Game, Tony Yayo", "The Massacre", 264, true),
        ("Do For Love", "2Pac", "R U Still Down? [Remember Me]", 282, true),
        ("GO BADDIE", "BEAUZ, Lockdown, Caroline Roxy", "GO BADDIE", 111, false),
    ]
    .iter()
    .enumerate()
    .map(|(i, (title, artist, album, secs, explicit))| Track {
        // Distinct ids: `Track::sample` gives every track id 0, which would
        // make the playing marker appear on all of them at once.
        id: ratidal::domain::TrackId(i as u64 + 1),
        album: (*album).into(),
        added: Some("2026-07-17T09:12:44.000+0000".into()),
        explicit: *explicit,
        tags: if i == 3 { vec!["HIRES_LOSSLESS".into()] } else { Vec::new() },
        ..Track::sample(title, artist, std::time::Duration::from_secs(*secs))
    })
    .collect();

    // The fourth track is the one playing, so the marker and the bar agree.
    app.now_playing = ratidal::shell::nowplaying::NowPlaying {
        track: app.tracks.get(3).cloned(),
        position: std::time::Duration::from_secs(111),
        playing: true,
        quality: Some("24-bit 176.4kHz".into()),
        tier: ratidal::shell::nowplaying::Tier::Max,
    };
    app.tracklist.selected = 3;

    app.home = HomeState {
        has_tabs: true,
        tab: 0,
        heading: None,
        shortcuts: [
            ("Coco 3.0", "Created by me"),
            ("Meet Her At The Love Parade", "Track radio"),
            ("Join me", "Created by me"),
            ("Coco summer", "Created by me"),
            ("Join me", "Created by me"),
            ("Coco", "Created by me"),
        ]
        .iter()
        .map(|(t, s)| Shortcut {
            title: (*t).into(),
            subtitle: (*s).into(),
            cover_url: None,
        })
        .collect(),
        rows: vec![
            Row {
                kind: ratidal::browse::RowKind::Carousel,
                heading: "New Albums".into(),
                cards: vec![
                    Card::new("August 26", "Post Malone"),
                    Card::new("DILLAGENCE II", "Busta Rhymes"),
                    Card::new("AM Clock Radio", "Charlie Hunter"),
                    Card::new("Weather Report", "Kurt Elling"),
                    Card::new("Live with the Norw", "Jaga Jazzist"),
                ],
                state: CarouselState::default(),
                more: None,
            },
            Row {
                kind: ratidal::browse::RowKind::Carousel,
                heading: "The Hits".into(),
                cards: (1..=5)
                    .map(|i| Card::new(format!("My Mix {i}"), "Various artists"))
                    .collect(),
                state: CarouselState::default(),
                more: None,
            },
        ],
        ..Default::default()
    };

    app
}

/// Paint the buffer, one cell per block.
fn paint(buf: &Buffer) -> RgbImage {
    let (cols, rows) = (buf.area.width as u32, buf.area.height as u32);
    let mut img = RgbImage::new(cols * CW, rows * CH);

    for y in 0..rows {
        for x in 0..cols {
            let cell = &buf[(x as u16, y as u16)];
            let bg = rgb(cell.bg, Rgb([13, 13, 13]));
            let fg = rgb(cell.fg, Rgb([230, 230, 230]));

            for py in 0..CH {
                for px in 0..CW {
                    img.put_pixel(x * CW + px, y * CH + py, bg);
                }
            }
            stamp(&mut img, x * CW, y * CH, cell.symbol(), fg);
        }
    }
    img
}

/// A 3x5 glyph, enough to see where text is and roughly how dense.
fn stamp(img: &mut RgbImage, ox: u32, oy: u32, symbol: &str, fg: Rgb<u8>) {
    let ch = symbol.chars().next().unwrap_or(' ');
    if ch == ' ' {
        return;
    }

    // Block-drawing characters fill their cell; everything else gets a
    // low-resolution mark whose density tracks the character's weight.
    let solid = matches!(ch, '█' | '▀' | '▄' | '▌' | '▐' | '─' | '│' | '┌' | '┐' | '└' | '┘');
    let rows: [u8; 5] = if solid {
        [0b111, 0b111, 0b111, 0b111, 0b111]
    } else if ch.is_ascii_uppercase() || ch.is_ascii_digit() {
        [0b111, 0b101, 0b111, 0b101, 0b101]
    } else if ch.is_ascii_lowercase() {
        [0b000, 0b110, 0b101, 0b101, 0b111]
    } else {
        [0b000, 0b010, 0b111, 0b010, 0b000]
    };

    for (r, bits) in rows.iter().enumerate() {
        for c in 0..3u32 {
            if bits & (1 << (2 - c)) != 0 {
                let (px, py) = (ox + 2 + c, oy + 4 + r as u32);
                if px < img.width() && py < img.height() {
                    img.put_pixel(px, py, fg);
                }
            }
        }
    }
}

fn rgb(c: Color, fallback: Rgb<u8>) -> Rgb<u8> {
    match c {
        Color::Rgb(r, g, b) => Rgb([r, g, b]),
        Color::Reset => fallback,
        Color::Black => Rgb([0, 0, 0]),
        Color::White => Rgb([255, 255, 255]),
        Color::Green => Rgb([0, 200, 120]),
        Color::Indexed(i) => indexed(i),
        _ => fallback,
    }
}

/// The xterm-256 ramp, enough of it for the palette's fallbacks.
fn indexed(i: u8) -> Rgb<u8> {
    match i {
        16 => Rgb([0, 0, 0]),
        231..=255 => {
            let v = 8 + (i as u32 - 232) * 10;
            let v = v.min(255) as u8;
            Rgb([v, v, v])
        }
        48 => Rgb([0, 255, 135]),
        _ if i >= 16 => {
            // 6x6x6 colour cube.
            let n = i as u32 - 16;
            let step = |v: u32| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            Rgb([step(n / 36 % 6), step(n / 6 % 6), step(n % 6)])
        }
        _ => Rgb([128, 128, 128]),
    }
}
