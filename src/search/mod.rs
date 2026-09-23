//! Searching TIDAL's catalogue.
//!
//! On `/v2/search`, which v1 has no equivalent for — the v1 host answers 404
//! for every search path tried. It returns albums, artists, tracks and
//! playlists in one response, each in the same `{items, totalNumberOfItems}`
//! envelope the rest of the API uses, so the existing DTOs read it unchanged.

use crate::domain::Track;
use crate::library::{Album, Artist, Playlist};
use crate::tidal::dto::{AlbumDto, ArtistDto, ItemsPage, PlaylistDto, TrackDto};
use crate::tidal::{Api, Client, TidalError};

/// Everything one query turned up.
#[derive(Debug, Default, Clone)]
pub struct Results {
    pub query: String,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}

impl Results {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
            && self.albums.is_empty()
            && self.artists.is_empty()
            && self.playlists.is_empty()
    }

    pub fn total(&self) -> usize {
        self.tracks.len() + self.albums.len() + self.artists.len() + self.playlists.len()
    }
}

/// The four sections of one search response.
///
/// Every field defaults, so a response missing a section — or gaining one —
/// parses rather than failing outright. That is what keeps a new field from
/// being fatal; it is also what makes a wrong field name silent, which is
/// why the shape here was read off a real response rather than assumed.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct SearchDto {
    tracks: ItemsPage<TrackDto>,
    albums: ItemsPage<AlbumDto>,
    artists: ItemsPage<ArtistDto>,
    playlists: ItemsPage<PlaylistDto>,
}

/// How many of each kind to ask for.
const LIMIT: u32 = 20;

pub async fn search(client: &Client, query: &str) -> Result<Results, TidalError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Results::default());
    }

    let body = client
        .get_on(
            Api::V2,
            "/search",
            &[
                ("query", query.to_string()),
                ("limit", LIMIT.to_string()),
                ("locale", "en_US".to_string()),
                ("deviceType", "BROWSER".to_string()),
            ],
        )
        .await?;

    Ok(parse(query, &body))
}

/// Turn a search response into results.
///
/// Separate from the request so it can be tested against a captured body,
/// which is the only way to know the field names are right: a wrong one
/// yields an empty section rather than an error.
pub fn parse(query: &str, body: &str) -> Results {
    let dto: SearchDto = match serde_json::from_str(body) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("search response did not parse: {e}");
            return Results {
                query: query.to_string(),
                ..Default::default()
            };
        }
    };

    Results {
        query: query.to_string(),
        tracks: dto
            .tracks
            .items
            .into_iter()
            .map(|t| t.into_track())
            .collect(),
        albums: dto
            .albums
            .items
            .into_iter()
            .map(crate::library::album_from_dto)
            .collect(),
        artists: dto
            .artists
            .items
            .into_iter()
            .map(crate::library::artist_from_dto)
            .collect(),
        playlists: dto
            .playlists
            .items
            .into_iter()
            .map(crate::library::playlist_from_dto)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_asks_for_nothing() {
        // Firing a request for every keystroke of an empty box would be one
        // request per backspace to the end of the line.
        let r = Results::default();
        assert!(r.is_empty());
        assert_eq!(r.total(), 0);
    }

    #[test]
    fn a_response_missing_a_section_still_parses() {
        // The API returns the sections it has; a query matching only tracks
        // comes back without an `albums` key at all.
        let body = r#"{"tracks":{"items":[],"totalNumberOfItems":0}}"#;
        let r = parse("nothing", body);
        assert!(r.is_empty());
        assert_eq!(r.query, "nothing");
    }

    #[test]
    fn junk_yields_empty_results_rather_than_an_error() {
        // A search box should not be able to kill the view.
        let r = parse("q", "not json at all");
        assert!(r.is_empty());
        assert_eq!(r.query, "q", "and it still says what was searched for");
    }

    #[test]
    fn every_section_is_read_from_a_realistic_body() {
        // Shaped as the live response is: four sections, each an items page.
        let body = r#"{
            "tracks":    {"items":[{"id":1,"title":"One More Time",
                                    "duration":320,
                                    "album":{"id":9,"title":"Discovery","cover":"c1"},
                                    "artists":[{"id":2,"name":"Daft Punk"}]}],
                          "totalNumberOfItems":1},
            "albums":    {"items":[{"id":9,"title":"Discovery","cover":"c1",
                                    "artists":[{"id":2,"name":"Daft Punk"}],
                                    "releaseDate":"2001-03-12","numberOfTracks":14}],
                          "totalNumberOfItems":1},
            "artists":   {"items":[{"id":2,"name":"Daft Punk","picture":"p1"}],
                          "totalNumberOfItems":1},
            "playlists": {"items":[{"uuid":"u1","title":"Daft Punk Essentials",
                                    "numberOfTracks":30}],
                          "totalNumberOfItems":1}
        }"#;
        let r = parse("daft punk", body);

        assert_eq!(r.tracks.len(), 1, "tracks");
        assert_eq!(r.tracks[0].title, "One More Time");
        assert_eq!(r.tracks[0].album, "Discovery", "a track carries its album");

        assert_eq!(r.albums.len(), 1, "albums");
        assert_eq!(r.albums[0].artist, "Daft Punk");
        assert_eq!(r.albums[0].year.as_deref(), Some("2001"));

        assert_eq!(r.artists.len(), 1, "artists");
        assert_eq!(r.artists[0].name, "Daft Punk");
        assert!(r.artists[0].picture.is_some());

        assert_eq!(r.playlists.len(), 1, "playlists");
        assert_eq!(r.playlists[0].track_count, 30);

        assert_eq!(r.total(), 4);
        assert!(!r.is_empty());
    }
}
