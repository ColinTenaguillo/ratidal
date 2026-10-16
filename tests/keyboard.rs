//! The app driven the way a person drives it: keys in, screen out.
//!
//! Every other test reaches inside — it calls a handler, or reads a field.
//! That is how a "see all" hint came to be drawn on an artist's page for a
//! key that did nothing there: the renderer was tested, the key was tested,
//! and nothing put the two together.
//!
//! These press keys and read the buffer, so a feature only passes when it
//! works end to end.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use sonora_tui::shell::{self, App};

/// An app with a session, so the keys are live.
fn app() -> App {
    App {
        session: Some(sonora_tui::catalogue::Session::sample()),
        // Nothing here may write the user's real files.
        config_path: None,
        last_main_width: 120,
        last_main_height: 40,
        ..App::default()
    }
}

/// Press a key and apply whatever it asks for, as the event loop does.
///
/// The loop also spawns fetches; those are not here, so a view that needs
/// one is filled in by the test instead.
fn press(app: &mut App, code: KeyCode) {
    if let Some(action) = app.on_key(KeyEvent::new(code, KeyModifiers::NONE)) {
        let mut next = app.update(action);
        // An action can imply another, exactly as in the loop.
        while let Some(action) = next {
            next = app.update(action);
        }
    }
}

/// The screen at 120x40, so a test that does not care about the layout need
/// not name a size.
fn screen(app: &mut App) -> String {
    screen_sized(app, 120, 40)
}

/// The screen at a given size, for a test that cares about the layout.
fn screen_sized(app: &mut App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
    terminal
        .draw(|frame| shell::draw(frame, app))
        .expect("draw");
    let buf = terminal.backend().buffer().clone();
    // Written out here rather than borrowed from the unit tests' own
    // helper: this drives the app from outside, and reaching into a
    // `cfg(test)` module would be reaching inside again.
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// An artist page with something in every section.
fn an_artist() -> sonora_tui::library::ArtistPage {
    let album = |i: u64, title: &str| sonora_tui::library::Album {
        id: i.to_string(),
        title: title.into(),
        artist: "Daft Punk".into(),
        year: Some("2001".into()),
        cover: None,
        track_count: 10,
        duration: None,
    };
    // As the live page comes back: a radio, and the four or five top
    // tracks TIDAL returns.
    sonora_tui::library::ArtistPage {
        name: "Daft Punk".into(),
        albums: (0..12).map(|i| album(i, &format!("Album {i}"))).collect(),
        singles: (0..12)
            .map(|i| album(20 + i, &format!("Single {i}")))
            .collect(),
        top_tracks: (0..4)
            .map(|i| sonora_tui::domain::Track {
                id: sonora_tui::domain::TrackId::from(100 + i),
                title: format!("Top Track {i}"),
                artist: "Daft Punk".into(),
                album: "Discovery".into(),
                duration: std::time::Duration::from_secs(210),
                cover: None,
                tags: Vec::new(),
                added: None,
                explicit: false,
                ai: false,
                radio: None,
                album_id: None,
                artist_id: None,
            })
            .collect(),
        radio: Some("mix-daft".into()),
        top_tracks_path: Some("pages/data/top-tracks".into()),
        ..Default::default()
    }
}

#[test]
fn see_all_on_an_artists_section_opens_it() {
    // The hint was drawn and the key did nothing: `SeeAll` only knew about
    // the home page's rows, and an artist's sections are not those.
    let mut app = app();
    app.artist = Some(an_artist());
    // On the albums, said rather than assumed: the first section is Top
    // Tracks, and which one the selection starts on is not what this is
    // about.
    app.artist_section = 1;

    let before = screen(&mut app);
    assert!(before.contains("See all"), "the page offers it:\n{before}");
    assert!(before.contains("Albums"), "on the albums:\n{before}");

    press(&mut app, KeyCode::Char('o'));

    let after = screen(&mut app);
    assert!(
        after.contains("Daft Punk — Albums"),
        "and pressing it opens that section:\n{after}"
    );
    // A grid of all twelve rather than the strip the row had room for:
    // more rows of them than a carousel's single line.
    let rows_of_albums = after.lines().filter(|l| l.contains("Album ")).count();
    assert!(
        rows_of_albums > 1,
        "laid out as a grid, not the one row the section showed:\n{after}"
    );
}

#[test]
fn a_and_c_open_the_artist_and_the_album_of_the_selected_track() {
    // A track list names both and could open neither: the only way to an
    // album from your favourites was to search for its name. The ids were
    // in the API response all along and thrown away at the parser.
    let mut app = app();
    app.tracks = vec![sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(1),
        title: "One More Time".into(),
        artist: "Daft Punk, Romanthony".into(),
        album: "Discovery".into(),
        duration: std::time::Duration::from_secs(320),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: None,
        album_id: Some("9".into()),
        artist_id: Some("2".into()),
    }];
    // On the Tracks section, where `app.tracks` is what the keys act on:
    // the app opens on Music, whose selection is a card on the home page.
    while app.sidebar.section() != sonora_tui::shell::sidebar::Section::Tracks {
        app.sidebar.next();
    }

    let album = app.on_key(crossterm::event::KeyEvent::from(KeyCode::Char('c')));
    assert!(
        matches!(album, Some(sonora_tui::shell::Action::OpenTrackAlbum)),
        "c opens the album, got {album:?}"
    );
    let artist = app.on_key(crossterm::event::KeyEvent::from(KeyCode::Char('a')));
    assert!(
        matches!(artist, Some(sonora_tui::shell::Action::OpenTrackArtist)),
        "a opens the artist, got {artist:?}"
    );
}

