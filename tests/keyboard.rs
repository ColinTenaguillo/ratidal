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
use ratidal::shell::{self, App};

/// An app with a session, so the keys are live.
fn app() -> App {
    App {
        session: Some(ratidal::auth::StoredToken::default()),
        // Nothing here may write the user's real files.
        token_path: None,
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
fn an_artist() -> ratidal::library::ArtistPage {
    let album = |i: u64, title: &str| ratidal::library::Album {
        id: i,
        title: title.into(),
        artist: "Daft Punk".into(),
        year: Some("2001".into()),
        cover: None,
        track_count: 10,
        duration: None,
    };
    // As the live page comes back: a radio, and the four or five top
    // tracks TIDAL returns.
    ratidal::library::ArtistPage {
        name: "Daft Punk".into(),
        albums: (0..12).map(|i| album(i, &format!("Album {i}"))).collect(),
        singles: (0..12).map(|i| album(20 + i, &format!("Single {i}"))).collect(),
        top_tracks: (0..4)
            .map(|i| ratidal::domain::Track {
                id: ratidal::domain::TrackId(100 + i),
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
    let rows_of_albums = after
        .lines()
        .filter(|l| l.contains("Album "))
        .count();
    assert!(
        rows_of_albums > 1,
        "laid out as a grid, not the one row the section showed:\n{after}"
    );
}


#[test]
fn the_artist_pages_hint_names_the_key_that_works() {
    // The hint said `R` for a while after `R` had been given to the
    // selected track's radio -- a key nobody can see is a key nobody uses,
    // and one that is advertised wrongly is worse.
    let mut app = app();
    app.artist = Some(an_artist());

    let shown = screen(&mut app);
    let hint = shown
        .lines()
        .find(|l| l.contains("for radio"))
        .expect("the page offers its radio")
        .to_string();

    // Whichever key it names has to be the one that opens the radio.
    let key = hint
        .split_whitespace()
        .find(|w| w.len() == 1 && w.chars().all(char::is_alphabetic))
        .and_then(|w| w.chars().next())
        .unwrap_or_else(|| panic!("the hint names a key: {hint}"));

    let acted = app.on_key(crossterm::event::KeyEvent::from(KeyCode::Char(key)));
    assert!(
        matches!(acted, Some(ratidal::shell::Action::PlayArtistRadio)),
        "the hint says {key:?}, which does {acted:?}"
    );
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
        KeyCode::Char('9'),
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
            ratidal::domain::Track::sample(
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
    assert!(text.contains("Albums"), "with the albums after them:\n{text}");
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
    let mut card = ratidal::shell::carousel::Card::new("Hip-Hop", "");
    card.target = Some(ratidal::shell::carousel::Target::Page(
        "pages/genre_hip_hop".into(),
    ));
    app.explore.rows.push(ratidal::shell::home::Row {
        heading: "Genres".into(),
        kind: ratidal::browse::RowKind::Carousel,
        cards: vec![card],
        state: ratidal::shell::carousel::CarouselState::default(),
        more: None,
    });
    while app.sidebar.section() != ratidal::shell::sidebar::Section::Explore {
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
    assert!(!text.contains("Staff Picks"), "and not the home tabs:\n{text}");
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
        matches!(
            asked,
            Some((ratidal::shell::Collection::Page { .. }, _))
        ),
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
    let mut home = ratidal::browse::Home::default();
    home.rows.push(ratidal::browse::HomeRow {
        heading: "Essential Rap".into(),
        kind: ratidal::browse::RowKind::Carousel,
        cards: vec![ratidal::shell::carousel::Card::new("A Playlist", "TIDAL")],
        more: None,
    });
    app.update(ratidal::shell::Action::PageLoaded {
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
    let mut home = ratidal::browse::Home::default();
    home.rows.push(ratidal::browse::HomeRow {
        heading: "Essential Rap".into(),
        kind: ratidal::browse::RowKind::Carousel,
        cards: vec![ratidal::shell::carousel::Card::new("A Playlist", "TIDAL")],
        more: None,
    });
    app.update(ratidal::shell::Action::PageLoaded {
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
    assert!(back.contains("Genres"), "back reaches the genre list:\n{back}");

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
    app.home.rows.push(ratidal::shell::home::Row {
        heading: "New Tracks".into(),
        kind: ratidal::browse::RowKind::Tracks,
        cards: (0..9)
            .map(|i| ratidal::shell::carousel::Card::new(format!("Track {i}"), "Artist"))
            .collect(),
        state: ratidal::shell::carousel::CarouselState::default(),
        more: Some("pages/data/new-tracks".into()),
    });
    while app.sidebar.section() != ratidal::shell::sidebar::Section::Music {
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
    assert!(waiting.contains("Hip-Hop"), "the pane names what it opens:\n{waiting}");
    assert!(waiting.contains("Loading"), "and says it is on its way:\n{waiting}");
    assert!(
        !waiting.contains("Filter this list") && !waiting.contains("TITLE"),
        "and draws none of the track list that is not coming:\n{waiting}"
    );

    // The reply ends the wait and the page takes the pane.
    let mut home = ratidal::browse::Home::default();
    home.rows.push(ratidal::browse::HomeRow {
        heading: "Essential Rap".into(),
        kind: ratidal::browse::RowKind::Carousel,
        cards: vec![ratidal::shell::carousel::Card::new("A Playlist", "TIDAL")],
        more: None,
    });
    app.update(ratidal::shell::Action::PageLoaded {
        title: "Hip-Hop".into(),
        home: Box::new(home),
    });

    let loaded = screen(&mut app);
    assert!(loaded.contains("Essential Rap"), "the genre's rows:\n{loaded}");
    assert!(!loaded.contains("Loading"), "and the wait is over:\n{loaded}");
}

#[test]
fn see_all_on_top_tracks_fetches_the_whole_list() {
    // `o` did nothing on Top Tracks: the see-all path built cards for the
    // section, and `cards` has none for tracks -- it builds covers -- so
    // the empty list it got back made the key a no-op.
    //
    // And it has to fetch: the artist page carries four of the hundred
    // TIDAL holds, with the rest behind a path. Reopening the four already
    // on screen would be a "see all" that shows nothing new.
    let mut app = app();
    app.artist = Some(an_artist());
    app.artist_section = 0; // Top Tracks

    let before = screen(&mut app);
    assert!(before.contains("Top Tracks"), "on the top tracks:\n{before}");

    // The question the loop asks before it spawns the fetch.
    let asked = app.selected_row();
    assert!(
        matches!(
            asked,
            Some(ratidal::shell::Collection::Row { ref path, .. })
                if path == "pages/data/top-tracks"
        ),
        "see-all fetches the whole list, got {asked:?}"
    );

    press(&mut app, KeyCode::Char('o'));
    let after = screen(&mut app);
    assert!(
        after.contains("Daft Punk — Top Tracks"),
        "opened under its own heading:\n{after}"
    );

    // The reply fills it, as the loop would.
    app.update(ratidal::shell::Action::CollectionLoaded {
        for_title: "Daft Punk — Top Tracks".into(),
        tracks: (0..30)
            .map(|i| ratidal::domain::Track {
                id: ratidal::domain::TrackId(200 + i),
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
    let track = |i: u64| ratidal::domain::Track {
        id: ratidal::domain::TrackId(i),
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
    };
    app.update(ratidal::shell::Action::TracksLoaded(
        (0..5).map(track).collect(),
    ));

    press(&mut app, KeyCode::Char('7'));
    assert_eq!(
        app.sidebar.section(),
        ratidal::shell::sidebar::Section::Tracks,
        "on the Tracks section"
    );
    let shown = screen(&mut app);
    assert!(
        shown.contains("Fav 0") && shown.contains("Fav 4"),
        "the favourites are still there:\n{shown}"
    );

    // Away and back again: still there, and still no request to make.
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Char('7'));
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
    app.open = Some(ratidal::shell::OpenCollection {
        title: "An Album".into(),
        subtitle: String::new(),
        detail: String::new(),
        cover: None,
        round_cover: false,
        came_from: ratidal::shell::sidebar::Section::Albums,
    });
    app.tracks = vec![ratidal::domain::Track {
        id: ratidal::domain::TrackId(1),
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
    }];

    press(&mut app, KeyCode::Char('7'));
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
            let mut c = ratidal::shell::carousel::Card::new(*n, "");
            c.target = Some(ratidal::shell::carousel::Target::Page(format!(
                "pages/genre_{n}"
            )));
            c
        })
        .collect();
    app.explore.rows.push(ratidal::shell::home::Row {
        heading: "Genres".into(),
        kind: ratidal::browse::RowKind::Links,
        cards: cards.clone(),
        state: ratidal::shell::carousel::CarouselState::default(),
        more: Some("pages/data/genres".into()),
    });
    while app.sidebar.section() != ratidal::shell::sidebar::Section::Explore {
        app.sidebar.next();
    }

    press(&mut app, KeyCode::Char('o'));
    app.update(ratidal::shell::Action::RowLoaded {
        for_title: "Genres".into(),
        cards,
    });

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
    let track = |explicit, ai| ratidal::domain::Track {
        id: ratidal::domain::TrackId(1),
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
    };

    // Allowed by default, as TIDAL has it.
    assert!(app.why_blocked(&track(true, false)).is_none(), "explicit plays");
    assert!(app.why_blocked(&track(false, true)).is_none(), "and so does AI");

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

#[test]
fn autoplay_follows_the_last_tracks_radio_only_when_it_is_on() {
    // TIDAL's "continue with similar content". The track names its own
    // radio, so there is nothing to fetch until the queue is actually out.
    let with_radio = ratidal::domain::Track {
        id: ratidal::domain::TrackId(1),
        title: "A Track".into(),
        artist: "Someone".into(),
        album: "An Album".into(),
        duration: std::time::Duration::from_secs(200),
        cover: None,
        tags: Vec::new(),
        added: None,
        explicit: false,
        ai: false,
        radio: Some("mix-1".into()),
    };

    // Off: the queue runs out and that is the end of it.
    let mut off = app();
    off.config.playback.autoplay = false;
    off.now_playing.track = Some(with_radio.clone());
    off.now_playing.playing = true;
    let next = off.update(ratidal::shell::Action::Playback(
        ratidal::playback::PlaybackEvent::Finished,
    ));
    assert!(next.is_none(), "nothing follows, got {next:?}");
    assert!(!off.now_playing.playing, "and the bar stops");

    // On: it asks for the radio the track named.
    let mut on = app();
    on.config.playback.autoplay = true;
    on.now_playing.track = Some(with_radio);
    on.now_playing.playing = true;
    let next = on.update(ratidal::shell::Action::Playback(
        ratidal::playback::PlaybackEvent::Finished,
    ));
    assert!(
        matches!(next, Some(ratidal::shell::Action::Autoplay(ref m)) if m == "mix-1"),
        "it follows the track's own radio, got {next:?}"
    );

    // And the reply becomes a queue like any other, so the skip keys work.
    let tracks: Vec<ratidal::domain::Track> = (0..3)
        .map(|i| ratidal::domain::Track {
            id: ratidal::domain::TrackId(100 + i),
            title: format!("Radio {i}"),
            artist: "Someone".into(),
            album: "An Album".into(),
            duration: std::time::Duration::from_secs(200),
            cover: None,
            tags: Vec::new(),
            added: None,
            explicit: false,
            ai: false,
            radio: None,
        })
        .collect();
    let next = on.update(ratidal::shell::Action::QueueRadio(tracks));
    assert!(
        matches!(next, Some(ratidal::shell::Action::PlayQueued)),
        "and plays from the top of it, got {next:?}"
    );
    assert_eq!(on.queue.len(), 3, "the radio is the queue now");
}

#[test]
fn r_starts_the_radio_of_the_selected_track() {
    // Every other key acts on the selection -- enter opens it, `A`
    // favourites it -- so this does too. A radio started from a track that
    // is not on screen reads as the app doing something of its own accord.
    let track = |id: u64, title: &str, radio: &str| ratidal::domain::Track {
        id: ratidal::domain::TrackId(id),
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
    };

    let mut app = app();
    app.update(ratidal::shell::Action::TracksLoaded(vec![
        track(1, "First", "mix-first"),
        track(2, "Second", "mix-second"),
    ]));
    press(&mut app, KeyCode::Char('7'));

    // Something else is playing: the key still follows the selection.
    app.now_playing.track = Some(track(9, "Playing", "mix-playing"));
    assert_eq!(
        app.track_radio().as_deref(),
        Some("mix-first"),
        "the selected track, not the one in the bar"
    );

    press(&mut app, KeyCode::Char('j'));
    assert_eq!(
        app.track_radio().as_deref(),
        Some("mix-second"),
        "and it follows the selection as it moves"
    );

    let action = app
        .on_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE))
        .expect("R is bound where a track is selected");
    assert!(matches!(action, ratidal::shell::Action::PlayTrackRadio));
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
    while app.sidebar.section() != ratidal::shell::sidebar::Section::Music {
        app.sidebar.next();
    }
    assert!(
        app.on_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE)).is_none(),
        "nothing selected and nothing playing: the key does nothing"
    );

    app.now_playing.track = Some(ratidal::domain::Track {
        id: ratidal::domain::TrackId(9),
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
    });
    assert_eq!(
        app.track_radio().as_deref(),
        Some("mix-playing"),
        "with nothing selected it follows the bar"
    );
}

#[test]
fn a_rebound_key_drives_the_app_the_way_the_default_would() {
    // The whole point: the keys are guarded by what is on screen, so
    // rebinding translates the keystroke rather than replacing the match.
    // Whatever the guards do for the default, they do for the new key.
    let binds: std::collections::HashMap<String, String> =
        [("quit".to_string(), "x".to_string())].into_iter().collect();
    let (keymap, problems) = ratidal::shell::keymap::Keymap::from_config(&binds);
    assert!(problems.is_empty(), "{problems:?}");

    let mut app = app();
    app.keymap = keymap;

    // The new key does what the old one did.
    assert!(
        matches!(
            app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            Some(ratidal::shell::Action::Quit)
        ),
        "the rebound key quits"
    );

    // And the old key is free: it is no longer bound to anything, since
    // nothing translates to it any more.
    assert!(
        !matches!(
            app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(ratidal::shell::Action::Quit)
        ),
        "the default key stopped quitting once it was rebound away"
    );
}

#[test]
fn rebinding_does_not_reach_the_search_box() {
    // While the box has the keyboard, every character is text. A config
    // that could change that would be a config that breaks typing.
    let binds: std::collections::HashMap<String, String> =
        [("quit".to_string(), "x".to_string())].into_iter().collect();
    let (keymap, _) = ratidal::shell::keymap::Keymap::from_config(&binds);

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
fn renewing_the_session_does_not_refetch_the_library() {
    // A renewal used to send `Authenticated`, which is a fresh login and
    // fetches everything -- so the log showed 12 playlists, 42 albums, 173
    // artists and 374 tracks twice over, and the burst was enough to be
    // rate-limited for the requests that mattered.
    let mut app = app();
    let token = ratidal::auth::StoredToken {
        access_token: "at".into(),
        refresh_token: "rt".into(),
        expires_at: 1_800_000_000,
        country_code: "US".into(),
        user_id: 1,
    };

    // A fresh login asks for the library.
    let next = app.update(ratidal::shell::Action::Authenticated(token.clone()));
    assert!(next.is_none(), "the fetch is spawned by the loop, not returned");
    assert!(app.session.is_some(), "and the session is held");

    // A renewal only replaces the token.
    let renewed = ratidal::auth::StoredToken {
        access_token: "fresh".into(),
        ..token
    };
    app.update(ratidal::shell::Action::SessionRenewed(renewed));
    assert_eq!(
        app.session.as_ref().map(|t| t.access_token.as_str()),
        Some("fresh"),
        "the new token is in use"
    );
}

#[test]
fn r_on_a_home_track_card_fetches_the_radio_the_page_left_out() {
    // The home page sends its track cards with `mixes: null` -- checked
    // against a real response -- so the radio is not in hand there. The
    // track's own endpoint has it, so the key asks for the track rather
    // than doing nothing on the page most people start from.
    let mut app = app();
    let mut card = ratidal::shell::carousel::Card::new("A Track", "Someone");
    card.target = Some(ratidal::shell::carousel::Target::Track(42));
    app.home.rows.push(ratidal::shell::home::Row {
        heading: "New Tracks".into(),
        kind: ratidal::browse::RowKind::Tracks,
        cards: vec![card],
        state: ratidal::shell::carousel::CarouselState::default(),
        more: None,
    });
    while app.sidebar.section() != ratidal::shell::sidebar::Section::Music {
        app.sidebar.next();
    }

    let action = app
        .on_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE))
        .expect("R is bound on a track card");
    assert!(
        matches!(
            action,
            ratidal::shell::Action::FetchTrackRadio(ratidal::domain::TrackId(42))
        ),
        "it asks for the selected track, got {action:?}"
    );

    // And the reply opens the radio like any other view.
    app.update(ratidal::shell::Action::OpenMix {
        mix: "mix-1".into(),
        title: "A Track Radio".into(),
        cover: Some("https://example.invalid/a.jpg".into()),
    });
    assert_eq!(
        app.open.as_ref().and_then(|o| o.cover.as_deref()),
        Some("https://example.invalid/a.jpg"),
        "a radio carries the artwork of what it was built from"
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
    app.now_playing.track = Some(ratidal::domain::Track {
        id: ratidal::domain::TrackId(1),
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
    });

    press(&mut app, KeyCode::Char('R'));
    assert_eq!(
        app.open.as_ref().and_then(|o| o.cover.as_deref()),
        Some("https://example.invalid/track.jpg"),
        "the track's radio shows the track's artwork"
    );

}

#[test]
fn an_artist_radio_opened_by_a_key_carries_the_portrait() {
    // And an artist's radio shows their portrait, round as it is elsewhere.
    let mut app = app();
    let mut page = an_artist();
    page.picture = Some("https://example.invalid/artist.jpg".into());
    app.artist = Some(page);
    press(&mut app, KeyCode::Char('S'));
    let open = app.open.as_ref().expect("the artist radio opened");
    assert_eq!(
        open.cover.as_deref(),
        Some("https://example.invalid/artist.jpg"),
        "the artist's radio shows their portrait"
    );
    assert!(open.round_cover, "and it is round, as an artist's is everywhere");
}
