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

    let body = ratidal::browse::page_body(&client, ratidal::browse::Tab::ForYou)
        .await
        .expect("home request");
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

#[test]
#[ignore = "reads this machine's terminal"]
fn what_image_protocol_this_terminal_has() {
    // Which protocol is in use decides whether a cover can be clipped at
    // all: half blocks and kitty stop at the area they are given, iTerm2
    // and sixel draw nothing when the encoding is larger than it.
    match ratatui_image::picker::Picker::from_query_stdio() {
        Ok(p) => println!("protocol {:?}, font size {:?}", p.protocol_type(), p.font_size()),
        Err(e) => println!("no protocol: {e}"),
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn whether_an_artist_lists_the_same_album_twice() {
    // Kaaris' page showed BYAKUGAN twice, Day One three times and SVR
    // twice. Either the endpoint returns a row per release — versions,
    // territories, explicit and clean — or the page is asking twice and
    // concatenating. This says which.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let hits = match client
        .get_raw("/search/artists", &[("query", "Kaaris".to_string()), ("limit", "1".to_string())])
        .await
    {
        Ok(b) => b,
        Err(e) => {
            println!("search: {e}");
            return;
        }
    };
    let v: serde_json::Value = serde_json::from_str(&hits).expect("json");
    let Some(id) = v["items"][0]["id"].as_u64() else {
        println!("no artist found");
        return;
    };
    println!("artist id {id}");

    let body = match client
        .get_raw(&format!("/artists/{id}/albums"), &[("limit", "50".to_string())])
        .await
    {
        Ok(b) => b,
        Err(e) => {
            println!("albums: {e}");
            return;
        }
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let items = v["items"].as_array().cloned().unwrap_or_default();
    println!("{} albums returned", items.len());
    for a in items.iter().take(12) {
        println!(
            "  id={:?} title={:?} version={:?} explicit={:?} tracks={:?} released={:?}",
            a["id"].as_u64(),
            a["title"].as_str(),
            a["version"].as_str(),
            a["explicit"].as_bool(),
            a["numberOfTracks"].as_u64(),
            a["releaseDate"].as_str(),
        );
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn whether_an_opened_row_carries_its_albums() {
    // Opening a home row into a track list showed no album column. The
    // album travels item -> card.detail -> track.album, so this reads
    // whether the endpoint says it at all.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let home = match ratidal::browse::tab_page(&client, ratidal::browse::Tab::ForYou).await {
        Ok(h) => h,
        Err(e) => {
            println!("home: {e}");
            return;
        }
    };
    for row in home.rows.iter() {
        let Some(path) = row.more.as_ref() else { continue };
        if row.kind != ratidal::browse::RowKind::Tracks {
            continue;
        }
        match ratidal::browse::module_items(&client, path, 5).await {
            Ok(cards) => {
                println!("row {:?}:", row.heading);
                for c in cards.iter().take(5) {
                    println!(
                        "  title={:?} subtitle={:?} detail={:?} target={:?}",
                        c.title, c.subtitle, c.detail, c.target
                    );
                }
            }
            Err(e) => println!("row {:?}: {e}", row.heading),
        }
        return;
    }
    println!("no track row with a `more` path");
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn which_home_rows_arrive_and_which_are_dropped() {
    // "From Our Editors" does not show its heading. Either the row never
    // reaches the page, or it arrives with no heading, or it is dropped for
    // having no cards. This says which.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let home = match ratidal::browse::tab_page(&client, ratidal::browse::Tab::ForYou).await {
        Ok(h) => h,
        Err(e) => {
            println!("home: {e}");
            return;
        }
    };
    println!("{} rows", home.rows.len());
    for row in home.rows.iter() {
        println!(
            "  {:?} kind={:?} cards={} more={:?}",
            row.heading,
            row.kind,
            row.cards.len(),
            row.more.is_some()
        );
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_the_mixes_page_actually_returns() {
    // `parse_home` yields no rows for it, which could be an empty page or a
    // shape the parser does not read. Print the raw skeleton.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    for path in [
        "/pages/my_collection_my_mixes",
        "/pages/home",
        "/pages/for_you",
    ] {
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
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let rows = v["rows"].as_array().cloned().unwrap_or_default();
                println!("{path}: {} bytes, {} rows", body.len(), rows.len());
                for row in rows.iter().take(8) {
                    for m in row["modules"].as_array().cloned().unwrap_or_default() {
                        println!(
                            "    type={:?} title={:?} items={}",
                            m["type"].as_str(),
                            m["title"].as_str(),
                            m["pagedList"]["items"]
                                .as_array()
                                .map_or(0, |a| a.len()),
                        );
                    }
                }
            }
            Err(e) => println!("{path}: {e}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_a_mix_item_carries() {
    // MIX_LIST is a row type the home parser does not know, so its items
    // were dropped. What a card needs — a title, a subtitle, artwork, and
    // something to open — has to be read off a real one.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = match client
        .get_raw(
            "/pages/my_collection_my_mixes",
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => {
            println!("{e}");
            return;
        }
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let items = v["rows"][0]["modules"][0]["pagedList"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    println!("{} mixes", items.len());
    for m in items.iter().take(3) {
        if let serde_json::Value::Object(o) = m {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            println!("  keys {keys:?}");
        }
        println!(
            "    id={:?} title={:?} subTitle={:?} mixType={:?} images={:?}",
            m["id"].as_str(),
            m["title"].as_str(),
            m["subTitle"].as_str(),
            m["mixType"].as_str(),
            m["images"].as_object().map(|o| {
                let mut k: Vec<&String> = o.keys().collect();
                k.sort();
                k
            }),
        );
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn how_a_mix_names_its_art_and_yields_its_tracks() {
    // A mix's id is a string, not a number, and its artwork is an object
    // per size rather than the uuid every other item uses. Both have to be
    // read rather than guessed.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = match client
        .get_raw(
            "/pages/my_collection_my_mixes",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => {
            println!("{e}");
            return;
        }
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let first = v["rows"][0]["modules"][0]["pagedList"]["items"][0].clone();
    println!("images: {}", serde_json::to_string(&first["images"]).unwrap_or_default());

    let Some(id) = first["id"].as_str() else {
        println!("no id");
        return;
    };
    // What opening a mix would have to call.
    for (label, path) in [
        ("v1 mix items", format!("/mixes/{id}/items")),
        ("v1 pages/mix", "/pages/mix".to_string()),
    ] {
        let mut q = vec![
            ("deviceType", "BROWSER".to_string()),
            ("locale", "en_US".to_string()),
            ("limit", "3".to_string()),
        ];
        if path == "/pages/mix" {
            q.push(("mixId", id.to_string()));
        }
        match client.get_raw(&path, &q).await {
            Ok(b) => {
                let v: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
                let n = v["items"].as_array().map_or(0, |a| a.len());
                let rows = v["rows"].as_array().map_or(0, |a| a.len());
                println!("{label} ({path}): {} bytes, {n} items, {rows} rows", b.len());
            }
            Err(e) => println!("{label} ({path}): {e}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_mixs_items_parse_as_tracks() {
    // The existing `parse_items` reads `{items: [...]}`; whether a mix's
    // items are the same shape decides whether opening one can reuse it.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let page = match client
        .get_raw(
            "/pages/my_collection_my_mixes",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => {
            println!("{e}");
            return;
        }
    };
    let v: serde_json::Value = serde_json::from_str(&page).expect("json");
    let Some(id) = v["rows"][0]["modules"][0]["pagedList"]["items"][0]["id"].as_str() else {
        println!("no mix");
        return;
    };

    match client
        .get_raw(
            &format!("/mixes/{id}/items"),
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
                ("limit", "5".to_string()),
            ],
        )
        .await
    {
        Ok(b) => {
            let cards = ratidal::browse::parse_items(&b);
            println!("{} cards from parse_items", cards.len());
            for c in cards.iter().take(3) {
                println!("  {:?} / {:?} / target {:?}", c.title, c.subtitle, c.target);
            }
        }
        Err(e) => println!("{e}"),
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn whether_a_mixs_items_are_wrapped() {
    // `#[serde(default)]` makes a wrong guess silent, so which envelope a
    // mix uses has to be read rather than assumed: album tracks come back
    // as `{items:[{item:{...}}]}` and a bare list as `{items:[{...}]}`.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let page = match client
        .get_raw(
            "/pages/my_collection_my_mixes",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => return println!("{e}"),
    };
    let v: serde_json::Value = serde_json::from_str(&page).expect("json");
    let Some(id) = v["rows"][0]["modules"][0]["pagedList"]["items"][0]["id"].as_str() else {
        return println!("no mix");
    };
    let body = match client
        .get_raw(
            &format!("/mixes/{id}/items"),
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
                ("limit", "3".to_string()),
            ],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => return println!("{e}"),
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let first = &v["items"][0];
    println!("wrapped in `item`: {}", first.get("item").is_some());
    println!("has a title of its own: {}", first.get("title").is_some());
    println!("totalNumberOfItems: {:?}", v["totalNumberOfItems"].as_u64());
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn the_mixes_section_comes_back_populated() {
    // The section fell through to the favourites list for weeks because
    // `/my-collection/mixes` answers 200 with nothing. The page endpoint
    // has them; this is what the section will actually show.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    match ratidal::browse::mixes(&client).await {
        Ok(mixes) => {
            println!("{} mine, {} radio", mixes.mine.len(), mixes.radio.len());
            for c in mixes.mine.iter().take(3) {
                println!("  mine  {:?} / cover {}", c.title, c.cover_url.is_some());
            }
            for c in mixes.radio.iter().take(3) {
                println!("  radio {:?} / cover {}", c.title, c.cover_url.is_some());
            }
            // Whatever the account has saved: this one has four daily
            // mixes and no stations, and a collection with neither is a
            // legitimate empty section rather than a failure.
            assert!(
                mixes.mine.iter().chain(mixes.radio.iter()).count() > 0,
                "this account has saved mixes"
            );
            // Video mixes cannot be played, so they must not be listed.
            assert!(
                !mixes.mine.iter().chain(mixes.radio.iter())
                    .any(|c| c.title.contains("Video")),
                "video mixes are left out"
            );
            // Seven ids appear on both pages; each must be listed once.
            let ids: Vec<_> = mixes.mine.iter().chain(mixes.radio.iter())
                .filter_map(|c| match &c.target {
                    Some(ratidal::shell::carousel::Target::Mix(id)) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            let unique: std::collections::HashSet<_> = ids.iter().collect();
            assert_eq!(ids.len(), unique.len(), "no mix is listed twice");
        }
        Err(e) => println!("mixes: {e}"),
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn why_a_mix_item_yields_no_card() {
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let body = match client
        .get_raw(
            "/pages/my_collection_my_mixes",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => return println!("{e}"),
    };
    let home = ratidal::browse::parse_home(&body);
    println!("parse_home: {} rows", home.rows.len());
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let m = &v["rows"][0]["modules"][0];
    println!("module type={:?}", m["type"].as_str());
    println!(
        "pagedList keys: {:?}",
        m["pagedList"].as_object().map(|o| {
            let mut k: Vec<&String> = o.keys().collect();
            k.sort();
            k
        })
    );
    println!("first item: {}", serde_json::to_string(&m["pagedList"]["items"][0]["title"]).unwrap_or_default());
}

#[test]
fn a_string_id_does_not_break_the_item_parser() {
    // `id` is a number on an album and a string on a mix. Two Rust fields
    // renamed onto the same JSON key is a duplicate-field error, which
    // fails the whole page rather than the one item.
    let body = r#"{"rows":[{"modules":[{"type":"MIX_LIST","title":"",
        "pagedList":{"items":[
            {"id":"01637c71e43c","title":"My Daily Discovery","subTitle":"Songs",
             "images":{"SMALL":{"url":"https://x/small","width":320,"height":320}}}
        ]}}]}]}"#;
    let home = ratidal::browse::parse_home(body);
    let cards: Vec<_> = home.rows.into_iter().flat_map(|r| r.cards).collect();
    assert_eq!(cards.len(), 1, "the mix parsed");
    assert_eq!(cards[0].title, "My Daily Discovery");
    assert_eq!(cards[0].cover_url.as_deref(), Some("https://x/small"));
    // And it opens: the string id is what says this is a mix, since
    // nothing else on these pages has one.
    assert!(
        matches!(
            cards[0].target,
            Some(ratidal::shell::carousel::Target::Mix(ref id)) if id == "01637c71e43c"
        ),
        "the mix carries its own id, got {:?}",
        cards[0].target
    );
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_the_two_mix_pages_hold() {
    // Mixes & Radio wants two tabs: the user's own mixes and TIDAL's. Which
    // page holds which, and whether they overlap, has to be read.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let mut seen: std::collections::HashMap<String, Vec<String>> = Default::default();
    for path in ["/pages/my_collection_my_mixes", "/pages/for_you"] {
        let body = match client
            .get_raw(
                path,
                &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
            )
            .await
        {
            Ok(b) => b,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        println!("== {path}");
        for row in v["rows"].as_array().cloned().unwrap_or_default() {
            for m in row["modules"].as_array().cloned().unwrap_or_default() {
                let items = m["pagedList"]["items"].as_array().cloned().unwrap_or_default();
                println!(
                    "  module type={:?} title={:?} items={}",
                    m["type"].as_str(),
                    m["title"].as_str(),
                    items.len()
                );
                for it in items.iter() {
                    let Some(id) = it["id"].as_str() else { continue };
                    seen.entry(id.to_string())
                        .or_default()
                        .push(path.to_string());
                    if it["mixType"].as_str().is_some() {
                        println!(
                            "      {:?} mixType={:?}",
                            it["title"].as_str(),
                            it["mixType"].as_str()
                        );
                    }
                }
            }
        }
    }
    let both = seen.values().filter(|v| v.len() > 1).count();
    println!("ids in both pages: {both}");
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_the_mixes_collection_looks_like_empty() {
    // The section is the user's *saved* mixes — /my-collection/mixes — which
    // is empty on this account. What was built instead is the "discover
    // mixes" page behind it. Read the real one: its shape, and whether it
    // says anything about favouriting.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    for path in ["/my-collection/mixes", "/favorites/mixes"] {
        match client
            .get_raw_v2(path, &[("limit", "10".to_string())])
            .await
        {
            Ok(body) => {
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                println!("v2 {path}: {} bytes", body.len());
                if let serde_json::Value::Object(o) = &v {
                    let mut keys: Vec<&String> = o.keys().collect();
                    keys.sort();
                    println!("    keys {keys:?}");
                }
                println!("    body {}", &body[..body.len().min(300)]);
            }
            Err(e) => println!("v2 {path}: {e}"),
        }
    }
    for path in ["/favorites/mixes", "/users/me/favorites/mixes"] {
        match client
            .get_raw(
                path,
                &[
                    ("deviceType", "BROWSER".to_string()),
                    ("locale", "en_US".to_string()),
                    ("limit", "10".to_string()),
                ],
            )
            .await
        {
            Ok(body) => println!("v1 {path}: {}", &body[..body.len().min(300)]),
            Err(e) => println!("v1 {path}: {e}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_the_saved_mixes_carry() {
    // The section is the user's *saved* mixes. Now that the account has
    // some, the shape can be read rather than guessed: items[].data holds
    // the mix, and the wrapper carries when it was added.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = match client
        .get_raw_v2("/my-collection/mixes", &[("limit", "50".to_string())])
        .await
    {
        Ok(b) => b,
        Err(e) => return println!("{e}"),
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let items = v["items"].as_array().cloned().unwrap_or_default();
    println!("{} saved mixes, cursor {:?}", items.len(), v["cursor"].as_str());
    let mut kinds: std::collections::HashMap<String, usize> = Default::default();
    for it in items.iter() {
        *kinds
            .entry(it["data"]["mixType"].as_str().unwrap_or("?").to_string())
            .or_default() += 1;
    }
    println!("kinds: {kinds:?}");
    for it in items.iter().take(2) {
        let d = &it["data"];
        println!(
            "    id={:?} title={:?} mixType={:?} images={:?}",
            d["id"].as_str(),
            d["title"].as_str(),
            d["mixType"].as_str(),
            d["images"].as_object().map(|o| {
                let mut k: Vec<&String> = o.keys().collect();
                k.sort();
                k
            }),
        );
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn how_many_rows_the_home_page_really_has() {
    // The web client's home page has far more sections than the five that
    // reach this app. Either the endpoint returns more and the parser drops
    // them, or the rows live on other pages.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = match client
        .get_raw(
            "/pages/home",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => return println!("{e}"),
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let rows = v["rows"].as_array().cloned().unwrap_or_default();
    println!("{} rows in the raw page, {} bytes", rows.len(), body.len());
    for row in rows.iter() {
        for m in row["modules"].as_array().cloned().unwrap_or_default() {
            println!(
                "  type={:?} title={:?} items={} more={:?}",
                m["type"].as_str(),
                m["title"].as_str(),
                m["pagedList"]["items"].as_array().map_or(0, |a| a.len()),
                m["pagedList"]["dataApiPath"].as_str().is_some(),
            );
        }
    }
    println!("parse_home keeps {} rows", ratidal::browse::parse_home(&body).rows.len());
    // A section the parser misses may not be under `rows` at all; a sibling
    // key at the top level would say where the web client's extra rows come
    // from.
    if let serde_json::Value::Object(o) = &v {
        let mut keys: Vec<&String> = o.keys().collect();
        keys.sort();
        println!("page keys {keys:?}");
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn which_other_pages_carry_home_like_rows() {
    // /pages/home gives five rows; the web client's home shows many more.
    // They come from other pages, so this is a sweep of the likely names.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    for path in [
        "/pages/home",
        "/pages/for_you",
        "/pages/explore",
        "/pages/my_daily_discovery",
        "/pages/new",
        "/pages/videos",
        "/pages/genres",
        "/pages/rising",
        "/pages/hires",
        "/pages/my_activity",
        "/pages/suggested_for_you",
        "/pages/recently_played",
    ] {
        match client
            .get_raw(
                path,
                &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
            )
            .await
        {
            Ok(body) => {
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let titles: Vec<String> = v["rows"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .flat_map(|r| r["modules"].as_array().cloned().unwrap_or_default())
                    .map(|m| {
                        format!(
                            "{}({})",
                            m["title"].as_str().unwrap_or(""),
                            m["pagedList"]["items"].as_array().map_or(0, |a| a.len())
                        )
                    })
                    .collect();
                println!("{path}: {}", titles.join(", "));
            }
            Err(e) => println!("{path}: {}", e.to_string().lines().next().unwrap_or("")),
        }
    }
}


#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn which_home_modules_the_api_will_serve() {
    // The web client builds its home from tidal.com/v2/home/feed/static,
    // which refuses a non-browser client with a 403 — so the rows it shows
    // have to be found on api.tidal.com instead. Its "View all" links name
    // the modules, so this asks for each by name.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    // Straight off the web client's own markup, in the order it draws them.
    for module in [
        "CONTINUE_LISTEN_TO",
        "NEW_ALBUM_SUGGESTIONS",
        "DAILY_MIXES",
        "NEW_TRACK_SUGGESTIONS",
        "SUGGESTED_RADIOS_MIXES",
        "UPLOADS_FOR_YOU",
        "BECAUSE_YOU_ADDED_ALBUM",
        "BECAUSE_YOU_LISTENED_TO_ALBUM",
        "BASED_ON_YOUR_INTERESTS_1",
    ] {
        for path in [
            format!("/home/pages/{module}"),
            format!("/pages/home/{module}"),
        ] {
            match client
                .get_raw(
                    &path,
                    &[
                        ("deviceType", "BROWSER".to_string()),
                        ("locale", "en_US".to_string()),
                        ("limit", "5".to_string()),
                    ],
                )
                .await
            {
                Ok(b) => {
                    let v: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
                    let n = v["items"].as_array().map_or(0, |a| a.len());
                    let rows = v["rows"].as_array().map_or(0, |a| a.len());
                    println!("OK  {path}: {n} items, {rows} rows, {} bytes", b.len());
                }
                Err(e) => {
                    let msg = e.to_string();
                    println!("--  {path}: {}", &msg[..msg.len().min(60)]);
                }
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn hunting_the_two_missing_home_rows() {
    // "Suggested new albums for you" and "Because you liked X" are the two
    // the web shows that nothing here answers for. Four lines of enquiry:
    // v2 for the module paths, the dataApiPath the modules carry, an
    // album-similarity endpoint, and other pages that might hold them.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    println!("== v2 module paths");
    for path in [
        "/home/pages/NEW_ALBUM_SUGGESTIONS",
        "/home/pages/BECAUSE_YOU_ADDED_ALBUM",
        "/home/feed",
        "/home",
    ] {
        match client.get_raw_v2(path, &[("limit", "3".to_string())]).await {
            Ok(b) => println!("  OK v2 {path}: {} bytes", b.len()),
            Err(e) => {
                let m = e.to_string();
                println!("  -- v2 {path}: {}", &m[..m.len().min(50)]);
            }
        }
    }

    println!("== what dataApiPath the for_you modules carry");
    if let Ok(body) = client
        .get_raw(
            "/pages/for_you",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        for row in v["rows"].as_array().cloned().unwrap_or_default() {
            for m in row["modules"].as_array().cloned().unwrap_or_default() {
                println!(
                    "  {:?} -> {:?}",
                    m["title"].as_str(),
                    m["pagedList"]["dataApiPath"].as_str()
                );
            }
        }
    }

    println!("== an album's own suggestions");
    for path in ["/albums/553598593/similar", "/albums/553598593/recommendations"] {
        match client
            .get_raw(
                path,
                &[
                    ("deviceType", "BROWSER".to_string()),
                    ("locale", "en_US".to_string()),
                    ("limit", "3".to_string()),
                ],
            )
            .await
        {
            Ok(b) => println!("  OK {path}: {} cards", ratidal::browse::parse_items(&b).len()),
            Err(e) => {
                let m = e.to_string();
                println!("  -- {path}: {}", &m[..m.len().min(50)]);
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn whether_a_pages_data_uuid_can_be_asked_for_directly() {
    // Modules carry `pages/data/{uuid}`, and "Because you listened to"
    // appends ?albumId=. If that endpoint takes an album of its own, the
    // missing "Because you liked X" is the same call with a different
    // album — the one the user added rather than listened to.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = match client
        .get_raw(
            "/pages/for_you",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => return println!("{e}"),
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let mut listened: Option<String> = None;
    for row in v["rows"].as_array().cloned().unwrap_or_default() {
        for m in row["modules"].as_array().cloned().unwrap_or_default() {
            if let Some(p) = m["pagedList"]["dataApiPath"].as_str() {
                if p.contains("albumId") {
                    listened = Some(p.to_string());
                }
            }
        }
    }
    let Some(path) = listened else {
        return println!("no album-parameterised module on the page");
    };
    println!("found {path}");

    // The same uuid with an album from the user's collection rather than
    // the one they listened to.
    let uuid = path.split('?').next().unwrap_or_default().to_string();
    let mine = match ratidal::library::albums(&client).await {
        Ok(a) => a,
        Err(e) => return println!("albums: {e}"),
    };
    let Some(first) = mine.first() else {
        return println!("no albums in the collection");
    };
    println!("asking {uuid} for album {} ({:?})", first.id, first.title);
    // The album the web client itself passes, to tell "no suggestions for
    // this record" apart from "the uuid only works for its own album".
    for (label, id) in [("web's own", 553598593u64), ("ours", first.id)] {
        match client
            .get_raw(
                &format!("/{uuid}"),
                &[
                    ("deviceType", "BROWSER".to_string()),
                    ("locale", "en_US".to_string()),
                    ("albumId", id.to_string()),
                    ("limit", "5".to_string()),
                ],
            )
            .await
        {
            Ok(b) => println!(
                "  {label} ({id}): {} items",
                ratidal::browse::parse_items(&b).len()
            ),
            Err(e) => {
                let m = e.to_string();
                println!("  {label} ({id}): {}", &m[..m.len().min(60)]);
            }
        }
    }

    match client
        .get_raw(
            &format!("/{uuid}"),
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
                ("albumId", first.id.to_string()),
                ("limit", "5".to_string()),
            ],
        )
        .await
    {
        Ok(b) => {
            let v: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
            println!(
                "  {} bytes, title {:?}, {} items",
                b.len(),
                v["title"].as_str(),
                v["items"].as_array().map_or(0, |a| a.len())
            );
            for c in ratidal::browse::parse_items(&b).iter().take(3) {
                println!("    {:?} / {:?}", c.title, c.subtitle);
            }
        }
        Err(e) => {
            let m = e.to_string();
            println!("  -- {}", &m[..m.len().min(120)]);
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_sweep_for_the_two_missing_modules() {
    // Last look: every page that answers, listing its modules with the
    // `type` and title, to see whether a suggestions-for-albums or a
    // because-you-liked row is on any of them under another name.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    for path in [
        "/pages/home",
        "/pages/for_you",
        "/pages/explore",
        "/pages/recently_played",
        "/pages/rising",
        "/pages/hires",
        "/pages/staff_picks",
        "/pages/my_collection_my_mixes",
        "/pages/album_suggestions",
        "/pages/because_you_liked",
        "/pages/suggestions",
        "/pages/discovery",
    ] {
        match client
            .get_raw(
                path,
                &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
            )
            .await
        {
            Ok(body) => {
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let mods: Vec<String> = v["rows"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .flat_map(|r| r["modules"].as_array().cloned().unwrap_or_default())
                    .map(|m| {
                        format!(
                            "{}:{}({})",
                            m["type"].as_str().unwrap_or("?"),
                            m["title"].as_str().unwrap_or(""),
                            m["pagedList"]["items"].as_array().map_or(0, |a| a.len())
                        )
                    })
                    .collect();
                println!("OK {path}: {}", mods.join(" | "));
            }
            Err(e) => {
                let m = e.to_string();
                println!("-- {path}: {}", &m[..m.len().min(40)]);
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn every_home_tab_returns_rows() {
    // Four tabs now. A tab whose page answers with nothing is a tab that
    // looks broken, so each is asked for once.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    for tab in ratidal::browse::Tab::ALL {
        match ratidal::browse::tab_page(&client, tab).await {
            Ok(home) => {
                let headings: Vec<&str> =
                    home.rows.iter().map(|r| r.heading.as_str()).collect();
                println!("{:?} ({}): {}", tab, tab.label(), headings.join(" | "));
                assert!(!home.rows.is_empty(), "{:?} has rows", tab);
                // For you is the home page, arrived at by tab or on
                // first load — one call now, so it carries the rows
                // gathered from the other pages either way.
                if tab == ratidal::browse::Tab::ForYou {
                    assert!(
                        home.rows.iter().any(|r| r.heading == "Recently played"),
                        "including the rows from the other pages"
                    );
                }
            }
            Err(e) => panic!("{tab:?}: {e}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn why_the_rising_artists_row_is_dropped() {
    // /pages/rising carries an ARTIST_LIST of fifteen, and the tab comes
    // back without it. Either the items yield no card or the row is
    // filtered somewhere.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let body = match client
        .get_raw(
            "/pages/rising",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => return println!("{e}"),
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    for row in v["rows"].as_array().cloned().unwrap_or_default() {
        for m in row["modules"].as_array().cloned().unwrap_or_default() {
            if m["type"].as_str() != Some("ARTIST_LIST") {
                continue;
            }
            let items = m["pagedList"]["items"].as_array().cloned().unwrap_or_default();
            println!("ARTIST_LIST has {} items", items.len());
            if let Some(serde_json::Value::Object(o)) = items.first() {
                let mut k: Vec<&String> = o.keys().collect();
                k.sort();
                println!("  first item keys {k:?}");
                println!("  id={:?} name={:?} title={:?} picture={:?}",
                    o.get("id"), o.get("name"), o.get("title"), o.get("picture"));
            }
        }
    }
    println!("parse_home keeps: {:?}",
        ratidal::browse::parse_home(&body).rows.iter()
            .map(|r| r.heading.clone()).collect::<Vec<_>>());
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_an_artist_page_could_carry() {
    // Read off the web client's own artist page, in its order: fan count
    // and an artist radio in the header, then Top Tracks, Albums, EP &
    // Singles, Playlists, Videos, Fans Also Like, Credits, Appears On.
    // This app has three of those. What else the API will serve:
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let id = 4847816u64; // Kaaris

    for (label, path, query) in [
        ("albums", format!("/artists/{id}/albums"), vec![]),
        (
            "eps & singles",
            format!("/artists/{id}/albums"),
            vec![("filter", "EPSANDSINGLES".to_string())],
        ),
        (
            "appears on",
            format!("/artists/{id}/albums"),
            vec![("filter", "COMPILATIONS".to_string())],
        ),
        ("playlists", format!("/artists/{id}/playlists"), vec![]),
        ("radio", format!("/artists/{id}/radio"), vec![]),
        ("mix", format!("/artists/{id}/mix"), vec![]),
        ("bio", format!("/artists/{id}/bio"), vec![]),
    ] {
        let mut q = vec![
            ("deviceType", "BROWSER".to_string()),
            ("locale", "en_US".to_string()),
            ("limit", "3".to_string()),
        ];
        q.extend(query);
        match client.get_raw(&path, &q).await {
            Ok(b) => {
                let v: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
                let n = v["items"].as_array().map_or(0, |a| a.len());
                let total = v["totalNumberOfItems"].as_u64();
                println!("OK  {label} ({path}): {n} items, total {total:?}");
                if let Some(first) = v["items"].as_array().and_then(|a| a.first()) {
                    println!("      {:?}", first["title"].as_str().or(first["id"].as_str()));
                }
            }
            Err(e) => {
                let m = e.to_string();
                println!("--  {label} ({path}): {}", &m[..m.len().min(50)]);
            }
        }
    }
    // And what the artist object itself carries, for the header.
    if let Ok(b) = client.get_raw(&format!("/artists/{id}"), &[]).await {
        let v: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
        if let serde_json::Value::Object(o) = &v {
            let mut k: Vec<&String> = o.keys().collect();
            k.sort();
            println!("artist keys {k:?}");
        }
        println!("  popularity={:?}", v["popularity"]);
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn hunting_the_fan_count_and_the_bio() {
    // The web page shows "23.8K fans" and the artist's own blurb; the v1
    // artist object has neither. Where they live has to be found rather
    // than assumed missing.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let id = 4847816u64;

    println!("== v2");
    for path in [
        format!("/artists/{id}"),
        format!("/artists/{id}/profile"),
        format!("/artists/{id}/bio"),
        format!("/artists/{id}/stats"),
        format!("/artists/{id}/followers"),
    ] {
        match client.get_raw_v2(&path, &[]).await {
            Ok(b) => {
                let v: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
                if let serde_json::Value::Object(o) = &v {
                    let mut k: Vec<&String> = o.keys().collect();
                    k.sort();
                    println!("  OK {path}: {k:?}");
                } else {
                    println!("  OK {path}: {} bytes", b.len());
                }
            }
            Err(e) => {
                let m = e.to_string();
                println!("  -- {path}: {}", &m[..m.len().min(45)]);
            }
        }
    }

    println!("== the artist page module, which the web draws from");
    match client
        .get_raw(
            "/pages/artist",
            &[
                ("artistId", id.to_string()),
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
    {
        Ok(b) => {
            let v: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
            println!("  {} bytes", b.len());
            for row in v["rows"].as_array().cloned().unwrap_or_default() {
                for m in row["modules"].as_array().cloned().unwrap_or_default() {
                    println!(
                        "    type={:?} title={:?} items={}",
                        m["type"].as_str(),
                        m["title"].as_str(),
                        m["pagedList"]["items"].as_array().map_or(0, |a| a.len())
                    );
                    // An artist header module would carry the blurb and count.
                    if m["type"].as_str() == Some("ARTIST_HEADER") {
                        if let serde_json::Value::Object(o) = &m {
                            let mut k: Vec<&String> = o.keys().collect();
                            k.sort();
                            println!("      header keys {k:?}");
                        }
                    }
                }
            }
        }
        Err(e) => {
            let m = e.to_string();
            println!("  -- /pages/artist: {}", &m[..m.len().min(60)]);
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_the_artist_header_carries() {
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    // Kaaris has no blurb; an artist with a long catalogue is likelier to.
    for (who, id) in [("Kaaris", "4847816"), ("Daft Punk", "12377"), ("Prince", "4847")] {
    let body = match client
        .get_raw(
            "/pages/artist",
            &[
                ("artistId", id.to_string()),
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => { println!("{who}: {e}"); continue }
    };
    println!("-- {who}");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    for row in v["rows"].as_array().cloned().unwrap_or_default() {
        for m in row["modules"].as_array().cloned().unwrap_or_default() {
            if m["type"].as_str() != Some("ARTIST_HEADER") {
                continue;
            }
            println!("title={:?} preTitle={:?}", m["title"].as_str(), m["preTitle"].as_str());
            println!("description={:?}", m["description"].as_str());
            let bio = &m["bio"];
            if let serde_json::Value::Object(o) = bio {
                let mut k: Vec<&String> = o.keys().collect();
                k.sort();
                println!("bio keys {k:?}");
                let text = bio["text"].as_str().unwrap_or("");
                println!("bio text ({} chars): {}", text.len(), &text[..text.len().min(200)]);
            }
            let a = &m["artist"];
            if let serde_json::Value::Object(o) = a {
                let mut k: Vec<&String> = o.keys().collect();
                k.sort();
                println!("artist keys {k:?}");
            }
            println!("artistMix={:?}", m["artistMix"]["id"].as_str());
        }
    }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn whether_the_fan_count_is_anywhere() {
    // The web page reads "23.8K fans" under the name. Nothing on the v1
    // artist or the page header carries it, so this looks for a follower
    // count wherever one might live.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let id = 4847816u64;

    // The page's whole body, searched for the number the web shows.
    if let Ok(b) = client
        .get_raw(
            "/pages/artist",
            &[
                ("artistId", id.to_string()),
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
    {
        for needle in ["fans", "follower", "Follower", "23.8", "23800", "238"] {
            let found = b.contains(needle);
            println!("page body contains {needle:?}: {found}");
        }
    }

    for path in [
        format!("/artists/{id}/followers"),
        format!("/users/{id}/followers"),
    ] {
        match client.get_raw(&path, &[("limit", "1".to_string())]).await {
            Ok(b) => println!("OK {path}: {}", &b[..b.len().min(120)]),
            Err(e) => {
                let m = e.to_string();
                println!("-- {path}: {}", &m[..m.len().min(45)]);
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn the_artist_page_carries_every_section() {
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    for (who, id) in [("Kaaris", 4847816u64), ("Prince", 4847)] {
        match ratidal::library::artist_page(&client, id).await {
            Ok(p) => println!(
                "{who}: name={:?} picture={} bio={} tracks={} albums={} singles={} appears={} similar={} radio={}",
                p.name,
                p.picture.is_some(),
                p.bio.as_ref().map_or(0, |b| b.len()),
                p.top_tracks.len(),
                p.albums.len(),
                p.singles.len(),
                p.appears_on.len(),
                p.similar.len(),
                p.radio.is_some(),
            ),
            Err(e) => println!("{who}: {e}"),
        }
    }
    // The regression this came from: an artist whose page carries Credits
    // and Social parsed to nothing at all.
    let kaaris = ratidal::library::artist_page(&client, 4847816)
        .await
        .expect("Kaaris");
    assert_eq!(kaaris.name, "Kaaris", "the header survives the fuller page");
    assert!(!kaaris.albums.is_empty(), "and every section under it");
    assert!(!kaaris.singles.is_empty());
    assert!(!kaaris.appears_on.is_empty());
    assert!(!kaaris.similar.is_empty());
    assert!(kaaris.radio.is_some(), "with the artist radio from its header");
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn why_kaaris_parses_to_nothing() {
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let body = client
        .get_raw(
            "/pages/artist",
            &[
                ("artistId", "4847816".to_string()),
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
        .expect("page");
    println!("{} bytes", body.len());
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    // How the rows are nested: maybe not rows[].modules[] here.
    if let serde_json::Value::Object(o) = &v {
        let mut k: Vec<&String> = o.keys().collect();
        k.sort();
        println!("top keys {k:?}");
    }
    println!("rows: {:?}", v["rows"].as_array().map(|a| a.len()));
    if let Some(serde_json::Value::Object(o)) =
        v["rows"].as_array().and_then(|a| a.first())
    {
        let mut k: Vec<&String> = o.keys().collect();
        k.sort();
        println!("first row keys {k:?}");
    }
    // Which module fails: try each row on its own.
    for (i, row) in v["rows"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let one = serde_json::json!({"rows": [row]});
        let text = serde_json::to_string(&one).unwrap();
        let got = ratidal::library::parse_artist_page(&text);
        let m = &row["modules"][0];
        let kind = m["type"].as_str().unwrap_or("?");
        println!(
            "  row {i} ({kind}) title={:?}: name={:?} albums={} singles={} appears={} similar={}",
            m["title"].as_str(),
            got.name, got.albums.len(), got.singles.len(),
            got.appears_on.len(), got.similar.len()
        );
        if kind == "ARTIST_HEADER" {
            println!("      artist obj: {}", serde_json::to_string(&m["artist"]).unwrap_or_default());
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn what_the_explore_page_actually_yields() {
    // The Explore section is reported as not working at all. Its modules
    // are PAGE_LINKS_CLOUD, a type nothing else uses.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let body = match client
        .get_raw(
            "/pages/explore",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => return println!("{e}"),
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    for row in v["rows"].as_array().cloned().unwrap_or_default() {
        for m in row["modules"].as_array().cloned().unwrap_or_default() {
            let items = m["pagedList"]["items"].as_array().cloned().unwrap_or_default();
            println!(
                "module type={:?} title={:?} items={}",
                m["type"].as_str(),
                m["title"].as_str(),
                items.len()
            );
            if let Some(serde_json::Value::Object(o)) = items.first() {
                let mut k: Vec<&String> = o.keys().collect();
                k.sort();
                println!("    item keys {k:?}");
                println!("    {}", serde_json::to_string(&items[0]).unwrap_or_default());
            }
        }
    }
    let home = ratidal::browse::explore(&client).await.expect("explore");
    println!("explore() yields {} rows", home.rows.len());
    for row in home.rows.iter() {
        println!("  {:?}: {} cards, more={:?}", row.heading, row.cards.len(), row.more);
        for c in row.cards.iter().take(2) {
            println!(
                "     {:?} target={:?} cover={}",
                c.title, c.target, c.cover_url.is_some()
            );
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn whether_an_explore_link_opens_a_page() {
    // Each explore item carries `apiPath`, e.g. "pages/genre_hip_hop".
    // Whether that answers, and what shape it comes back as, decides
    // whether these cards can open anything.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    for path in ["pages/genre_hip_hop", "pages/mood_djselector", "pages/m_1950s"] {
        match client
            .get_raw(
                &format!("/{path}"),
                &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
            )
            .await
        {
            Ok(b) => {
                let home = ratidal::browse::parse_home(&b);
                println!(
                    "OK {path}: {} bytes, {} rows",
                    b.len(),
                    home.rows.len()
                );
                for row in home.rows.iter().take(4) {
                    println!("    {:?}: {} cards", row.heading, row.cards.len());
                }
            }
            Err(e) => {
                let m = e.to_string();
                println!("-- {path}: {}", &m[..m.len().min(50)]);
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn whether_an_explore_links_image_can_be_fetched() {
    // The items carry `imageId: "hiphop"` rather than a uuid. Whether that
    // composes into a URL the way a cover does has to be tried.
    let http = reqwest::Client::new();
    for candidate in [
        "https://resources.tidal.com/images/hiphop/320x320.jpg",
        "https://resources.tidal.com/images/genres/hiphop/320x320.jpg",
        "https://resources.tidal.com/images/hiphop/160x160.jpg",
    ] {
        match http.get(candidate).send().await {
            Ok(r) => println!("{} {candidate}", r.status()),
            Err(e) => println!("-- {candidate}: {e}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn whether_an_artist_survives_a_mixed_row() {
    // The web's Recently played carries an artist among the albums and
    // playlists — Kendrick Lamar — and this drops it.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let body = match client
        .get_raw(
            "/pages/recently_played",
            &[("deviceType", "BROWSER".to_string()), ("locale", "en_US".to_string())],
        )
        .await
    {
        Ok(b) => b,
        Err(e) => return println!("{e}"),
    };
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    let items = v["rows"][0]["modules"][0]["pagedList"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    println!("{} items in the row", items.len());
    for it in items.iter() {
        let kind = it["type"].as_str().unwrap_or("?");
        let inner = &it["item"];
        println!(
            "  {kind}: title={:?} name={:?} id={:?}",
            inner["title"].as_str(),
            inner["name"].as_str(),
            inner["id"]
        );
    }
    let cards = ratidal::browse::parse_home(&body).rows[0].cards.len();
    println!("{cards} of them became cards");
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_genre_link_from_explore_opens_a_page_of_rows() {
    // The reported bug: opening a genre from Explore does nothing. The unit
    // test for it fills the reply in by hand, so it passes whatever the API
    // does -- this walks the whole path instead: fetch Explore, take the
    // first card that carries a page link, and fetch that page.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let explore = ratidal::browse::explore(&client).await.expect("explore request");
    println!("explore rows: {}", explore.rows.len());
    for row in &explore.rows {
        let links = row
            .cards
            .iter()
            .filter(|c| {
                matches!(c.target, Some(ratidal::shell::carousel::Target::Page(_)))
            })
            .count();
        println!("  {}: {} cards, {links} with a page link", row.heading, row.cards.len());
    }

    assert!(!explore.rows.is_empty(), "Explore came back with no rows at all");

    // The card the user presses enter on.
    let link = explore
        .rows
        .iter()
        .flat_map(|r| r.cards.iter())
        .find_map(|c| match &c.target {
            Some(ratidal::shell::carousel::Target::Page(p)) => {
                Some((c.title.clone(), p.clone()))
            }
            _ => None,
        });
    let (title, path) = link.expect(
        "no card on Explore carries a page link -- every genre would do nothing",
    );
    println!("opening {title:?} at {path:?}");

    let page = ratidal::browse::page_of_rows(&client, &path)
        .await
        .unwrap_or_else(|e| panic!("fetching {path:?} for {title:?} failed: {e}"));

    for row in &page.rows {
        println!("  {}: {:?}, {} cards", row.heading, row.kind, row.cards.len());
    }
    assert!(
        page.rows.iter().any(|r| !r.cards.is_empty()),
        "{title:?} ({path}) came back with nothing to show -- \
         this is what makes opening a genre look like it did nothing"
    );
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_genre_page_is_captured_for_the_fixture() {
    // Writes a genre page to /tmp so its module types can be read rather
    // than guessed -- the video rows have to be told apart by what the API
    // calls them, not by an English title that changes with the locale.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = ratidal::browse::raw_page_body(&client, "pages/genre_hip_hop")
        .await
        .expect("genre request");
    let out = std::env::temp_dir().join("ratidal-genre-hip-hop.json");
    std::fs::write(&out, &body).expect("write capture");
    println!("wrote {} ({} bytes)", out.display(), body.len());
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn no_page_reached_from_explore_keeps_a_row_of_videos() {
    // Videos are dropped by module type. This walks every link Explore
    // offers -- genres, moods, decades -- and asserts none of them comes
    // back with a video row still in it, so a type this app has not seen
    // shows up here rather than on screen.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let explore = ratidal::browse::explore(&client).await.expect("explore");
    let links: Vec<(String, String)> = explore
        .rows
        .iter()
        .flat_map(|r| r.cards.iter())
        .filter_map(|c| match &c.target {
            Some(ratidal::shell::carousel::Target::Page(p)) => {
                Some((c.title.clone(), p.clone()))
            }
            _ => None,
        })
        .collect();
    assert!(!links.is_empty(), "Explore offered no pages to check");

    let mut checked = 0;
    for (title, path) in links.iter().take(8) {
        let raw = match ratidal::browse::raw_page_body(&client, path).await {
            Ok(b) => b,
            Err(e) => {
                println!("  {title}: could not fetch ({e})");
                continue;
            }
        };
        // What the API called every module, before the parse drops any.
        let types: Vec<String> = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v.get("rows").cloned())
            .and_then(|r| r.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .flat_map(|r| {
                r.get("modules")
                    .and_then(|m| m.as_array())
                    .cloned()
                    .unwrap_or_default()
            })
            .filter_map(|m| m.get("type")?.as_str().map(str::to_string))
            .collect();
        let video_modules: Vec<&String> =
            types.iter().filter(|t| t.contains("VIDEO")).collect();

        let page = ratidal::browse::parse_home(&raw);
        println!(
            "  {title}: {} modules ({} video), {} rows kept",
            types.len(),
            video_modules.len(),
            page.rows.len()
        );

        for row in &page.rows {
            assert!(
                !row.heading.to_lowercase().contains("video"),
                "{title} kept a video row: {:?} -- module types were {types:?}",
                row.heading
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "no page could be fetched, so nothing was checked");
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn the_explore_page_is_captured_for_the_fixture() {
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let body = ratidal::browse::raw_page_body(&client, "pages/explore")
        .await
        .expect("explore request");
    let out = std::env::temp_dir().join("ratidal-explore.json");
    std::fs::write(&out, &body).expect("write capture");
    println!("wrote {} ({} bytes)", out.display(), body.len());
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn an_artist_page_carries_a_radio_and_more_top_tracks() {
    // Two reports: R does nothing on an artist, and `o` on Top Tracks does
    // nothing. Both depend on what the live page actually carries, which a
    // hand-written fixture cannot say.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    // An artist from the user's own favourites, so the page is a real one.
    let artists = ratidal::library::artists(&client)
        .await
        .expect("favourite artists");
    let artist = artists.first().expect("no favourite artists to test with");
    println!("artist: {} ({})", artist.name, artist.id);

    let page = ratidal::library::artist_page(&client, artist.id)
        .await
        .expect("artist page");

    println!("  radio: {:?}", page.radio);
    println!("  top tracks: {}", page.top_tracks.len());
    println!("  albums: {}", page.albums.len());
    println!("  singles: {}", page.singles.len());
    println!("  similar: {}", page.similar.len());

    assert!(
        page.radio.is_some(),
        "{}'s page carries no radio, so R has nothing to play",
        artist.name
    );
    // Top Tracks comes back with four of a hundred; the path is how the
    // rest is reached, and a wrong field name here is silent.
    println!("  top tracks path: {:?}", page.top_tracks_path);
    assert!(
        page.top_tracks_path.is_some(),
        "no path behind Top Tracks, so see-all can only show the four drawn"
    );
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn an_artist_page_is_captured_for_the_fixture() {
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);
    let artists = ratidal::library::artists(&client).await.expect("artists");
    let artist = artists.first().expect("no favourite artists");
    let body = ratidal::browse::raw_page_body(
        &client,
        &format!("pages/artist?artistId={}", artist.id),
    )
    .await
    .expect("artist page");
    let out = std::env::temp_dir().join("ratidal-artist.json");
    std::fs::write(&out, &body).expect("write");
    println!("wrote {} for {} ({} bytes)", out.display(), artist.name, body.len());
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn every_see_all_row_comes_back_with_artwork() {
    // "See all" opens a grid of cards, and a card with no cover is drawn as
    // an empty grey square. This asks each row that offers see-all for its
    // whole contents and reports how many came back without artwork --
    // through the real parser, not by reading the JSON by hand.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let mut pages: Vec<(String, ratidal::browse::Home)> = Vec::new();
    for tab in [ratidal::browse::Tab::ForYou] {
        if let Ok(home) = ratidal::browse::tab_page(&client, tab).await {
            pages.push((format!("{tab:?}"), home));
        }
    }
    if let Ok(home) = ratidal::browse::explore(&client).await {
        pages.push(("Explore".into(), home));
    }

    for (where_, home) in &pages {
        for row in &home.rows {
            let Some(path) = row.more.as_deref() else { continue };
            let cards = match ratidal::browse::module_items(&client, path, 150).await {
                Ok(c) => c,
                Err(e) => {
                    println!("  {where_}/{}: fetch failed ({e})", row.heading);
                    continue;
                }
            };
            let blank = cards.iter().filter(|c| c.cover_url.is_none()).count();
            println!(
                "  {where_}/{:<24} kind={:?} {blank}/{} without artwork",
                row.heading,
                row.kind,
                cards.len()
            );
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_track_carries_the_radio_autoplay_follows() {
    // Autoplay needs somewhere to go when the queue runs out. TIDAL's own
    // setting continues "with similar content", and the track itself names
    // that: `mixes.TRACK_MIX` is a mix id like any other, so it is fetched
    // through the path mixes already use.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let tracks = ratidal::library::favourite_tracks(&client)
        .await
        .expect("favourite tracks");
    let with_radio = tracks.iter().find(|t| t.radio.is_some());
    let Some(seed) = with_radio else {
        panic!(
            "not one of {} favourite tracks names a TRACK_MIX, so autoplay \
             has nowhere to go",
            tracks.len()
        );
    };
    println!("seed: {} -> radio {:?}", seed.title, seed.radio);

    let radio = ratidal::library::mix_tracks(&client, seed.radio.as_ref().expect("a radio"))
        .await
        .expect("the radio fetches");
    println!("  {} tracks", radio.len());
    for t in radio.iter().take(3) {
        println!("    {} — {}", t.title, t.artist);
    }
    assert!(!radio.is_empty(), "a radio with no tracks is nowhere to go");
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_track_fetched_on_its_own_carries_its_radio() {
    // The home page sends `mixes: null` on its track cards, so `R` there
    // has no radio to follow. This asks whether fetching the track by id
    // gives one -- if it does, the card can be filled in on demand.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let tracks = ratidal::library::favourite_tracks(&client)
        .await
        .expect("favourite tracks");
    let seed = tracks.first().expect("no favourite tracks");
    println!("track {} ({})", seed.title, seed.id.0);

    match ratidal::library::track(&client, seed.id).await {
        Ok(t) => {
            println!("  radio: {:?}", t.radio);
            assert!(
                t.radio.is_some(),
                "fetching a track by id gives no radio either"
            );
        }
        Err(e) => panic!("no per-track endpoint: {e}"),
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn recently_played_says_what_kind_of_thing_it_holds() {
    // Whether the row belongs in a carousel of covers or a grid of track
    // rows depends on what is in it, which only a real response says.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let body = ratidal::browse::raw_page_body(&client, "pages/recently_played")
        .await
        .expect("recently played");
    let home = ratidal::browse::parse_home(&body);

    for row in &home.rows {
        println!("row {:?}: kind={:?}, {} cards", row.heading, row.kind, row.cards.len());
        for card in row.cards.iter().take(8) {
            println!("    {:?} -> {:?}", card.title, card.target);
        }
    }
}

#[tokio::test]
#[ignore = "needs the network and a signed-in session"]
async fn a_mix_says_whether_it_has_a_cover() {
    // A radio opens with no artwork above it. This asks whether the API has
    // one to give -- the mixes page carries `images` per mix, and the
    // question is whether the mix's own endpoint does too.
    let Some(token) = session() else { return };
    let client = ratidal::tidal::Client::new(token);

    let mixes = ratidal::browse::mixes(&client).await.expect("mixes");
    let mix = mixes
        .mine
        .iter()
        .chain(mixes.radio.iter())
        .next()
        .expect("no mixes to test with");
    println!("mix card: {:?} cover={:?}", mix.title, mix.cover_url);
    assert!(
        mix.cover_url.is_some(),
        "the mixes page gives no cover either, so there is none to draw"
    );
}
