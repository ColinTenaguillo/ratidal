use crate::domain::Track;
use crate::tidal::dto::{FavouriteItem, ItemsPage, PlaylistDto};
use crate::tidal::{Client, TidalError};

#[derive(Debug, Clone)]
pub struct Playlist {
    pub uuid: String,
    pub title: String,
    pub track_count: u32,
}

/// Items per request. TIDAL rejects larger pages on some endpoints, so this
/// stays conservative and `fetch_all` walks the offsets.
const PAGE_LIMIT: u32 = 100;

/// Stop after this many items even if the server keeps offering more. A
/// runaway `total`, or an endpoint that ignores `offset` and returns the same
/// page forever, must not spin here indefinitely.
const MAX_ITEMS: usize = 10_000;

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<ItemsPage<T>, TidalError> {
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
    Ok(items
        .into_iter()
        .map(|p| Playlist {
            uuid: p.uuid,
            title: p.title,
            track_count: p.number_of_tracks,
        })
        .collect())
}

pub async fn favourite_tracks(client: &Client) -> Result<Vec<Track>, TidalError> {
    let path = format!("/users/{}/favorites/tracks", client.user_id());
    let items: Vec<FavouriteItem> = fetch_all(client, "favourites", &path).await?;
    Ok(items.into_iter().map(|i| i.item.into_track()).collect())
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
