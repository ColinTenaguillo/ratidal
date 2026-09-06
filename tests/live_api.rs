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
    for row in &home.rows {
        let with = row.cards.iter().filter(|c| c.cover_url.is_some()).count();
        println!(
            "  {}: {:?}, {} cards, {with} with covers",
            row.heading,
            row.kind,
            row.cards.len()
        );
    }

    // Every row that has cards should have covers for them. A row of cards
    // with none is the bug this is here to catch.
    for row in &home.rows {
        let (heading, cards) = (&row.heading, &row.cards);
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

#[tokio::test]
#[ignore = "needs the network and a signed-in session; writes to the account"]
async fn a_favourite_can_be_removed_and_put_back() {
    // These endpoints were written from the shape of the read side rather
    // than from a response, and a wrong path here is a button that silently
    // does nothing. The only way to know is to call them.
    //
    // This writes to the real account, so it takes a track that is ALREADY a
    // favourite, removes it, and puts it back — leaving the account as it
    // was found. A track that was never a favourite is never added.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let before = ratidal::library::favourite_tracks(&client)
        .await
        .expect("the favourites read back");
    let Some(track) = before.first().cloned() else {
        eprintln!("no favourites to test with; skipping");
        return;
    };
    println!("using {:?} ({})", track.title, track.id);

    ratidal::library::remove_favourite_track(&client, track.id)
        .await
        .expect("removing a favourite");
    let without = ratidal::library::favourite_tracks(&client)
        .await
        .expect("the favourites read back");
    assert!(
        !without.iter().any(|t| t.id == track.id),
        "the track is gone after removing it"
    );
    assert_eq!(without.len(), before.len() - 1, "and nothing else changed");

    ratidal::library::add_favourite_track(&client, track.id)
        .await
        .expect("adding it back");
    let after = ratidal::library::favourite_tracks(&client)
        .await
        .expect("the favourites read back");
    assert!(
        after.iter().any(|t| t.id == track.id),
        "the track is back after adding it"
    );
    assert_eq!(
        after.len(),
        before.len(),
        "the account is left as it was found"
    );
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn how_many_tracks_a_track_list_module_returns() {
    // The home page draws these as a grid of six; if the API returns fewer
    // than that the grid has holes in it, and the fix is a request
    // parameter rather than anything in the renderer.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    for limit in ["", "6", "12", "50"] {
        let mut query = vec![
            ("deviceType", "BROWSER".to_string()),
            ("locale", "en_US".to_string()),
        ];
        if !limit.is_empty() {
            query.push(("limit", limit.to_string()));
        }
        let body = match client.get_raw("/pages/home", &query).await {
            Ok(b) => b,
            Err(e) => {
                println!("limit={limit:?}: {e}");
                continue;
            }
        };
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        let mut found = Vec::new();
        collect_track_lists(&v, &mut found);
        println!("limit={limit:?} -> {found:?}");
    }
}

fn collect_track_lists(v: &serde_json::Value, out: &mut Vec<(String, usize, u64)>) {
    match v {
        serde_json::Value::Object(m) => {
            if m.get("type").and_then(|t| t.as_str()) == Some("TRACK_LIST") {
                let title = m
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("?")
                    .to_string();
                let paged = m.get("pagedList");
                let n = paged
                    .and_then(|p| p.get("items"))
                    .and_then(|i| i.as_array())
                    .map_or(0, |a| a.len());
                let total = paged
                    .and_then(|p| p.get("totalNumberOfItems"))
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);
                out.push((title, n, total));
            }
            for x in m.values() {
                collect_track_lists(x, out);
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                collect_track_lists(x, out);
            }
        }
        _ => {}
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_track_list_module_can_be_paged_for_more() {
    // The module itself returns five whatever `limit` the page is asked
    // for, but carries a `dataApiPath` and says `supportsPaging`. If that
    // path takes a limit, the grid can be filled; if not, six cells is the
    // wrong shape for this row.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = client
        .get_raw(
            "/pages/home",
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
        .expect("the home page");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");

    let mut paths = Vec::new();
    collect_data_paths(&v, &mut paths);
    println!("data paths: {paths:?}");

    for (title, path) in paths {
        for limit in ["6", "12"] {
            let result = client
                .get_raw(
                    &format!("/{path}"),
                    &[
                        ("deviceType", "BROWSER".to_string()),
                        ("locale", "en_US".to_string()),
                        ("limit", limit.to_string()),
                        ("offset", "0".to_string()),
                    ],
                )
                .await;
            match result {
                Ok(b) => {
                    let d: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
                    let n = d
                        .get("items")
                        .and_then(|i| i.as_array())
                        .map_or(0, |a| a.len());
                    println!("{title} limit={limit} -> {n} items");
                }
                Err(e) => println!("{title} limit={limit} -> {e}"),
            }
        }
    }
}

fn collect_data_paths(v: &serde_json::Value, out: &mut Vec<(String, String)>) {
    match v {
        serde_json::Value::Object(m) => {
            if m.get("type").and_then(|t| t.as_str()) == Some("TRACK_LIST") {
                if let Some(p) = m
                    .get("pagedList")
                    .and_then(|p| p.get("dataApiPath"))
                    .and_then(|p| p.as_str())
                {
                    let title = m
                        .get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or("?")
                        .to_string();
                    out.push((title, p.to_string()));
                }
            }
            for x in m.values() {
                collect_data_paths(x, out);
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                collect_data_paths(x, out);
            }
        }
        _ => {}
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_limit_a_module_endpoint_accepts() {
    // "See all" asked for 100 and the API answered with an error about a
    // maximum. Find the real ceiling rather than guessing again.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = client
        .get_raw(
            "/pages/home",
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
        .expect("the home page");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let mut paths = Vec::new();
    collect_data_paths(&v, &mut paths);
    let Some((title, path)) = paths.first().cloned() else {
        eprintln!("no paged modules to probe");
        return;
    };

    for limit in [10u32, 50, 51, 100] {
        let result = client
            .get_raw(
                &format!("/{path}"),
                &[
                    ("deviceType", "BROWSER".to_string()),
                    ("locale", "en_US".to_string()),
                    ("limit", limit.to_string()),
                    ("offset", "0".to_string()),
                ],
            )
            .await;
        match result {
            Ok(b) => {
                let d: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
                let n = d.get("items").and_then(|i| i.as_array()).map_or(0, |a| a.len());
                println!("{title} limit={limit} -> {n} items");
            }
            Err(e) => println!("{title} limit={limit} -> ERROR {e}"),
        }
    }
}
