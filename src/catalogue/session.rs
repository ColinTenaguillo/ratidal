//! A signed-in service, through its `MusicApi`.
//!
//! sonora's models come back as this crate's: a `GenreItem` is a card, a
//! `HomeFeed` a page of rows, an `Artist` the artist page. What a provider
//! does not have — the feed, the mixes, the rest of a cut row — comes back
//! empty rather than failing, so the view it fills is simply blank.

use std::sync::Arc;

use music::{GenreItem, GenreSection, MusicApi, ProviderSession, ReleaseType};

use crate::browse::{Home, HomeRow, RowKind};
use crate::catalogue::{Favourable, Provider, Result};
use crate::domain::{Quality, Track, TrackId};
use crate::library::{Album, Artist, ArtistPage, FavouriteIds, PageKind, Playlist};
use crate::playback::StreamMeta;
use crate::search::Results;
use crate::shell::carousel::{Card, Target};
use crate::tidal::TidalError;

pub struct Session {
    name: &'static str,
    api: Arc<dyn MusicApi>,
    playback: Arc<dyn music::PlaybackFactory>,
    meta: Arc<dyn StreamMeta>,
}

/// The shell's state derives `Debug`; a session prints as its service.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

impl Session {
    /// What `provider` signed in to, with what it knows about its streams.
    pub fn open(provider: &dyn Provider, session: ProviderSession) -> Self {
        Self {
            name: provider.name(),
            api: session.api,
            playback: session.playback,
            meta: provider.stream_meta(),
        }
    }

    /// A TIDAL session with no token behind it, for tests and examples:
    /// every request fails, and nothing touches the network until one is
    /// awaited.
    #[doc(hidden)]
    pub fn sample() -> Arc<Self> {
        let provider = crate::provider::TidalProvider::new(
            crate::config::AuthConfig::default(),
            Quality::Lossless,
        )
        .expect("an HTTP client");
        Arc::new(provider.open_session())
    }

    /// What the service is called, for the log and the status line.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The engine that plays this session's tracks.
    pub fn playback(&self) -> Arc<dyn music::PlaybackFactory> {
        self.playback.clone()
    }

    /// What the engine's events leave out about a stream, where the
    /// service's fetch records it. The badge reads it.
    pub fn stream_meta(&self) -> Arc<dyn StreamMeta> {
        self.meta.clone()
    }

    /// What to ask for from now on. A service with one quality ignores it.
    pub fn set_quality(&self, wanted: Quality) {
        self.meta.set_quality(wanted);
    }

    /// The tracks a station built on `seed` plays, for the radio key and
    /// autoplay. Empty where the service has no stations.
    pub async fn track_radio(&self, seed: &TrackId) -> Result<Vec<Track>> {
        let (tracks, _) = self.api.track_radio(&seed.0, None).await.map_err(error)?;
        Ok(tracks.into_iter().map(track).collect())
    }
}

/// sonora reports with `anyhow`; the shell matches on `TidalError`. The
/// one case it tells apart, a session the service no longer accepts, is
/// what sonora calls a credentials failure.
fn error(error: anyhow::Error) -> TidalError {
    match error.downcast_ref::<music::SignInFailure>() {
        Some(music::SignInFailure(music::SignInProblem::Credentials)) => TidalError::Unauthorized,
        _ => TidalError::Other(format!("{error:#}")),
    }
}

fn track(t: music::Track) -> Track {
    let artist_id = t.artist_refs.first().and_then(|a| a.id.clone());
    Track {
        id: TrackId(t.id.unwrap_or_default()),
        title: t.name,
        artist: t.artists,
        album: t.album,
        duration: t.duration,
        cover: t.cover,
        tags: t.tags,
        added: t.added_at.map(iso_date),
        explicit: t.explicit,
        ai: false,
        radio: None,
        album_id: t.album_id,
        artist_id,
    }
}

fn album(a: music::Album) -> Album {
    Album {
        id: a.id,
        title: a.name,
        artist: a.artists,
        year: (a.year != 0).then(|| a.year.to_string()),
        cover: a.cover,
        track_count: a.track_count,
        duration: None,
    }
}

fn artist(a: music::SavedArtist) -> Artist {
    Artist {
        id: a.id,
        name: a.name,
        picture: a.cover,
    }
}

fn playlist(p: music::Playlist) -> Playlist {
    Playlist {
        uuid: p.id,
        title: p.name,
        track_count: p.track_count,
        duration: None,
        creator: p.owner,
        cover: p.cover,
    }
}

