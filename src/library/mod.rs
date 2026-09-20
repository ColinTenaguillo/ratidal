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
///
/// Not symmetric with `add_favourite_track`: TIDAL wants the id in the path
/// for the delete, and takes no body at all.
pub async fn remove_favourite_track(
    client: &Client,
    id: crate::domain::TrackId,
) -> Result<(), TidalError> {
    let path = format!("/users/{}/favorites/tracks/{}", client.user_id(), id);
    client.delete(&path).await?;
    Ok(())
}

/// Follow an artist: the collection `artists` reads. Same shape as a
/// favourite track, with `artistIds` for the field.
pub async fn add_favourite_artist(client: &Client, id: u64) -> Result<(), TidalError> {
    let path = format!("/users/{}/favorites/artists", client.user_id());
    client
        .post_form(&path, &[("artistIds", id.to_string()), ("onArtifactNotFound", "FAIL".into())])
        .await?;
    Ok(())
}

/// Unfollow an artist. The id goes in the path, as for a track.
pub async fn remove_favourite_artist(client: &Client, id: u64) -> Result<(), TidalError> {
    let path = format!("/users/{}/favorites/artists/{}", client.user_id(), id);
    client.delete(&path).await?;
    Ok(())
}

/// Favourite an album: the collection `albums` reads, `albumIds` for the
/// field.
pub async fn add_favourite_album(client: &Client, id: u64) -> Result<(), TidalError> {
    let path = format!("/users/{}/favorites/albums", client.user_id());
    client
        .post_form(&path, &[("albumIds", id.to_string()), ("onArtifactNotFound", "FAIL".into())])
        .await?;
    Ok(())
}

pub async fn remove_favourite_album(client: &Client, id: u64) -> Result<(), TidalError> {
    let path = format!("/users/{}/favorites/albums/{}", client.user_id(), id);
    client.delete(&path).await?;
    Ok(())
}

/// Favourite a playlist: `uuids` for the field, since a playlist has no
/// number.
pub async fn add_favourite_playlist(client: &Client, uuid: &str) -> Result<(), TidalError> {
    let path = format!("/users/{}/favorites/playlists", client.user_id());
    client
        .post_form(&path, &[("uuids", uuid.to_string()), ("onArtifactNotFound", "FAIL".into())])
        .await?;
    Ok(())
}

pub async fn remove_favourite_playlist(client: &Client, uuid: &str) -> Result<(), TidalError> {
    let path = format!("/users/{}/favorites/playlists/{}", client.user_id(), uuid);
    client.delete(&path).await?;
    Ok(())
}

/// Save a mix. Mixes are the one kind whose favourites live on v2, as a
/// PUT to `add` or `remove` rather than a POST and a DELETE.
pub async fn save_mix(client: &Client, id: &str) -> Result<(), TidalError> {
    client
        .put_form_v2(
            "/favorites/mixes/add",
            &[("mixIds", id.to_string()), ("onArtifactNotFound", "FAIL".into())],
        )
        .await?;
    Ok(())
}

pub async fn unsave_mix(client: &Client, id: &str) -> Result<(), TidalError> {
    client
        .put_form_v2("/favorites/mixes/remove", &[("mixIds", id.to_string())])
        .await?;
    Ok(())
}

/// The ids of everything the user has favourited, in one reply: albums,
/// playlists and artists. What the key that toggles a favourite reads to
/// know which way to go, without walking the three collections.
#[derive(Debug, Clone, Default)]
pub struct FavouriteIds {
    pub albums: Vec<u64>,
    pub playlists: Vec<String>,
    pub artists: Vec<u64>,
}

pub async fn favourite_ids(client: &Client) -> Result<FavouriteIds, TidalError> {
    let path = format!("/users/{}/favorites/ids", client.user_id());
    let body = client.get(&path, &[]).await?;
    Ok(parse_favourite_ids(&body))
}