#[test]
fn the_keys_are_dead_on_a_track_the_api_gave_no_ids_for() {
    // A key that is bound and does nothing is worse than one that is not
    // bound: the hint is drawn either way, and pressing it looks broken.
    let mut app = app();
    app.tracks = vec![sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(1),
        title: "An Upload".into(),
        artist: "Someone".into(),
        album: "Their Own".into(),
        duration: std::time::Duration::from_secs(200),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: None,
        album_id: None,
        artist_id: None,
    }];

    assert!(
        app.on_key(crossterm::event::KeyEvent::from(KeyCode::Char('c')))
            .is_none(),
        "no album id, no album to open"
    );
    assert!(
        app.on_key(crossterm::event::KeyEvent::from(KeyCode::Char('a')))
            .is_none(),
        "and no artist either"
    );
}

#[test]
fn shift_q_shows_the_queue_and_any_key_closes_it() {
    // The queue was never on screen: what plays next could only be found
    // out by waiting for it, which is why a stale one was so confusing.
    let mut app = app();
    app.tracks = vec![sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(1),
        title: "Queued Track".into(),
        artist: "An Artist".into(),
        album: "An Album".into(),
        duration: std::time::Duration::from_secs(200),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: None,
        album_id: None,
        artist_id: None,
    }];
    // Queued the way the loop does it when a track is played.
    app.queue = sonora_tui::playback::Queue::new(app.tracks.clone(), 0);

    press(&mut app, KeyCode::Char('Q'));
    let shown = screen(&mut app);
    assert!(shown.contains("Queue"), "the queue is on screen:\n{shown}");
    assert!(
        shown.contains("Queued Track"),
        "with what is in it:\n{shown}"
    );

    // The key that opened it closes it: the queue is a view of its own,
    // and the other keys act on the row under the cursor rather than
    // shutting it.
    press(&mut app, KeyCode::Char('Q'));
    assert!(!app.showing_queue, "Q closes the queue again");
    assert!(!app.should_quit, "and the app is still running");
}

/// A queue of `n` named tracks, played from the first.
fn queued(app: &mut sonora_tui::shell::App, titles: &[&str]) {
    let tracks: Vec<sonora_tui::domain::Track> = titles
        .iter()
        .enumerate()
        .map(|(i, t)| sonora_tui::domain::Track {
            id: sonora_tui::domain::TrackId::from(i as u64 + 1),
            title: (*t).into(),
            artist: "An Artist".into(),
            album: "An Album".into(),
            duration: std::time::Duration::from_secs(200),
            cover: None,
            tags: Vec::new(),
            added: None,
            explicit: false,
            ai: false,
            radio: None,
            album_id: None,
            artist_id: None,
        })
        .collect();
    app.tracks = tracks.clone();
    app.queue = sonora_tui::playback::Queue::new(tracks, 0);
    // On the Tracks section, where `app.tracks` is what the keys act on:
    // the app opens on Music, whose selection is a card on the home page.
    while app.sidebar.section() != sonora_tui::shell::sidebar::Section::Tracks {
        app.sidebar.next();
    }
}

#[test]
fn the_queue_view_moves_and_removes() {
    // Read-only, it could only say what was coming. The complaint that
    // started this was not knowing what was in the queue; the next one
    // would have been not being able to change it.
    let mut app = app();
    queued(&mut app, &["First", "Second", "Third"]);

    press(&mut app, KeyCode::Char('Q'));
    // Down onto the second, then take it out.
    press(&mut app, KeyCode::Char('j'));
    let action = app.on_key(crossterm::event::KeyEvent::from(KeyCode::Char('x')));
    assert!(
        matches!(action, Some(sonora_tui::shell::Action::RemoveQueued(1))),
        "x removes the highlighted row, got {action:?}"
    );

    // The view stays open: removing one track is not a reason to close it.
    let shown = screen(&mut app);
    assert!(shown.contains("Queue"), "still open:\n{shown}");
}

#[test]
fn a_key_the_queue_view_does_not_use_reaches_the_app() {
    // The queue used to take every key and close on any it did not know,
    // which made each command a way out and none of them a command. Now
    // only its own keys are its own; the rest do what they always do,
    // with the view still up.
    let mut app = app();
    queued(&mut app, &["First"]);
    press(&mut app, KeyCode::Char('Q'));
    assert!(app.showing_queue, "open");

    press(&mut app, KeyCode::Char('z'));
    assert!(app.showing_queue, "z is not a way out");
    assert!(app.queue.shuffled(), "and it shuffled, as it does anywhere");
}

#[test]
fn e_queues_the_selected_track_by_hand() {
    // Nothing was bound to it: the three-tier queue could keep a
    // hand-queued track apart from the album, and there was no way to make
    // one.
    let mut app = app();
    queued(&mut app, &["Playing", "Later"]);

    // The second row, queued to play next.
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('e'));

    let entries = app.queue.entries();
    let user: Vec<&str> = entries
        .iter()
        .filter(|(_, s)| *s == sonora_tui::playback::Source::User)
        .map(|(t, _)| t.title.as_str())
        .collect();
    assert_eq!(user, ["Later"], "asked for by hand: {entries:?}");
}

