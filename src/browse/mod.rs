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

/// The raw home page, for capturing a fixture against the live endpoint.
///
/// The fixture in this repo was captured by hand with only the fields the
/// parser read at the time, which made it useless for finding out why a row
/// had no covers: it could not answer a question about a field it did not
/// contain.
pub async fn home_body(client: &Client) -> Result<String, TidalError> {
    page_body(client, Tab::ForYou).await
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
    /// The web client's third tab. Its rows come from a service this API
    /// does not expose: every plausible `/pages/*` id for it returns 404,
    /// and the endpoint the web client uses is restricted to its own
    /// client. So the tab exists and says it has nothing rather than
    /// pretending to be another copy of the first.
    Uploads,
}

impl Tab {
    pub fn from_index(i: usize) -> Self {
        match i {
            1 => Tab::StaffPicks,
            2 => Tab::Uploads,
            _ => Tab::ForYou,
        }
    }

    /// The `/pages/*` id this tab reads, if the API serves one.
    pub fn page(&self) -> Option<&'static str> {
        match self {
            Tab::ForYou => Some("/pages/home"),
            Tab::StaffPicks => Some("/pages/staff_picks"),
            Tab::Uploads => None,
        }
    }
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
    limit: u32,
) -> Result<Vec<Card>, TidalError> {
    // Anything past the ceiling is a 400, not a shorter page — so clamp
    // rather than let a caller's number reach the API and fail the request.
    let limit = limit.min(MAX_PAGE);
    let body = client
        .get(
            &format!("/{}", path.trim_start_matches('/')),
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
                ("limit", limit.to_string()),
                ("offset", "0".to_string()),
            ],
        )
        .await?;
    Ok(parse_items(&body))
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
    fill_track_rows(client, &mut home).await;
    Ok(home)
}

pub async fn home(client: &Client) -> Result<Home, TidalError> {
    let body = home_body(client).await?;
    let mut home = parse_home(&body);
    fill_track_rows(client, &mut home).await;
    Ok(home)
}

/// How many cards a track grid draws.
pub const GRID_CARDS: u32 = 6;

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
            // TRACK_LIST is the web client's 3x3 grid of track rows;
            // everything else with items is a strip of covers.
            _ => {
                let kind = match module.module_type.as_str() {
                    "TRACK_LIST" => RowKind::Tracks,
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
    /// Seconds. Present on tracks; a playlist's is its whole running time,
    /// which is not what a player's progress bar wants.
    duration: Option<u64>,
    /// Albums and tracks are numbered; playlists are uuids.
    id: Option<u64>,
    uuid: Option<String>,
    #[serde(rename = "numberOfTracks")]
    number_of_tracks: Option<u32>,
    #[serde(rename = "cover")]
    cover: Option<String>,
    #[serde(rename = "squareImage")]
    square_image: Option<String>,
    #[serde(rename = "image")]
    image: Option<String>,
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
        if self.title.is_empty() {
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
            .or_else(|| self.album.as_ref()?.cover.as_ref())
            .map(|uuid| cover_url(uuid, 320));

        let target = self.target();
        Some(Card {
            title: self.title.clone(),
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
        let id = self.id?;
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
    use super::*;

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
