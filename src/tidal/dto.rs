use std::time::Duration;

use crate::domain::{Track, TrackId};

/// TIDAL's uniform pagination envelope.
#[derive(Debug, serde::Deserialize)]
#[serde(default, bound(deserialize = "T: serde::Deserialize<'de>"))]
pub struct ItemsPage<T> {
    pub items: Vec<T>,
    #[serde(rename = "totalNumberOfItems")]
    pub total: u32,
}

impl<T> Default for ItemsPage<T> {
    fn default() -> Self {
        Self { items: Vec::new(), total: 0 }
    }
}

/// Favourites wrap each entry in an `item` object; plain listings do not.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct FavouriteItem {
    pub item: TrackDto,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct TrackDto {
    pub id: u64,
    pub title: String,
    /// Seconds.
    pub duration: u64,
    #[serde(rename = "streamReady")]
    pub stream_ready: bool,
    #[serde(rename = "allowStreaming")]
    pub allow_streaming: bool,
    pub album: AlbumRef,
    pub artists: Vec<ArtistRef>,
    #[serde(rename = "mediaMetadata")]
    pub media_metadata: MediaMetadata,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct AlbumRef {
    pub id: u64,
    pub title: String,
    /// Dashed uuid, or absent.
    pub cover: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct ArtistRef {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct MediaMetadata {
    /// e.g. ["LOSSLESS", "HIRES_LOSSLESS"]
    pub tags: Vec<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct PlaylistDto {
    pub uuid: String,
    pub title: String,
    #[serde(rename = "numberOfTracks")]
    pub number_of_tracks: u32,
}

/// Cover art URL. TIDAL stores a dashed uuid but serves it slash-separated.
pub fn cover_url(uuid: &str, size: u32) -> String {
    let path = uuid.replace('-', "/");
    format!("https://resources.tidal.com/images/{path}/{size}x{size}.jpg")
}

impl TrackDto {
    pub fn into_track(self) -> Track {
        let artist = self
            .artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        Track {
            id: TrackId(self.id),
            title: self.title,
            artist,
            duration: Duration::from_secs(self.duration),
            cover: self.album.cover.as_deref().map(|c| cover_url(c, 320)),
            tags: self.media_metadata.tags,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("tests/fixtures/json/{name}")).unwrap()
    }

    #[test]
    fn parses_favourite_tracks_into_domain_tracks() {
        let page: ItemsPage<FavouriteItem> =
            serde_json::from_str(&fixture("favorites-tracks.json")).unwrap();
        let tracks: Vec<_> = page.items.into_iter().map(|i| i.item.into_track()).collect();

        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].id, crate::domain::TrackId(88070065));
        assert_eq!(tracks[0].title, "1 Thot 2 Thot Red Thot Blue Thot");
        // Multiple artists are joined, as the web client shows them.
        assert_eq!(tracks[0].artist, "An Artist, Another");
        assert_eq!(tracks[0].duration, std::time::Duration::from_secs(195));
        assert!(tracks[0].is_hires());
        assert!(!tracks[1].is_hires());
    }

    #[test]
    fn builds_a_cover_url_from_the_uuid() {
        // TIDAL stores a dashed uuid; the URL needs it slash-separated.
        assert_eq!(
            cover_url("aaaa-bbbb-cccc", 320),
            "https://resources.tidal.com/images/aaaa/bbbb/cccc/320x320.jpg"
        );
    }

    #[test]
    fn a_missing_cover_is_none_not_a_broken_url() {
        let page: ItemsPage<FavouriteItem> =
            serde_json::from_str(&fixture("favorites-tracks.json")).unwrap();
        let tracks: Vec<_> = page.items.into_iter().map(|i| i.item.into_track()).collect();
        assert!(tracks[0].cover.is_some());
        assert!(tracks[1].cover.is_none());
    }

    #[test]
    fn parses_playlists() {
        let page: ItemsPage<PlaylistDto> =
            serde_json::from_str(&fixture("playlists.json")).unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].uuid, "abc-123");
        assert_eq!(page.items[0].number_of_tracks, 206);
    }

    #[test]
    fn unknown_fields_do_not_break_parsing() {
        // The API adds fields without notice; that must never be fatal.
        let json = r#"{"limit":1,"offset":0,"totalNumberOfItems":1,
                       "brandNewField":{"nested":true},
                       "items":[{"uuid":"x","title":"T","numberOfTracks":1,
                                 "anotherNewThing":42}]}"#;
        let page: ItemsPage<PlaylistDto> = serde_json::from_str(json).unwrap();
        assert_eq!(page.items[0].uuid, "x");
    }
}