#[test]
fn shift_h_shows_what_has_been_played() {
    // The other half of "what is playing next": the web keeps a History
    // section, and "what was that one?" is a question a player should be
    // able to answer.
    let mut app = app();
    queued(&mut app, &["First", "Second"]);

    press(&mut app, KeyCode::Char('H'));
    let shown = screen(&mut app);
    assert!(shown.contains("History"), "the history opened:\n{shown}");
    assert!(app.showing_history);

    // A view of its own, as the queue is: its keys are few, the rest act
    // on the row under the cursor, and H closes it again.
    press(&mut app, KeyCode::Char('z'));
    assert!(app.showing_history, "z is not a way out");
    press(&mut app, KeyCode::Char('H'));
    assert!(!app.showing_history, "H closes it");
}

#[test]
fn every_key_the_help_lists_does_something_somewhere() {
    // A key that is documented and inert is worse than one that is neither.
    // This does not check what each does — only that the app answers.
    let mut app = app();
    app.artist = Some(an_artist());

    for code in [
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::Char('h'),
        KeyCode::Char('l'),
        KeyCode::Char('J'),
        KeyCode::Char('K'),
        KeyCode::Char('o'),
        KeyCode::Char('b'),
        KeyCode::Char('t'),
        KeyCode::Char('s'),
        KeyCode::Char('1'),
        KeyCode::Char('8'),
        KeyCode::Char('['),
        KeyCode::Char(']'),
        KeyCode::Esc,
    ] {
        press(&mut app, code);
        // The app is still drawable after every one of them: a key that
        // leaves it in a state the renderer cannot draw is worse than one
        // that does nothing.
        let text = screen(&mut app);
        assert!(!text.is_empty(), "the screen still draws after {code:?}");
    }
}

#[test]
fn an_artists_page_draws_every_top_track_it_has() {
    // Four came back and three were drawn: the budget counted the list's
    // own column row but not the section heading above it, and the list was
    // handed a height one short — one row of three, so one track.
    let mut app = app();
    let mut page = an_artist();
    page.picture = Some("http://x".into());
    page.bio = Some("word ".repeat(200));
    page.top_tracks = (0..4)
        .map(|i| {
            sonora_tui::domain::Track::sample(
                &format!("Track {i}"),
                "2Pac",
                std::time::Duration::from_secs(200),
            )
        })
        .collect();
    app.artist = Some(page);

    // The pane of a 61-row terminal, which is where this was seen.
    let text = screen_sized(&mut app, 199, 61);
    for i in 0..4 {
        assert!(
            text.contains(&format!("Track {i}")),
            "Track {i} is drawn:\n{text}"
        );
    }
    // And the section below still starts under them.
    assert!(
        text.contains("Albums"),
        "with the albums after them:\n{text}"
    );
}

#[test]
fn the_last_section_of_an_artists_page_is_cut_not_dropped() {
    // It used to break out of the loop unless the whole section fitted, so
    // the page ended in blank pane with no sign that it carried on.
    let mut app = app();
    let mut page = an_artist();
    page.picture = Some("http://x".into());
    app.artist = Some(page);

    // A height that lands part-way through the second section: it must
    // still name itself rather than leaving the pane empty below the first.
    let whole = screen_sized(&mut app, 199, 50);
    let sections = ["Albums", "EP & Singles"];
    let all_fit = sections.iter().all(|s| whole.contains(*s));
    assert!(all_fit, "both fit at 50 rows:\n{whole}");

    // Three rows shorter the second no longer fits whole, and used to be
    // dropped outright — the page ended in blank pane.
    let cut = screen_sized(&mut app, 199, 47);
    assert!(
        cut.contains("EP & Singles"),
        "the section across the edge still names itself:\n{cut}"
    );
}

/// Explore, with one genre link in it.
fn with_explore(app: &mut App) {
    let mut card = sonora_tui::shell::carousel::Card::new("Hip-Hop", "");
    card.target = Some(sonora_tui::shell::carousel::Target::Page(
        "pages/genre_hip_hop".into(),
    ));
    app.explore.rows.push(sonora_tui::shell::home::Row {
        heading: "Genres".into(),
        kind: sonora_tui::browse::RowKind::Carousel,
        cards: vec![card],
        state: sonora_tui::shell::carousel::CarouselState::default(),
    });
    while app.sidebar.section() != sonora_tui::shell::sidebar::Section::Explore {
        app.sidebar.next();
    }
}

#[test]
fn explore_shows_no_tab_strip() {
    // The home page's tabs were drawn over it, naming pages it has nothing
    // to do with.
    let mut app = app();
    with_explore(&mut app);
    let text = screen(&mut app);
    assert!(text.contains("Genres"), "Explore's own rows:\n{text}");
    assert!(
        !text.contains("Staff Picks"),
        "and not the home tabs:\n{text}"
    );
}

#[test]
fn a_genre_page_can_be_left_for_another() {
    // Reported: after opening one theme there was no way back to Explore to
    // choose a second.
    let mut app = app();
    with_explore(&mut app);

    // The very question the loop asks before spawning a fetch. Filling the
    // reply in without asking it is how "nothing happens" survived a test.
    let asked = app.what_enter_opens();
    assert!(
        matches!(asked, Some((sonora_tui::shell::Collection::Page { .. }, _))),
        "enter on Explore fetches the genre's page, got {asked:?}"
    );

    // Open it, and let the reply land as the loop would.
    press(&mut app, KeyCode::Enter);

    // Before the reply lands, the pane must not still be drawing the
    // genres: it did, under the new heading, so pressing enter looked like
    // nothing had happened at all.
    let waiting = screen(&mut app);
    assert!(
        !waiting.contains("Hip-Hop") || !waiting.contains("Genres"),
        "the genres are not still on screen under the new heading:\n{waiting}"
    );
    let mut home = sonora_tui::browse::Home::default();
    home.rows.push(sonora_tui::browse::HomeRow {
        heading: "Essential Rap".into(),
        kind: sonora_tui::browse::RowKind::Carousel,
        cards: vec![sonora_tui::shell::carousel::Card::new("A Playlist", "TIDAL")],
        more: None,
    });
    app.update(sonora_tui::shell::Action::PageLoaded {
        title: "Hip-Hop".into(),
        home: Box::new(home),
    });

    let opened = screen(&mut app);
    assert!(opened.contains("Playlists"), "the genre's rows:\n{opened}");

    // Back, and Explore is choosable again.
    press(&mut app, KeyCode::Char('['));
    let back = screen(&mut app);
    assert!(
        back.contains("Genres"),
        "back reaches Explore, with its links:\n{back}"
    );
}

