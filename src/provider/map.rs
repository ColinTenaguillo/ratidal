//! This crate's types as sonora's models.
//!
//! sonora keys everything by string id; TIDAL numbers most things. The
//! number goes through `to_string` here and `parse` on the way back, in
//! `tidal.rs`. A mix is a playlist whose id is prefixed, since sonora has
//! no mix of its own and a mix is a list of tracks under a name.

use music::{
    Album, ArtistRef, Genre, GenreItem, GenreSection, Playlist, ReleaseType, SavedArtist, Track,
};

use crate::browse::{Home, HomeRow};
use crate::domain;
use crate::library;
use crate::shell::carousel::{Card, Target};

/// The prefix a mix wears as a playlist id.
pub const MIX: &str = "mix:";

pub fn track(t: domain::Track) -> Track {
    Track {
        id: Some(t.id.0.clone()),
        name: t.title,
        playable: true,
        artist_refs: vec![ArtistRef {
            name: t.artist.clone(),
            id: t.artist_id.map(|id| id.to_string()),
        }],
        artists: t.artist,
        album: t.album,
        album_id: t.album_id.map(|id| id.to_string()),
        cover: t.cover,
        duration: t.duration,
        added_at: music::iso_8601_to_epoch(t.added.as_deref()),
        added_by: None,
        playcount: None,
        popularity: 0,
        explicit: t.explicit,
        track_number: 0,
        disc_number: 0,
        tags: t.tags,
        languages: Vec::new(),
        credits: Vec::new(),
    }
}

pub fn album(a: library::Album) -> Album {
    let year = a.year.as_deref().and_then(|y| y.parse().ok()).unwrap_or(0);
    Album {
        id: a.id.to_string(),
        name: a.title,
        artists: a.artist,
        artist_refs: Vec::new(),
        cover_large: a.cover.clone(),
        cover: a.cover,
        release_type: ReleaseType::Album,
        year,
        track_count: a.track_count,
        release_date: a.year.unwrap_or_default(),
        label: String::new(),
        copyrights: Vec::new(),
        added_at: None,
    }
}

pub fn artist(a: library::Artist) -> SavedArtist {
    SavedArtist {
        id: a.id.to_string(),
        name: a.name,
        cover: a.picture,
        added_at: None,
    }
}

pub fn playlist(p: library::Playlist) -> Playlist {
    Playlist {
        id: p.uuid,
        name: p.title,
        // TIDAL's editorial playlists have no user behind them; anything
        // with a name is someone's, and the only someone this account
        // lists is itself.
        owned: p.creator != "TIDAL",
        owner: p.creator,
        owner_id: String::new(),
        collaborative: false,
        blend: false,
        public: false,
        cover: p.cover,
        track_count: p.track_count,
        modified_at: None,
    }
}

/// A mix as a playlist: named as its card was, under the prefixed id.
pub fn mix(id: &str, name: &str, cover: Option<String>) -> Playlist {
    Playlist {
        id: format!("{MIX}{id}"),
        name: name.to_owned(),
        owner: "TIDAL".into(),
        owner_id: String::new(),
        owned: false,
        collaborative: false,
        blend: false,
        public: false,
        cover,
        track_count: 0,
        modified_at: None,
    }
}

/// A card as whatever it opens. A link to one of this app's own views has
/// no meaning to another player and is dropped.
pub fn item(card: Card) -> Option<GenreItem> {
    let Card {
        title,
        subtitle,
        cover_url,
        target,
        duration,
        ..
    } = card;
    Some(match target? {
        Target::Track(id) => GenreItem::Track(Track {
            id: Some(id.to_string()),
            name: title,
            playable: true,
            artist_refs: Vec::new(),
            artists: subtitle,
            album: String::new(),
            album_id: None,
            cover: cover_url,
            duration,
            added_at: None,
            added_by: None,
            playcount: None,
            popularity: 0,
            explicit: false,
            track_number: 0,
            disc_number: 0,
            tags: Vec::new(),
            languages: Vec::new(),
            credits: Vec::new(),
        }),
        Target::Album(id) => GenreItem::Album(Album {
            id: id.to_string(),
            name: title,
            artists: subtitle,
            artist_refs: Vec::new(),
            cover_large: cover_url.clone(),
            cover: cover_url,
            release_type: ReleaseType::Album,
            year: 0,
            track_count: 0,
            release_date: String::new(),
            label: String::new(),
            copyrights: Vec::new(),
            added_at: None,
        }),
        Target::Artist(id) => GenreItem::Artist(SavedArtist {
            id: id.to_string(),
            name: title,
            cover: cover_url,
            added_at: None,
        }),
        Target::Playlist(uuid) => GenreItem::Playlist(Playlist {
            id: uuid,
            name: title,
            owner: subtitle,
            owner_id: String::new(),
            owned: false,
            collaborative: false,
            blend: false,
            public: false,
            cover: cover_url,
            track_count: 0,
            modified_at: None,
        }),
        Target::Mix(id) => GenreItem::Playlist(mix(&id, &title, cover_url)),
        Target::Page(path) => GenreItem::Genre(Genre {
            id: path,
            name: title,
            cover: cover_url,
        }),
        Target::Section(_) => return None,
    })
}