/// The reply keys its lists by kind, every id a string -- numbers too.
pub fn parse_favourite_ids(body: &str) -> FavouriteIds {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let strings = |key: &str| -> Vec<String> {
        v[key]
            .as_array()
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    let numbers = |key: &str| -> Vec<u64> {
        strings(key).iter().filter_map(|s| s.parse().ok()).collect()
    };
    FavouriteIds {
        albums: numbers("ALBUM"),
        playlists: strings("PLAYLIST"),
        artists: numbers("ARTIST"),
    }
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
const TOP_TRACKS: usize = 4;

/// Which page this is. An album's page is the artist's shape -- a cover,
/// a name, a blurb, a list of tracks, rows of related records -- so it is
/// the same struct, and this says what the header draws and what `F`
/// favourites.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    #[default]
    Artist,
    Album,
}

/// Everything an artist's page shows -- or an album's, see [`PageKind`].
#[derive(Debug, Default, Clone)]
pub struct ArtistPage {
    pub kind: PageKind,
    pub id: u64,
    pub name: String,
    /// How many follow them: the "23.8K fans" the web puts under the name.
    /// Only v2 has it, and that request is best-effort.
    pub fans: Option<u64>,
    /// Whether this account follows them, as the server says.
    pub following: bool,
    pub picture: Option<String>,
    /// The artist's own blurb, when TIDAL has one. Editorial rather than
    /// generated, so plenty of artists have none — Prince has six thousand
    /// words and Kaaris nothing at all.
    pub bio: Option<String>,
    pub top_tracks: Vec<Track>,
    pub albums: Vec<Album>,
    /// EPs and singles, which the web client shows as its own row under the
    /// albums.
    pub singles: Vec<Album>,
    /// Records the artist appears on rather than made: compilations, other
    /// people's albums.
    pub appears_on: Vec<Album>,
    pub similar: Vec<Artist>,
    /// The artist's radio, which the web client offers in its header.
    pub radio: Option<String>,
    /// Where the whole of Top Tracks lives. The page returns four of a
    /// hundred, so "see all" fetches the rest rather than reopening the
    /// four already drawn.
    pub top_tracks_path: Option<String>,
    /// Where the rest of each row lives, when the page cut it: v2 hands
    /// back ten of an artist's thirty-two singles and names the rest.
    pub albums_more: Option<String>,
    pub singles_more: Option<String>,
    pub appears_on_more: Option<String>,
    pub similar_more: Option<String>,
}

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
    // One request: `/v2/artist/{id}` is what the web client draws from,
    // and it carries the header, the fan count, the top tracks, the
    // albums, the EPs and singles, the compilations and the similar
    // artists in the order the web shows them -- each row cut to ten
    // with a path to the rest.
    let body = client.get_raw_v2(&format!("/artist/{id}"), &page_query()).await?;
    let mut page = parse_artist_page(&body);
    // The page does not say whose it is; the caller does. Kept so a key
    // pressed on the page can name the artist to the API.
    page.id = id;
    Ok(page)
}

/// An album's page: its tracks, whole, from v1 -- v2 cuts the list to
/// five -- and from v2 the review, whether the account has it, and the
/// rows the web draws under it: more by the artist, other versions,
/// related albums and artists. The rows are best-effort: an album without
/// them is an album, one that fails for want of them is not.
pub async fn album_page(client: &Client, id: u64, title: &str) -> Result<ArtistPage, TidalError> {
    let tracks = album_tracks(client, id).await?;
    let mut page = match client.get_raw_v2(&format!("/album/{id}"), &page_query()).await {
        Ok(body) => parse_album_page(&body),
        Err(e) => {
            tracing::warn!("no v2 page for album {id}: {e}");
            ArtistPage::default()
        }
    };
    page.kind = PageKind::Album;
    page.id = id;
    // Named as the card was: the view waiting for this reply is headed by
    // the card's title, and a reply under another name is dropped.
    page.name = title.to_string();
    page.top_tracks = tracks;
    Ok(page)
}