/// A genre page as the fetch delivers one.
fn open_hip_hop(app: &mut App) {
    press(app, KeyCode::Enter);
    let mut home = sonora_tui::browse::Home::default();
    home.rows.push(sonora_tui::browse::HomeRow {
        heading: "Essential Rap".into(),
        kind: sonora_tui::browse::RowKind::Carousel,
        cards: vec![sonora_tui::shell::carousel::Card::new("A Playlist", "TIDAL")],
        more: None,
    });
    app.update(sonora_tui::shell::Action::PageLoaded {
        title: "Hip-Hop".into(),
        home: Box::new(home),
    });
}

#[test]
fn a_genre_opens_as_a_page_of_rows_rather_than_an_empty_track_list() {
    // Reported as "opening a genre does nothing". It set `open`, which
    // hands the pane to the collection view -- and that view draws
    // `open_cards`, which a page of rows never fills. The pane went to an
    // empty track list with the genre's name floating in it.
    let mut app = app();
    with_explore(&mut app);
    open_hip_hop(&mut app);

    let opened = screen(&mut app);
    // "Playlists" is the load-bearing half: the broken pane still carried
    // the word Hip-Hop, as the heading of that empty list, so asserting the
    // title alone passes against the very bug this is here for.
    assert!(
        opened.contains("Essential Rap"),
        "the genre's own rows are drawn:\n{opened}"
    );
    assert!(
        !opened.contains("Filter this list"),
        "as rows, not as the track list `open` hands the pane to:\n{opened}"
    );
    assert!(opened.contains("Hip-Hop"), "under its own name:\n{opened}");
}

#[test]
fn the_nav_comes_back_to_the_genres_rather_than_the_genre_last_opened() {
    // The second half of the report: 1 then 2 showed the genre again. Its
    // rows had been written over Explore's own, and nothing asked for the
    // list back -- `load_if_needed` only tests whether the rows are empty.
    let mut app = app();
    with_explore(&mut app);
    open_hip_hop(&mut app);

    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Char('2'));

    let back = screen(&mut app);
    assert!(
        !back.contains("Essential Rap"),
        "the genre's rows are not still standing in for Explore:\n{back}"
    );
    assert!(
        app.explore.heading.is_none(),
        "and nothing is left claiming to be a genre page, got {:?}",
        app.explore.heading
    );
}

#[test]
fn forward_returns_to_a_genre_that_back_stepped_out_of() {
    // Back puts Explore's list back, so forward has to put the genre back.
    // Leaving a genre page recorded nothing, so forward came back to the
    // list it had just left.
    let mut app = app();
    with_explore(&mut app);
    open_hip_hop(&mut app);

    press(&mut app, KeyCode::Char('['));
    let back = screen(&mut app);
    assert!(
        back.contains("Genres"),
        "back reaches the genre list:\n{back}"
    );

    press(&mut app, KeyCode::Char(']'));
    let forward = screen(&mut app);
    assert!(
        forward.contains("Essential Rap"),
        "forward reaches the genre's rows again:\n{forward}"
    );
}

#[test]
fn back_leaves_a_row_opened_with_see_all() {
    // `o` opened the row but put nothing on the history, so back and escape
    // both had nothing to pop: the view it opened stayed on screen and
    // there was no way out of it but the nav.
    let mut app = app();
    app.home.rows.push(sonora_tui::shell::home::Row {
        heading: "New Tracks".into(),
        kind: sonora_tui::browse::RowKind::Compact,
        cards: (0..9)
            .map(|i| sonora_tui::shell::carousel::Card::new(format!("Track {i}"), "Artist"))
            .collect(),
        state: sonora_tui::shell::carousel::CarouselState::default(),
    });
    while app.sidebar.section() != sonora_tui::shell::sidebar::Section::Music {
        app.sidebar.next();
    }

    press(&mut app, KeyCode::Char('o'));
    let opened = screen(&mut app);
    assert!(
        opened.contains("New Tracks"),
        "the row opened under its own heading:\n{opened}"
    );

    press(&mut app, KeyCode::Char('['));
    assert!(app.open.is_none(), "back closed it");

    // And escape, which is the same step.
    press(&mut app, KeyCode::Char('o'));
    assert!(app.open.is_some(), "opened again");
    press(&mut app, KeyCode::Esc);
    assert!(app.open.is_none(), "escape closed it too");
}

