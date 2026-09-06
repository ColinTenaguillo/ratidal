use crate::domain::Track;
use crate::tidal::dto::{
    AlbumDto, ArtistDto, FavouriteEntry, FavouriteItem, ItemsPage, PlaylistDto, TrackDto,
};
use crate::tidal::{Client, TidalError};

#[derive(Debug, Clone)]
pub struct Playlist {
    pub uuid: String,
    pub title: String,
    pub track_count: u32,
    /// Running time, as the web client shows under the count.
    pub duration: Option<std::time::Duration>,
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
            duration: None,
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
    pub track_count: u32,
    /// Running time, which the web client shows beside the track count.
    pub duration: Option<std::time::Duration>,
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
        duration: p.duration.map(std::time::Duration::from_secs),
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
        track_count: a.number_of_tracks,
        duration: a.duration.map(std::time::Duration::from_secs),
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

/// How many of an artist's top tracks the page shows.
///
/// The endpoint returns a hundred. A page is a summary — the albums and
/// similar artists below it are the rest of the point — and scrolling
/// through a hundred rows to reach them is not.
const TOP_TRACKS: usize = 5;

/// Everything an artist's page shows.
#[derive(Debug, Default, Clone)]
pub struct ArtistPage {
    pub name: String,
    pub picture: Option<String>,
    pub top_tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub similar: Vec<Artist>,
}

/// Fetch an artist's page.
///
/// Three requests, since the API has no single endpoint for the lot. Each
/// is an `ItemsPage` in the shape the existing DTOs already read; a bio is
/// left out, since `/bio` answers 404 for artists that have none and there
/// is nowhere to put prose in a row of cards.
/// Drop the same record listed more than once.
///
/// TIDAL returns a row per release rather than per record: Kaaris' page
/// carries BYAKUGAN twice and Day One three times, each with its own id,
/// the same title, the same date and the same track count — territory
/// reissues, which the web client does not show either.
///
/// Keyed on the track count as well as the title and year, because a
/// genuine second version is a different record and must survive: the same
/// artist has three of "Or Noir Part 3" at fifteen, sixteen and twenty-three
/// tracks. The first of a run is kept, which is the order TIDAL sent.
fn dedupe_releases(albums: Vec<Album>) -> Vec<Album> {
    let mut seen = std::collections::HashSet::new();
    albums
        .into_iter()
        .filter(|a| seen.insert((a.title.clone(), a.year.clone(), a.track_count)))
        .collect()
}

pub async fn artist_page(client: &Client, id: u64) -> Result<ArtistPage, TidalError> {
    // The artist itself, for the heading and its picture.
    let body = client.get(&format!("/artists/{id}"), &[]).await?;
    let dto: ArtistDto = serde_json::from_str(&body).unwrap_or_default();
    let artist = artist_from_dto(dto);

    // The rest is best-effort: an artist with no albums should still show
    // their top tracks rather than an error.
    // Plain tracks, not the `{item: ...}` envelope a favourites list uses:
    // wrapped in that, every field defaulted and the titles came back
    // empty rather than the parse failing.
    let top_tracks = match fetch_page::<TrackDto>(
        client,
        &format!("/artists/{id}/toptracks"),
    )
    .await
    {
        Ok(items) => items
            .into_iter()
            .take(TOP_TRACKS)
            .map(TrackDto::into_track)
            .collect(),
        Err(e) => {
            tracing::warn!("no top tracks for artist {id}: {e}");
            Vec::new()
        }
    };
    let albums = match fetch_page::<AlbumDto>(client, &format!("/artists/{id}/albums")).await {
        Ok(items) => dedupe_releases(items.into_iter().map(album_from_dto).collect()),
        Err(e) => {
            tracing::warn!("no albums for artist {id}: {e}");
            Vec::new()
        }
    };
    let similar = match fetch_page::<ArtistDto>(client, &format!("/artists/{id}/similar")).await
    {
        Ok(items) => items.into_iter().map(artist_from_dto).collect(),
        Err(e) => {
            tracing::warn!("no similar artists for {id}: {e}");
            Vec::new()
        }
    };

    Ok(ArtistPage {
        name: artist.name,
        picture: artist.picture,
        top_tracks,
        albums,
        similar,
    })
}

/// One page of an items endpoint.
///
/// Unlike `fetch_all` this stops at the first page: an artist's own lists
/// are a section of a page, not a collection to scroll to the end of.
async fn fetch_page<T>(client: &Client, path: &str) -> Result<Vec<T>, TidalError>
where
    T: serde::de::DeserializeOwned,
{
    let body = client
        .get(path, &[("limit", PAGE_LIMIT.to_string())])
        .await?;
    let page: ItemsPage<T> = parse(&body)?;
    Ok(page.items)
}

pub async fn album_tracks(client: &Client, album_id: u64) -> Result<Vec<Track>, TidalError> {
    let path = format!("/albums/{album_id}/items");
    let items: Vec<FavouriteItem> = fetch_all(client, "album tracks", &path).await?;
    Ok(items.into_iter().map(|i| i.item.into_track()).collect())
}

/// The tracks of a mix.
///
/// A mix's items come back in the same `{item: ...}` envelope an album's
/// tracks use — checked against a real response, since `#[serde(default)]`
/// would have made the wrong guess silent and yielded a list of tracks
/// with blank titles, which is how this went wrong for `toptracks`.
pub async fn mix_tracks(client: &Client, mix_id: &str) -> Result<Vec<Track>, TidalError> {
    let path = format!("/mixes/{mix_id}/items");
    let items: Vec<FavouriteItem> = fetch_all(client, "mix tracks", &path).await?;
    Ok(items.into_iter().map(|i| i.item.into_track()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(id: u64, title: &str, year: &str, tracks: u32) -> Album {
        Album {
            id,
            title: title.into(),
            artist: "Kaaris".into(),
            year: Some(year.into()),
            cover: None,
            track_count: tracks,
            duration: None,
        }
    }

    #[test]
    fn a_record_listed_twice_is_shown_once() {
        // Straight from the endpoint: Kaaris' page carried BYAKUGAN twice
        // and Day One three times, each row its own id with the same title,
        // date and track count.
        let got = dedupe_releases(vec![
            album(530804891, "BYAKUGAN", "2026", 14),
            album(530804732, "BYAKUGAN", "2026", 14),
            album(341386054, "Day One", "2023", 17),
            album(334013844, "Day One", "2023", 17),
            album(334010836, "Day One", "2023", 17),
        ]);

        let titles: Vec<&str> = got.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["BYAKUGAN", "Day One"]);
        // The first of each run, which is the order TIDAL sent.
        assert_eq!(got[0].id, 530804891);
        assert_eq!(got[1].id, 341386054);
    }

    #[test]
    fn a_genuinely_different_version_survives() {
        // The same artist has three of "Or Noir Part 3" at fifteen, sixteen
        // and twenty-three tracks. Those are different records, and keying
        // on the title and year alone would have thrown two of them away.
        let got = dedupe_releases(vec![
            album(102639627, "Or Noir Part 3", "2019", 15),
            album(104395163, "Or Noir Part 3", "2019", 23),
            album(103039437, "Or Noir Part 3", "2019", 16),
        ]);
        assert_eq!(got.len(), 3, "three different records must all be kept");
    }

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