/// What the v2 page endpoints want said about the client.
fn page_query() -> [(&'static str, String); 3] {
    [
        ("deviceType", "BROWSER".to_string()),
        ("locale", "en_US".to_string()),
        ("platform", "WEB".to_string()),
    ]
}

/// A blurb with its markup taken out.
///
/// TIDAL writes these with HTML in them — `<br/>` between paragraphs, and
/// the odd tag besides — which a terminal shows as the tag itself. Breaks
/// become spaces rather than newlines: the blurb is wrapped to the width
/// beside the portrait, and a hard break there would leave a ragged hole
/// mid-paragraph.
fn plain_text(html: &str) -> String {
    // TIDAL's own link markup, `[wimpLink artistId="…"]Name[/wimpLink]`,
    // wraps a name inline: the tag goes and the name stays where it was.
    let mut html = html.replace("[/wimpLink]", "");
    while let Some(start) = html.find("[wimpLink") {
        let Some(len) = html[start..].find(']') else { break };
        html.replace_range(start..=start + len, "");
    }
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    // A tag stands for a word break — "one.<br/>Two" runs the words
    // together without one — but only where a break belongs: the space is
    // held back until a word character follows, so "<b>three</b>." does not
    // come out as "three ."
    let mut pending_break = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                pending_break = true;
            }
            _ if in_tag => {}
            _ => {
                if pending_break {
                    pending_break = false;
                    if !c.is_whitespace() && !c.is_ascii_punctuation() {
                        out.push(' ');
                    }
                }
                out.push(c);
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The artist page of a `/v2/artist/{id}` response.
///
/// Separate from the request so it can be tested against a captured body.
/// Rows are found by their `moduleId`, never by their title: the titles
/// come in the account's language -- "Titres les plus écoutés" for an
/// account set to French -- and the ids do not.
pub fn parse_artist_page(body: &str) -> ArtistPage {
    let v = page_value(body);
    let mut out = ArtistPage::default();
    if let Ok(artist) = serde_json::from_value::<ArtistDto>(v["item"]["data"].clone()) {
        let artist = artist_from_dto(artist);
        out.name = artist.name;
        out.picture = artist.picture;
    }
    out.radio = v["item"]["data"]["mixes"]["ARTIST_MIX"].as_str().map(str::to_string);
    out.following = v["item"]["following"].as_bool().unwrap_or(false);
    out.fans = v["header"]["followersAmount"].as_u64();
    out.bio = blurb(&v["header"]["biography"]);
    read_rows(
        &v,
        &mut out,
        &[
            ("ARTIST_TOP_TRACKS", Slot::Tracks),
            ("ARTIST_ALBUMS", Slot::Albums),
            ("ARTIST_TOP_SINGLES", Slot::Singles),
            ("ARTIST_APPEARS_ON", Slot::AppearsOn),
            ("ARTIST_SIMILAR_ARTISTS", Slot::Similar),
        ],
    );
    out
}

/// The album page of a `/v2/album/{id}` response, without its tracks: the
/// caller has the whole list from v1, this carries five of them.
///
/// The rows land in the artist page's slots in the web's order: more by
/// the artist where an artist's albums go, other versions where the EPs
/// go, related albums where the compilations go, related artists where
/// the similar artists go. The headings are the page kind's to choose.
pub fn parse_album_page(body: &str) -> ArtistPage {
    let v = page_value(body);
    let mut out = ArtistPage { kind: PageKind::Album, ..ArtistPage::default() };
    out.picture = v["item"]["data"]["cover"]
        .as_str()
        .map(|c| crate::tidal::dto::cover_url(c, 320));
    out.following = v["item"]["following"].as_bool().unwrap_or(false);
    out.bio = blurb(&v["header"]["review"]);
    read_rows(
        &v,
        &mut out,
        &[
            ("ALBUM_MORE_BY_ARTIST", Slot::Albums),
            ("ALBUM_OTHER_VERSIONS", Slot::Singles),
            ("ALBUM_RELATED_ALBUMS", Slot::AppearsOn),
            ("ALBUM_RELATED_ARTISTS", Slot::Similar),
        ],
    );
    out
}

fn page_value(body: &str) -> serde_json::Value {
    match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("the page did not parse: {e}");
            serde_json::Value::Null
        }
    }
}

