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

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_module_pages_past_the_first_fifty() {
    // One request reaches fifty and stops; New Tracks is two hundred and
    // thirty-six deep. Walking it has to actually return more than a page,
    // and the items have to differ — an endpoint that ignored `offset`
    // would hand back the same fifty each time and look like it worked.
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

    let cards = ratidal::browse::module_items(&client, &path, 120)
        .await
        .expect("the module's items");
    println!("{title}: {} cards", cards.len());
    assert!(
        cards.len() > 50,
        "{title} came back with {} — the walk stopped at one page",
        cards.len()
    );

    let titles: std::collections::HashSet<&str> =
        cards.iter().map(|c| c.title.as_str()).collect();
    assert!(
        titles.len() > 50,
        "only {} distinct titles in {} cards — offset is being ignored",
        titles.len(),
        cards.len()
    );
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn the_playlists_come_back_once_each() {
    // Every playlist showed twice in the grid. Either the endpoint repeats
    // them or the walk does; the uuids say which.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let all = ratidal::library::playlists(&client).await.expect("playlists");
    let unique: std::collections::HashSet<&str> =
        all.iter().map(|p| p.uuid.as_str()).collect();
    println!("{} playlists, {} distinct", all.len(), unique.len());

    // One page at a time, to see whether the endpoint itself repeats.
    let body = client
        .get_raw(
            &format!("/users/{}/playlists", client.user_id()),
            &[("limit", "50".to_string()), ("offset", "0".to_string())],
        )
        .await
        .expect("one page");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let n = v.get("items").and_then(|i| i.as_array()).map_or(0, |a| a.len());
    let total = v.get("totalNumberOfItems").and_then(|t| t.as_u64()).unwrap_or(0);
    println!("one page: {n} items, server says {total} in total");

    assert_eq!(all.len(), unique.len(), "no playlist appears twice");
}


#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_the_unbuilt_sections_can_be_filled_with() {
    // Explore, Feed and Mixes & Radio all fall through to the favourites
    // list, so three sidebar entries show the same thing and none of them
    // says what it claims. These were probed once before; this records the
    // shapes rather than trusting the note.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let id = client.user_id();

    let v2 = [
        ("mixes", "/my-collection/mixes".to_string()),
        ("feed", "/feed/activities".to_string()),
    ];
    for (what, path) in v2 {
        match client.get_raw_v2(&path, &[("limit", "5".to_string())]).await {
            Ok(body) => {
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let keys: Vec<&String> = match &v {
                    serde_json::Value::Object(m) => m.keys().collect(),
                    _ => Vec::new(),
                };
                println!("v2 {what}: keys {keys:?}");
                if let Some(items) = v.get("items").and_then(|i| i.as_array()) {
                    println!("   {} items", items.len());
                    if let Some(serde_json::Value::Object(m)) = items.first() {
                        let mut ks: Vec<&String> = m.keys().collect();
                        ks.sort();
                        println!("   first item keys: {ks:?}");
                    }
                }
            }
            Err(e) => println!("v2 {what}: {e}"),
        }
    }

    // Explore is a page like the home one.
    for page in ["/pages/explore", "/pages/genres", "/pages/moods"] {
        match client
            .get_raw(
                page,
                &[
                    ("deviceType", "BROWSER".to_string()),
                    ("locale", "en_US".to_string()),
                ],
            )
            .await
        {
            Ok(body) => {
                let home = ratidal::browse::parse_home(&body);
                println!(
                    "{page}: {} rows, {} shortcuts",
                    home.rows.len(),
                    home.shortcuts.len()
                );
                for row in home.rows.iter().take(6) {
                    println!("   {:?} ({} cards)", row.heading, row.cards.len());
                }
            }
            Err(e) => println!("{page}: {e}"),
        }
    }
    let _ = id;
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_an_artist_page_holds() {
    // Enter on an artist card does nothing: there is no artist view. The
    // web client shows top tracks, albums and a bio. Find which of those
    // this API serves before building a page around a guess.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    // An artist id from the user's own favourites, so the probe is not
    // pinned to one that might vanish.
    let artists = ratidal::library::artists(&client).await.expect("artists");
    let Some(artist) = artists.first() else {
        eprintln!("no favourite artists to probe with");
        return;
    };
    println!("probing {:?} ({})", artist.name, artist.id);

    for (what, path) in [
        ("artist", format!("/artists/{}", artist.id)),
        ("top tracks", format!("/artists/{}/toptracks", artist.id)),
        ("albums", format!("/artists/{}/albums", artist.id)),
        ("bio", format!("/artists/{}/bio", artist.id)),
        ("similar", format!("/artists/{}/similar", artist.id)),
        ("videos", format!("/artists/{}/videos", artist.id)),
    ] {
        match client
            .get_raw(&path, &[("limit", "3".to_string())])
            .await
        {
            Ok(body) => {
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                match &v {
                    serde_json::Value::Object(m) => {
                        let mut keys: Vec<&String> = m.keys().collect();
                        keys.sort();
                        let n = m
                            .get("items")
                            .and_then(|i| i.as_array())
                            .map(|a| a.len());
                        println!("{what}: keys {keys:?}{}", match n {
                            Some(n) => format!(", {n} items"),
                            None => String::new(),
                        });
                    }
                    _ => println!("{what}: not an object"),
                }
            }
            Err(e) => println!("{what}: {e}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn an_artist_page_comes_back_populated() {
    // Every DTO field defaults, so a wrong name yields an empty section
    // rather than an error. Only a real response proves the shapes.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let artists = ratidal::library::artists(&client).await.expect("artists");
    let Some(artist) = artists.first() else {
        eprintln!("no favourite artists to probe with");
        return;
    };

    let page = ratidal::library::artist_page(&client, artist.id)
        .await
        .expect("the artist page");
    println!(
        "{:?}: {} top tracks, {} albums, {} similar",
        page.name,
        page.top_tracks.len(),
        page.albums.len(),
        page.similar.len()
    );

    assert_eq!(page.name, artist.name, "the artist's own name comes through");
    assert!(!page.top_tracks.is_empty(), "top tracks parse");
    assert!(
        page.top_tracks.iter().all(|t| !t.title.is_empty()),
        "and carry their titles"
    );
    assert!(!page.albums.is_empty(), "albums parse");
    assert!(
        page.albums.iter().all(|a| !a.title.is_empty()),
        "and carry theirs"
    );
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_the_feed_actually_carries() {
    // The Feed section still falls through to the favourites list. Its
    // reply is `{activities, stats}` rather than the items page every other
    // endpoint returns, so the shape has to be read before anything is
    // built on it.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = match client.get_raw_v2("/feed/activities", &[("limit", "5".to_string())]).await
    {
        Ok(b) => b,
        Err(e) => {
            println!("feed: {e}");
            return;
        }
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let Some(items) = v.get("activities").and_then(|a| a.as_array()) else {
        println!("no activities array");
        return;
    };
    println!("{} activities", items.len());

    for a in items.iter().take(3) {
        if let serde_json::Value::Object(m) = a {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            println!("  keys {keys:?}");
            // One level down, where the actual item should be.
            for k in m.keys() {
                if let Some(serde_json::Value::Object(inner)) = m.get(k) {
                    let mut ks: Vec<&String> = inner.keys().collect();
                    ks.sort();
                    println!("    {k}: {ks:?}");
                }
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn the_feed_parses_into_cards() {
    // Every field defaults, so a wrong name yields an empty feed rather
    // than an error — the same trap the artist page's top tracks fell into.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let cards = ratidal::browse::feed(&client).await.expect("the feed");
    println!("{} cards", cards.len());
    for c in cards.iter().take(3) {
        println!("  {:?} by {:?} ({:?})", c.title, c.subtitle, c.target);
    }

    assert!(!cards.is_empty(), "the feed has something in it");
    assert!(
        cards.iter().all(|c| !c.title.is_empty()),
        "and every card carries its title"
    );
    assert!(
        cards.iter().any(|c| c.target.is_some()),
        "and something to open"
    );
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn where_the_mixes_actually_live() {
    // `/my-collection/mixes` came back with nothing, which could mean the
    // account has no saved mixes or that this is the wrong path. The home
    // page carries mixes too, so compare.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    for path in ["/my-collection/mixes", "/mixes/daily", "/pages/my_collection_my_mixes"] {
        match client.get_raw_v2(path, &[("limit", "10".to_string())]).await {
            Ok(body) => {
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let n = v.get("items").and_then(|i| i.as_array()).map_or(0, |a| a.len());
                println!("v2 {path}: {n} items");
            }
            Err(e) => println!("v2 {path}: {e}"),
        }
    }
    for path in ["/pages/my_collection_my_mixes", "/pages/mixes"] {
        match client
            .get_raw(
                path,
                &[
                    ("deviceType", "BROWSER".to_string()),
                    ("locale", "en_US".to_string()),
                ],
            )
            .await
        {
            Ok(body) => {
                let home = ratidal::browse::parse_home(&body);
                println!("v1 {path}: {} rows", home.rows.len());
                for r in home.rows.iter().take(4) {
                    println!("    {:?} ({} cards)", r.heading, r.cards.len());
                }
            }
            Err(e) => println!("v1 {path}: {e}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_shape_a_mix_has() {
    // The collection is empty on this account, so the item shape cannot be
    // read from it. The home page carries mixes in its own rows; those are
    // the same objects, and what a mix card needs is there.
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

    fn find_mix(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                if m.contains_key("mixType") || m.get("id").is_some_and(|i| {
                    i.as_str().is_some_and(|s| s.len() > 20)
                }) {
                    let mut keys: Vec<&String> = m.keys().collect();
                    keys.sort();
                    out.push(format!("{keys:?}"));
                }
                for x in m.values() {
                    find_mix(x, out);
                }
            }
            serde_json::Value::Array(a) => {
                for x in a {
                    find_mix(x, out);
                }
            }
            _ => {}
        }
    }
    let mut shapes = Vec::new();
    find_mix(&v, &mut shapes);
    shapes.sort();
    shapes.dedup();
    for s in shapes.iter().take(4) {
        println!("mix-shaped item: {s}");
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn the_stream_we_get_is_the_best_the_account_allows() {
    // A full audit of the quality path: what we ask for, what TIDAL says it
    // gave, and whether the manifest holds a choice we might be taking the
    // wrong branch of.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let tracks = ratidal::library::favourite_tracks(&client).await.expect("tracks");
    let Some(track) = tracks.first() else {
        eprintln!("no favourites to probe with");
        return;
    };
    println!("probing {:?} ({})", track.title, track.id);

    // Ask for each tier in turn: if a lower one comes back identical to the
    // top, the request is not reaching TIDAL's chooser.
    for want in [
        ratidal::domain::Quality::HiResLossless,
        ratidal::domain::Quality::Lossless,
        ratidal::domain::Quality::High,
    ] {
        match client.playback_info(track.id, want).await {
            Ok(info) => {
                println!(
                    "asked {want:?} -> delivered {:?}, {:?} bit, {:?} Hz",
                    info.delivered, info.bit_depth, info.sample_rate
                );
            }
            Err(e) => println!("asked {want:?} -> {e}"),
        }
    }

    // And the manifest itself: does it carry more than one representation?
    let body = client
        .get_raw(
            &format!("/tracks/{}/playbackinfopostpaywall", track.id),
            &[
                ("audioquality", "HI_RES_LOSSLESS".to_string()),
                ("playbackmode", "STREAM".to_string()),
                ("assetpresentation", "FULL".to_string()),
            ],
        )
        .await
        .expect("playback info");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let mime = v.get("manifestMimeType").and_then(|m| m.as_str()).unwrap_or("");
    println!("manifest type: {mime}");

    if let Some(b64) = v.get("manifest").and_then(|m| m.as_str()) {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("base64");
        let xml = String::from_utf8_lossy(&raw);
        let reps = xml.matches("<Representation").count();
        let sets = xml.matches("<AdaptationSet").count();
        println!("manifest holds {reps} representation(s) in {sets} adaptation set(s)");
        for line in xml.lines().filter(|l| l.contains("Representation") || l.contains("codecs")) {
            println!("   {}", line.trim());
        }
        assert_eq!(
            reps, 1,
            "more than one representation means a choice is being made \
             somewhere, and the parser takes whichever comes last"
        );
    }
}


#[test]
#[ignore = "reads this machine's audio device"]
fn what_the_output_device_is_configured_for() {
    // rodio opens the device at its *default* configuration. If that is
    // 48kHz and the stream is 44.1, every sample is resampled on the way
    // out — a real loss, and a silent one.
    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    let host = rodio::cpal::default_host();
    let Some(device) = host.default_output_device() else {
        println!("no output device");
        return;
    };
    println!("device: {:?}", device.description());

    match device.default_output_config() {
        Ok(cfg) => println!(
            "default: {} Hz, {} ch, {:?}",
            cfg.sample_rate(),
            cfg.channels(),
            cfg.sample_format()
        ),
        Err(e) => println!("no default config: {e}"),
    }

    println!("supported:");
    if let Ok(ranges) = device.supported_output_configs() {
        for r in ranges.take(12) {
            println!(
                "   {}..{} Hz, {} ch, {:?}",
                r.min_sample_rate(),
                r.max_sample_rate(),
                r.channels(),
                r.sample_format()
            );
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_an_album_and_a_playlist_say_about_themselves() {
    // The web client shows "5 TITRES (41:01)" over an album and a running
    // time on a playlist. Both are missing here; find whether the API
    // carries them before adding a field that stays empty.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let albums = ratidal::library::albums(&client).await.expect("albums");
    if let Some(album) = albums.first() {
        let body = client
            .get_raw(&format!("/albums/{}", album.id), &[])
            .await
            .expect("the album");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if let serde_json::Value::Object(m) = &v {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            println!("album keys: {keys:?}");
            for k in ["duration", "numberOfTracks", "releaseDate", "audioQuality"] {
                println!("   {k} = {:?}", m.get(k));
            }
        }
    }

    let playlists = ratidal::library::playlists(&client).await.expect("playlists");
    if let Some(p) = playlists.first() {
        let body = client
            .get_raw(&format!("/playlists/{}", p.uuid), &[])
            .await
            .expect("the playlist");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if let serde_json::Value::Object(m) = &v {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            println!("playlist keys: {keys:?}");
            for k in ["duration", "numberOfTracks"] {
                println!("   {k} = {:?}", m.get(k));
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn how_deep_the_feed_goes() {
    // The Feed drew four rows and left the rest of the pane empty. The
    // layout turned out to be right: one request for fifty activities, of
    // which only those carrying an album become cards, is simply fewer
    // cards than the grid has room for. What this reads is how many the
    // endpoint will give and whether it pages at all.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    for (limit, offset) in [("50", "0"), ("50", "50"), ("100", "0"), ("500", "0")] {
        let body = match client
            .get_raw_v2(
                "/feed/activities",
                &[
                    ("limit", limit.to_string()),
                    ("offset", offset.to_string()),
                ],
            )
            .await
        {
            Ok(b) => b,
            Err(e) => {
                println!("limit={limit} offset={offset}: {e}");
                continue;
            }
        };
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        let n = v
            .get("activities")
            .and_then(|a| a.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let cards = ratidal::browse::parse_feed(&body).len();
        // Whatever the reply carries besides the activities themselves: a
        // cursor is what would let this be walked past one page.
        let top: Vec<&String> = match &v {
            serde_json::Value::Object(m) => m.keys().collect(),
            _ => vec![],
        };
        println!("limit={limit} offset={offset}: {n} activities, {cards} cards, keys {top:?}");
        // What the constant is set from: the limit is honoured up to a
        // ceiling, and offset does nothing.
        assert!(n <= 99, "the feed served {n}, more than the ceiling this assumes");
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_the_feed_grid_actually_gets() {
    // Nine cards across and only four rows drawn, where five rows fit and
    // the endpoint serves forty-nine. Either the cards are fewer than they
    // look or the grid is dropping some, so this counts what reaches it.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let cards = match ratidal::browse::feed(&client).await {
        Ok(c) => c,
        Err(e) => {
            println!("feed: {e}");
            return;
        }
    };
    println!("{} cards reach the grid", cards.len());
    for c in cards.iter().take(3) {
        println!("  {:?} / {:?} / cover {:?}", c.title, c.subtitle, c.cover_url.is_some());
    }
}