pub fn section(row: HomeRow) -> GenreSection {
    GenreSection {
        title: row.heading,
        items: row.cards.into_iter().filter_map(item).collect(),
    }
}

/// A page of rows as sections, dropping a row nothing survived from.
pub fn sections(home: Home) -> Vec<GenreSection> {
    home.rows
        .into_iter()
        .map(section)
        .filter(|section| !section.items.is_empty())
        .collect()
}

/// The genres an Explore page links to: its `Links` rows, flattened.
pub fn genres(home: Home) -> Vec<Genre> {
    home.rows
        .into_iter()
        .flat_map(|row| row.cards)
        .filter_map(|card| match item(card)? {
            GenreItem::Genre(genre) => Some(genre),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::TrackId;

    #[test]
    fn a_track_keeps_its_ids_as_strings_and_its_date_as_epoch() {
        let mut t = domain::Track::sample("So What", "Miles Davis", Duration::from_secs(562));
        t.id = TrackId::from(655911);
        t.album_id = Some("77640617".into());
        t.artist_id = Some("7804".into());
        t.added = Some("2021-03-20T13:45:07.000+00:00".into());
        t.tags = vec!["HIRES_LOSSLESS".into()];

        let out = track(t);
        assert_eq!(out.id.as_deref(), Some("655911"));
        assert_eq!(out.album_id.as_deref(), Some("77640617"));
        assert_eq!(out.artist_refs[0].id.as_deref(), Some("7804"));
        assert_eq!(out.artists, "Miles Davis");
        assert_eq!(out.added_at, Some(1_616_247_907));
        assert_eq!(out.tags, ["HIRES_LOSSLESS"]);
        assert!(out.playable);
    }

    #[test]
    fn an_album_year_is_a_number_or_nothing() {
        let a = library::Album {
            id: (1).to_string(),
            title: "Kind of Blue".into(),
            artist: "Miles Davis".into(),
            year: Some("1959".into()),
            cover: Some("https://x/cover.jpg".into()),
            track_count: 5,
            duration: None,
        };
        let out = album(a.clone());
        assert_eq!(out.year, 1959);
        assert_eq!(out.release_date, "1959");
        assert_eq!(out.cover_large, out.cover);

        let out = album(library::Album { year: None, ..a });
        assert_eq!(out.year, 0, "no year is zero, not a guess");
    }

    #[test]
    fn an_editorial_playlist_is_not_the_users_own() {
        let mut p = library::Playlist::sample("Jazz", 10);
        p.uuid = "u-1".into();
        assert!(playlist(p.clone()).owned, "a named creator owns it");
        p.creator = "TIDAL".into();
        assert!(!playlist(p).owned);
    }

    #[test]
    fn a_mix_is_a_playlist_under_a_prefixed_id() {
        let p = mix("000a1b", "Daily Mix 1", None);
        assert_eq!(p.id, "mix:000a1b");
        assert_eq!(p.name, "Daily Mix 1");
    }

    #[test]
    fn cards_become_what_they_open_and_shortcuts_vanish() {
        let card = |target| Card {
            title: "T".into(),
            subtitle: "S".into(),
            target: Some(target),
            ..Card::default()
        };
        assert!(matches!(item(card(Target::Track("1".into()))), Some(GenreItem::Track(t)) if t.id.as_deref() == Some("1")));
        assert!(matches!(item(card(Target::Album("2".into()))), Some(GenreItem::Album(a)) if a.id == "2"));
        assert!(matches!(item(card(Target::Artist("3".into()))), Some(GenreItem::Artist(a)) if a.id == "3"));
        assert!(matches!(item(card(Target::Playlist("u".into()))), Some(GenreItem::Playlist(p)) if p.id == "u"));
        assert!(matches!(item(card(Target::Mix("m".into()))), Some(GenreItem::Playlist(p)) if p.id == "mix:m"));
        assert!(matches!(item(card(Target::Page("/pages/genre".into()))), Some(GenreItem::Genre(g)) if g.id == "/pages/genre"));
        assert!(item(card(Target::Section(crate::shell::sidebar::Section::Music))).is_none());
        assert!(item(Card::default()).is_none(), "a card with nowhere to go");
    }

    #[test]
    fn a_row_with_nothing_left_in_it_is_dropped() {
        let home = Home {
            rows: vec![
                HomeRow {
                    heading: "Shortcuts".into(),
                    cards: vec![Card {
                        target: Some(Target::Section(crate::shell::sidebar::Section::Music)),
                        ..Card::default()
                    }],
                    kind: Default::default(),
                    more: None,
                },
                HomeRow {
                    heading: "Albums".into(),
                    cards: vec![Card {
                        target: Some(Target::Album("9".into())),
                        ..Card::default()
                    }],
                    kind: Default::default(),
                    more: None,
                },
            ],
        };
        let out = sections(home);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Albums");
    }
}