/// A shelf item as a card: what the home page and a genre page draw.
fn card(item: GenreItem) -> Card {
    match item {
        GenreItem::Track(t) => Card {
            title: t.name,
            subtitle: t.artists,
            cover_url: t.cover,
            target: t.id.map(Target::Track),
            duration: t.duration,
            ..Card::default()
        },
        GenreItem::Album(a) => Card {
            title: a.name,
            subtitle: a.artists,
            detail: match a.year {
                0 => String::new(),
                year => year.to_string(),
            },
            cover_url: a.cover,
            target: Some(Target::Album(a.id)),
            ..Card::default()
        },
        GenreItem::Artist(a) => Card {
            title: a.name,
            cover_url: a.cover,
            round: true,
            target: Some(Target::Artist(a.id)),
            ..Card::default()
        },
        GenreItem::Playlist(p) => Card {
            title: p.name,
            subtitle: p.owner,
            detail: match p.track_count {
                0 => String::new(),
                n => format!("{n} tracks"),
            },
            cover_url: p.cover,
            target: Some(Target::Playlist(p.id)),
            ..Card::default()
        },
        GenreItem::Genre(g) => Card {
            title: g.name,
            cover_url: g.cover,
            target: Some(Target::Page(g.id)),
            ..Card::default()
        },
    }
}

fn rows(sections: Vec<GenreSection>) -> Home {
    Home {
        rows: sections
            .into_iter()
            .map(|section| {
                let cards: Vec<Card> = section.items.into_iter().map(card).collect();
                // A row of links is the Explore shape; anything with
                // artwork is a strip of covers.
                let kind = match cards
                    .iter()
                    .all(|c| matches!(c.target, Some(Target::Page(_))))
                {
                    true if !cards.is_empty() => RowKind::Links,
                    _ => RowKind::Carousel,
                };
                HomeRow {
                    heading: section.title,
                    kind,
                    cards,
                    more: None,
                }
            })
            .filter(|row| !row.cards.is_empty())
            .collect(),
    }
}

/// The page an artist or an album is shown on, from what the service said.
fn page(id: &str, a: music::Artist) -> ArtistPage {
    let (albums, singles): (Vec<_>, Vec<_>) = a
        .albums
        .into_iter()
        .partition(|a| !matches!(a.release_type, ReleaseType::Single | ReleaseType::Ep));
    ArtistPage {
        kind: PageKind::Artist,
        id: id.to_owned(),
        name: a.name,
        fans: a.monthly_listeners,
        picture: a.cover_large,
        bio: a.biography,
        top_tracks: a.top_tracks.into_iter().map(track).collect(),
        albums: albums.into_iter().map(album).collect(),
        singles: singles.into_iter().map(album).collect(),
        ..ArtistPage::default()
    }
}

