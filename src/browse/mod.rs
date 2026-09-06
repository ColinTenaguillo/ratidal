//! Discovery: the home page, already composed by TIDAL into typed modules.
//!
//! `/v1/pages/home` returns the same rows the web client shows, so the
//! carousels are mapped rather than rebuilt. The response is deeply nested and
//! undocumented, so everything here is defensive: an unrecognised module is
//! skipped rather than failing the page, and a row that ends up with no items
//! is dropped instead of rendering as an empty heading.

use crate::shell::carousel::Card;
use crate::shell::home::Shortcut;
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
    /// Tracks, which the web client lays out as a grid of thumbnail rows.
    Tracks,
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
    pub shortcuts: Vec<Shortcut>,
    pub rows: Vec<HomeRow>,
}

/// Which page a home tab shows.
///
/// The web client's three tabs are three separate pages, not one page
/// filtered — changing tab there changes the URL. `/pages/staff_picks`
/// returns exactly the rows the web client shows on that tab, checked
/// against the running client.
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
}

impl Tab {
    /// Every tab, in the order they are drawn.
    pub const ALL: [Tab; 4] = [Tab::ForYou, Tab::StaffPicks, Tab::Rising, Tab::HiRes];

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
        }
    }

    /// The `/pages/*` id this tab reads, if the API serves one.
    pub fn page(&self) -> Option<&'static str> {
        match self {
            Tab::ForYou => Some("/pages/home"),
            Tab::StaffPicks => Some("/pages/staff_picks"),
            Tab::Rising => Some("/pages/rising"),
            Tab::HiRes => Some("/pages/hires"),
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
        .filter_map(|a| a.activity?.album?.to_card())
        .collect()
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

/// Whether a mix of this type belongs in the section at all.
///
/// Video mixes are dropped: this plays audio, and seven of the sixteen on
/// the mixes page were video, which is most of a screenful of things that
/// cannot be played.
fn is_playable_mix(mix_type: &str) -> bool {
    !mix_type.starts_with("VIDEO")
}

/// Whether this is a radio station rather than one of the user's own mixes.
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
    Ok(split_mixes(parse_saved_mixes(&body)))
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

/// The rows of one home tab.
/// Fetch a module's items from its own endpoint.
///
/// The page returns five of these whatever it is asked for; this path
/// honours a limit. Used both to fill a grid and, with a larger limit, to
/// show the whole row.
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
    while (out.len() as u32) < wanted {
        let limit = (wanted - out.len() as u32).min(MAX_PAGE);
        let body = client
            .get(
                &path,
                &[
                    ("deviceType", "BROWSER".to_string()),
                    ("locale", "en_US".to_string()),
                    ("limit", limit.to_string()),
                    ("offset", offset.to_string()),
                ],
            )
            .await?;

        let page = parse_items(&body);
        let got = page.len() as u32;
        out.extend(page);
        // A short page is the end of the module; a page of nothing would
        // otherwise loop forever against an endpoint that ignores `offset`.
        if got < limit {
            break;
        }
        offset += got;
    }
    Ok(out)
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
        items: Vec<ItemDto>,
    }
    let dto: ItemsDto = match serde_json::from_str(body) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("module items did not parse: {e}");
            return Vec::new();
        }
    };
    dto.items.iter().filter_map(ItemDto::to_card).collect()
}

