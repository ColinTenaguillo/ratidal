//! Discovery: the home page, already composed by TIDAL into typed modules.
//!
//! `/v1/pages/home` returns the same rows the web client shows, so the
//! carousels are mapped rather than rebuilt. The response is deeply nested and
//! undocumented, so everything here is defensive: an unrecognised module is
//! skipped rather than failing the page, and a row that ends up with no items
//! is dropped instead of rendering as an empty heading.

use crate::shell::carousel::Card;
use crate::tidal::dto::cover_url;
use crate::tidal::{Client, TidalError};

/// How a row's items should be laid out.
///
/// The API says which in each module's `type`, so this is read rather than
/// guessed: a TRACK_LIST is a grid of small rows in the web client, not a
/// carousel of covers, and drawing every module the same way made two of
/// the home page's five rows wrong.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Covers in a scrolling strip: albums, playlists, mixes.
    #[default]
    Carousel,
    /// The web client's compact grid: wide cells of thumbnail beside two
    /// lines of text, three across and three deep. `COMPACT_GRID_CARD` on
    /// the v2 feed whatever it holds -- Recently played is one, of albums
    /// and mixes -- and `TRACK_LIST` on a v1 page.
    Compact,
    /// The web client's shortcut grid: the same cells, two deep.
    /// `SHORTCUT_LIST` on the v2 feed, `HIGHLIGHT_MODULE` on a v1 page.
    Shortcuts,
    /// Links to other pages -- the genres, moods and decades on Explore.
    /// These carry no artwork of any kind: their `imageId` is a name like
    /// "hiphop" rather than a uuid, and there is no image behind it. Drawn
    /// as covers they were a row of empty grey squares, so they get the
    /// web client's own shape instead: a rounded pill holding the title.
    Links,
}

impl RowKind {
    /// How many lines of cells a grid of this kind draws, or `None` for a
    /// kind that is not a grid. The web's track grid is three deep, its
    /// shortcut grid two.
    pub fn grid_rows(&self) -> Option<usize> {
        match self {
            RowKind::Compact => Some(crate::shell::trackgrid::ROWS),
            RowKind::Shortcuts => Some(2),
            RowKind::Carousel | RowKind::Links => None,
        }
    }
}

/// One titled row of the home page.
#[derive(Debug, Clone)]
pub struct HomeRow {
    pub heading: String,
    pub kind: RowKind,
    pub cards: Vec<Card>,
    /// Where to ask for more of this row's items.
    ///
    /// A TRACK_LIST module returns five whatever `limit` the page is asked
    /// for — the count is fixed on the server — but it carries a path that
    /// takes one. Without it a grid of six always has a hole in it.
    pub more: Option<String>,
}

/// What the home page turned out to contain.
#[derive(Debug, Default, Clone)]
pub struct Home {
    pub rows: Vec<HomeRow>,
}

/// Which page a home tab shows.
///
/// The web client's three tabs are three separate pages, not one page
/// filtered — changing tab there changes the URL. `/pages/staff_picks`
/// returns exactly the rows the web client shows on that tab, checked
/// against the running client.
///
/// It shows a fourth, Uploads, that this list leaves out: its rows come
/// from a service this API does not expose — every plausible `/pages/*` id
/// for it is a 404, and the endpoint the web client uses is restricted to
/// its own client. A tab that can only ever be empty is worse than no tab.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    #[default]
    ForYou,
    StaffPicks,
    /// What is climbing: playlists, tracks, albums and artists. Not a tab
    /// the web client has — it draws this as its own page — but it is a
    /// page of home-shaped rows that the API serves, and a tab is where
    /// those belong here.
    Rising,
    /// TIDAL's own hi-res selections, which is the tab most worth having
    /// in a client that goes to this much trouble over the stream.
    HiRes,
    /// The web client's third tab: what artists have uploaded themselves.
    /// Only the v2 feed serves it.
    Uploads,
}

impl Tab {
    /// Every tab, in the order they are drawn.
    pub const ALL: [Tab; 5] =
        [Tab::ForYou, Tab::StaffPicks, Tab::Rising, Tab::HiRes, Tab::Uploads];

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(Tab::ForYou)
    }

    /// What the tab strip shows.
    pub fn label(&self) -> &'static str {
        match self {
            Tab::ForYou => "For you",
            Tab::StaffPicks => "Staff Picks",
            Tab::Rising => "Rising",
            Tab::HiRes => "Hi-Res",
            Tab::Uploads => "Uploads",
        }
    }

    /// The v2 feed this tab reads, when there is one: the web client's own
    /// pages, under `/v2/home/feed/`.
    pub fn feed(&self) -> Option<&'static str> {
        match self {
            Tab::ForYou => Some("static"),
            Tab::Uploads => Some("uploads"),
            Tab::StaffPicks | Tab::Rising | Tab::HiRes => None,
        }
    }

    /// The `/pages/*` id this tab reads, if the API serves one.
    pub fn page(&self) -> Option<&'static str> {
        match self {
            Tab::ForYou => Some("/pages/home"),
            Tab::StaffPicks => Some("/pages/staff_picks"),
            Tab::Rising => Some("/pages/rising"),
            Tab::HiRes => Some("/pages/hires"),
            Tab::Uploads => None,
        }
    }
}

/// The activity feed: what the artists you follow have released.
///
/// A different shape from every other endpoint — `{activities, stats}`
/// rather than an items page — and each activity wraps the album it is
/// about. Checked against the running API: every one seen was an album
/// release, so anything else is skipped rather than drawn as a blank card.
pub async fn feed(client: &Client) -> Result<Vec<Card>, TidalError> {
    let body = client
        .get_raw_v2("/feed/activities", &[("limit", FEED_CARDS.to_string())])
        .await?;
    Ok(parse_feed(&body))
}

/// How many releases the feed asks for.
///
/// Fifty left the last row of a wide grid four cards long and the rest of
/// the pane empty: a nine-column grid five rows deep wants forty-five, and
/// the endpoint answers one short of whatever it is asked for.
///
/// It ignores `offset` — asking for fifty at offset fifty returns the same
/// fifty — so there is no walking this a page at a time. The only lever is
/// the limit, and it stops giving more at ninety-nine however much is
/// asked for, so a hundred is the whole feed.
const FEED_CARDS: u32 = 100;

/// The cards of a feed response.
///
/// Separate from the request so it can be tested against a captured body:
/// every field defaults, so a wrong name yields an empty feed rather than
/// an error.
pub fn parse_feed(body: &str) -> Vec<Card> {
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct FeedDto {
        activities: Vec<ActivityDto>,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct ActivityDto {
        #[serde(rename = "followableActivity")]
        activity: Option<InnerDto>,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct InnerDto {
        album: Option<ItemDto>,
        /// When the release happened, which is what the Feed groups by.
        #[serde(rename = "occurredAt")]
        occurred_at: Option<String>,
    }

    let dto: FeedDto = match serde_json::from_str(body) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("the feed did not parse: {e}");
            return Vec::new();
        }
    };
    dto.activities
        .into_iter()
        .filter_map(|a| {
            let activity = a.activity?;
            let day = activity.occurred_at.as_deref().and_then(day_from_iso);
            let mut card = activity.album?.to_card()?;
            card.day = day;
            Some(card)
        })
        .collect()
}