/// A blurb or a review: `{text}` on v2, with the markup taken out, and
/// none rather than an empty one.
fn blurb(v: &serde_json::Value) -> Option<String> {
    v["text"]
        .as_str()
        .or_else(|| v.as_str())
        .map(plain_text)
        .filter(|t| !t.trim().is_empty())
}

/// Where a v2 row lands on the page.
#[derive(Clone, Copy)]
enum Slot {
    Tracks,
    Albums,
    Singles,
    AppearsOn,
    Similar,
}

/// Read the rows of a v2 page into `out`, by module id.
///
/// Every item is a `{type, data}` wrapper, read through the value: TIDAL
/// sends a mix's `type` twice and serde refuses a duplicate. Each row
/// names where the rest of it lives, kept beside it for "See all".
fn read_rows(v: &serde_json::Value, out: &mut ArtistPage, slots: &[(&str, Slot)]) {
    let empty = Vec::new();
    for row in v["items"].as_array().unwrap_or(&empty) {
        let Some(module) = row["moduleId"].as_str() else { continue };
        let Some((_, slot)) = slots.iter().find(|(m, _)| *m == module) else { continue };
        let datas: Vec<serde_json::Value> = row["items"]
            .as_array()
            .map(|a| a.iter().map(|i| i["data"].clone()).collect())
            .unwrap_or_default();
        let more = row["viewAll"].as_str().map(str::to_string);
        // A row can come back as stubs -- an id and nothing else, every
        // other field null: good kid, m.A.A.d city's Related Albums are
        // ten of them. A card with no title opens nothing worth seeing.
        let albums = |datas: Vec<serde_json::Value>| -> Vec<Album> {
            dedupe_releases(
                datas
                    .into_iter()
                    .filter_map(|d| serde_json::from_value::<AlbumDto>(d).ok())
                    .map(album_from_dto)
                    .filter(|a| !a.title.is_empty())
                    .collect(),
            )
        };
        match slot {
            Slot::Tracks => {
                out.top_tracks = datas
                    .into_iter()
                    .filter_map(|d| serde_json::from_value::<TrackDto>(d).ok())
                    .take(TOP_TRACKS)
                    .map(TrackDto::into_track)
                    .collect();
                out.top_tracks_path = more;
            }
            Slot::Albums => {
                out.albums = albums(datas);
                out.albums_more = more;
            }
            Slot::Singles => {
                out.singles = albums(datas);
                out.singles_more = more;
            }
            Slot::AppearsOn => {
                out.appears_on = albums(datas);
                out.appears_on_more = more;
            }
            Slot::Similar => {
                out.similar = datas
                    .into_iter()
                    .filter_map(|d| serde_json::from_value::<ArtistDto>(d).ok())
                    .map(artist_from_dto)
                    .filter(|a| !a.name.is_empty())
                    .collect();
                out.similar_more = more;
            }
        }
    }
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

/// One track, by id.
///
/// The home page sends its track cards with `mixes: null`, so the radio a
/// track names is missing there. This fills it in on demand rather than
/// leaving the radio key dead on the one page most people start from.
pub async fn track(client: &Client, id: crate::domain::TrackId) -> Result<Track, TidalError> {
    let body = client.get(&format!("/tracks/{}", id.0), &[]).await?;
    let dto: crate::tidal::dto::TrackDto = serde_json::from_str(&body)
        .map_err(|e| TidalError::Parse(format!("a track did not parse: {e}")))?;
    Ok(dto.into_track())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_favourite_ids_come_by_kind_as_strings() {
        // Cut from a real reply: numbers quoted, and the kinds this reads.
        let body = r#"{"ALBUM":["107005564","20556792"],"ARTIST":["1003"],"PLAYLIST":["u-1"],
            "VIDEO":[],"TRACK":["102245271"]}"#;
        let ids = parse_favourite_ids(body);
        assert_eq!(ids.albums, [107005564, 20556792]);
        assert_eq!(ids.artists, [1003]);
        assert_eq!(ids.playlists, ["u-1"]);
        assert!(parse_favourite_ids("nope").albums.is_empty(), "best-effort");
    }


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
    fn a_blurb_comes_back_without_its_markup() {
        // TIDAL writes these with HTML in them, and a terminal shows the
        // tag itself: 2Pac's began "Revolutionary. <br/><br/>Tupac Shakur".
        let body = r#"{"header":{"biography":{"text":"One.<br/><br/>Two <b>three</b>.","source":"TiVo"}},
            "item":{"type":"ARTIST","data":{"id":1,"name":"An Artist"}},"items":[]}"#;
        let bio = parse_artist_page(body).bio.expect("a blurb");
        assert!(!bio.contains('<'), "no markup survives: {bio:?}");
        assert_eq!(bio, "One. Two three.", "and the words run on cleanly");
    }

    #[test]
    fn a_blurb_keeps_the_names_inside_its_links() {
        // The links are dropped, the names in them stay: 21 Savage's blurb
        // read "Rapper  made  (2016)" with the tags' contents thrown out.
        let body = r#"{"header":{"biography":{"text":"Rapper [wimpLink artistId=\"1\"]21 Savage[/wimpLink] made [wimpLink albumId=\"2\"]Savage Mode[/wimpLink] (2016)."}},
            "item":{"type":"ARTIST","data":{"id":1,"name":"21 Savage"}},"items":[]}"#;
        let bio = parse_artist_page(body).bio.expect("a blurb");
        assert_eq!(bio, "Rapper 21 Savage made Savage Mode (2016).");
    }

    #[test]
    fn the_page_is_read_by_module_id_not_by_title() {
        // Cut from a captured /v2/artist/21221030, on an account set to
        // French: the titles come in the account's language, the ids do
        // not. Rows this does not show -- playlists, videos, credits --
        // are passed over without harm.
        let body = r#"{"header":{"followersAmount":3701,"biography":null},
            "item":{"type":"ARTIST","following":true,"data":{"id":21221030,"name":"Yamê",
                "picture":"5e3e","mixes":{"ARTIST_MIX":"mix-1"}}},
            "items":[
              {"type":"TRACK_LIST","moduleId":"ARTIST_TOP_TRACKS","title":"Titres les plus écoutés",
               "viewAll":"artist/ARTIST_TOP_TRACKS/view-all?artistId=21221030","items":[
                 {"type":"TRACK","data":{"id":10,"title":"Bécane","duration":182,"album":{"id":1,"title":"Elowi"},
                   "artists":[{"id":21221030,"name":"Yamê"}],"mixes":{"TRACK_MIX":"tm-1"}}}]},
              {"type":"HORIZONTAL_LIST","moduleId":"ARTIST_ALBUMS","title":"Albums",
               "viewAll":"artist/ARTIST_ALBUMS/view-all?artistId=21221030","items":[
                 {"type":"ALBUM","data":{"id":1,"title":"Elowi","numberOfTracks":12,"releaseDate":"2023-01-01"}}]},
              {"type":"HORIZONTAL_LIST","moduleId":"ARTIST_TOP_SINGLES","title":"EP & Singles","items":[
                 {"type":"ALBUM","data":{"id":2,"title":"Bécane","numberOfTracks":1}},
                 {"type":"ALBUM","data":{"id":3,"title":"Bécane","numberOfTracks":1,"type":"SINGLE"}}]},
              {"type":"HORIZONTAL_LIST","moduleId":"ARTIST_PLAYLIST","title":"Listes de lecture","items":[
                 {"type":"PLAYLIST","data":{"uuid":"u","title":"P"}}]},
              {"type":"ARTIST_TRACK_CREDITS_CARD","moduleId":"ARTIST_CREDITS","title":"Crédits","items":[]},
              {"type":"HORIZONTAL_LIST","moduleId":"ARTIST_SIMILAR_ARTISTS","title":"Les fans aiment aussi","items":[
                 {"type":"ARTIST","data":{"id":7,"name":"Kaaris"}}]},
              {"type":"HORIZONTAL_LIST","moduleId":"ARTIST_APPEARS_ON","title":"Apparaît sur","items":[
                 {"type":"ALBUM","data":{"id":4,"title":"Comp","numberOfTracks":20}}]}
            ]}"#;
        let page = parse_artist_page(body);
        assert_eq!(page.name, "Yamê");
        assert!(page.picture.is_some());
        assert_eq!(page.radio.as_deref(), Some("mix-1"));
        assert_eq!(page.fans, Some(3701));
        assert!(page.following);
        assert!(page.bio.is_none(), "null is none");
        assert_eq!(page.top_tracks.len(), 1);
        assert_eq!(page.top_tracks[0].radio.as_deref(), Some("tm-1"), "a track keeps its radio");
        assert_eq!(
            page.top_tracks_path.as_deref(),
            Some("artist/ARTIST_TOP_TRACKS/view-all?artistId=21221030")
        );
        assert_eq!(page.albums.len(), 1);
        assert!(page.albums_more.is_some());
        assert_eq!(page.singles.len(), 1, "the same single listed twice is shown once");
        assert_eq!(page.similar.len(), 1);
        assert_eq!(page.appears_on.len(), 1);
    }

    #[test]
    fn an_album_page_puts_the_webs_rows_in_the_artists_slots() {
        // Cut from a captured /v2/album/20556792. The tracks come from v1;
        // this reads the review, the account's flag and the four rows.
        let body = r#"{"header":{"review":{"text":"A classic.<br/>Still.","source":"TiVo"}},
            "item":{"type":"ALBUM","following":true,"data":{"id":20556792,"title":"good kid","cover":"db5f"}},
            "items":[
              {"type":"TRACK_LIST","moduleId":"ALBUM_TRACKS","items":[{"type":"ALBUM_ITEM","data":{"id":1,"title":"Cut"}}]},
              {"type":"HORIZONTAL_LIST","moduleId":"ALBUM_MORE_BY_ARTIST","title":"More Albums by Artist",
               "viewAll":"album/ALBUM_MORE_BY_ARTIST/view-all?albumId=20556792","items":[
                 {"type":"ALBUM","data":{"id":2,"title":"DAMN.","numberOfTracks":14}}]},
              {"type":"HORIZONTAL_LIST","moduleId":"ALBUM_OTHER_VERSIONS","title":"Other versions","items":[
                 {"type":"ALBUM","data":{"id":3,"title":"good kid","numberOfTracks":12}}]},
              {"type":"HORIZONTAL_LIST","moduleId":"ALBUM_RELATED_ALBUMS","title":"Related Albums","items":[
                 {"type":"ALBUM","data":{"id":4,"title":"Blonde","numberOfTracks":17}},
                 {"type":"ALBUM","data":{"id":5,"title":null,"artists":null,"numberOfTracks":0}}]},
              {"type":"HORIZONTAL_LIST","moduleId":"ALBUM_RELATED_ARTISTS","title":"Related Artists","items":[
                 {"type":"ARTIST","data":{"id":5,"name":"Frank Ocean"}}]}
            ]}"#;
        let page = parse_album_page(body);
        assert_eq!(page.kind, PageKind::Album);
        assert!(page.picture.as_deref().unwrap().contains("db5f"));
        assert!(page.following, "favourited, as the account says");
        assert_eq!(page.bio.as_deref(), Some("A classic. Still."));
        assert!(page.top_tracks.is_empty(), "the cut list is not the tracks");
        assert_eq!(page.albums[0].title, "DAMN.");
        assert!(page.albums_more.is_some());
        assert_eq!(page.singles[0].title, "good kid");
        assert_eq!(page.appears_on.len(), 1, "a stub with no title is dropped");
        assert_eq!(page.appears_on[0].title, "Blonde");
        assert_eq!(page.similar[0].name, "Frank Ocean");
    }


    #[test]
    fn an_artist_with_no_blurb_has_none_rather_than_an_empty_one() {
        // TIDAL has six thousand words for Prince and nothing for Kaaris;
        // an empty string would draw a heading over blank space.
        let body = r#"{"header":{"biography":{"text":"   "}},"item":{"data":{"id":1,"name":"K"}},"items":[]}"#;
        assert!(parse_artist_page(body).bio.is_none());
        assert!(parse_artist_page("not json").bio.is_none(), "best-effort");
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