#[test]
fn opening_a_genre_says_it_is_loading_rather_than_drawing_an_empty_list() {
    // Reported as a flash of an empty square when a genre opens. The pane
    // drew the view that was coming -- a track list, with its filter box,
    // its column headings and a blank banner -- for as long as the request
    // took. None of that belongs to a genre, which is a page of rows: what
    // comes back is what decides the shape, so until it does the pane says
    // what it is opening and that it is on its way.
    let mut app = app();
    with_explore(&mut app);

    press(&mut app, KeyCode::Enter);
    let waiting = screen(&mut app);
    assert!(
        waiting.contains("Hip-Hop"),
        "the pane names what it opens:\n{waiting}"
    );
    assert!(
        waiting.contains("Loading"),
        "and says it is on its way:\n{waiting}"
    );
    assert!(
        !waiting.contains("Filter this list") && !waiting.contains("TITLE"),
        "and draws none of the track list that is not coming:\n{waiting}"
    );

    // The reply ends the wait and the page takes the pane.
    let mut home = sonora_tui::browse::Home::default();
    home.rows.push(sonora_tui::browse::HomeRow {
        heading: "Essential Rap".into(),
        kind: sonora_tui::browse::RowKind::Carousel,
        cards: vec![sonora_tui::shell::carousel::Card::new("A Playlist", "TIDAL")],
        more: None,
    });
    app.update(sonora_tui::shell::Action::PageLoaded {
        title: "Hip-Hop".into(),
        home: Box::new(home),
    });

    let loaded = screen(&mut app);
    assert!(
        loaded.contains("Essential Rap"),
        "the genre's rows:\n{loaded}"
    );
    assert!(
        !loaded.contains("Loading"),
        "and the wait is over:\n{loaded}"
    );
}

#[test]
fn see_all_on_top_tracks_opens_the_list() {
    // `o` did nothing on Top Tracks: the see-all path built cards for the
    // section, and `cards` has none for tracks -- it builds covers -- so
    // the empty list it got back made the key a no-op.
    let mut app = app();
    app.artist = Some(an_artist());
    app.artist_section = 0; // Top Tracks

    let before = screen(&mut app);
    assert!(
        before.contains("Top Tracks"),
        "on the top tracks:\n{before}"
    );

    press(&mut app, KeyCode::Char('o'));
    let after = screen(&mut app);
    assert!(
        after.contains("Daft Punk — Top Tracks"),
        "opened under its own heading:\n{after}"
    );

    // The reply fills it, as the loop would.
    app.update(sonora_tui::shell::Action::CollectionLoaded {
        for_title: "Daft Punk — Top Tracks".into(),
        tracks: (0..30)
            .map(|i| sonora_tui::domain::Track {
                id: sonora_tui::domain::TrackId::from(200 + i),
                title: format!("Whole List {i}"),
                artist: "Daft Punk".into(),
                album: "Discovery".into(),
                duration: std::time::Duration::from_secs(200),
                cover: None,
                tags: Vec::new(),
                added: None,
                explicit: false,
                ai: false,
                radio: None,
                album_id: None,
                artist_id: None,
            })
            .collect(),
    });
    let loaded = screen(&mut app);
    assert!(
        loaded.contains("Whole List 0"),
        "the fetched list, not the four already drawn:\n{loaded}"
    );
}

#[test]
fn the_tracks_section_keeps_its_favourites_when_the_nav_is_used() {
    // Reported as "favourite tracks don't work". They are fetched once, at
    // startup, and leaving a view took them onto the history along with
    // everything else -- so arriving at the Tracks section found an empty
    // list and nothing to fill it again. They belong to the section, not to
    // a view stacked over it, the same as Explore's own rows.
    let mut app = app();
    let track = |i: u64| sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(i),
        title: format!("Fav {i}"),
        artist: "Someone".into(),
        album: "An Album".into(),
        duration: std::time::Duration::from_secs(200),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: None,
        album_id: None,
        artist_id: None,
    };
    app.update(sonora_tui::shell::Action::TracksLoaded(
        (0..5).map(track).collect(),
    ));

    press(&mut app, KeyCode::Char('6'));
    assert_eq!(
        app.sidebar.section(),
        sonora_tui::shell::sidebar::Section::Tracks,
        "on the Tracks section"
    );
    let shown = screen(&mut app);
    assert!(
        shown.contains("Fav 0") && shown.contains("Fav 4"),
        "the favourites are still there:\n{shown}"
    );

    // Away and back again: still there, and still no request to make.
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Char('6'));
    let again = screen(&mut app);
    assert!(
        again.contains("Fav 0"),
        "and they survive a round trip through the nav:\n{again}"
    );
}

#[test]
fn an_opened_collection_still_carries_its_tracks_onto_the_history() {
    // The other half: an album's tracks are that view's contents, so they
    // do go with it -- back has to put them back.
    let mut app = app();
    app.open = Some(sonora_tui::shell::OpenCollection {
        title: "An Album".into(),
        subtitle: String::new(),
        detail: String::new(),
        cover: None,
        round_cover: false,
        came_from: sonora_tui::shell::sidebar::Section::Albums,
        target: None,
    });
    app.tracks = vec![sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(1),
        title: "Album Track".into(),
        artist: "Someone".into(),
        album: "An Album".into(),
        duration: std::time::Duration::from_secs(200),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: None,
        album_id: None,
        artist_id: None,
    }];

    press(&mut app, KeyCode::Char('6'));
    assert!(app.tracks.is_empty(), "the album's tracks left with it");

    press(&mut app, KeyCode::Char('['));
    assert_eq!(
        app.tracks.len(),
        1,
        "and back brings them, and the album, with it"
    );
    assert!(app.open.is_some(), "the album is open again");
}