/// The day an ISO-8601 stamp falls on, counted from 1970-01-01.
///
/// Not a date crate: the feed hands back date-only midnight UTC -- checked
/// against a real response -- so there is no zone and no clock to get
/// wrong, and the whole of what the Feed needs is which day two stamps are
/// apart. Anything that does not start with `YYYY-MM-DD` yields `None`,
/// which groups under the oldest heading rather than throwing the card out.
fn day_from_iso(stamp: &str) -> Option<i64> {
    let date = stamp.get(..10)?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// Days from 1970-01-01 to a civil date, by Howard Hinnant's algorithm.
///
/// Integer arithmetic over the proleptic Gregorian calendar, leap years and
/// centuries included: the shift puts the year's start at March so that the
/// leap day lands at the end of a cycle and needs no special case.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// What the Mixes & Radio section shows, in its two tabs.
///
/// The split is `mixType`, not the page: seven of the account's mixes
/// appear on both pages, so taking one page per tab would have listed them
/// twice.
#[derive(Debug, Default, Clone)]
pub struct Mixes {
    /// Built from what the user listens to: DAILY_MIX, plus the discovery
    /// and new-release mixes.
    pub mine: Vec<Card>,
    /// ARTIST_MIX — a station per artist, which is TIDAL's own selection
    /// rather than a mix of the user's listening.
    pub radio: Vec<Card>,
}

/// Whether a module is a row of videos, which this app cannot play.
///
/// VIDEO_LIST is what a genre page's "New Music Videos" comes back as; the
/// prefix covers the neighbours TIDAL adds without warning, the way the
/// mixes page grew VIDEO_DAILY_MIX.
fn is_video_module(module_type: &str) -> bool {
    module_type.starts_with("VIDEO")
}

/// Whether a mix of this type belongs in the section at all.
///
/// Video mixes are dropped: this plays audio, and seven of the sixteen on
/// the mixes page were video, which is most of a screenful of things that
/// cannot be played.
fn is_playable_mix(mix_type: &str) -> bool {
    !mix_type.starts_with("VIDEO")
}

/// Whether this is a radio station rather than one of the user's own mixes.
///
/// ARTIST_MIX is the only type TIDAL returns that is a station rather than
/// a mix built from listening.
fn is_radio(mix_type: &str) -> bool {
    mix_type == "ARTIST_MIX"
}

/// The mixes the user has saved.
///
/// `/my-collection/mixes`, which is the section the web client calls My
/// Mixes — not the page of suggestions behind it. That page was built
/// first, when this endpoint answered with nothing because the account had
/// saved none; it is a catalogue to pick from rather than the user's own.
pub async fn mixes(client: &Client) -> Result<Mixes, TidalError> {
    let body = client
        .get_raw_v2("/my-collection/mixes", &[("limit", MAX_PAGE.to_string())])
        .await?;
    let mut out = split_mixes(parse_saved_mixes(&body));

    // The stations come from elsewhere. Splitting the collection on
    // `mixType` left the tab permanently empty: what the user has saved is
    // DAILY_MIX and TRACK_MIX -- checked against a real account -- and a
    // station is never among it. `/pages/for_you` is where the web client
    // gets them, under a heading of its own.
    if out.radio.is_empty() {
        out.radio = radio_stations(client).await;
    }
    Ok(out)
}

/// The radio stations TIDAL suggests, or nothing if the page cannot be read.
///
/// Best-effort: a station list that fails to load costs the tab its
/// contents, not the section.
async fn radio_stations(client: &Client) -> Vec<Card> {
    let Ok(page) = page_of_rows(client, "/pages/for_you").await else {
        return Vec::new();
    };
    page.rows
        .into_iter()
        .find(|row| is_radio_heading(&row.heading))
        .map(|row| row.cards)
        .unwrap_or_default()
}

/// Whether a row of `/pages/for_you` is the station list.
///
/// Matched on the heading because the page gives every row the same module
/// type: "Radio stations for you" is what it is called, and matching the
/// two words rather than the whole phrase leaves room for it to be
/// reworded around them.
fn is_radio_heading(heading: &str) -> bool {
    let lower = heading.to_lowercase();
    lower.contains("radio") && lower.contains("station")
}

/// Sort mixes into the section's two tabs, dropping what cannot be played.
fn split_mixes(mixes: Vec<(Card, String)>) -> Mixes {
    let mut out = Mixes::default();
    for (card, mix_type) in mixes {
        if !is_playable_mix(&mix_type) {
            continue;
        }
        if is_radio(&mix_type) {
            out.radio.push(card);
        } else {
            out.mine.push(card);
        }
    }
    out
}

/// The mixes of a `/my-collection/mixes` response.
///
/// Each item wraps the mix in `data` alongside when it was added; the mix
/// itself is the same shape the pages use, so it goes through the same
/// parser.
///
/// Separate from the request so it can be tested against a captured body.
pub fn parse_saved_mixes(body: &str) -> Vec<(Card, String)> {
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct CollectionDto {
        items: Vec<SavedDto>,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct SavedDto {
        data: Option<ItemDto>,
    }

    let dto: CollectionDto = match serde_json::from_str(body) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("the saved mixes did not parse: {e}");
            return Vec::new();
        }
    };
    dto.items
        .into_iter()
        .filter_map(|saved| {
            let item = saved.data?;
            let mix_type = item.mix_type.clone()?;
            Some((item.to_card()?, mix_type))
        })
        .collect()
}

