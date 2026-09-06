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

/// What the home page turned out to contain.
#[derive(Debug, Default, Clone)]
pub struct Home {
    pub shortcuts: Vec<Shortcut>,
    pub rows: Vec<(String, Vec<Card>)>,
}

/// The raw home page, for capturing a fixture against the live endpoint.
///
/// The fixture in this repo was captured by hand with only the fields the
/// parser read at the time, which made it useless for finding out why a row
/// had no covers: it could not answer a question about a field it did not
/// contain.
pub async fn home_body(client: &Client) -> Result<String, TidalError> {
    // The /pages/* endpoints reject a request without `deviceType`, with a
    // 400 and "Bad request: deviceType missing" — none of the other endpoints
    // ask for it. BROWSER returns the richest page (the same rows the web
    // client shows); PHONE and TABLET return a smaller one.
    client
        .get(
            "/pages/home",
            &[
                ("deviceType", "BROWSER".to_string()),
                ("locale", "en_US".to_string()),
            ],
        )
        .await
}

pub async fn home(client: &Client) -> Result<Home, TidalError> {
    let body = home_body(client).await?;
    Ok(parse_home(&body))
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
            _ => out.rows.push((module.title, cards)),
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
}

/// One entry in a module. The shape varies by module: an album carousel has
/// the fields inline, while a mix or playlist row wraps them.
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct ItemDto {
    title: String,
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

        Some(Card { title: self.title.clone(), subtitle, cover_url: cover, ..Default::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for (heading, cards) in &home.rows {
            assert!(!heading.is_empty(), "every row keeps its heading");
            assert!(!cards.is_empty(), "an empty row should have been dropped");
            for card in cards {
                assert!(!card.title.is_empty(), "a card without a title is not renderable");
            }

            // The failure this fixture was recaptured for: whole rows drew
            // grey rectangles because their items were tracks, which carry
            // no cover of their own — the artwork is on the album inside.
            // The old fixture had been trimmed to the fields the parser
            // already read, so it could not have caught this.
            assert!(
                cards.iter().any(|c| c.cover_url.is_some()),
                "{heading:?} has {} cards and not one cover — an item type \
                 whose artwork lives under a field the parser does not read",
                cards.len()
            );
        }

        // Albums carry artists; playlists carry a square image.
        let album_row = home
            .rows
            .iter()
            .find(|(h, _)| h == "New Albums")
            .expect("the album row");
        assert!(
            album_row.1[0].cover_url.is_some(),
            "an album card needs its cover"
        );
        assert!(
            !album_row.1[0].subtitle.is_empty(),
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
        let (heading, cards) = &home.rows[0];
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
        assert_eq!(home.rows[0].1[0].title, "My Mix 1");
        assert_eq!(home.rows[0].1[0].subtitle, "Kaaris, Ninho");
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
        assert_eq!(home.rows[0].0, "Real");
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
        assert_eq!(home.rows[0].1[0].title, "Item");
    }
}
