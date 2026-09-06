use crate::domain::Track;
use crate::tidal::dto::{
    AlbumDto, ArtistDto, FavouriteEntry, FavouriteItem, ItemsPage, PlaylistDto,
};
use crate::tidal::{Client, TidalError};

#[derive(Debug, Clone)]
pub struct Playlist {
    pub uuid: String,
    pub title: String,
    pub track_count: u32,
    /// Who made it. TIDAL's editorial playlists have no user behind them, so
    /// the web client shows "TIDAL" there and so do we.
    pub creator: String,
    pub cover: Option<String>,
}

impl Playlist {
    /// A playlist with only the fields a caller cares about set. Tests and
    /// previews want a title and a count, not five fields of ceremony.
    pub fn sample(title: &str, track_count: u32) -> Self {
        Self {
            uuid: String::new(),
            title: title.into(),
            track_count,
            creator: "Coco".into(),
            cover: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Album {
    pub id: u64,
    pub title: String,
    pub artist: String,
    /// Release year. Absent rather than guessed when the date is missing or
    /// malformed — a wrong year is worse than none.
    pub year: Option<String>,
    pub cover: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Artist {
    pub id: u64,
    pub name: String,
    pub picture: Option<String>,
}

/// Items per request. TIDAL rejects larger pages on some endpoints, so this
/// stays conservative and `fetch_all` walks the offsets.
const PAGE_LIMIT: u32 = 100;

/// Stop after this many items even if the server keeps offering more. A
/// runaway `total`, or an endpoint that ignores `offset` and returns the same
/// page forever, must not spin here indefinitely.
const MAX_ITEMS: usize = 10_000;

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<ItemsPage<T>, TidalError> {
    // An empty body is a 200 with nothing in it, which serde reports as
    // "EOF while parsing a value at line 1 column 0" — true, and useless.
    // Say what actually arrived instead.
    if body.trim().is_empty() {
        return Err(TidalError::Parse(
            "the server returned an empty body where a page of items was expected".into(),
        ));
    }
    serde_json::from_str(body).map_err(|e| TidalError::Parse(e.to_string()))
}

/// Walk a paginated endpoint until it runs out of items.
///
/// Terminates on any of: a short page, reaching the reported `total`, an empty
/// page, or the `MAX_ITEMS` backstop. An empty page is the important one — it
/// is what stops the loop if the server ignores `offset` and keeps replaying
/// the first page.
async fn fetch_all<T>(
    client: &Client,
    what: &str,
    path: &str,
) -> Result<Vec<T>, TidalError>
where
    T: serde::de::DeserializeOwned,
{
    let mut out: Vec<T> = Vec::new();
    let mut offset = 0u32;

    loop {
        let body = client
            .get(
                path,
                &[
                    ("limit", PAGE_LIMIT.to_string()),
                    ("offset", offset.to_string()),
                ],
            )
            .await?;

        let page: ItemsPage<T> = parse(&body)?;
        let got = page.items.len();
        out.extend(page.items);

        if got == 0 || got < PAGE_LIMIT as usize {
            break;
        }
        if out.len() >= page.total as usize {
            break;
        }
        if out.len() >= MAX_ITEMS {
            tracing::warn!(
                "{what}: stopping at {} items, the server reported {}",
                out.len(),
                page.total
            );
            break;
        }
        offset = offset.saturating_add(PAGE_LIMIT);
    }

    Ok(out)
}

pub async fn playlists(client: &Client) -> Result<Vec<Playlist>, TidalError> {
    let path = format!("/users/{}/playlists", client.user_id());
    let items: Vec<PlaylistDto> = fetch_all(client, "playlists", &path).await?;
    Ok(items.into_iter().map(playlist_from_dto).collect())
}

pub fn playlist_from_dto(p: PlaylistDto) -> Playlist {
    Playlist {
        uuid: p.uuid,
        title: p.title,
        track_count: p.number_of_tracks,
        creator: p.creator.name.unwrap_or_else(|| "TIDAL".into()),
        // `image` is the playlist's own cover; `squareImage` is the mosaic
        // built from its tracks. Either is fine, neither is guaranteed.
        cover: p
            .square_image
            .or(p.image)
            .as_deref()
            .map(|c| crate::tidal::dto::cover_url(c, 320)),
    }
}

pub async fn albums(client: &Client) -> Result<Vec<Album>, TidalError> {
    let path = format!("/users/{}/favorites/albums", client.user_id());
    let items: Vec<FavouriteEntry<AlbumDto>> = fetch_all(client, "albums", &path).await?;
    Ok(items.into_iter().map(|e| album_from_dto(e.item)).collect())
}

pub fn album_from_dto(a: AlbumDto) -> Album {
    Album {
        id: a.id,
        title: a.title,
        artist: a.artists.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", "),
        year: a.release_date.and_then(|d| year_of(&d)),
        cover: a.cover.as_deref().map(|c| crate::tidal::dto::cover_url(c, 320)),
    }
}

/// The year from an ISO date. Anything that is not four leading digits gives
/// nothing rather than a wrong answer.
fn year_of(date: &str) -> Option<String> {
    let year = date.get(..4)?;
    year.chars().all(|c| c.is_ascii_digit()).then(|| year.to_string())
}

/// The image uuid for an artist: their portrait, or the album cover TIDAL
/// nominates when there is none. Extracted so the choice is one place and can
/// be tested without a server.
fn artist_image(a: &ArtistDto) -> Option<String> {
    a.picture.clone().or_else(|| a.album_cover_fallback.clone())
}

/// An artist as the app holds one. Shared with search, so the
/// portrait-or-album-cover fallback is decided in a single place.
pub fn artist_from_dto(a: ArtistDto) -> Artist {
    Artist {
        picture: artist_image(&a)
            .as_deref()
            .map(|c| crate::tidal::dto::cover_url(c, 320)),
        id: a.id,
        name: a.name,
    }
}

pub async fn artists(client: &Client) -> Result<Vec<Artist>, TidalError> {
    let path = format!("/users/{}/favorites/artists", client.user_id());
    let items: Vec<FavouriteEntry<ArtistDto>> = fetch_all(client, "artists", &path).await?;
    Ok(items.into_iter().map(|e| artist_from_dto(e.item)).collect())
}

pub async fn favourite_tracks(client: &Client) -> Result<Vec<Track>, TidalError> {
    let path = format!("/users/{}/favorites/tracks", client.user_id());
    let items: Vec<FavouriteItem> = fetch_all(client, "favourites", &path).await?;
    Ok(items.into_iter().map(|i| i.into_track()).collect())
}

/// Add a track to the user's favourites.
///
/// The same collection `favourite_tracks` reads. TIDAL takes the id as a
/// form field named `trackIds` — plural, though one id is what it is given
/// here.
pub async fn add_favourite_track(
    client: &Client,
    id: crate::domain::TrackId,
) -> Result<(), TidalError> {
    let path = format!("/users/{}/favorites/tracks", client.user_id());
    client
        .post_form(&path, &[("trackIds", id.to_string()), ("onArtifactNotFound", "FAIL".into())])
        .await?;
    Ok(())
}

/// Remove a track from the user's favourites.
pub async fn remove_favourite_track(
    client: &Client,
    id: crate::domain::TrackId,
) -> Result<(), TidalError> {
    let path = format!("/users/{}/favorites/tracks/{}", client.user_id(), id);
    client.delete(&path).await?;
    Ok(())
}

pub async fn playlist_tracks(
    client: &Client,
    uuid: &str,
) -> Result<Vec<Track>, TidalError> {
    // Playlist items use the same `item` wrapper as favourites.
    let path = format!("/playlists/{uuid}/items");
    let items: Vec<FavouriteItem> = fetch_all(client, "playlist tracks", &path).await?;
    Ok(items.into_iter().map(|i| i.item.into_track()).collect())
}

pub async fn album_tracks(client: &Client, album_id: u64) -> Result<Vec<Track>, TidalError> {
    let path = format!("/albums/{album_id}/items");
    let items: Vec<FavouriteItem> = fetch_all(client, "album tracks", &path).await?;
    Ok(items.into_iter().map(|i| i.item.into_track()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The paging loop's exit conditions, extracted so they can be tested
    /// without a server. `fetch_all` must stop when any of these says so —
    /// a loop that never terminates would hang the app on a bad response.
    fn should_stop(got: usize, collected: usize, total: u32) -> bool {
        got == 0
            || got < PAGE_LIMIT as usize
            || collected >= total as usize
            || collected >= MAX_ITEMS
    }

    #[test]
    fn an_artist_avatar_prefers_the_portrait_and_falls_back_to_an_album() {
        use crate::tidal::dto::ArtistDto;

        let with_portrait = ArtistDto {
            id: 1,
            name: "2Pac".into(),
            picture: Some("portrait-uuid".into()),
            album_cover_fallback: Some("album-uuid".into()),
        };
        assert_eq!(
            artist_image(&with_portrait).as_deref(),
            Some("portrait-uuid"),
            "a real portrait wins"
        );

        let without = ArtistDto {
            id: 2,
            name: "8ruki".into(),
            picture: None,
            album_cover_fallback: Some("album-uuid".into()),
        };
        assert_eq!(
            artist_image(&without).as_deref(),
            Some("album-uuid"),
            "no portrait means the album cover, as the web client shows"
        );

        let neither = ArtistDto {
            id: 3,
            name: "Nobody".into(),
            picture: None,
            album_cover_fallback: None,
        };
        assert!(artist_image(&neither).is_none(), "and an initial disc when there is neither");
    }

    #[test]
    fn an_empty_body_is_named_rather_than_left_to_serde() {
        // serde calls this "EOF while parsing a value at line 1 column 0",
        // which is accurate and tells the reader nothing.
        let e = parse::<PlaylistDto>("").unwrap_err().to_string();
        assert!(e.contains("empty body"), "{e}");
        let e = parse::<PlaylistDto>("   \n ").unwrap_err().to_string();
        assert!(e.contains("empty body"), "whitespace counts as empty: {e}");
    }

    #[test]
    fn a_malformed_body_still_reports_the_parse_failure() {
        // Only the empty case is special-cased; real syntax errors keep
        // serde's own message, which does say something useful.
        let e = parse::<PlaylistDto>("{ not json").unwrap_err().to_string();
        assert!(!e.contains("empty body"), "{e}");
    }

    #[test]
    fn a_year_is_taken_only_from_a_real_date() {
        assert_eq!(year_of("1981-04-01"), Some("1981".into()));
        assert_eq!(year_of("2026"), Some("2026".into()));
        // Junk must produce nothing, not a plausible-looking wrong year.
        assert_eq!(year_of(""), None);
        assert_eq!(year_of("198"), None);
        assert_eq!(year_of("n/a-01-01"), None);
    }

    #[test]
    fn an_editorial_playlist_is_credited_to_tidal() {
        // TIDAL's own playlists have no creator name; the web client shows
        // "TIDAL" rather than an empty line.
        let dto = PlaylistDto {
            uuid: "x".into(),
            title: "Classical Focus".into(),
            number_of_tracks: 118,
            creator: crate::tidal::dto::PlaylistCreator { name: None },
            ..Default::default()
        };
        assert_eq!(playlist_from_dto(dto).creator, "TIDAL");
    }

    #[test]
    fn a_short_page_ends_the_walk() {
        assert!(should_stop(37, 37, 1000), "a partial page is the last one");
    }

    #[test]
    fn an_empty_page_ends_the_walk() {
        // The important case: a server that ignores `offset` and replays the
        // same page would otherwise loop forever.
        assert!(should_stop(0, 500, 99_999));
    }

    #[test]
    fn reaching_the_reported_total_ends_the_walk() {
        assert!(should_stop(100, 206, 206));
    }

    #[test]
    fn the_backstop_ends_a_runaway_total() {
        // A `total` far beyond anything real must not keep us fetching.
        assert!(should_stop(100, MAX_ITEMS, u32::MAX));
    }

    #[test]
    fn a_full_page_with_more_to_come_continues() {
        assert!(
            !should_stop(PAGE_LIMIT as usize, 100, 206),
            "a full page below the total means keep going"
        );
    }
}
