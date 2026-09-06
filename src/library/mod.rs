use crate::domain::Track;
use crate::tidal::dto::{FavouriteItem, ItemsPage, PlaylistDto};
use crate::tidal::{Client, TidalError};

#[derive(Debug, Clone)]
pub struct Playlist {
    pub uuid: String,
    pub title: String,
    pub track_count: u32,
}

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<ItemsPage<T>, TidalError> {
    serde_json::from_str(body).map_err(|e| TidalError::Parse(e.to_string()))
}

pub async fn playlists(client: &Client) -> Result<Vec<Playlist>, TidalError> {
    let body = client
        .get(
            &format!("/users/{}/playlists", client.user_id()),
            &[("limit", "50".to_string())],
        )
        .await?;

    let page: ItemsPage<PlaylistDto> = parse(&body)?;
    Ok(page
        .items
        .into_iter()
        .map(|p| Playlist {
            uuid: p.uuid,
            title: p.title,
            track_count: p.number_of_tracks,
        })
        .collect())
}

pub async fn favourite_tracks(client: &Client) -> Result<Vec<Track>, TidalError> {
    let body = client
        .get(
            &format!("/users/{}/favorites/tracks", client.user_id()),
            &[("limit", "50".to_string())],
        )
        .await?;

    let page: ItemsPage<FavouriteItem> = parse(&body)?;
    Ok(page.items.into_iter().map(|i| i.item.into_track()).collect())
}

pub async fn playlist_tracks(
    client: &Client,
    uuid: &str,
) -> Result<Vec<Track>, TidalError> {
    let body = client
        .get(
            &format!("/playlists/{uuid}/items"),
            &[("limit", "100".to_string())],
        )
        .await?;

    // Playlist items use the same `item` wrapper as favourites.
    let page: ItemsPage<FavouriteItem> = parse(&body)?;
    Ok(page.items.into_iter().map(|i| i.item.into_track()).collect())
}

pub async fn album_tracks(client: &Client, album_id: u64) -> Result<Vec<Track>, TidalError> {
    let body = client
        .get(
            &format!("/albums/{album_id}/items"),
            &[("limit", "100".to_string())],
        )
        .await?;

    let page: ItemsPage<FavouriteItem> = parse(&body)?;
    Ok(page.items.into_iter().map(|i| i.item.into_track()).collect())
}
