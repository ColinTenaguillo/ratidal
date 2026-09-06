//! Checks the DTOs against the real API, using the signed-in session.
//!
//! Every DTO field is `#[serde(default)]`, which is what keeps an added field
//! from being fatal — and also what makes a *wrong* field name silent: the
//! parse succeeds and the view comes back empty. Only a real response catches
//! that, and it caught it once already, when `/pages/home` was being rejected
//! for a missing parameter while the parser was fine.
//!
//! Ignored by default: needs a network and a signed-in session. Run with
//!
//!     cargo test --test live_api -- --ignored --nocapture

use ratidal::auth::store;

/// The stored session, or `None` when there is nothing to test with.
fn session() -> Option<ratidal::auth::StoredToken> {
    match store::load() {
        Ok(Some(token)) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if token.is_expired_at(now) {
                eprintln!("the stored session has expired; run the app to refresh it");
                return None;
            }
            Some(token)
        }
        _ => {
            eprintln!("no stored session; sign in with the app first");
            None
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn the_library_endpoints_parse_into_populated_values() {
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    // Playlists: the sidebar and the Playlists grid both read these.
    let playlists = ratidal::library::playlists(&client)
        .await
        .expect("playlists request");
    println!("playlists: {}", playlists.len());
    if let Some(p) = playlists.first() {
        println!("  {:?} by {:?}, {} tracks, cover {:?}", p.title, p.creator, p.track_count, p.cover);
        assert!(!p.uuid.is_empty(), "a playlist with no uuid cannot be opened");
        assert!(!p.title.is_empty(), "the title field name is wrong");
        assert!(!p.creator.is_empty(), "creator falls back to TIDAL, never empty");
    }

    // Albums: written against the favourites envelope, never yet seen.
    let albums = ratidal::library::albums(&client).await.expect("albums request");
    println!("albums: {}", albums.len());
    if let Some(a) = albums.first() {
        println!("  {:?} by {:?}, {:?}, cover {:?}", a.title, a.artist, a.year, a.cover);
        assert!(a.id != 0, "the id field name is wrong");
        assert!(!a.title.is_empty(), "the title field name is wrong");
        assert!(!a.artist.is_empty(), "the artists field name is wrong");
    }

    let artists = ratidal::library::artists(&client).await.expect("artists request");
    let without: Vec<&str> = artists
        .iter()
        .filter(|a| a.picture.is_none())
        .map(|a| a.name.as_str())
        .collect();
    println!("artists: {} ({} with no picture)", artists.len(), without.len());
    println!("  no picture: {:?}", &without[..without.len().min(15)]);
    if let Some(a) = artists.first() {
        println!("  {:?} ({}), picture {:?}", a.name, a.id, a.picture);
        assert!(a.id != 0, "the id field name is wrong");
        assert!(!a.name.is_empty(), "the name field name is wrong");
    }

    // Favourite tracks carry the fields the Tracks view added.
    let tracks = ratidal::library::favourite_tracks(&client)
        .await
        .expect("favourites request");
    println!("tracks: {}", tracks.len());
    if let Some(t) = tracks.first() {
        println!(
            "  {:?} / {:?} / {:?} added {:?} explicit {}",
            t.title, t.artist, t.album, t.added, t.explicit
        );
        assert!(!t.title.is_empty(), "the title field name is wrong");
        assert!(!t.album.is_empty(), "the album title field name is wrong");
        assert!(t.added.is_some(), "favourites must carry when they were added");
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_playlists_tracks_can_be_opened() {
    // What pressing enter on a playlist card does.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let playlists = ratidal::library::playlists(&client)
        .await
        .expect("playlists request");
    let Some(playlist) = playlists.iter().find(|p| p.track_count > 0) else {
        eprintln!("no non-empty playlist to open");
        return;
    };

    let tracks = ratidal::library::playlist_tracks(&client, &playlist.uuid)
        .await
        .expect("playlist tracks request");
    println!("{:?}: {} tracks", playlist.title, tracks.len());
    assert!(
        !tracks.is_empty(),
        "{:?} reports {} tracks but returned none — the items endpoint or its \
         envelope is wrong",
        playlist.title,
        playlist.track_count
    );
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn the_home_page_is_captured_for_the_fixture() {
    // Writes the live response to /tmp so the fixture can be replaced with a
    // complete one. The committed fixture was captured by hand with only the
    // fields the parser read, which is why it could not say why a row came
    // back without covers.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = ratidal::browse::home_body(&client).await.expect("home request");
    let out = std::env::temp_dir().join("ratidal-pages-home.json");
    std::fs::write(&out, &body).expect("write capture");
    println!("wrote {} ({} bytes)", out.display(), body.len());

    let home = ratidal::browse::parse_home(&body);
    println!("shortcuts: {}", home.shortcuts.len());
    for (heading, cards) in &home.rows {
        let with = cards.iter().filter(|c| c.cover_url.is_some()).count();
        println!("  {heading}: {} cards, {with} with covers", cards.len());
    }

    // Every row that has cards should have covers for them. A row of cards
    // with none is the bug this is here to catch.
    for (heading, cards) in &home.rows {
        if cards.is_empty() {
            continue;
        }
        let with = cards.iter().filter(|c| c.cover_url.is_some()).count();
        assert!(
            with > 0,
            "{heading:?} has {} cards and not one cover — its items carry the \
             image under a field the parser does not read",
            cards.len()
        );
    }
}