pub async fn tab_page(client: &Client, tab: Tab) -> Result<Home, TidalError> {
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


/// The home rows that live on other pages, in the order the web client
/// draws them.
///
/// `/pages/home` returns five rows; the web client shows a dozen. Its own
/// home is built from `tidal.com/v2/home/feed/static`, which refuses a
/// non-browser client with a 403, and the modules its "View all" links name
/// — CONTINUE_LISTEN_TO, DAILY_MIXES, SUGGESTED_RADIOS_MIXES and the rest —
/// all 404 on api.tidal.com. So the rows are gathered from the pages that
/// do answer, and put in the order the web client draws them:
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
    // Recently played comes back with no heading of its own.
    for row in before.iter_mut() {
        if row.heading.is_empty() {
            row.heading = "Recently played".to_string();
        }
    }

    let mut after = page_rows(client, "/pages/for_you").await.unwrap_or_default();
    // The page names both of its mix rows "Custom mixes"; the web client
    // shows one strip, so they are folded together.
    fold_rows_with_the_same_heading(&mut after);

    let existing: std::collections::HashSet<String> =
        home.rows.iter().map(|r| r.heading.clone()).collect();
    after.retain(|row| !row.heading.is_empty() && !existing.contains(&row.heading));

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
        if row.kind != RowKind::Tracks || row.cards.len() as u32 >= GRID_CARDS {
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
        let cards: Vec<Card> = module
            .paged_list
            .items
            .iter()
            .filter_map(|item| item.to_card())
            .collect();

        if cards.is_empty() {
            continue;
        }

        // The shortcut grid is its own module type; everything else with items
        // is a carousel.
        match module.module_type.as_str() {
            "HIGHLIGHT_MODULE" | "SHORTCUT_LIST" => {
                out.shortcuts.extend(cards.into_iter().map(|c| Shortcut {
                    title: c.title,
                    subtitle: c.subtitle,
                    cover_url: c.cover_url,
                }));
            }
            // TRACK_LIST is the web client's grid of track rows, and so is
            // MIXED_TYPES_LIST — which is what Recently played comes back
            // as, and what the web draws the same way. Everything else with
            // items is a strip of covers.
            _ => {
                let kind = match module.module_type.as_str() {
                    "TRACK_LIST" | "MIXED_TYPES_LIST" => RowKind::Tracks,
                    _ => RowKind::Carousel,
                };
                out.rows.push(HomeRow {
                    heading: module.title,
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
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct ItemDto {
    title: String,
    /// An artist is named rather than titled, and carries a picture rather
    /// than a cover. Without these the Rising page's row of fifteen
    /// artists yielded no cards at all and the row vanished.
    name: Option<String>,
    picture: Option<String>,
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


#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct AlbumRef {
    cover: Option<String>,
    /// A track card carries its album's name so the player and the track
    /// list can show it. Only the cover was read here before, so a track
    /// played from the home page had no album at all.
    title: String,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct ArtistDto {
    name: String,
}

impl ItemDto {
    fn to_card(&self) -> Option<Card> {
        // Unwrap one level of nesting before reading anything.
        if let Some(inner) = &self.item {
            return inner.to_card();
        }
        // An artist has a `name` where everything else has a `title`.
        let title = if self.title.is_empty() {
            self.name.clone().unwrap_or_default()
        } else {
            self.title.clone()
        };
        if title.is_empty() {
            return None;
        }

        // Artists when there are any, otherwise whatever subtitle the module
        // supplied ("Created by me", "Track radio", and so on).
        let subtitle = if self.artists.is_empty() {
            self.sub_title.clone().unwrap_or_default()
        } else {
            self.artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        // Mixes use `image`, albums `cover`, playlists `squareImage`, and a
        // track has none of them — its artwork belongs to its album.
        let cover = self
            .cover
            .as_ref()
            .or(self.square_image.as_ref())
            .or(self.image.as_ref())
            .or(self.picture.as_ref())
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
            .filter(|r| r.kind == RowKind::Tracks)
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
                "New Tracks" | "Spotlighted Uploads" => RowKind::Tracks,
                _ => RowKind::Carousel,
            };
            assert_eq!(*kind, expected, "{heading:?} has the wrong layout");
        }

        assert!(
            kinds.iter().any(|(_, k)| *k == RowKind::Tracks),
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
        assert!(home.shortcuts.is_empty());
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
    fn a_shortcut_module_goes_to_the_grid_not_a_row() {
        let body = r#"{"rows":[{"modules":[{
            "title":"","type":"HIGHLIGHT_MODULE",
            "pagedList":{"items":[
                {"item":{"title":"Coco 3.0","squareImage":"aaaa-bbbb-cccc",
                         "subTitle":"Created by me"}}
            ]}}]}]}"#;

        let home = parse_home(body);
        assert_eq!(home.shortcuts.len(), 1);
        assert!(home.rows.is_empty(), "a shortcut module must not become a carousel");
        assert_eq!(home.shortcuts[0].title, "Coco 3.0");
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