/// Any `/pages/*` page, as rows of cards.
///
/// What an Explore link opens: the genres, moods and decades each answer
/// with the same shape the home page does, so they are parsed and drawn by
/// the same code.
pub async fn page_of_rows(client: &Client, path: &str) -> Result<Home, TidalError> {
    let path = format!("/{}", path.trim_start_matches('/'));
    let body = client
        .get(
            &path,
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await?;
    let mut home = parse_home(&body);
    fill_track_rows(client, &mut home).await;
    Ok(home)
}

/// The Explore section's own page.
///
/// The same shape as a home tab — rows of cards — so it is parsed and drawn
/// by the same code. Checked against the running API: four rows, Genres,
/// Moods & Activities, Decades, and one the API leaves unnamed.
pub async fn explore(client: &Client) -> Result<Home, TidalError> {
    let body = client
        .get(
            "/pages/explore",
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await?;
    let mut home = parse_home(&body);
    fill_track_rows(client, &mut home).await;
    Ok(home)
}

/// The raw body of a home tab's page, for capturing a fixture.
pub async fn page_body(client: &Client, tab: Tab) -> Result<String, TidalError> {
    let Some(path) = tab.page() else {
        return Ok(String::new());
    };
    // The /pages/* endpoints reject a request without `deviceType`, with a
    // 400 and "Bad request: deviceType missing" — none of the other endpoints
    // ask for it. BROWSER returns the richest page (the same rows the web
    // client shows); PHONE and TABLET return a smaller one.
    client
        .get(
            path,
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
}

/// The raw body of any page, for capturing a fixture.
pub async fn raw_page_body(client: &Client, path: &str) -> Result<String, TidalError> {
    let path = format!("/{}", path.trim_start_matches('/'));
    client
        .get(
            &path,
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
}

/// Fetch a module's items from its own endpoint.
///
/// The page returns five of these whatever it is asked for; this path
/// honours a limit. Used both to fill a grid and, with a larger limit, to
/// show the whole row.
/// The raw body of a v2 GET, for probing.
///
/// The v1 helper above answers a different shape: the collection endpoints
/// live on v2, and reading one through the wrong API returns something that
/// parses to nothing rather than failing.
pub async fn raw_v2_body(client: &Client, path: &str) -> Result<String, TidalError> {
    client.get_raw_v2(path, &[("limit", MAX_PAGE.to_string())]).await
}

pub async fn module_items(
    client: &Client,
    path: &str,
    wanted: u32,
) -> Result<Vec<Card>, TidalError> {
    let path = format!("/{}", path.trim_start_matches('/'));
    let mut out: Vec<Card> = Vec::new();
    let mut offset = 0u32;

    // Fifty is all the API will serve at once, so anything larger is walked
    // a page at a time. New Tracks is two hundred and thirty-six deep; one
    // request reached the first fifty and stopped there.
    while let Some(limit) = next_page_limit(out.len() as u32, wanted) {
        let query = [
            ("deviceType", "BROWSER".to_string()),
            ("locale", "en_US".to_string()),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
        ];
        // The v2 feed's rows point at v2; a v1 page's at v1. Same shape of
        // reply either way, once the item wrapper is read. v2 answers 400
        // without `platform`, and honours the same limit and offset.
        let body = if is_feed_path(&path) {
            let mut query = query.to_vec();
            query.push(("platform", "WEB".to_string()));
            client.get_raw_v2(&path, &query).await?
        } else {
            client.get(&path, &query).await?
        };

        let page = parse_items(&body);
        let got = page.len() as u32;
        out.extend(page);
        if is_last_page(got, limit) {
            break;
        }
        offset += got;
    }
    Ok(out)
}

/// How many to ask for next, or `None` when there is nothing left to ask.
///
/// Never more than the API will serve at once, and never more than is
/// wanted — asking for the whole of a two-hundred-item module in one go is
/// refused, and asking past what was wanted wastes a request.
fn next_page_limit(have: u32, wanted: u32) -> Option<u32> {
    if have >= wanted {
        return None;
    }
    Some((wanted - have).min(MAX_PAGE))
}

/// Whether a page that returned `got` of the `limit` asked for is the last.
///
/// A short page is the end of the module. An empty one is too, and has to
/// be: an endpoint that ignores `offset` would otherwise be asked for the
/// same nothing for ever.
fn is_last_page(got: u32, limit: u32) -> bool {
    got < limit
}

/// The cards of a bare `{items: [...]}` response.
///
/// Separate from the request so it can be tested against a captured body:
/// every field defaults, so a wrong name yields an empty row rather than an
/// error.
pub fn parse_items(body: &str) -> Vec<Card> {
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct ItemsDto {
        items: Vec<serde_json::Value>,
    }
    let dto: ItemsDto = match serde_json::from_str(body) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("module items did not parse: {e}");
            return Vec::new();
        }
    };
    // A v1 module's items are the objects themselves; the v2 feed's
    // view-all wraps each as `{type, data}`.
    dto.items
        .iter()
        .filter_map(|v| {
            if v.get("data").is_some() {
                feed_item_card(v)
            } else {
                serde_json::from_value::<ItemDto>(v.clone()).ok()?.to_card()
            }
        })
        .collect()
}

/// The rows of one home tab.
pub async fn tab_page(client: &Client, tab: Tab) -> Result<Home, TidalError> {
    // For you and Uploads are the web client's own feeds, one endpoint
    // each with the rows in the web's order. For you keeps the stitched
    // v1 pages as its fallback for the day the feed refuses this client
    // again; Uploads has nothing on v1 to fall back to.
    if let Some(feed) = tab.feed() {
        match home_feed(client, feed).await {
            Ok(home) if !home.rows.is_empty() => return Ok(home),
            Ok(_) => tracing::warn!("the {feed} feed came back empty"),
            Err(e) if tab == Tab::ForYou => tracing::warn!("no home feed ({e}), stitching the pages"),
            Err(e) => return Err(e),
        }
    }
    let body = page_body(client, tab).await?;
    if body.is_empty() {
        return Ok(Home::default());
    }
    let mut home = parse_home(&body);
    // For you is the home page, wherever it is arrived at — going to
    // another tab and back used to drop the rows gathered from the other
    // pages, because only the first load went looking for them.
    if tab == Tab::ForYou {
        add_the_rest_of_the_home_rows(client, &mut home).await;
    }
    fill_track_rows(client, &mut home).await;
    Ok(home)
}

/// A home tab as the web client draws it: `/v2/home/feed/{feed}`, which
/// answers this client once it says which version of the web client it is.
/// Up to two pages joined by a cursor; the second holds the rows below the
/// fold.
pub async fn home_feed(client: &Client, feed: &str) -> Result<Home, TidalError> {
    let mut home = Home::default();
    let mut cursor: Option<String> = None;
    // The feed has been two pages long; a runaway cursor is not worth a
    // fourth request.
    for _ in 0..3 {
        let mut query = vec![
            ("deviceType", "BROWSER".to_string()),
            ("locale", "en_US".to_string()),
            ("platform", "WEB".to_string()),
        ];
        if let Some(c) = cursor.as_ref() {
            query.push(("cursor", c.clone()));
        }
        let body = client.get_raw_v2(&format!("/home/feed/{feed}"), &query).await?;
        let (page, next) = parse_home_feed(&body);
        home.rows.extend(page.rows);
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    Ok(home)
}

/// One page of the v2 feed: its rows, and the cursor to the next page when
/// there is one.
///
/// The feed's own module types, rather than v1's: `COMPACT_GRID_CARD` is
/// the web's grid of small rows, whatever it holds, and the horizontal
/// lists are strips of covers.
/// `SHORTCUT_LIST`, the web's grid of wide cards under the tabs, is a row
/// like the others here, keeping its shape: drawn as a fixed block it
/// could be neither selected nor scrolled past. Every row names where the
/// rest of it lives, as a v2 path.
pub fn parse_home_feed(body: &str) -> (Home, Option<String>) {
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct FeedDto {
        items: Vec<FeedRowDto>,
        page: PageCursor,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct PageCursor {
        cursor: Option<String>,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct FeedRowDto {
        #[serde(rename = "type")]
        kind: String,
        title: String,
        items: Vec<serde_json::Value>,
        #[serde(rename = "viewAll")]
        view_all: Option<String>,
        /// "Because you listened to" names what: an album or artist item.
        header: Option<serde_json::Value>,
    }
    let feed: FeedDto = match serde_json::from_str(body) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("the home feed did not parse: {e}");
            return (Home::default(), None);
        }
    };
    let mut out = Home::default();
    for row in feed.items {
        let cards: Vec<Card> = row.items.iter().filter_map(feed_item_card).collect();
        if cards.is_empty() {
            continue;
        }
        let kind = match row.kind.as_str() {
            "COMPACT_GRID_CARD" => RowKind::Compact,
            "SHORTCUT_LIST" => RowKind::Shortcuts,
            _ => RowKind::Carousel,
        };
        // "Because you listened to" alone says nothing; the web puts the
        // record's name in the heading.
        let heading = match row.header.as_ref().and_then(feed_item_card) {
            Some(what) if !row.title.is_empty() => format!("{} {}", row.title, what.title),
            _ => row.title,
        };
        out.rows.push(HomeRow { heading, kind, cards, more: row.view_all });
    }
    (out, feed.page.cursor)
}

/// Whether a "more of this row" path belongs to v2 rather than to a v1
/// page module: every v2 row names its rest as a `view-all`, under
/// `home/pages/`, `artist/` or `album/`, and those answer only on v2.
fn is_feed_path(path: &str) -> bool {
    path.contains("/view-all")
}


/// The home rows that live on other pages, in the order the web client
/// draws them.
///
/// `/pages/home` returns five rows; the web client shows a dozen. Its own
/// home is `/v2/home/feed/static`, which `home_feed` reads now; this is
/// the fallback for when that refuses, gathering rows from the v1 pages
/// that answer and putting them in the order the web client draws them:
///
///   Recently played, then the five from /pages/home, then the mixes,
///   radio stations and "Because you listened to" from /pages/for_you.
///
/// Best-effort throughout: a page that fails leaves the rows already
/// gathered rather than emptying the home page.
async fn add_the_rest_of_the_home_rows(client: &Client, home: &mut Home) {
    // Before everything: it is what the web client puts at the top, after
    // its shortcut strip.
    let mut before = page_rows(client, "/pages/recently_played")
        .await
        .unwrap_or_default();
    name_the_unnamed(&mut before, "Recently played");

    let mut after = page_rows(client, "/pages/for_you").await.unwrap_or_default();
    // The page names both of its mix rows "Custom mixes"; the web client
    // shows one strip, so they are folded together.
    fold_rows_with_the_same_heading(&mut after);
    drop_rows_already_here(&mut after, &home.rows);

    before.append(&mut home.rows);
    before.append(&mut after);
    home.rows = before;
}

/// The rows of a page, or `None` when it could not be read.
async fn page_rows(client: &Client, path: &str) -> Option<Vec<HomeRow>> {
    match client
        .get(
            path,
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
    {
        Ok(body) => Some(parse_home(&body).rows),
        Err(e) => {
            tracing::warn!("could not load {path}: {e}");
            None
        }
    }
}

/// Whether a row is made of tracks, and so belongs in the grid of track
/// rows rather than in a strip of covers.
///
/// Asked of the cards rather than of the module's name: `MIXED_TYPES_LIST`
/// says only that the row mixes kinds, and Recently played comes back as
/// ten albums, mixes and playlists with no track among them.
fn holds_tracks(cards: &[Card]) -> bool {
    !cards.is_empty()
        && cards.iter().all(|c| {
            matches!(c.target, Some(crate::shell::carousel::Target::Track(_)))
        })
}

/// Give a heading to any row that came back without one.
///
/// Recently played is one: the module carries no title, and a row with an
/// empty heading draws a blank line where its name belongs.
fn name_the_unnamed(rows: &mut [HomeRow], name: &str) {
    for row in rows.iter_mut() {
        if row.heading.is_empty() {
            row.heading = name.to_string();
        }
    }
}

/// Drop the rows another page already carries.
///
/// The pages overlap, and a row listed twice is worse than one missing. A
/// row with no heading goes too — there is nothing to tell it apart by.
fn drop_rows_already_here(incoming: &mut Vec<HomeRow>, existing: &[HomeRow]) {
    let seen: std::collections::HashSet<&str> =
        existing.iter().map(|r| r.heading.as_str()).collect();
    incoming.retain(|row| !row.heading.is_empty() && !seen.contains(row.heading.as_str()));
}

/// Fold rows sharing a heading into the first of them.
///
/// `/pages/for_you` returns "Custom mixes" twice, two and six cards; the
/// web client draws one strip of eight.
fn fold_rows_with_the_same_heading(rows: &mut Vec<HomeRow>) {
    let mut out: Vec<HomeRow> = Vec::new();
    for row in rows.drain(..) {
        match out.iter_mut().find(|r| r.heading == row.heading) {
            Some(first) => first.cards.extend(row.cards),
            None => out.push(row),
        }
    }
    *rows = out;
}

/// How many cards a track grid draws.
pub const GRID_CARDS: u32 = (crate::shell::trackgrid::COLUMNS * crate::shell::trackgrid::ROWS) as u32;

/// The largest page the API will serve.
///
/// `limit=51` is refused with a 400 and "Too big page, max page size is
/// [50]" — checked against the running API, since nothing in the response
/// says so.
pub const MAX_PAGE: u32 = 50;

// A grid asks for its own fill through the same endpoint, so it has to be
// within the ceiling too. Checked here rather than in a test: it is a
// constant, and a test of two constants can only ever pass or fail to
// compile.
const _: () = assert!(GRID_CARDS <= MAX_PAGE);

/// Top up any track row the page returned short.
///
/// The page hands back five items per TRACK_LIST however it is asked, so a
/// grid of six always had a hole in its last cell. A failure here leaves the
/// row as it came: a short row is worse than the page not loading at all.
async fn fill_track_rows(client: &Client, home: &mut Home) {
    for row in &mut home.rows {
        if row.kind != RowKind::Compact || row.cards.len() as u32 >= GRID_CARDS {
            continue;
        }
        let Some(path) = row.more.clone() else { continue };
        match module_items(client, &path, GRID_CARDS).await {
            Ok(cards) if cards.len() > row.cards.len() => row.cards = cards,
            Ok(_) => {}
            Err(e) => tracing::warn!("could not fill {:?}: {e}", row.heading),
        }
    }
}

/// Map the page into rows, ignoring anything unfamiliar.
///
/// Separated from the request so it can be tested against a captured
/// response: this is the part that breaks when TIDAL changes the page.
pub fn parse_home(body: &str) -> Home {
    let page: PageDto = match serde_json::from_str(body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("home page did not parse: {e}");
            return Home::default();
        }
    };

    let mut out = Home::default();
    for module in page.rows.into_iter().flat_map(|r| r.modules) {
        // Videos are dropped wherever they appear. This plays audio, and a
        // genre page carries a whole row of them -- "New Music Videos" on
        // Hip-Hop is fifteen cards that open nothing. Told apart by the
        // module type the API gives them rather than the row's title,
        // which is whatever the locale says.
        if is_video_module(&module.module_type) {
            continue;
        }
        let cards: Vec<Card> = module
            .paged_list
            .items
            .iter()
            .filter_map(|item| item.to_card())
            .collect();

        if cards.is_empty() {
            continue;
        }

        // TRACK_LIST is the web client's grid of track rows and the
        // shortcut grid its wide one; everything else with items is a
        // strip of covers.
        {
            {
                let kind = match module.module_type.as_str() {
                    "TRACK_LIST" => RowKind::Compact,
                    "HIGHLIGHT_MODULE" | "SHORTCUT_LIST" => RowKind::Shortcuts,
                    // Explore's genres, moods and decades: page links with
                    // no artwork behind them.
                    "PAGE_LINKS_CLOUD" | "PAGE_LINKS" => RowKind::Links,
                    // MIXED_TYPES_LIST is what Recently played comes back
                    // as, and what is in it decides how it is drawn rather
                    // than what the module is called: a real response holds
                    // ten albums, mixes and playlists and not one track.
                    // Drawn as a track grid, a card there opened its album
                    // where every other row of the same shape plays a
                    // track -- the row looked like one thing and behaved
                    // like another.
                    "MIXED_TYPES_LIST" if holds_tracks(&cards) => RowKind::Compact,
                    _ => RowKind::Carousel,
                };
                let heading = match module.module_type.as_str() {
                    "HIGHLIGHT_MODULE" | "SHORTCUT_LIST" if module.title.is_empty() => {
                        "Shortcuts".to_string()
                    }
                    _ => module.title,
                };
                out.rows.push(HomeRow {
                    heading,
                    kind,
                    cards,
                    // Every module carries one, and every row has more
                    // behind it than it shows — New Albums has 166 in the
                    // ten it draws.
                    more: module.paged_list.data_api_path,
                });
            }
        }
    }
    out
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct PageDto {
    rows: Vec<RowDto>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct RowDto {
    modules: Vec<ModuleDto>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct ModuleDto {
    title: String,
    #[serde(rename = "type")]
    module_type: String,
    #[serde(rename = "pagedList")]
    paged_list: PagedListDto,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct PagedListDto {
    items: Vec<ItemDto>,
    /// The endpoint that serves this module's items, which unlike the page
    /// itself honours a `limit`.
    #[serde(rename = "dataApiPath")]
    data_api_path: Option<String>,
}

/// One entry in a module. The shape varies by module: an album carousel has
/// the fields inline, while a mix or playlist row wraps them.
#[derive(serde::Deserialize, Default, Clone)]
#[serde(default)]
struct ItemDto {
    title: String,
    /// A v2 mix is titled through these rather than `title`/`subTitle`.
    #[serde(rename = "titleTextInfo")]
    title_text_info: Option<TextInfo>,
    #[serde(rename = "subtitleTextInfo")]
    subtitle_text_info: Option<TextInfo>,
    /// A v2 mix's artwork: a list with a `size` per entry, where v1 keys a
    /// map by size.
    #[serde(rename = "mixImages")]
    mix_images: Option<Vec<SizedImage>>,
    /// An artist is named rather than titled, and carries a picture rather
    /// than a cover. Without these the Rising page's row of fifteen
    /// artists yielded no cards at all and the row vanished.
    name: Option<String>,
    picture: Option<String>,
    /// An artist without a photograph: the record cover the web client
    /// shows in its place. Read only when `picture` is null.
    #[serde(rename = "selectedAlbumCoverFallback")]
    picture_fallback: Option<String>,
    /// Seconds. Present on tracks; a playlist's is its whole running time,
    /// which is not what a player's progress bar wants.
    duration: Option<u64>,
    /// Albums and tracks are numbered; playlists are uuids; a mix's is a
    /// string. One field for all three, since two Rust fields renamed onto
    /// the same JSON key is a duplicate-field error that fails the whole
    /// page rather than the one item.
    id: Option<serde_json::Value>,
    uuid: Option<String>,
    #[serde(rename = "numberOfTracks")]
    number_of_tracks: Option<u32>,
    #[serde(rename = "cover")]
    cover: Option<String>,
    #[serde(rename = "squareImage")]
    square_image: Option<String>,
    #[serde(rename = "image")]
    image: Option<String>,
    /// A mix's artwork: a whole URL per size, rather than the uuid every
    /// other item carries. Reading it as a uuid is what left the Mixes
    /// page empty.
    images: Option<MixImages>,
    /// What kind of mix this is: DAILY_MIX and its siblings are built from
    /// the user's own listening, ARTIST_MIX is a radio station, and
    /// VIDEO_DAILY_MIX is video, which this app cannot play.
    #[serde(rename = "mixType")]
    mix_type: Option<String>,
    /// Where an Explore link goes: "pages/genre_hip_hop" and the like.
    /// These carry no id at all, so without it every card on that page had
    /// nothing to open.
    #[serde(rename = "apiPath")]
    api_path: Option<String>,
    artists: Vec<ArtistDto>,
    #[serde(rename = "subTitle")]
    sub_title: Option<String>,
    /// A track has no cover of its own; it carries its album, and the cover
    /// is on that. Whole rows of tracks came back with grey placeholders
    /// until this was read.
    album: Option<AlbumRef>,
    /// Mixes and playlists nest their real payload here.
    item: Option<Box<ItemDto>>,
}

/// The card of a v2 feed item, which wraps the object v1 puts inline as
/// `{type, data}`.
///
/// Read through a `Value` rather than straight into the struct: TIDAL
/// sends a mix's `type` twice in the same object, and serde refuses a
/// duplicate field where a `Value` keeps the last. The wrapper's `type` is
/// the kind ("ALBUM", "MIX", ...); a mix's own `type` inside `data` is what
/// v1 calls `mixType`.
fn feed_item_card(v: &serde_json::Value) -> Option<Card> {
    let mut dto: ItemDto = serde_json::from_value(v.get("data")?.clone()).ok()?;
    if v["type"].as_str() == Some("MIX") && dto.mix_type.is_none() {
        dto.mix_type = v["data"]["type"].as_str().map(str::to_string);
    }
    dto.to_card()
}

/// A mix's artwork, one entry per size. Only the smallest is wanted: a
/// cover is a few cells across, and the 1500px one is a slow download for
/// the same picture.
#[derive(serde::Deserialize, Default, Debug, Clone)]
struct MixImages {
    #[serde(rename = "SMALL")]
    small: Option<MixImage>,
    #[serde(rename = "MEDIUM")]
    medium: Option<MixImage>,
}

#[derive(serde::Deserialize, Default, Debug, Clone)]
struct MixImage {
    url: Option<String>,
}

#[derive(serde::Deserialize, Default, Debug, Clone)]
#[serde(default)]
struct TextInfo {
    text: String,
}

#[derive(serde::Deserialize, Default, Debug, Clone)]
#[serde(default)]
struct SizedImage {
    size: String,
    url: Option<String>,
}


#[derive(serde::Deserialize, Default, Clone)]
#[serde(default)]
struct AlbumRef {
    cover: Option<String>,
    /// A track card carries its album's name so the player and the track
    /// list can show it. Only the cover was read here before, so a track
    /// played from the home page had no album at all.
    title: String,
}

#[derive(serde::Deserialize, Default, Clone)]
#[serde(default)]
struct ArtistDto {
    name: String,
}

impl ItemDto {
    fn to_card(&self) -> Option<Card> {
        // Mixes and playlists wrap their real payload in `item`; reading the
        // outer object gives an untitled shell and no card at all.
        if let Some(inner) = &self.item {
            return inner.to_card();
        }
        // An artist has a `name` where everything else has a `title`, and
        // a v2 mix has neither.
        let title = if !self.title.is_empty() {
            self.title.clone()
        } else if let Some(name) = self.name.clone().filter(|n| !n.is_empty()) {
            name
        } else {
            self.title_text_info.as_ref().map(|t| t.text.clone()).unwrap_or_default()
        };
        if title.is_empty() {
            return None;
        }

        // Artists when there are any named, otherwise whatever subtitle the
        // module supplied ("Created by me", "Track radio", and so on). A
        // v2 mix lists its artists under other keys -- `artistName`, not
        // `name` -- so its list is present and nameless, and joining it
        // drew ", , , ," where the web draws the mix's own subtitle.
        let names: Vec<&str> = self
            .artists
            .iter()
            .map(|a| a.name.as_str())
            .filter(|n| !n.is_empty())
            .collect();
        let subtitle = if names.is_empty() {
            self.sub_title
                .clone()
                .or_else(|| self.subtitle_text_info.as_ref().map(|t| t.text.clone()))
                .unwrap_or_default()
        } else {
            names.join(", ")
        };

        // Mixes use `image`, albums `cover`, playlists `squareImage`, and a
        // track has none of them — its artwork belongs to its album.
        let cover = self
            .cover
            .as_ref()
            .or(self.square_image.as_ref())
            .or(self.image.as_ref())
            .or(self.picture.as_ref())
            .or(self.picture_fallback.as_ref())
            .or_else(|| self.album.as_ref()?.cover.as_ref())
            .map(|uuid| cover_url(uuid, 320))
            // A mix names a whole URL per size instead, so there is nothing
            // to compose — the smallest is the one a few cells wide wants.
            .or_else(|| {
                let images = self.images.as_ref()?;
                images
                    .small
                    .as_ref()
                    .or(images.medium.as_ref())?
                    .url
                    .clone()
            })
            .or_else(|| {
                let images = self.mix_images.as_ref()?;
                images
                    .iter()
                    .find(|i| i.size == "SMALL")
                    .or_else(|| images.first())?
                    .url
                    .clone()
            });

        let target = self.target();
        Some(Card {
            title,
            subtitle,
            cover_url: cover,
            // A track's album, for the row and the player. Only a track
            // carries a nested album — that is what `target` tells them
            // apart by — so this is empty on everything else without
            // needing to ask which kind it is.
            detail: self.album.as_ref().map(|a| a.title.clone()).unwrap_or_default(),
            // Only a track's duration means anything to the player; a
            // playlist's is the sum of its contents.
            duration: match (&target, self.duration) {
                (Some(crate::shell::carousel::Target::Track(_)), Some(secs)) => {
                    std::time::Duration::from_secs(secs)
                }
                _ => std::time::Duration::ZERO,
            },
            // An artist is a face, and the artist cards everywhere else
            // draw it round. Square here, a row of uploaders read as a
            // row of records.
            round: matches!(target, Some(crate::shell::carousel::Target::Artist(_))),
            target,
            ..Default::default()
        })
    }

    /// What this item opens.
    ///
    /// The API does not label these by kind, so they are told apart by what
    /// they carry: a uuid is a playlist, a nested album means this is a
    /// track, and a track count on a numbered item is an album.
    fn target(&self) -> Option<crate::shell::carousel::Target> {
        use crate::shell::carousel::Target;

        if let Some(uuid) = &self.uuid {
            return Some(Target::Playlist(uuid.clone()));
        }
        // An Explore link has a path and no id of any kind.
        if let Some(path) = &self.api_path {
            return Some(Target::Page(path.clone()));
        }
        // A mix is told apart by its id being a string: nothing else on
        // these pages has one.
        if let Some(id) = self.id.as_ref().and_then(|v| v.as_str()) {
            return Some(Target::Mix(id.to_string()));
        }
        let id = self.id.as_ref()?.as_u64()?;
        if self.album.is_some() {
            return Some(Target::Track(id));
        }
        if self.number_of_tracks.is_some() {
            return Some(Target::Album(id));
        }
        // What is left with a name rather than a title is an artist: the
        // feed's Recently played and Rising's row of fifteen. They opened
        // nothing before.
        if self.name.is_some() {
            return Some(Target::Artist(id));
        }
        None
    }
}

#[cfg(test)]
mod tests {

    /// A saved-mixes response as the API sends one.
    fn saved(entries: &[(&str, &str)]) -> String {
        let items: Vec<String> = entries
            .iter()
            .enumerate()
            .map(|(i, (title, kind))| {
                format!(
                    r#"{{"trn":"trn:mix:m{i}","itemType":"MIX",
                        "addedAt":"2026-09-05T21:02:11.872+0000",
                        "data":{{"id":"m{i}","mixType":"{kind}","title":"{title}",
                                "subTitle":"Some artists",
                                "images":{{"SMALL":{{"url":"https://x/{i}"}}}}}}}}"#
                )
            })
            .collect();
        format!(r#"{{"items":[{}],"cursor":null}}"#, items.join(","))
    }

    #[test]
    fn the_saved_mixes_are_read_out_of_their_wrapper() {
        // The collection wraps each mix in `data` alongside when it was
        // added, which is not the shape the pages use.
        let got = parse_saved_mixes(&saved(&[("My Mix 2", "DAILY_MIX")]));
        assert_eq!(got.len(), 1, "the mix came out of its wrapper");
        assert_eq!(got[0].0.title, "My Mix 2");
        assert_eq!(got[0].0.subtitle, "Some artists");
        assert_eq!(got[0].0.cover_url.as_deref(), Some("https://x/0"));
        assert_eq!(got[0].1, "DAILY_MIX");
        assert!(
            matches!(
                got[0].0.target,
                Some(crate::shell::carousel::Target::Mix(ref id)) if id == "m0"
            ),
            "and it opens, got {:?}",
            got[0].0.target
        );
    }

    #[test]
    fn a_collection_with_no_mixes_is_an_empty_section_not_an_error() {
        // Which is what this account looked like until mixes were saved to
        // it — and what sent the first version of this off to build the
        // page of suggestions behind the section instead.
        let got = parse_saved_mixes(r#"{"items":[],"cursor":null}"#);
        assert!(got.is_empty());
        let split = split_mixes(got);
        assert!(split.mine.is_empty() && split.radio.is_empty());
    }

    fn row(heading: &str, n: usize) -> HomeRow {
        HomeRow {
            heading: heading.into(),
            kind: RowKind::Carousel,
            cards: (0..n)
                .map(|i| Card::new(format!("{heading} {i}"), "artist"))
                .collect(),
            more: None,
        }
    }

    #[test]
    fn a_row_with_no_heading_of_its_own_is_given_one() {
        // Recently played's module carries no title, and a row with an
        // empty heading draws a blank line where its name belongs.
        let mut rows = vec![row("", 3), row("The Hits", 2)];
        name_the_unnamed(&mut rows, "Recently played");
        assert_eq!(rows[0].heading, "Recently played");
        assert_eq!(rows[1].heading, "The Hits", "a named row keeps its name");
    }

    #[test]
    fn a_row_the_page_already_has_is_not_added_twice() {
        // The pages overlap, and a row listed twice is worse than one
        // missing. An unnamed row goes too: there is nothing to tell it
        // apart by, so it would collide with the next one.
        let here = vec![row("The Hits", 2), row("New Albums", 5)];
        let mut incoming = vec![row("Custom mixes", 8), row("The Hits", 2), row("", 1)];
        drop_rows_already_here(&mut incoming, &here);
        let headings: Vec<&str> = incoming.iter().map(|r| r.heading.as_str()).collect();
        assert_eq!(headings, ["Custom mixes"], "only what is new and named");
    }

    #[test]
    fn a_page_is_never_asked_for_more_than_the_api_serves() {
        // `limit=51` is refused outright, and asking past what is wanted
        // spends a request on items nobody will see.
        assert_eq!(next_page_limit(0, 236), Some(MAX_PAGE), "capped at the ceiling");
        assert_eq!(
            next_page_limit(200, 236),
            Some(36),
            "and at what is left when that is less"
        );
        assert_eq!(next_page_limit(0, 6), Some(6), "a small module in one go");
    }

    #[test]
    fn paging_stops_once_it_has_what_was_wanted() {
        // Off by one either way is a wasted request or a short row.
        assert_eq!(next_page_limit(236, 236), None, "exactly enough is enough");
        assert_eq!(next_page_limit(240, 236), None, "and more than enough");
        assert_eq!(next_page_limit(235, 236), Some(1), "one short is one more");
    }

    #[test]
    fn a_short_page_is_the_end_of_the_module() {
        // Including an empty one: an endpoint that ignores `offset` would
        // otherwise be asked for the same nothing for ever.
        assert!(is_last_page(0, 50), "nothing came back");
        assert!(is_last_page(49, 50), "one short of a full page");
        assert!(!is_last_page(50, 50), "a full page means there may be more");
    }

    #[test]
    fn a_row_of_videos_is_dropped_from_a_page() {
        // Straight off the live Hip-Hop page: it carries a VIDEO_LIST row
        // called "New Music Videos", fifteen cards this app cannot play.
        // Matched on the module type rather than the title, which is
        // whatever the locale returns.
        let body = r#"{"rows":[
            {"modules":[{"type":"PLAYLIST_LIST","title":"Playlists",
                "pagedList":{"items":[{"uuid":"u1","title":"A Playlist"}]}}]},
            {"modules":[{"type":"VIDEO_LIST","title":"New Music Videos",
                "pagedList":{"items":[{"id":1,"title":"A Video","album":{"cover":"c"}}]}}]},
            {"modules":[{"type":"ALBUM_LIST","title":"New Albums",
                "pagedList":{"items":[{"id":2,"title":"An Album","numberOfTracks":9}]}}]}
        ]}"#;
        let home = parse_home(body);

        let headings: Vec<&str> = home.rows.iter().map(|r| r.heading.as_str()).collect();
        assert_eq!(
            headings,
            vec!["Playlists", "New Albums"],
            "the video row is gone and the rest are untouched"
        );
    }

    #[test]
    fn the_v2_feed_parses_to_the_same_rows_the_web_draws() {
        // Cut from a captured /v2/home/feed/static: one row of each shape,
        // the wrapper on every item, a mix titled through its text info --
        // and, as TIDAL sends it, that mix's `type` twice over.
        let body = r#"{"items":[
          {"type":"SHORTCUT_LIST","title":"Shortcuts","items":[
            {"type":"ALBUM","data":{"id":20556792,"title":"good kid","numberOfTracks":17,
              "cover":"db5f","artists":[{"name":"Kendrick Lamar"}]}}]},
          {"type":"HORIZONTAL_LIST","title":"Suggested new albums for you",
            "viewAll":"home/pages/NEW_ALBUM_SUGGESTIONS/view-all","items":[
            {"type":"ALBUM","data":{"id":1,"title":"An Album","numberOfTracks":9,"cover":"c1",
              "artists":[{"name":"Someone"}]}}]},
          {"type":"COMPACT_GRID_CARD","title":"Recommended new tracks",
            "viewAll":"home/pages/NEW_TRACK_SUGGESTIONS/view-all","items":[
            {"type":"TRACK","data":{"id":2,"title":"A Track","duration":183,
              "album":{"id":3,"title":"Its Album","cover":"c2"},"artists":[{"name":"Someone"}]}}]},
          {"type":"COMPACT_GRID_CARD","title":"Recently played",
            "viewAll":"home/pages/CONTINUE_LISTEN_TO/view-all","items":[
            {"type":"ARTIST","data":{"id":21221030,"name":"Yamê","picture":"5e3e"}},
            {"type":"ARTIST","data":{"id":65102834,"name":"Mar de Medianoche","picture":null,
              "selectedAlbumCoverFallback":"f277"}}]},
          {"type":"HORIZONTAL_LIST","title":"Custom mixes","items":[
            {"type":"MIX","data":{"type":"DAILY_MIX","id":"0010c3","type":"DAILY_MIX","titleTextInfo":{"text":"My Mix 1"},
              "subtitleTextInfo":{"text":"Created by TIDAL"},
              "artists":[{"artistId":1,"artistName":"Sista Prod"},{"artistId":2,"artistName":"Juice WRLD"}],
              "mixImages":[{"size":"LARGE","url":"http://l"},{"size":"SMALL","url":"http://s"}]}}]},
          {"type":"HORIZONTAL_LIST_WITH_CONTEXT","title":"Because you listened to",
            "header":{"type":"ALBUM","data":{"id":20556792,"title":"good kid","numberOfTracks":17}},
            "items":[{"type":"PLAYLIST","data":{"uuid":"u1","title":"A Playlist","squareImage":"sq"}}]}
        ],"page":{"cursor":"NEXT"}}"#;
        let (home, cursor) = parse_home_feed(body);
        use crate::shell::carousel::Target;

        assert_eq!(cursor.as_deref(), Some("NEXT"));
        assert_eq!(home.rows[0].heading, "Shortcuts", "the grid is a row, so it can be reached");
        assert_eq!(home.rows[0].kind, RowKind::Shortcuts, "and keeps the web's shape");
        assert!(matches!(home.rows[0].cards[0].target, Some(Target::Album(20556792))));
        let home = Home { rows: home.rows.into_iter().skip(1).collect() };

        let headings: Vec<&str> = home.rows.iter().map(|r| r.heading.as_str()).collect();
        assert_eq!(
            headings,
            [
                "Suggested new albums for you",
                "Recommended new tracks",
                "Recently played",
                "Custom mixes",
                "Because you listened to good kid"
            ]
        );
        let kinds: Vec<RowKind> = home.rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [RowKind::Carousel, RowKind::Compact, RowKind::Compact, RowKind::Carousel, RowKind::Carousel],
            "a compact grid is a grid whatever it holds: Recently played is one of albums"
        );
        assert_eq!(
            home.rows[0].more.as_deref(),
            Some("home/pages/NEW_ALBUM_SUGGESTIONS/view-all"),
            "the rest of the row is where the feed says"
        );
        assert!(is_feed_path(home.rows[0].more.as_deref().unwrap()));
        assert!(is_feed_path("artist/ARTIST_TOP_SINGLES/view-all?artistId=1"));
        assert!(!is_feed_path("pages/data/abc"));

        let track = &home.rows[1].cards[0];
        assert!(matches!(track.target, Some(Target::Track(2))));
        assert_eq!(track.detail, "Its Album");
        assert_eq!(track.duration.as_secs(), 183);
        let artist = &home.rows[2].cards[0];
        assert!(matches!(artist.target, Some(Target::Artist(21221030))));
        assert!(artist.round, "an artist is drawn round, as the artist cards are");
        assert!(artist.cover_url.as_deref().unwrap().contains("5e3e"), "the photograph");
        let faceless = &home.rows[2].cards[1];
        assert!(
            faceless.cover_url.as_deref().unwrap().contains("f277"),
            "no photograph: the record cover the web falls back on"
        );
        assert!(faceless.round);

        let mix = &home.rows[3].cards[0];
        assert_eq!(mix.title, "My Mix 1", "titled through its text info");
        assert_eq!(
            mix.subtitle, "Created by TIDAL",
            "its artists are keyed `artistName`, so the web's own subtitle is what shows"
        );
        assert_eq!(mix.cover_url.as_deref(), Some("http://s"), "the small image");
        assert!(matches!(&mix.target, Some(Target::Mix(id)) if id == "0010c3"));
        assert!(matches!(&home.rows[4].cards[0].target, Some(Target::Playlist(u)) if u == "u1"));
    }

    #[test]
    fn a_mixed_row_is_drawn_for_what_is_in_it_not_for_its_module_name() {
        // Recently played comes back as MIXED_TYPES_LIST, and a real
        // response holds ten albums, mixes and playlists with not one track
        // among them. Drawn as a grid of track rows, a card there opened
        // its album while every other row of that shape plays a track --
        // the row looked like one thing and behaved like another.
        let covers = r#"{"rows":[{"modules":[{"type":"MIXED_TYPES_LIST","title":"",
            "pagedList":{"items":[
                {"id":553443990,"title":"GLORY","numberOfTracks":12},
                {"uuid":"edf3","title":"TIDAL's Top Hits"}
            ]}}]}]}"#;
        assert_eq!(
            parse_home(covers).rows[0].kind,
            RowKind::Carousel,
            "albums and playlists belong in a strip of covers"
        );

        // And a mixed row that really is tracks still gets the grid: a
        // track carries its album, which is how one is told apart.
        let tracks = r#"{"rows":[{"modules":[{"type":"MIXED_TYPES_LIST","title":"",
            "pagedList":{"items":[
                {"id":1,"title":"A Track","album":{"id":2,"title":"An Album","cover":"c"}},
                {"id":3,"title":"Another","album":{"id":2,"title":"An Album","cover":"c"}}
            ]}}]}]}"#;
        assert_eq!(
            parse_home(tracks).rows[0].kind,
            RowKind::Compact,
            "a row that is all tracks is still a track grid"
        );
    }

    #[test]
    fn a_cloud_of_page_links_is_a_row_of_links_not_of_covers() {
        // Straight off the live Explore page. These items carry no artwork:
        // `imageId` is a name like "hiphop" rather than a uuid, and nothing
        // is served for it -- so read as a carousel they drew a row of
        // empty grey squares.
        let body = r#"{"rows":[
            {"modules":[{"type":"PAGE_LINKS_CLOUD","title":"Genres",
                "pagedList":{"items":[
                    {"title":"Hip-Hop","icon":"hiphop","apiPath":"pages/genre_hip_hop","imageId":"hiphop"},
                    {"title":"Pop","icon":"pop","apiPath":"pages/genre_pop","imageId":"pop"}
                ]}}]},
            {"modules":[{"type":"PAGE_LINKS","title":"",
                "pagedList":{"items":[
                    {"title":"New","icon":"new","apiPath":"pages/explore_new_music","imageId":null}
                ]}}]},
            {"modules":[{"type":"ALBUM_LIST","title":"New Albums",
                "pagedList":{"items":[{"id":2,"title":"An Album","numberOfTracks":9}]}}]}
        ]}"#;
        let home = parse_home(body);

        let kinds: Vec<RowKind> = home.rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            vec![RowKind::Links, RowKind::Links, RowKind::Carousel],
            "both kinds of link cloud are links; the album row is untouched"
        );
        assert!(
            home.rows[0].cards.iter().all(|c| c.cover_url.is_none()),
            "and they carry no cover to draw"
        );
    }

    #[test]
    fn an_explore_link_opens_the_page_it_names() {
        // The Explore items carry a path and no id of any kind — no album
        // id, no uuid, nothing — so every card on that page had nothing to
        // open and the section did nothing at all.
        let body = r#"{"rows":[{"modules":[{"type":"PAGE_LINKS_CLOUD","title":"Genres",
            "pagedList":{"items":[
                {"apiPath":"pages/genre_hip_hop","icon":"hiphop",
                 "imageId":"hiphop","title":"Hip-Hop"}
            ]}}]}]}"#;
        let home = parse_home(body);
        let card = &home.rows[0].cards[0];
        assert_eq!(card.title, "Hip-Hop");
        assert!(
            matches!(
                card.target,
                Some(crate::shell::carousel::Target::Page(ref p))
                    if p == "pages/genre_hip_hop"
            ),
            "it opens the page it names, got {:?}",
            card.target
        );
    }

    #[test]
    fn an_artist_is_named_rather_than_titled() {
        // The Rising page carries a row of fifteen artists and it came back
        // empty: an artist has `name` where every other item has `title`,
        // and `picture` where they have `cover`, so every card was dropped
        // and the row with it.
        let body = r#"{"rows":[{"modules":[{"type":"ARTIST_LIST","title":"Artists",
            "pagedList":{"items":[
                {"id":53407296,"name":"girlsweetvoiced",
                 "picture":"66cb2fb6-7f2c-4758-8a88-991b23d22082"}
            ]}}]}]}"#;
        let home = parse_home(body);
        assert_eq!(home.rows.len(), 1, "the row survived");
        let cards = &home.rows[0].cards;
        assert_eq!(cards.len(), 1, "and its artist");
        assert_eq!(cards[0].title, "girlsweetvoiced");
        assert!(
            cards[0].cover_url.is_some(),
            "with the picture as its artwork"
        );
    }

    #[test]
    fn the_two_custom_mix_rows_are_drawn_as_one() {
        // /pages/for_you returns "Custom mixes" twice, two cards and six;
        // the web client draws one strip of eight.
        let mut rows = vec![
            row("Custom mixes", 2),
            row("Radio stations for you", 15),
            row("Custom mixes", 6),
        ];
        fold_rows_with_the_same_heading(&mut rows);

        assert_eq!(rows.len(), 2, "the two mix rows folded into one");
        assert_eq!(rows[0].heading, "Custom mixes");
        assert_eq!(rows[0].cards.len(), 8, "with all eight cards");
        assert_eq!(
            rows[1].heading, "Radio stations for you",
            "and the row between them kept its place"
        );
    }

    #[test]
    fn folding_leaves_distinct_rows_alone() {
        let mut rows = vec![row("A", 1), row("B", 2), row("C", 3)];
        fold_rows_with_the_same_heading(&mut rows);
        let headings: Vec<&str> = rows.iter().map(|r| r.heading.as_str()).collect();
        assert_eq!(headings, ["A", "B", "C"]);
    }

    #[test]
    fn a_saved_video_mix_is_not_offered() {
        // Nothing stops a video mix being saved, and this app cannot play
        // one — a card that does nothing is worse than no card.
        let split = split_mixes(parse_saved_mixes(&saved(&[
            ("My Mix 1", "DAILY_MIX"),
            ("My Video Mix 1", "VIDEO_DAILY_MIX"),
            ("Miley Cyrus", "ARTIST_MIX"),
        ])));
        assert_eq!(split.mine.len(), 1, "the daily mix");
        assert_eq!(split.radio.len(), 1, "the station");
        assert!(
            !split.mine.iter().chain(split.radio.iter())
                .any(|c| c.title.contains("Video")),
            "and no video mix anywhere"
        );
    }

    #[test]
    fn the_station_row_is_found_by_its_heading() {
        // The stations are not in the user's collection at all: what is
        // saved there is DAILY_MIX and TRACK_MIX, checked against a real
        // account, so splitting it on `mixType` left the Radio tab empty
        // whatever the account held. They come from `/pages/for_you`,
        // where every row has the same module type and only the heading
        // says which is which.
        assert!(is_radio_heading("Radio stations for you"));
        assert!(is_radio_heading("RADIO STATIONS"), "however it is cased");
        assert!(
            !is_radio_heading("Custom mixes"),
            "the mixes beside it are not stations"
        );
        assert!(
            !is_radio_heading("Because you listened to Daft Punk"),
            "nor the suggestions under them"
        );
    }

    #[test]
    fn a_station_is_told_from_the_users_own_mixes() {
        // ARTIST_MIX is TIDAL's selection for an artist; the daily,
        // discovery and new-release mixes are built from what the user
        // listens to. They are different things, so they get a tab each.
        for (kind, radio) in [
            ("ARTIST_MIX", true),
            ("DAILY_MIX", false),
            ("DISCOVERY_MIX", false),
            ("NEW_RELEASE_MIX", false),
        ] {
            assert_eq!(is_radio(kind), radio, "{kind}");
        }
    }

    use super::*;

    #[test]
    fn the_feed_reads_the_album_out_of_each_activity() {
        // A shape of its own: `{activities, stats}`, each activity wrapping
        // the album it is about. Every field defaults, so a wrong name here
        // is an empty feed rather than an error.
        let body = r#"{
            "activities": [
                {"seen": false, "followableActivity": {
                    "activityType": "ALBUM_RELEASE",
                    "occurredAt": "2026-09-01T00:00:00.000+0000",
                    "album": {"id": 9, "title": "Discovery", "cover": "c1",
                              "numberOfTracks": 14,
                              "artists": [{"id": 2, "name": "Daft Punk"}]}
                }}
            ],
            "stats": {}
        }"#;
        let cards = parse_feed(body);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Discovery");
        assert_eq!(cards[0].subtitle, "Daft Punk", "the artist comes through");
        assert!(matches!(
            cards[0].target,
            Some(crate::shell::carousel::Target::Album(9))
        ));
        // The date the Feed groups by. Thrown away before, which left the
        // sections with nothing to sort on.
        assert_eq!(
            cards[0].day,
            Some(days_from_civil(2026, 9, 1)),
            "the release date comes through"
        );
    }

    #[test]
    fn a_date_is_read_as_the_day_it_falls_on() {
        // Days from the epoch, by arithmetic rather than a date crate: the
        // feed hands back date-only midnight UTC, so there is no zone to
        // get wrong. The leap day is the case that catches a wrong formula.
        assert_eq!(day_from_iso("1970-01-01T00:00:00.000+0000"), Some(0));
        assert_eq!(day_from_iso("1970-01-02T00:00:00.000+0000"), Some(1));
        // 2000 is a leap year, 1900 was not: the century rule.
        assert_eq!(
            day_from_iso("2000-03-01").unwrap() - day_from_iso("2000-02-28").unwrap(),
            2,
            "the leap day falls between them"
        );
        assert_eq!(
            day_from_iso("2001-03-01").unwrap() - day_from_iso("2001-02-28").unwrap(),
            1,
            "and there is none in an ordinary year"
        );
        // Anything else is no date at all rather than a wrong one.
        assert_eq!(day_from_iso("not a date"), None);
        assert_eq!(day_from_iso("2026-13-01"), None, "month out of range");
        assert_eq!(day_from_iso(""), None);
    }

    #[test]
    fn an_activity_about_something_else_is_skipped() {
        // Every activity seen was an album release, but the type is a field
        // rather than a promise: one without an album should be left out
        // rather than drawn as a blank card.
        let body = r#"{"activities":[
            {"seen": true, "followableActivity": {"activityType": "SOMETHING_ELSE"}},
            {"seen": true}
        ]}"#;
        assert!(parse_feed(body).is_empty());
    }

    #[test]
    fn junk_from_the_feed_is_empty_rather_than_a_panic() {
        assert!(parse_feed("not json").is_empty());
        assert!(parse_feed("{}").is_empty());
    }

    #[test]
    fn a_track_row_carries_where_to_ask_for_more_of_it() {
        // The page returns five items per TRACK_LIST however it is asked,
        // so a grid of six always had a hole in it; the module's own path
        // honours a limit. Without this the row cannot be filled and "See
        // all" has nowhere to go.
        let body = std::fs::read_to_string("tests/fixtures/json/pages-home.json")
            .expect("fixture");
        let home = parse_home(&body);

        let tracks: Vec<&HomeRow> = home
            .rows
            .iter()
            .filter(|r| r.kind == RowKind::Compact)
            .collect();
        assert!(!tracks.is_empty(), "the fixture has track rows");
        for row in tracks {
            let path = row.more.as_deref().unwrap_or_default();
            assert!(
                path.starts_with("pages/data/"),
                "{:?} carries its data path, found {path:?}",
                row.heading
            );
        }

        // Carousels carry one too: every module on the page has more behind
        // it than it shows, and "See all" opens any of them.
        for row in home.rows.iter().filter(|r| r.kind == RowKind::Carousel) {
            assert!(
                row.more.is_some(),
                "{:?} carries a paging path as well",
                row.heading
            );
        }
    }

    #[test]
    fn a_short_page_ends_the_walk() {
        // The loop stops when a page comes back smaller than it asked for.
        // Without that an endpoint returning nothing would be asked forever.
        let full: Vec<String> = (0..MAX_PAGE)
            .map(|i| format!(r#"{{"id":{i},"title":"T{i}","duration":1,
                 "album":{{"id":9,"title":"A","cover":"c"}},
                 "artists":[{{"id":2,"name":"X"}}]}}"#))
            .collect();
        let body = format!(r#"{{"items":[{}]}}"#, full.join(","));
        assert_eq!(
            parse_items(&body).len(),
            MAX_PAGE as usize,
            "a full page parses whole"
        );

        // And an empty one yields nothing rather than looping.
        assert!(parse_items(r#"{"items":[]}"#).is_empty());
    }

    #[test]
    fn a_modules_items_parse_from_its_own_endpoint() {
        // A different shape from the page: a bare `{items: [...]}` rather
        // than modules inside rows.
        let body = r#"{"items":[
            {"id":1,"title":"One More Time","duration":320,
             "album":{"id":9,"title":"Discovery","cover":"c1"},
             "artists":[{"id":2,"name":"Daft Punk"}]},
            {"id":2,"title":"Aerodynamic","duration":212,
             "album":{"id":9,"title":"Discovery","cover":"c1"},
             "artists":[{"id":2,"name":"Daft Punk"}]}
        ]}"#;
        let cards = parse_items(body);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].title, "One More Time");
        assert_eq!(cards[0].detail, "Discovery", "the album comes through");
        assert!(matches!(
            cards[0].target,
            Some(crate::shell::carousel::Target::Track(1))
        ));
    }

    #[test]
    fn junk_from_a_module_endpoint_is_an_empty_row_not_a_panic() {
        assert!(parse_items("not json").is_empty());
        assert!(parse_items("{}").is_empty());
    }

    #[test]
    fn a_track_card_carries_its_album() {
        // The nested album was read for its cover but not its name, so a
        // track played from the home page reached the player and the track
        // list with an empty album column.
        let body = std::fs::read_to_string("tests/fixtures/json/pages-home.json")
            .expect("fixture");
        let home = parse_home(&body);

        let card = home
            .rows
            .iter()
            .flat_map(|r| r.cards.iter())
            .find(|c| c.title.starts_with("CUERPO (TUMBAO)"))
            .expect("a track card from the fixture");

        assert!(
            card.detail.starts_with("B'DAY"),
            "the card carries its album, found {:?}",
            card.detail
        );
        assert!(
            matches!(card.target, Some(crate::shell::carousel::Target::Track(_))),
            "and it is a track card"
        );
    }

    #[test]
    fn only_a_track_card_carries_an_album_name() {
        // `detail` is the playlist grid's third line elsewhere, so an album
        // name on the wrong card would print under the card's own title. A
        // nested album is exactly what marks an item as a track, so the two
        // cannot come apart — this holds that rule.
        let body = std::fs::read_to_string("tests/fixtures/json/pages-home.json")
            .expect("fixture");
        let home = parse_home(&body);

        let mut tracks_with_album = 0;
        for card in home.rows.iter().flat_map(|r| r.cards.iter()) {
            let is_track =
                matches!(card.target, Some(crate::shell::carousel::Target::Track(_)));
            if is_track {
                tracks_with_album += usize::from(!card.detail.is_empty());
            } else {
                assert!(
                    card.detail.is_empty(),
                    "{:?} is not a track but carries {:?}",
                    card.title,
                    card.detail
                );
            }
        }
        assert!(tracks_with_album > 0, "the fixture has track cards with albums");
    }

    #[test]
    fn a_track_module_is_marked_as_tracks_and_the_rest_as_carousels() {
        // The API says which layout a module wants in its `type`, and every
        // module was being drawn as a carousel regardless — so the two
        // TRACK_LIST rows of the home page were wrong, where the web client
        // lays them out as a grid of track rows.
        let body = std::fs::read_to_string("tests/fixtures/json/pages-home.json")
            .expect("fixture");
        let home = parse_home(&body);

        let kinds: Vec<(&str, RowKind)> = home
            .rows
            .iter()
            .map(|r| (r.heading.as_str(), r.kind))
            .collect();
        assert!(!kinds.is_empty(), "the captured page has rows");

        for (heading, kind) in &kinds {
            let expected = match *heading {
                "New Tracks" | "Spotlighted Uploads" => RowKind::Compact,
                _ => RowKind::Carousel,
            };
            assert_eq!(*kind, expected, "{heading:?} has the wrong layout");
        }

        assert!(
            kinds.iter().any(|(_, k)| *k == RowKind::Compact),
            "the fixture covers the track case: {kinds:?}"
        );
        assert!(
            kinds.iter().any(|(_, k)| *k == RowKind::Carousel),
            "and the carousel case: {kinds:?}"
        );
    }

    #[test]
    fn a_track_card_carries_the_duration_the_player_needs() {
        // Playing from a card is all the player knows until the stream
        // starts. Without a duration the progress bar had nothing to divide
        // by: it showed 0:00 and drew itself full over a track that had
        // just begun.
        let body = std::fs::read_to_string("tests/fixtures/json/pages-home.json")
            .expect("fixture");
        let home = parse_home(&body);

        let tracks: Vec<&Card> = home
            .rows
            .iter()
            .flat_map(|row| &row.cards)
            .filter(|c| matches!(c.target, Some(crate::shell::carousel::Target::Track(_))))
            .collect();
        assert!(!tracks.is_empty(), "the captured page has track cards");
        for card in tracks {
            assert!(
                !card.duration.is_zero(),
                "{:?} is a track card with no duration",
                card.title
            );
        }
    }

    #[test]
    fn only_tracks_carry_a_playable_duration() {
        // A playlist's duration is the sum of its contents, which is not
        // what a player's progress bar wants.
        let body = std::fs::read_to_string("tests/fixtures/json/pages-home.json")
            .expect("fixture");
        for row in parse_home(&body).rows {
            for card in &row.cards {
                if !matches!(card.target, Some(crate::shell::carousel::Target::Track(_))) {
                    assert!(
                        card.duration.is_zero(),
                        "{:?} is not a track but carries a duration",
                        card.title
                    );
                }
            }
        }
    }

    #[test]
    fn the_real_captured_page_produces_rows() {
        // Captured from the live endpoint. Every other test here uses hand
        // written JSON, which is exactly how the parse came to be written
        // against a shape TIDAL does not return — the fields were right, but
        // nothing verified that against a real response.
        let body = std::fs::read_to_string("tests/fixtures/json/pages-home.json")
            .expect("fixture");
        let home = parse_home(&body);

        assert!(!home.rows.is_empty(), "the captured page must yield rows");
        for row in &home.rows {
            assert!(!row.heading.is_empty(), "every row keeps its heading");
            assert!(!row.cards.is_empty(), "an empty row should have been dropped");
            for card in &row.cards {
                assert!(!card.title.is_empty(), "a card without a title is not renderable");
            }

            // The failure this fixture was recaptured for: whole rows drew
            // grey rectangles because their items were tracks, which carry
            // no cover of their own — the artwork is on the album inside.
            // The old fixture had been trimmed to the fields the parser
            // already read, so it could not have caught this.
            assert!(
                row.cards.iter().any(|c| c.cover_url.is_some()),
                "{:?} has {} cards and not one cover — an item type whose \
                 artwork lives under a field the parser does not read",
                row.heading,
                row.cards.len()
            );
        }

        // Albums carry artists; playlists carry a square image.
        let album_row = home
            .rows
            .iter()
            .find(|row| row.heading == "New Albums")
            .expect("the album row");
        assert!(
            album_row.cards[0].cover_url.is_some(),
            "an album card needs its cover"
        );
        assert!(
            !album_row.cards[0].subtitle.is_empty(),
            "an album card needs its artists"
        );
    }

    #[test]
    fn an_unparseable_page_yields_an_empty_home_rather_than_an_error() {
        // TIDAL can change this page's shape without notice. An empty home is
        // recoverable; a hard error on startup is not.
        let home = parse_home("not json at all");
        assert!(home.rows.is_empty());
    }

    #[test]
    fn a_carousel_module_becomes_a_titled_row() {
        let body = r#"{"rows":[{"modules":[{
            "title":"New albums for you",
            "type":"ALBUM_LIST",
            "pagedList":{"items":[
                {"title":"August 26","cover":"aaaa-bbbb-cccc",
                 "artists":[{"name":"Post Malone"}]},
                {"title":"DILLAGENCE II","cover":"dddd-eeee-ffff",
                 "artists":[{"name":"Busta Rhymes"},{"name":"J Dilla"}]}
            ]}}]}]}"#;

        let home = parse_home(body);
        assert_eq!(home.rows.len(), 1);
        let (heading, cards) = (&home.rows[0].heading, &home.rows[0].cards);
        assert_eq!(heading, "New albums for you");
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].title, "August 26");
        assert_eq!(cards[0].subtitle, "Post Malone");
        assert_eq!(cards[1].subtitle, "Busta Rhymes, J Dilla");
        assert!(cards[0].cover_url.as_deref().unwrap().contains("aaaa/bbbb/cccc"));
    }

    #[test]
    fn a_nested_item_is_unwrapped() {
        // Mixes and playlists wrap their payload one level down.
        let body = r#"{"rows":[{"modules":[{
            "title":"Mixes for you","type":"MIX_LIST",
            "pagedList":{"items":[
                {"item":{"title":"My Mix 1","image":"1111-2222-3333",
                         "subTitle":"Kaaris, Ninho"}}
            ]}}]}]}"#;

        let home = parse_home(body);
        assert_eq!(home.rows.len(), 1);
        assert_eq!(home.rows[0].cards[0].title, "My Mix 1");
        assert_eq!(home.rows[0].cards[0].subtitle, "Kaaris, Ninho");
    }

    #[test]
    fn a_shortcut_module_is_a_row_named_for_what_it_is() {
        // A fixed grid under the tabs could be neither selected nor
        // scrolled past; a row can. The module comes untitled.
        let body = r#"{"rows":[{"modules":[{
            "title":"","type":"HIGHLIGHT_MODULE",
            "pagedList":{"items":[
                {"item":{"title":"Coco 3.0","squareImage":"aaaa-bbbb-cccc",
                         "subTitle":"Created by me"}}
            ]}}]}]}"#;

        let home = parse_home(body);
        assert_eq!(home.rows.len(), 1);
        assert_eq!(home.rows[0].heading, "Shortcuts");
        assert_eq!(home.rows[0].kind, RowKind::Shortcuts, "the web's wide grid, two deep");
        assert_eq!(home.rows[0].cards[0].title, "Coco 3.0");
    }

    #[test]
    fn an_empty_module_is_dropped_rather_than_shown_as_a_bare_heading() {
        let body = r#"{"rows":[{"modules":[
            {"title":"Empty","type":"ALBUM_LIST","pagedList":{"items":[]}},
            {"title":"Real","type":"ALBUM_LIST","pagedList":{"items":[
                {"title":"An album","cover":"a-b-c"}]}}
        ]}]}"#;

        let home = parse_home(body);
        assert_eq!(home.rows.len(), 1);
        assert_eq!(home.rows[0].heading, "Real");
    }

    #[test]
    fn unknown_fields_and_modules_do_not_break_the_page() {
        // The whole point of the defensive parse: a new module type or field
        // must not cost the user their home page.
        let body = r#"{"rows":[{"modules":[
            {"title":"Something new","type":"A_TYPE_FROM_THE_FUTURE",
             "brandNewField":{"nested":true},
             "pagedList":{"items":[{"title":"Item","cover":"a-b-c",
                                    "anotherNewThing":42}]}}
        ]}],"alsoNew":"ignored"}"#;

        let home = parse_home(body);
        assert_eq!(home.rows.len(), 1, "an unknown module type still renders as a row");
        assert_eq!(home.rows[0].cards[0].title, "Item");
    }
}
