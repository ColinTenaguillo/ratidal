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
    pub explicit: bool,
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
    /// Square cover, when the playlist has one.
    pub image: Option<String>,
    /// The tiled four-cover mosaic the web client shows when it does not.
    #[serde(rename = "squareImage")]
    pub square_image: Option<String>,
    pub creator: PlaylistCreator,
    /// Running time in seconds.
    pub duration: Option<u64>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct PlaylistCreator {
    /// Absent for TIDAL's own editorial playlists, which have no user behind
    /// them; the web client labels those "TIDAL".
    pub name: Option<String>,
}

/// A favourited album. Unlike `AlbumRef`, which is the stub embedded in a
/// track, this is the full listing entry.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct AlbumDto {
    pub id: u64,
    pub title: String,
    pub cover: Option<String>,
    pub artists: Vec<ArtistRef>,
    /// "1981-04-01". Only the year is shown.
    #[serde(rename = "releaseDate")]
    pub release_date: Option<String>,
    #[serde(rename = "numberOfTracks")]
    pub number_of_tracks: u32,
    /// Running time in seconds, as the web client shows beside the count.
    pub duration: Option<u64>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct ArtistDto {
    pub id: u64,
    pub name: String,
    /// Artist pictures live in the same image store as covers but under a
    /// different aspect; the square rendition is the one the web client uses
    /// for these round avatars.
    pub picture: Option<String>,
    /// What TIDAL shows when there is no portrait: the cover of one of the
    /// artist's albums. Plenty of artists have only this — 8ruki, for one —
    /// and the web client draws it rather than an empty circle, so the name
    /// of the field is the API telling us what to do with it.
    #[serde(rename = "selectedAlbumCoverFallback")]
    pub album_cover_fallback: Option<String>,
}

/// Favourites wrap each entry in an `item` object; plain listings do not.
/// The wrapper also carries when the item was added, which the Titres view
/// shows as its "Date d'ajout" column.
#[derive(Debug, serde::Deserialize)]
#[serde(default, bound(deserialize = "T: serde::Deserialize<'de> + Default"))]
pub struct FavouriteEntry<T> {
    pub item: T,
    /// ISO 8601, e.g. "2026-07-17T09:12:44.000+0000".
    pub created: Option<String>,
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
            album: self.album.title,
            duration: Duration::from_secs(self.duration),
            cover: self.album.cover.as_deref().map(|c| cover_url(c, 320)),
            tags: self.media_metadata.tags,
            added: None,
            explicit: self.explicit,
        }
    }
}

impl FavouriteEntry<TrackDto> {
    /// A favourite carries when it was added; the track inside it does not.
    pub fn into_track(self) -> Track {
        let added = self.created;
        Track { added, ..self.item.into_track() }
    }
}

// `#[serde(default)]` needs a Default, and deriving one would demand
// `T: Default` on the struct itself. Only the field defaults are wanted here.
impl<T: Default> Default for FavouriteEntry<T> {
    fn default() -> Self {
        Self { item: T::default(), created: None }
    }
}

/// The track-shaped favourites entry, named for how it reads at call sites.
pub type FavouriteItem = FavouriteEntry<TrackDto>;

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
    fn an_artist_without_a_portrait_falls_back_to_an_album_cover() {
        // TIDAL leaves `picture` null for a large share of artists and puts
        // an album cover in `selectedAlbumCoverFallback` instead — the web
        // client draws that, and reading only `picture` left 72 of 173
        // artists in one real library as blank circles.
        let json = r#"{"id":9922952,"name":"8ruki","picture":null,
                       "selectedAlbumCoverFallback":"aaaa-bbbb-cccc"}"#;
        let dto: ArtistDto = serde_json::from_str(json).unwrap();
        assert!(dto.picture.is_none());
        assert_eq!(dto.album_cover_fallback.as_deref(), Some("aaaa-bbbb-cccc"));
    }

    #[test]
    fn a_real_portrait_wins_over_the_fallback() {
        let json = r#"{"id":5192,"name":"2Pac","picture":"real-portrait",
                       "selectedAlbumCoverFallback":"an-album"}"#;
        let dto: ArtistDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.picture.as_deref(), Some("real-portrait"));
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