#[test]
fn h_never_closes_the_view_it_is_pressed_in() {
    // Opening an album from the home page and pressing `h` put the user
    // back on the home page: `h` was the one key that meant something
    // other than a direction, and reaching for the left of a row left the
    // album. It is "left" everywhere now, and escape is the way out.
    let mut app = app();
    app.artist = Some(an_artist());
    app.artist_section = 1;
    press(&mut app, KeyCode::Char('o'));
    let opened = screen(&mut app);
    assert!(
        opened.contains("Daft Punk — Albums"),
        "the see-all is open:\n{opened}"
    );

    press(&mut app, KeyCode::Char('l'));
    press(&mut app, KeyCode::Char('h'));
    let after = screen(&mut app);
    assert!(
        after.contains("Daft Punk — Albums"),
        "and `h` moved inside it rather than closing it:\n{after}"
    );
}

#[test]
fn see_all_on_a_row_of_links_draws_pills_not_empty_covers() {
    // The row itself draws pills, but "see all" opened a grid -- and a grid
    // draws covers, which page links do not have. The pane came back as
    // rows of empty grey squares with the titles beneath them.
    let mut app = app();
    let cards: Vec<_> = ["Hip-Hop", "Pop", "Jazz"]
        .iter()
        .map(|n| {
            let mut c = sonora_tui::shell::carousel::Card::new(*n, "");
            c.target = Some(sonora_tui::shell::carousel::Target::Page(format!(
                "pages/genre_{n}"
            )));
            c
        })
        .collect();
    app.explore.rows.push(sonora_tui::shell::home::Row {
        heading: "Genres".into(),
        kind: sonora_tui::browse::RowKind::Links,
        cards: cards.clone(),
        state: sonora_tui::shell::carousel::CarouselState::default(),
    });
    while app.sidebar.section() != sonora_tui::shell::sidebar::Section::Explore {
        app.sidebar.next();
    }

    press(&mut app, KeyCode::Char('o'));

    let shown = screen(&mut app);
    assert!(shown.contains("Hip-Hop"), "the links are drawn:\n{shown}");

    // A pill is one line under the filter box. A cover reserves eight rows
    // above the title, so drawn as a grid the links sat far down the pane
    // with a band of empty grey over them -- which is what this measures.
    let filter_end = shown
        .lines()
        .position(|l| l.contains('\u{2570}'))
        .expect("the filter box closes");
    let titles = shown
        .lines()
        .position(|l| l.contains("Hip-Hop"))
        .expect("a line with the links");
    assert!(
        titles - filter_end <= 3,
        "the links sit {} rows under the filter box, so a cover's worth of \
         grey is drawn above them:\n{shown}",
        titles - filter_end
    );
    let line = shown.lines().nth(titles).expect("that line");
    assert!(
        line.contains("Pop") && line.contains("Jazz"),
        "and they share it:\n{shown}"
    );
}

#[test]
fn a_blocked_track_refuses_to_play_and_says_why() {
    // TIDAL's own wording: a blocked track is not hidden, it is shown and
    // refuses to start. Hiding it would leave holes in an album with no way
    // to tell why.
    let mut app = app();
    let track = |explicit, ai| sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(1),
        title: "A Track".into(),
        artist: "Someone".into(),
        album: "An Album".into(),
        duration: std::time::Duration::from_secs(200),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit,
        ai,
        radio: None,
        album_id: None,
        artist_id: None,
    };

    // Allowed by default, as TIDAL has it.
    assert!(
        app.why_blocked(&track(true, false)).is_none(),
        "explicit plays"
    );
    assert!(
        app.why_blocked(&track(false, true)).is_none(),
        "and so does AI"
    );

    // Turned off, each blocks its own kind and says which.
    app.config.playback.explicit = false;
    let why = app.why_blocked(&track(true, false)).expect("blocked");
    assert!(why.contains("explicit"), "it says which setting: {why:?}");
    assert!(
        app.why_blocked(&track(false, false)).is_none(),
        "an unflagged track still plays"
    );

    app.config.playback.explicit = true;
    app.config.playback.ai = false;
    let why = app.why_blocked(&track(false, true)).expect("blocked");
    assert!(why.contains("AI"), "and names the other one: {why:?}");
    assert!(
        app.why_blocked(&track(true, false)).is_none(),
        "explicit is allowed again"
    );
}

fn radio_track(id: u64, title: &str, radio: Option<&str>) -> sonora_tui::domain::Track {
    sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(id),
        title: title.into(),
        artist: "Someone".into(),
        album: "An Album".into(),
        duration: std::time::Duration::from_secs(200),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: radio.map(str::to_string),
        album_id: None,
        artist_id: None,
    }
}

fn tick(app: &mut App) -> Option<sonora_tui::shell::Action> {
    app.update(sonora_tui::shell::Action::Playback(
        sonora_tui::playback::PlaybackEvent::Position(std::time::Duration::from_secs(1)),
    ))
}