/// `YYYY-MM-DD` from unix seconds: what the ADDED column reads, as TIDAL's
/// ISO timestamps gave it. Days to a civil date, after Howard Hinnant.
fn iso_date(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

impl Session {
    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        let lists = self.api.playlists().await.map_err(error)?;
        Ok(lists.into_iter().map(playlist).collect())
    }

    pub async fn albums(&self) -> Result<Vec<Album>> {
        let albums = self.api.saved_albums().await.map_err(error)?;
        Ok(albums.into_iter().map(album).collect())
    }

    pub async fn artists(&self) -> Result<Vec<Artist>> {
        let artists = self.api.saved_artists().await.map_err(error)?;
        Ok(artists.into_iter().map(artist).collect())
    }

    pub async fn favourite_tracks(&self) -> Result<Vec<Track>> {
        let tracks = self.api.saved_tracks().await.map_err(error)?;
        Ok(tracks.into_iter().map(track).collect())
    }

    /// Read off the saved listings: sonora has no list of ids on its own.
    pub async fn favourite_ids(&self) -> Result<FavouriteIds> {
        let (albums, artists) = tokio::join!(self.api.saved_albums(), self.api.saved_artists());
        Ok(FavouriteIds {
            albums: albums.map_err(error)?.into_iter().map(|a| a.id).collect(),
            artists: artists.map_err(error)?.into_iter().map(|a| a.id).collect(),
            playlists: Vec::new(),
        })
    }

    pub async fn set_track_favourite(&self, id: &TrackId, on: bool) -> Result<()> {
        self.api.set_track_saved(&id.0, on).await.map_err(error)
    }

    pub async fn set_favourite(&self, what: &Favourable, on: bool) -> Result<()> {
        let api = &self.api;
        match what {
            Favourable::Artist(id) => api.set_artist_saved(id, on).await,
            Favourable::Album(id) => api.set_album_saved(id, on).await,
            Favourable::Playlist(id) => match on {
                true => api.add_playlist_to_library(id).await,
                false => api.remove_playlist_from_library(id).await,
            },
        }
        .map_err(error)
    }

    pub async fn playlist_tracks(&self, uuid: &str) -> Result<Vec<Track>> {
        let tracks = self.api.playlist_tracks(uuid).await.map_err(error)?;
        Ok(tracks.into_iter().map(track).collect())
    }

    /// The album's tracks, and under them the rail the service draws:
    /// related records and artists. The rail is extra, so a service that
    /// cannot answer for it leaves it empty rather than failing the page.
    pub async fn album_page(&self, id: &str, title: &str) -> Result<ArtistPage> {
        let (detail, rail) = tokio::join!(self.api.album(id), self.api.album_catalogue(id, None));
        let detail = detail.map_err(error)?;
        let rail = rail.unwrap_or_default();
        Ok(ArtistPage {
            kind: PageKind::Album,
            id: id.to_owned(),
            // Named as the card was: the view waiting for this reply is
            // headed by the card's title.
            name: title.to_owned(),
            picture: detail.album.cover_large.or(detail.album.cover),
            top_tracks: detail.tracks.into_iter().map(track).collect(),
            appears_on: rail.also_like.into_iter().map(album).collect(),
            similar: rail.similar.into_iter().map(artist).collect(),
            ..ArtistPage::default()
        })
    }

    /// The artist's page, with what the service's catalogue adds to it:
    /// the whole of the top tracks where the page carried a few, and what
    /// the artist appears on.
    pub async fn artist_page(&self, id: &str) -> Result<ArtistPage> {
        let (artist, more) = tokio::join!(self.api.artist(id), self.api.artist_catalogue(id, &[]));
        let mut page = page(id, artist.map_err(error)?);
        if let Ok(more) = more {
            if !more.top_tracks.is_empty() {
                page.top_tracks = more.top_tracks.into_iter().map(track).collect();
            }
            page.appears_on = more.appears_on.into_iter().map(album).collect();
        }
        Ok(page)
    }

    pub async fn track(&self, id: &TrackId) -> Result<Track> {
        Ok(track(self.api.track(&id.0).await.map_err(error)?))
    }

    /// Tracks, albums and playlists: `MusicApi` has no artist search.
    pub async fn search(&self, query: &str) -> Result<Results> {
        let (tracks, albums, playlists) = tokio::join!(
            self.api.search(query),
            self.api.search_albums(query),
            self.api.search_playlists(query)
        );
        Ok(Results {
            query: query.to_owned(),
            tracks: tracks.map_err(error)?.into_iter().map(track).collect(),
            albums: albums.map_err(error)?.into_iter().map(album).collect(),
            artists: Vec::new(),
            playlists: playlists.map_err(error)?.into_iter().map(playlist).collect(),
        })
    }

    /// The home page: the service's feed, with what it played lately and
    /// its picks ahead of it where the service has them.
    pub async fn home(&self) -> Result<Home> {
        let feed = self.api.home().await.map_err(error)?;
        let mut sections = feed.sections;
        if let Some(picks) = feed.quick_picks.filter(|p| !p.is_empty()) {
            sections.insert(
                0,
                GenreSection {
                    title: "Quick picks".into(),
                    items: picks.into_iter().map(GenreItem::Track).collect(),
                },
            );
        }
        if !feed.listen_again.is_empty() {
            sections.insert(
                0,
                GenreSection {
                    title: "Listen again".into(),
                    items: feed.listen_again,
                },
            );
        }
        Ok(rows(sections))
    }

    pub async fn page_of_rows(&self, path: &str) -> Result<Home> {
        let genre = self.api.genre(path).await.map_err(error)?;
        Ok(rows(genre.sections))
    }

    /// What the account played lately, as a row of track cards.
    pub async fn feed(&self) -> Result<Vec<Card>> {
        let recent = self.api.recently_played().await.map_err(error)?;
        Ok(recent.into_iter().map(GenreItem::Track).map(card).collect())
    }

    pub async fn explore(&self) -> Result<Home> {
        let genres = self.api.genres().await.map_err(error)?;
        Ok(rows(vec![GenreSection {
            title: "Genres".into(),
            items: genres.into_iter().map(GenreItem::Genre).collect(),
        }]))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn unix_seconds_become_the_day_the_column_shows() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(1_616_247_907), "2021-03-20");
        assert_eq!(iso_date(951_782_400), "2000-02-29", "a leap day");
        assert_eq!(iso_date(-86_400), "1969-12-31", "before the epoch");
    }

    fn a_track(id: &str) -> music::Track {
        music::Track {
            id: Some(id.into()),
            name: "So What".into(),
            playable: true,
            artists: "Miles Davis".into(),
            artist_refs: vec![music::ArtistRef {
                name: "Miles Davis".into(),
                id: Some("ar-1".into()),
            }],
            album: "Kind of Blue".into(),
            album_id: Some("al-1".into()),
            cover: Some("https://x/c.jpg".into()),
            duration: Duration::from_secs(562),
            added_at: Some(1_616_247_907),
            added_by: None,
            playcount: None,
            popularity: 0,
            explicit: false,
            track_number: 1,
            disc_number: 1,
            tags: Vec::new(),
            languages: Vec::new(),
            credits: Vec::new(),
        }
    }

    #[test]
    fn a_track_keeps_what_the_rows_show_and_the_keys_open() {
        let t = track(a_track("tr-9"));
        assert_eq!(t.id, TrackId::from("tr-9"));
        assert_eq!(t.artist_id.as_deref(), Some("ar-1"), "a opens the artist");
        assert_eq!(t.album_id.as_deref(), Some("al-1"), "c opens the album");
        assert_eq!(t.added.as_deref(), Some("2021-03-20"));
    }

    #[test]
    fn shelf_items_become_cards_that_open_something() {
        let t = card(GenreItem::Track(a_track("tr-9")));
        assert_eq!(t.target, Some(Target::Track("tr-9".into())));
        assert_eq!(t.duration, Duration::from_secs(562), "the bar needs it");

        let g = card(GenreItem::Genre(music::Genre {
            id: "rock".into(),
            name: "Rock".into(),
            cover: None,
        }));
        assert_eq!(g.target, Some(Target::Page("rock".into())));
    }

    #[test]
    fn a_shelf_of_links_is_drawn_as_links_and_an_empty_one_is_dropped() {
        let home = rows(vec![
            GenreSection {
                title: "Genres".into(),
                items: vec![GenreItem::Genre(music::Genre {
                    id: "rock".into(),
                    name: "Rock".into(),
                    cover: None,
                })],
            },
            GenreSection {
                title: "Nothing".into(),
                items: Vec::new(),
            },
        ]);
        assert_eq!(home.rows.len(), 1);
        assert_eq!(home.rows[0].kind, RowKind::Links);
    }

    #[test]
    fn eps_and_singles_go_under_the_albums() {
        let release = |name: &str, release_type| music::Album {
            id: name.into(),
            name: name.into(),
            artists: String::new(),
            artist_refs: Vec::new(),
            cover: None,
            cover_large: None,
            release_type,
            year: 1959,
            track_count: 5,
            release_date: String::new(),
            label: String::new(),
            copyrights: Vec::new(),
            added_at: None,
        };
        let p = page(
            "ar-1",
            music::Artist {
                name: "Miles".into(),
                cover_large: None,
                biography: None,
                monthly_listeners: Some(7),
                top_tracks: Vec::new(),
                albums: vec![
                    release("Kind of Blue", ReleaseType::Album),
                    release("Blue Haze", ReleaseType::Ep),
                ],
            },
        );
        assert_eq!(p.id, "ar-1");
        assert_eq!(p.fans, Some(7));
        assert_eq!(p.albums.len(), 1);
        assert_eq!(p.singles[0].title, "Blue Haze");
        assert_eq!(p.albums[0].year.as_deref(), Some("1959"));
    }

    #[test]
    fn a_refused_session_is_the_one_error_the_shell_tells_apart() {
        let refused = anyhow::Error::from(music::SignInFailure(music::SignInProblem::Credentials));
        assert!(matches!(error(refused), TidalError::Unauthorized));
        assert!(matches!(error(anyhow::anyhow!("down")), TidalError::Other(m) if m == "down"));
    }
}
