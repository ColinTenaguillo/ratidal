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

/// What is on screen.
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
    ratidal::library::ArtistPage {
        name: "Daft Punk".into(),
        albums: (0..12).map(|i| album(i, &format!("Album {i}"))).collect(),
        singles: (0..12).map(|i| album(20 + i, &format!("Single {i}"))).collect(),
        ..Default::default()
    }
}

#[test]
fn see_all_on_an_artists_section_opens_it() {
    // The hint was drawn and the key did nothing: `SeeAll` only knew about
    // the home page's rows, and an artist's sections are not those.
    let mut app = app();
    app.artist = Some(an_artist());

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
    let whole = screen_sized(&mut app, 199, 46);
    let sections = ["Albums", "EP & Singles"];
    let all_fit = sections.iter().all(|s| whole.contains(*s));
    assert!(all_fit, "both fit at 46 rows:\n{whole}");

    // Six rows shorter the second no longer fits whole, and used to be
    // dropped outright — the page ended in blank pane.
    let cut = screen_sized(&mut app, 199, 40);
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