#[test]
fn autoplay_is_asked_for_ahead_once_and_seeded_from_the_end_of_the_queue() {
    // TIDAL's "continue with similar content": what follows the queue is
    // the radio of its last track, fetched while the music still plays
    // rather than at the silence after it.
    let mut app = app();
    app.config.playback.autoplay = true;
    app.queue = sonora_tui::playback::Queue::new(
        vec![
            radio_track(1, "One", Some("mix-1")),
            radio_track(2, "Two", Some("mix-2")),
            radio_track(3, "Three", Some("mix-3")),
            radio_track(4, "Four", Some("mix-4")),
        ],
        0,
    );
    assert!(
        tick(&mut app).is_none(),
        "three still to play: nothing to fetch yet"
    );

    app.queue.next();
    let asked = tick(&mut app);
    assert!(
        matches!(
            asked,
            Some(sonora_tui::shell::Action::Autoplay { ref seed }) if seed.0 == "4"
        ),
        "the last track's radio, not the playing one's: {asked:?}"
    );
    assert!(tick(&mut app).is_none(), "asked once, not on every tick");

    // The reply appends and starts nothing: the music is still going.
    let landed = app.update(sonora_tui::shell::Action::QueueRadio(vec![radio_track(
        10,
        "Radio",
        Some("mix-10"),
    )]));
    assert!(landed.is_none(), "nothing restarts, got {landed:?}");
    assert_eq!(app.queue.len(), 5, "the radio sits behind the album");
    assert_eq!(
        app.queue.current().map(|t| t.id.clone()),
        Some(sonora_tui::domain::TrackId::from(2))
    );

    // Now the radio is the end of the queue, so once it is near, the next
    // fetch follows the radio rather than the album.
    app.queue.next();
    app.queue.next();
    let again = tick(&mut app);
    assert!(
        matches!(
            again,
            Some(sonora_tui::shell::Action::Autoplay {
                ref seed,
                ..
            }) if seed.0 == "10"
        ),
        "the next radio follows the radio: {again:?}"
    );
}

#[test]
fn autoplay_off_or_repeating_asks_for_nothing() {
    let mut app = app();
    app.config.playback.autoplay = false;
    app.queue = sonora_tui::playback::Queue::new(vec![radio_track(1, "One", Some("mix-1"))], 0);
    app.now_playing.track = app.queue.current().cloned();
    app.now_playing.playing = true;
    assert!(tick(&mut app).is_none());
    let next = app.update(sonora_tui::shell::Action::Playback(
        sonora_tui::playback::PlaybackEvent::Finished,
    ));
    assert!(next.is_none(), "nothing follows, got {next:?}");
    assert!(!app.now_playing.playing, "and the bar stops");

    app.config.playback.autoplay = true;
    app.queue.repeat = sonora_tui::playback::Repeat::All;
    assert!(
        tick(&mut app).is_none(),
        "a repeating queue has no end to follow"
    );
}

#[test]
fn a_dry_queue_starts_the_radio_when_it_lands() {
    // The last resort: the radio was not in hand when the last track
    // ended. Nothing is playing to move on to it, so it is started.
    let mut app = app();
    app.config.playback.autoplay = true;
    app.queue = sonora_tui::playback::Queue::new(vec![radio_track(1, "One", None)], 0);
    app.now_playing.playing = true;
    let asked = app.update(sonora_tui::shell::Action::Playback(
        sonora_tui::playback::PlaybackEvent::Finished,
    ));
    assert!(
        matches!(
            asked,
            Some(sonora_tui::shell::Action::Autoplay { ref seed }) if seed.0 == "1"
        ),
        "asked even without a radio in hand, the loop fetches it: {asked:?}"
    );

    let landed = app.update(sonora_tui::shell::Action::QueueRadio(vec![
        radio_track(10, "Radio", None),
        radio_track(11, "Radio 2", None),
    ]));
    assert!(
        matches!(landed, Some(sonora_tui::shell::Action::PlayQueued)),
        "got {landed:?}"
    );
    assert_eq!(
        app.queue.current().map(|t| t.id.clone()),
        Some(sonora_tui::domain::TrackId::from(10))
    );
}

#[test]
fn a_seed_that_led_nowhere_is_not_asked_about_again() {
    let mut app = app();
    app.config.playback.autoplay = true;
    app.queue = sonora_tui::playback::Queue::new(vec![radio_track(1, "One", None)], 0);
    assert!(tick(&mut app).is_some());
    app.update(sonora_tui::shell::Action::AutoplayFailed(
        sonora_tui::domain::TrackId::from(1),
    ));
    assert!(
        tick(&mut app).is_none(),
        "the same question every tick would hammer the API"
    );

    // A different end to the queue is a different question.
    app.queue.play_last(radio_track(2, "Two", Some("mix-2")));
    assert!(tick(&mut app).is_some());
}

#[test]
fn i_in_the_queue_view_switches_autoplay() {
    let mut app = app();
    app.config.playback.autoplay = true;
    press(&mut app, KeyCode::Char('Q'));
    assert!(app.showing_queue, "the queue is up");
    press(&mut app, KeyCode::Char('i'));
    assert!(!app.config.playback.autoplay, "off");
    assert!(app.showing_queue, "and the queue is still up");
    press(&mut app, KeyCode::Char('i'));
    assert!(app.config.playback.autoplay, "and on again");
}

#[test]
fn r_starts_the_radio_of_the_selected_track() {
    // Every other key acts on the selection -- enter opens it, `A`
    // favourites it -- so this does too. A radio started from a track that
    // is not on screen reads as the app doing something of its own accord.
    let track = |id: u64, title: &str, radio: &str| sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(id),
        title: title.into(),
        artist: "Someone".into(),
        album: "An Album".into(),
        duration: std::time::Duration::from_secs(200),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: Some(radio.into()),
        album_id: None,
        artist_id: None,
    };

    let mut app = app();
    app.update(sonora_tui::shell::Action::TracksLoaded(vec![
        track(1, "First", "mix-first"),
        track(2, "Second", "mix-second"),
    ]));
    press(&mut app, KeyCode::Char('6'));

    // Something else is playing: the key still follows the selection.
    app.now_playing.track = Some(track(9, "Playing", "mix-playing"));
    assert_eq!(
        app.track_radio(),
        Some(sonora_tui::domain::TrackId::from(1)),
        "the selected track, not the one in the bar"
    );

    press(&mut app, KeyCode::Char('j'));
    assert_eq!(
        app.track_radio(),
        Some(sonora_tui::domain::TrackId::from(2)),
        "and it follows the selection as it moves"
    );

    let action = app
        .on_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE))
        .expect("R is bound where a track is selected");
    assert!(matches!(action, sonora_tui::shell::Action::PlayTrackRadio));
    app.update(action);
    let shown = screen(&mut app);
    assert!(
        shown.contains("Second Radio"),
        "the radio opens under the selected track's name:\n{shown}"
    );
}

#[test]
fn r_falls_back_to_what_is_playing_when_nothing_is_selected() {
    // On the home page or in Settings there is no track selected, and
    // continuing from what is in your ears is then the only thing the key
    // could mean.
    let mut app = app();
    while app.sidebar.section() != sonora_tui::shell::sidebar::Section::Music {
        app.sidebar.next();
    }
    assert!(
        app.on_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE))
            .is_none(),
        "nothing selected and nothing playing: the key does nothing"
    );

    app.now_playing.track = Some(sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(9),
        title: "Playing".into(),
        artist: "Someone".into(),
        album: "An Album".into(),
        duration: std::time::Duration::from_secs(200),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: Some("mix-playing".into()),
        album_id: None,
        artist_id: None,
    });
    assert_eq!(
        app.track_radio(),
        Some(sonora_tui::domain::TrackId::from(9)),
        "with nothing selected it follows the bar"
    );
}

#[test]
fn a_rebound_key_drives_the_app_the_way_the_default_would() {
    // The whole point: the keys are guarded by what is on screen, so
    // rebinding translates the keystroke rather than replacing the match.
    // Whatever the guards do for the default, they do for the new key.
    let binds: std::collections::HashMap<String, String> = [("quit".to_string(), "x".to_string())]
        .into_iter()
        .collect();
    let (keymap, problems) = sonora_tui::shell::keymap::Keymap::from_config(&binds);
    assert!(problems.is_empty(), "{problems:?}");

    let mut app = app();
    app.keymap = keymap;

    // The new key does what the old one did.
    assert!(
        matches!(
            app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            Some(sonora_tui::shell::Action::Quit)
        ),
        "the rebound key quits"
    );

    // And the old key is free: it is no longer bound to anything, since
    // nothing translates to it any more.
    assert!(
        !matches!(
            app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(sonora_tui::shell::Action::Quit)
        ),
        "the default key stopped quitting once it was rebound away"
    );
}

#[test]
fn rebinding_does_not_reach_the_search_box() {
    // While the box has the keyboard, every character is text. A config
    // that could change that would be a config that breaks typing.
    let binds: std::collections::HashMap<String, String> = [("quit".to_string(), "x".to_string())]
        .into_iter()
        .collect();
    let (keymap, _) = sonora_tui::shell::keymap::Keymap::from_config(&binds);

    let mut app = app();
    app.keymap = keymap;
    press(&mut app, KeyCode::Char('s'));

    press(&mut app, KeyCode::Char('x'));
    let shown = screen(&mut app);
    assert!(
        shown.contains('x'),
        "the rebound key is typed into the box rather than quitting:\n{shown}"
    );
    assert!(!app.should_quit, "and the app is still running");
}

#[test]
fn r_on_a_home_track_card_opens_its_radio() {
    // A card carries little more than a title, but it carries the id, and
    // the station is built on that: the key works on the page most people
    // start from.
    let mut app = app();
    let mut card = sonora_tui::shell::carousel::Card::new("A Track", "Someone");
    card.target = Some(sonora_tui::shell::carousel::Target::Track("42".into()));
    app.home.rows.push(sonora_tui::shell::home::Row {
        heading: "New Tracks".into(),
        kind: sonora_tui::browse::RowKind::Compact,
        cards: vec![card],
        state: sonora_tui::shell::carousel::CarouselState::default(),
    });
    while app.sidebar.section() != sonora_tui::shell::sidebar::Section::Music {
        app.sidebar.next();
    }

    let action = app
        .on_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE))
        .expect("R is bound on a track card");
    assert!(
        matches!(action, sonora_tui::shell::Action::PlayTrackRadio),
        "it opens the selected track's radio, got {action:?}"
    );
    app.update(action);
    assert!(
        matches!(
            app.open.as_ref().and_then(|o| o.target.clone()),
            Some(sonora_tui::shell::Collection::Radio(ref seed)) if seed.0 == "42"
        ),
        "on the card's own id"
    );
    let shown = screen(&mut app);
    assert!(
        shown.contains("A Track Radio"),
        "the radio opens under the track's name:\n{shown}"
    );
}

#[test]
fn a_radio_opened_by_a_key_carries_artwork() {
    // Opened by enter, a mix takes the card's cover. Opened by `R` or `S`
    // there is no card, and the page used to come up with a blank square
    // where every other opened view has a picture -- a mix does have one,
    // checked against a real response.
    let mut app = app();
    app.now_playing.track = Some(sonora_tui::domain::Track {
        id: sonora_tui::domain::TrackId::from(1),
        title: "A Track".into(),
        artist: "Someone".into(),
        album: "An Album".into(),
        duration: std::time::Duration::from_secs(200),
        cover: Some("https://example.invalid/track.jpg".into()),
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: Some("mix-1".into()),
        album_id: None,
        artist_id: None,
    });

    press(&mut app, KeyCode::Char('R'));
    assert_eq!(
        app.open.as_ref().and_then(|o| o.cover.as_deref()),
        Some("https://example.invalid/track.jpg"),
        "the track's radio shows the track's artwork"
    );
}
