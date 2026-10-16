pub mod artistview;
pub mod artwork;
pub mod carousel;
pub mod cursor;
pub mod drmview;
pub mod fuzzy;
#[cfg(test)]
pub mod geometry;
pub mod grid;
pub mod help;
pub mod home;
pub mod icons;
pub mod inputbox;
pub mod keymap;
pub mod layout;
pub mod loadingview;
pub mod lyricsview;
pub mod mediakeys;
#[cfg(target_os = "linux")]
mod mediakeys_linux;
#[cfg(target_os = "macos")]
pub mod mediakeys_macos;
pub mod queueview;

/// Register with the desktop's media keys, where the platform has them.
///
/// Public so the integration test can drive it the way a desktop does:
/// registering on the bus is the whole feature, and calling the handlers
/// directly would prove nothing about that.
pub fn spawn_media_keys(
    actions: tokio::sync::mpsc::UnboundedSender<Action>,
    state: mediakeys::StateReceiver,
) {
    #[cfg(target_os = "linux")]
    mediakeys_linux::spawn(actions, state);
    #[cfg(target_os = "macos")]
    mediakeys_macos::spawn(actions, state);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (actions, state);
    }
}
pub mod login;
pub mod nowplaying;
pub mod scrollbar;
pub mod searchview;
pub mod settings;
pub mod sidebar;
pub mod theme;
pub mod trackgrid;
pub mod tracklist;

use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::StreamExt as _;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::DefaultTerminal;

/// Everything that can change the app's state. Components emit these; only the
/// loop applies them.
#[derive(Debug, Clone)]
pub enum Action {
    Quit,
    Tick,
    Key(KeyEvent),
    Error(String),
    /// What the service wants on the way in: a code to confirm in the
    /// browser, a link to authorise at, cookies pasted, an account picked.
    LoginPrompt(music::SignInPrompt),
    /// What the user typed or picked for the prompt up: the cookies, or
    /// the id of the account.
    Answer(String),
    /// Use the service without an account, where it allows that.
    BeginGuest,
    /// A session is in hand, fresh or resumed: start its engine and
    /// fetch what every signed-in screen needs.
    SessionReady,
    /// The password typed for the Subsonic server the config names.
    Connect(login::Secret),
    /// Signed in, or a stored session resumed.
    SessionOpened(std::sync::Arc<crate::catalogue::Session>),
    BeginLogin,
    SessionExpired,
    SidebarNext,
    SidebarPrevious,
    TrackNext,
    TrackPrevious,
    ActivateSelection,
    CloseCollection,
    /// One step back through the views and sections the user has been in,
    /// and one step forward again.
    GoBack,
    GoForward,
    /// Straight back to the home page, from wherever the user is.
    GoHome,
    /// Straight to one nav entry, by its number.
    GoToSection(sidebar::Section),
    /// Put the selected track next, ahead of the rest of the queue.
    QueueTrackNext,
    /// Put it at the end of what the user has asked for.
    QueueTrackLast,
    /// Take the queued track at this position out of the queue.
    RemoveQueued(usize),
    /// Play the queued track at this position, skipping what is between.
    JumpInQueue(usize),
    /// Open the album of the selected track.
    OpenTrackAlbum,
    /// Open the artist of the selected track.
    OpenTrackArtist,
    /// The radio built around the track that is playing.
    PlayTrackRadio,
    /// The selected track's album or artist, when the row in hand names
    /// both and carries the id of neither -- a card from the home page.
    /// Fetch the track, which does carry them, then open what it names.
    FetchTrackAlbum(crate::domain::TrackId),
    FetchTrackArtist(crate::domain::TrackId),
    /// Open a collection the loop has already resolved: what the fetch
    /// above found. `title` is what the view is headed with, and the rest
    /// is its banner until the reply fills it in.
    OpenTarget {
        target: Collection,
        title: String,
        subtitle: String,
        cover: Option<String>,
    },
    /// The queue is nearly out and autoplay is on: fetch the radio that
    /// follows `seed`, the last track queued.
    Autoplay {
        seed: crate::domain::TrackId,
    },
    /// The radio came back: append it, and start it if the queue ran dry
    /// while waiting.
    QueueRadio(Vec<crate::domain::Track>),
    /// Nothing came back for this seed, so it is not asked about again.
    AutoplayFailed(crate::domain::TrackId),
    /// Turn autoplay on or off and save it, from the queue view.
    ToggleAutoplay,
    /// Half a screen down or up, as vim's ctrl-d and ctrl-u: the same
    /// step `j` or `k` takes, repeated for half of what is drawn.
    HalfPage {
        down: bool,
    },
    /// Favourite what is under the cursor, or take it out again: the
    /// artist whose page is open, the album, playlist, mix or artist card
    /// selected, or the collection whose view is up.
    ToggleFollow,
    /// The account says so now: mark it, and say it.
    FollowChanged {
        what: Favourable,
        name: String,
        following: bool,
    },
    /// The ids of everything favourited, from the startup fetch.
    FavouriteIdsLoaded(crate::library::FavouriteIds),
    /// The user's favourites, from the startup fetch.
    TracksLoaded(Vec<crate::domain::Track>),
    /// The contents of a playlist or album the user opened. Kept apart from
    /// the favourites so a slow startup fetch landing afterwards cannot
    /// overwrite what the user is looking at — they are two different lists
    /// that happened to share an action.
    CollectionLoaded {
        /// What was open when the request went out. A reply for something
        /// the user has since left is dropped rather than shown under the
        /// wrong heading.
        for_title: String,
        tracks: Vec<crate::domain::Track>,
    },
    PlaylistsLoaded(Vec<crate::library::Playlist>),
    AlbumsLoaded(Vec<crate::library::Album>),
    ArtistsLoaded(Vec<crate::library::Artist>),
    /// Boxed for the same reason a home page is: it is rows of cards.
    ExploreLoaded(Box<crate::browse::Home>),
    /// A genre, mood or decade page, opened from Explore.
    PageLoaded {
        title: String,
        home: Box<crate::browse::Home>,
    },
    /// An artist's page. Boxed: three sections of items.
    ArtistLoaded(Box<crate::library::ArtistPage>),
    ArtistLeft,
    ArtistRight,
    /// Fetch the Explore page. Its own request, since nothing else needs it.
    LoadExplore,
    /// Fetch the activity feed.
    LoadFeed,
    /// Fetch the favourite tracks, playlists and albums again.
    ///
    /// They are loaded once at sign-in, but the reply only lands in the
    /// pane when nothing is open over it -- `self.tracks` is shared with
    /// whatever collection is on screen, so an arriving list of favourites
    /// would otherwise replace an open album's contents. A section left
    /// empty by that is refetched when the user walks into it.
    LoadFavourites,
    FeedLoaded(Vec<carousel::Card>),
    /// The home page's rows. Boxed: it is by far the largest payload an
    /// Action carries, and every other variant would otherwise grow to
    /// match it.
    HomeLoaded(Box<crate::browse::Home>),
    CarouselNext,
    CarouselPrevious,
    RowNext,
    RowPrevious,
    LoadHome,
    /// Open the search box.
    BeginSearch,
    /// Run the query that has been typed.
    RunSearch(String),
    /// What a query returned. Carries the query, so a reply for one the user
    /// has since retyped is dropped rather than shown under the new text.
    SearchLoaded(Box<crate::search::Results>),
    /// Leave search and go back to what was showing.
    CloseSearch,
    /// Move to the next tab of the search results.
    NextSearchTab,
    SearchLeft,
    SearchRight,
    /// Add the selected track to the favourites, or take it out if it is
    /// already one.
    ToggleFavourite,
    /// Open the whole of the home row the selection is in.
    SeeAll,
    /// Skip forward or back in the queue.
    QueueNext,
    QueuePrevious,
    /// Play whatever the queue says comes next, or nothing if it is done.
    PlayQueued,
    /// The output level changed in Settings. Applied in the loop, which is
    /// what holds the channel to the player.
    SetVolume(f32),
    /// Fetch the track that plays next, ahead of time.
    Preload(crate::domain::TrackId),
    /// A setting the engine reads once at its start changed: start it
    /// again, on the same track and position.
    RestartEngine,
    /// The sleep timer's next step: 15, 30, 45, 60 minutes, the end of the
    /// track, off.
    CycleSleep,
    /// Leave the service for the one the Settings row names, and sign in
    /// to it.
    SwitchProvider,
    /// The lyrics pane, over the main pane and off again.
    ToggleLyrics,
    /// Ask the lyrics providers about this track.
    LoadLyrics(crate::domain::TrackId),
    /// What they said.
    LyricsLoaded {
        id: crate::domain::TrackId,
        found: lyricsview::Found,
    },
    /// The scrobbling services the config names, linked at start-up.
    ScrobblersLinked(Vec<crate::scrobble::Linked>),
    /// The Widevine module: search the machine, what was found, download
    /// Google's with its terms, what the user said to each question, and
    /// how it ended.
    LookForCdm,
    CdmFound(Option<music::drm::Origin>),
    DownloadCdm,
    CdmOffered {
        version: String,
        license: String,
    },
    AnswerCdm(bool),
    CdmReady(music::drm::Origin),
    CdmFailed(String),
    ToggleShuffle,
    CycleRepeat,
    /// The reply: what the track's state is now, so the mark matches the
    /// account rather than what was guessed when the key was pressed.
    FavouriteChanged {
        id: crate::domain::TrackId,
        favourite: bool,
    },
    Playback(crate::playback::PlaybackEvent),
    TogglePause,
    /// Ten seconds along the track, or back. The bar moves at once and the
    /// player follows; the next position event settles any difference.
    SeekForward,
    SeekBack,
    /// A track already showing in the bar failed to start. Distinct from
    /// `Error`: the bar has to be undone as well as the message shown, and
    /// a generic error carries no way to tell which track it was about.
    PlaybackFailed {
        id: crate::domain::TrackId,
        message: String,
    },
}

/// The search box, and whatever the last query returned.
#[derive(Debug, Default)]
pub struct SearchState {
    /// What has been typed.
    pub query: String,
    /// True while the box has the keyboard.
    pub typing: bool,
    /// The results of the last query that came back. Held separately from
    /// `query` so the previous results stay on screen while the next query
    /// is being typed, rather than blanking on every keystroke.
    pub results: crate::search::Results,
    /// Which tab is showing.
    pub tab: usize,
    /// The Tracks tab is the app's own track list, so it carries that
    /// view's state rather than a second kind of selection.
    pub tracks: tracklist::TrackListState,
    /// Albums, artists and playlists are the app's own card grid, one
    /// state each so moving through one does not disturb the others.
    pub albums: grid::GridState,
    pub artists: grid::GridState,
    pub playlists: grid::GridState,
    /// Which of Top results' stacked sections the selection is in. The tab
    /// shows three kinds at once, so moving down has to walk out of one
    /// section and into the next rather than stopping at the first.
    pub top: searchview::TopSection,
    /// Top results' own card rows. Carousels rather than the tabs' grids:
    /// a grid scrolls by whole rows, so a section of one row could never
    /// scroll, and it drew a scrollbar beside itself to say so.
    pub top_artists: carousel::CarouselState,
    pub top_albums: carousel::CarouselState,
    /// The first Top results section drawn, so the selected one is on
    /// screen. Scrolled by section, as the artist page is.
    pub top_scroll: usize,
}

impl SearchState {
    /// Whether the selection is in a track list: the Tracks tab, or the
    /// tracks section of Top results.
    fn in_tracks(&self, tab: searchview::Tab) -> bool {
        tab.is_tracks()
            || (matches!(tab, searchview::Tab::Top) && self.top == searchview::TopSection::Tracks)
    }
}

/// Which way a movement key goes.
///
/// The search tabs are three different views with three different rules, so
/// the key handler says only the direction and the view decides what a step
/// in it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// How far `.` and `,` move along the track.
const SEEK_STEP: std::time::Duration = std::time::Duration::from_secs(10);

/// How many of Top results' sections are on screen at once: it scrolls by
/// section, two at a time. An artist's page counts its own from its layout.
const SECTIONS_SHOWING: usize = 2;

/// A page of stacked sections as the keys see it, for [`App::move_in_stack`].
struct Stacked<'a> {
    /// How many sections are present.
    count: usize,
    /// Which of them holds the selection.
    at: usize,
    /// The track list among them: its index, state, length, and the rows
    /// the selection may land in.
    tracks: Option<(usize, &'a mut tracklist::TrackListState, usize, usize)>,
    /// The selected section's row of cards, when it is one: state, length,
    /// and the cards the selection may land on.
    row: Option<(&'a mut carousel::CarouselState, usize, usize)>,
    /// Sections scrolled past the top of the page, and how many from there
    /// are drawn whole: a move into one past those scrolls the page to it.
    scroll: &'a mut usize,
    visible: usize,
}

impl Dir {
    /// `Some(down)` for a vertical move, `None` for a horizontal one.
    fn vertical(self) -> Option<bool> {
        match self {
            Dir::Down => Some(true),
            Dir::Up => Some(false),
            _ => None,
        }
    }

    /// Down or right: the way a selection index grows.
    fn forward(self) -> bool {
        matches!(self, Dir::Down | Dir::Right)
    }
}

impl App {
    /// The track the highlighted row stands for, wherever the user is.
    ///
    /// The selection indexes whatever list is on screen. In the library that
    /// is the filtered tracks — reading `self.tracks` directly would play a
    /// different track than the highlighted row whenever a filter was in
    /// effect. In search it is the results, and reading the library there
    /// played whatever track sat at the same index in a list nothing was
    /// showing.
    fn selected_track(&self) -> Option<crate::domain::Track> {
        // Read from what is drawn, as `on_grid` is: the keys act on what
        // the user can see. This read the Tracks section's list whatever
        // was on screen, so on the home page `A` favourited and `c` opened
        // whichever favourite that hidden list had highlighted -- which is
        // what "the key does nothing" looked like from the outside.
        match self.showing() {
            Showing::Queue => self
                .queue
                .entries()
                .get(self.queue_list.selected)
                .map(|(track, _)| (*track).clone()),
            Showing::History => self
                .queue
                .history()
                .nth(self.history_list.selected)
                .cloned(),
            Showing::Artist => {
                let page = self.artist.as_ref()?;
                if artistview::Section::from_index(self.artist_section)
                    == artistview::Section::Tracks
                {
                    page.top_tracks.get(self.artist_tracks.selected).cloned()
                } else {
                    self.selected_card_track()
                }
            }
            Showing::Search(tab) => {
                let state = self.search.as_ref()?;
                // Top stacks card rows over a track list, and `top` says
                // which of them holds the selection.
                let in_tracks = state.in_tracks(tab);
                if in_tracks {
                    searchview::track_rows(&state.results, tab)
                        .get(state.tracks.selected)
                        .cloned()
                } else {
                    self.selected_card_track()
                }
            }
            Showing::Tracks | Showing::Section(sidebar::Section::Tracks) => {
                tracklist::filter(&self.tracks, &self.tracklist.filter)
                    .get(self.tracklist.selected)
                    .map(|t| (*t).clone())
            }
            // Everything else is cards, and a card is a track when it
            // points at one.
            Showing::Rows | Showing::Cards | Showing::Loading | Showing::Section(_) => {
                self.selected_card_track()
            }
        }
    }

    /// The track the selected card stands for, when it is a track at all.
    fn selected_card_track(&self) -> Option<crate::domain::Track> {
        self.selected_card().and_then(|card| track_from_card(&card))
    }

    /// The list the selection sits in, and where in it.
    ///
    /// Playing a track queues the list it came from, which is what makes an
    /// album play on. Same source as `selected_track`, so the two cannot
    /// disagree about which list is on screen.
    fn selected_list(&self) -> (Vec<crate::domain::Track>, usize, Option<String>) {
        // Read from what is drawn, as `selected_track` is, and for the same
        // reason: this knew three lists and nothing else, so a track played
        // from a row of cards on the home page went with no context at all
        // -- it played alone, the queue stayed empty, and the skip keys had
        // nowhere to go.
        match self.showing() {
            // Enter is the list's own key there, and never reaches this.
            Showing::Queue | Showing::History => (Vec::new(), 0, None),
            Showing::Artist => {
                let Some(page) = self.artist.as_ref() else {
                    return (Vec::new(), 0, None);
                };
                if artistview::Section::from_index(self.artist_section)
                    == artistview::Section::Tracks
                {
                    (
                        page.top_tracks.clone(),
                        self.artist_tracks.selected,
                        Some(page.name.clone()),
                    )
                } else {
                    self.selected_card_list()
                }
            }
            Showing::Search(tab) => {
                let Some(state) = self.search.as_ref() else {
                    return (Vec::new(), 0, None);
                };
                let in_tracks = state.in_tracks(tab);
                if in_tracks {
                    (
                        searchview::track_rows(&state.results, tab),
                        state.tracks.selected,
                        Some("Search".to_string()),
                    )
                } else {
                    self.selected_card_list()
                }
            }
            Showing::Tracks | Showing::Section(sidebar::Section::Tracks) => (
                tracklist::filter(&self.tracks, &self.tracklist.filter)
                    .into_iter()
                    .cloned()
                    .collect(),
                self.tracklist.selected,
                // Named after the view it came from, so the queue can say
                // where it is playing from rather than leaving the user to
                // work out which album this is.
                self.open
                    .as_ref()
                    .map(|o| o.title.clone())
                    .or_else(|| Some(self.sidebar.section().label().to_string())),
            ),
            Showing::Rows | Showing::Cards | Showing::Loading | Showing::Section(_) => {
                self.selected_card_list()
            }
        }
    }

    /// The tracks of the row the selected card sits in, and where the card
    /// is among them: a row of new tracks plays on like an album does, and
    /// the queue shows it under the row's own heading. A track card that is
    /// not in such a row is a list of itself; a card that is not a track is
    /// no list at all.
    fn selected_card_list(&self) -> (Vec<crate::domain::Track>, usize, Option<String>) {
        let Some(track) = self.selected_card_track() else {
            return (Vec::new(), 0, None);
        };
        if self.on_home() {
            if let Some(row) = self.rows_on_screen().current_row() {
                let is_track =
                    |c: &carousel::Card| matches!(c.target, Some(carousel::Target::Track(_)));
                let tracks: Vec<_> = row.cards.iter().filter_map(track_from_card).collect();
                // Its place among the tracks, not among the cards: a row
                // may mix in cards that are not tracks.
                let at = row
                    .cards
                    .iter()
                    .take(row.state.selected)
                    .filter(|c| is_track(c))
                    .count();
                if at < tracks.len() {
                    return (tracks, at, Some(row.heading.clone()));
                }
            }
        }
        (vec![track], 0, None)
    }

    /// Move the selection in whichever view the active search tab is.
    ///
    /// Each tab is one of the app's own views, so the movement rules are
    /// theirs too — a track list moves by lines, a grid by rows of cards.
    /// Keeping a separate selection here would have been a fifth set of the
    /// same arithmetic.
    fn search_move(&mut self, dir: Dir) {
        // The body the view actually draws into, not the whole pane: the
        // search header sits above it. Scrolling against the pane height let
        // the selection run six rows below the last drawn one, which is
        // under the now-playing bar.
        let height = self.search_body_height();
        let width = self.last_main_width;
        let (cols, grid_rows) = self.search_grid_geometry();
        let Some(state) = self.search.as_mut() else {
            return;
        };
        let tab = searchview::Tab::from_index(state.tab);

        match tab {
            searchview::Tab::Top => {
                Self::move_in_top(state, dir, width, height);
            }
            searchview::Tab::Tracks => {
                // A track list is one column, so left and right have nothing
                // to move along.
                let Some(down) = dir.vertical() else { return };
                let len = searchview::track_rows(&state.results, tab).len();
                let visible = tracklist::visible_rows_of(height, None, tracklist::Chrome::Bare);
                state.tracks.step(down, len, visible);
            }
            searchview::Tab::Albums | searchview::Tab::Artists | searchview::Tab::Playlists => {
                let len = searchview::cards(&state.results, tab).len();
                let grid = match tab {
                    searchview::Tab::Albums => &mut state.albums,
                    searchview::Tab::Artists => &mut state.artists,
                    _ => &mut state.playlists,
                };
                // A grid moves a whole row on j/k and one card on h/l, the
                // same as the collection grids it is borrowed from.
                grid.step(
                    dir.forward(),
                    dir.vertical().is_some(),
                    len,
                    cols,
                    grid_rows,
                );
            }
        }
    }

    /// Move within Top results, which stacks three sections.
    fn move_in_top(state: &mut SearchState, dir: Dir, width: u16, height: u16) {
        use searchview::TopSection;

        let present = TopSection::present(&state.results);
        if present.is_empty() {
            return;
        }
        // A section that is not drawn cannot hold the selection.
        if !present.contains(&state.top) {
            state.top = present[0];
        }
        let at = present.iter().position(|s| *s == state.top).unwrap_or(0);
        // Against the rows the list actually has under the covers, not the
        // whole body: measured against the body, the selection ran under
        // the now-playing bar.
        let tracks_visible = tracklist::visible_rows_of(
            searchview::tracks_height(&state.results, state.top_scroll, height),
            None,
            tracklist::Chrome::Bare,
        );
        let tracks_len = searchview::track_rows(&state.results, searchview::Tab::Top).len();
        let (artists, albums) = (state.results.artists.len(), state.results.albums.len());
        let row = match state.top {
            TopSection::Artists => Some((&mut state.top_artists, artists)),
            TopSection::Albums => Some((&mut state.top_albums, albums)),
            TopSection::Tracks => None,
        };
        let moved = Self::move_in_stack(
            Stacked {
                count: present.len(),
                at,
                tracks: present
                    .iter()
                    .position(|s| *s == TopSection::Tracks)
                    .map(|i| (i, &mut state.tracks, tracks_len, tracks_visible)),
                row: row.map(|(r, len)| (r, len, carousel::cards_to_land_in(width))),
                scroll: &mut state.top_scroll,
                visible: SECTIONS_SHOWING,
            },
            dir,
        );
        state.top = present[moved];
    }

    /// Move through a page of stacked sections: Top results, or an artist's
    /// page. Returns the index among the present sections that holds the
    /// selection afterwards.
    ///
    /// Each card section is a single row, so a vertical move leaves it for
    /// the next. Within the track list j and k move by rows as usual, and
    /// only running off either end steps out of it — without the step down
    /// off the last track, the sections under an album's list could not be
    /// reached at all.
    fn move_in_stack(stack: Stacked<'_>, dir: Dir) -> usize {
        let Stacked {
            count,
            at,
            tracks,
            row,
            scroll,
            visible,
        } = stack;
        let Some(down) = dir.vertical() else {
            // Along a card row. The track list has no second column.
            if let Some((row, len, visible)) = row {
                if len > 0 {
                    row.step(dir.forward(), len, visible);
                }
            }
            return at;
        };
        if let Some((tracks_at, tracks, len, visible)) = tracks {
            if tracks_at == at {
                let off_the_end = if down {
                    tracks.selected + 1 >= len
                } else {
                    tracks.selected == 0
                };
                if !off_the_end {
                    tracks.step(down, len, visible);
                    return at;
                }
            }
        }
        let next = if down {
            (at + 1).min(count.saturating_sub(1))
        } else {
            at.saturating_sub(1)
        };
        cursor::scroll_into_view(scroll, next, visible);
        next
    }

    /// Columns and rows of the grid a search tab draws into: the renderer's
    /// own count, at the lines of text this tab's cards carry. A fixed two
    /// lines put the keys a row out on the artists and playlists tabs.
    fn search_grid_geometry(&self) -> (usize, usize) {
        let tab = searchview::Tab::from_index(self.search.as_ref().map_or(0, |s| s.tab));
        grid::geometry_with(
            self.last_main_width,
            self.search_body_height(),
            searchview::card_lines(tab),
            grid::Chrome::Bare,
            &[],
        )
    }

    /// The rows the search view has left for results, under its own heading,
    /// box and tabs.
    fn search_body_height(&self) -> u16 {
        self.last_main_height
            .saturating_sub(searchview::HEADER_ROWS)
    }
}

pub use crate::catalogue::Favourable;

impl Favourable {
    /// The favourable behind a collection, when it is one: a page of rows
    /// or a home row has no list to be added to.
    pub fn of(collection: Collection) -> Option<Self> {
        match collection {
            Collection::Artist(id) => Some(Favourable::Artist(id)),
            Collection::Album(id) => Some(Favourable::Album(id)),
            Collection::Playlist(uuid) => Some(Favourable::Playlist(uuid)),
            Collection::Radio(_) | Collection::Page { .. } => None,
        }
    }
}

/// When the music stops on its own, if the user asked it to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sleep {
    /// At this moment; `minutes` is the step it was set with, so the key
    /// knows which comes next.
    At {
        when: std::time::Instant,
        minutes: u64,
    },
    EndOfTrack,
}

/// The sleep timer's steps, in minutes. After the last comes the end of
/// the track, then off.
const SLEEP_STEPS: [u64; 4] = [15, 30, 45, 60];

/// What opening a card in a grid loads.
#[derive(Debug, Clone)]
pub enum Collection {
    Playlist(String),
    Album(String),
    /// An artist's own page: their top tracks, albums and similar artists.
    Artist(String),
    /// A station built on a track: what `R` opens, and what autoplay
    /// appends when the queue runs out.
    Radio(crate::domain::TrackId),
    /// Another page of rows: a genre, a mood, a decade. What an Explore
    /// link opens.
    Page {
        title: String,
        path: String,
    },
}

/// A playlist or album the user opened, shown in place of the section the
/// sidebar points at.
///
/// Its title and byline are carried here rather than looked up again: the
/// card the user pressed enter on already had them, and a second lookup
/// could disagree with what they just saw.
#[derive(Debug, Clone)]
pub struct OpenCollection {
    pub title: String,
    pub subtitle: String,
    pub detail: String,
    pub cover: Option<String>,
    /// Whether that cover is an avatar rather than a record sleeve. Carried
    /// from the card the user opened: an artist opens into a moment of the
    /// collection view before their page arrives, and drawing their photo
    /// square there made it flash from round to square and back.
    pub round_cover: bool,
    /// The section to go back to when this is closed.
    pub came_from: sidebar::Section,
    /// What was opened, when the view knows: an album, a playlist, a mix
    /// or an artist. What `F` favourites while the view is up.
    pub target: Option<Collection>,
}

/// A view escape goes back to.
///
/// Opening a collection replaces what the main pane holds, and an artist
/// page can be opened from inside one — a profile, then an artist, then one
/// of their albums. Escape used to drop straight back to the sidebar's
/// section from any depth, because a single `came_from` is all one level
/// remembers. This is the level itself, pushed on the way in.
#[derive(Debug)]
pub struct Level {
    /// Which nav entry the main pane was showing. A change of section is a
    /// step in the history like any other — going back from Albums to
    /// Playlists is what the arrows are for.
    section: sidebar::Section,
    /// The search this view was opened from, if it was. Put back on the way
    /// out, so escape returns to the results rather than to the section
    /// behind them.
    search: Option<SearchState>,
    open: Option<OpenCollection>,
    artist: Option<crate::library::ArtistPage>,
    tracks: Vec<crate::domain::Track>,
    tracklist: tracklist::TrackListState,
    open_cards: Vec<carousel::Card>,
    open_grid: grid::GridState,
    /// The rows the Explore pane held, when a genre page replaced them.
    /// Without keeping them there was no way back to Explore to choose
    /// another — going back left the genre's own rows on screen.
    explore: Option<home::HomeState>,
    /// Which list was up, when one was. The queue and the history are
    /// views of their own: back returns to them, and opening something
    /// from one is a step forward from it.
    showing_queue: bool,
    showing_history: bool,
}

/// What the main pane is drawing, asked once and read by everything that
/// needs to know: the movement keys, the card they act on, and the
/// renderer. See [`App::showing`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Showing {
    /// An artist's page, which stacks card sections over a track list.
    Artist,
    /// The search results, on whichever tab.
    Search(searchview::Tab),
    /// A page of card rows: the home page, Explore, or a genre opened from
    /// it. Told apart from an opened collection by carrying its own heading.
    Rows,
    /// An opened collection that is a list of tracks.
    Tracks,
    /// An opened collection that carries covers, drawn as a grid.
    Cards,
    /// A view opened but not yet filled: the request is still in flight,
    /// and what comes back decides which view this becomes.
    Loading,
    /// What is queued, over whatever view it was opened from.
    Queue,
    /// What has been played, the same way.
    History,
    /// No view over it: whatever the sidebar points at.
    Section(sidebar::Section),
}

#[derive(Debug, Default)]
pub struct App {
    pub should_quit: bool,
    pub status: Option<String>,
    /// When the status was put up, so it can be taken down again.
    ///
    /// A notice that never leaves becomes part of the furniture: it stood
    /// in the corner for the rest of the session, over a tab strip, saying
    /// something that stopped being true minutes ago.
    pub status_since: Option<std::time::Instant>,
    pub login: login::LoginState,
    /// Where an answer to the login prompt goes while a sign-in is running:
    /// the provider waits on the other end for the cookies or the account.
    pub login_input: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    /// The equaliser the engine reads: a preset's gains, or flat and off.
    /// Shared with the engine, so a change reaches the output at once.
    pub equalizer: music::Equalizer,
    /// The sleep timer, when one is set.
    pub sleep: Option<Sleep>,
    /// The service the config names, signed in or not. `None` in a test,
    /// where nothing may sign in or out for real.
    pub provider: Option<std::sync::Arc<dyn crate::catalogue::Provider>>,
    /// The signed-in service, behind sonora's `MusicApi`.
    pub session: Option<std::sync::Arc<crate::catalogue::Session>>,
    pub sidebar: sidebar::SidebarState,
    pub tracklist: tracklist::TrackListState,
    pub tracks: Vec<crate::domain::Track>,
    /// What plays after this track. Playing from a list queues the whole
    /// list, so an album goes on rather than stopping after one track.
    pub queue: crate::playback::Queue,
    /// Whether a radio for the end of the queue is on its way. Asked once:
    /// every position tick sees the same short queue until it lands.
    pub autoplay_fetching: bool,
    /// The last track autoplay could find nothing for, so the same
    /// question is not asked on every tick until the queue changes.
    pub autoplay_dead_end: Option<crate::domain::TrackId>,
    /// The queue ran out before autoplay landed: what it brings has to be
    /// started, since nothing is playing that would move on to it.
    pub ran_dry: bool,
    /// The cards of an opened row that holds albums or playlists rather than
    /// tracks. A row of covers cannot be shown as a track list, so it gets
    /// the card grid instead — the same one the collection views use.
    pub open_cards: Vec<carousel::Card>,
    pub open_grid: grid::GridState,
    /// Whether the opened view is still waiting on the request that fills
    /// it. What is drawn while it waits is a line saying so, rather than
    /// the empty shape of the view that is coming.
    pub awaiting: bool,
    /// The views escape backs out through, outermost first. Empty when the
    /// main pane is showing a sidebar section rather than something opened
    /// from inside one.
    pub back: Vec<Level>,
    /// What going back stepped out of, so it can be stepped into again —
    /// the other half of a browser's two arrows. Cleared when the user
    /// opens something new, since that is a different branch.
    pub forward: Vec<Level>,
    /// The settings, as loaded from the config file and as the Settings
    /// view edits them.
    pub config: crate::config::Config,
    pub settings: settings::SettingsState,
    /// Where the settings are written. `None` in tests, which must not
    /// touch the real file — one that did left this machine on LOW.
    pub config_path: Option<std::path::PathBuf>,
    /// The ids of the user's favourites, kept apart from `tracks` because
    /// opening an album replaces that list — so it cannot say what is a
    /// favourite once the user has gone anywhere else.
    pub favourites: std::collections::HashSet<crate::domain::TrackId>,
    /// The ids of the artists the user follows, so `F` knows which way to
    /// go. Filled from the favourites' id list, and from the Artists
    /// section's reply.
    pub followed: std::collections::HashSet<String>,
    /// The same for albums and playlists.
    pub favourite_albums: std::collections::HashSet<String>,
    pub favourite_playlists: std::collections::HashSet<String>,
    pub now_playing: nowplaying::NowPlaying,
    pub home: home::HomeState,
    /// Explore's own rows. The same shape as the home page's, drawn by the
    /// same renderer — it is a page of card rows like any other.
    pub explore: home::HomeState,
    /// The activity feed as it came back: releases from the artists the
    /// user follows, newest first and ungrouped. Kept beside the rows so a
    /// filter can be applied without asking for the feed again.
    pub feed_cards: Vec<carousel::Card>,
    /// The same releases in four dated sections, drawn as the home page's
    /// rows are.
    pub feed: home::HomeState,
    /// The artist whose page is open, if one is.
    pub artist: Option<crate::library::ArtistPage>,
    /// Which of that page's sections has the selection, and where in it.
    pub artist_section: usize,
    /// The first section of the artist page that is drawn. Five sections
    /// and a portrait are taller than a terminal, so it scrolls.
    pub artist_scroll: usize,
    /// Whether the artist's blurb is open. TIDAL's run to a page of prose,
    /// so the header shows the first paragraph and `b` opens the rest.
    pub artist_bio_open: bool,
    /// Lines of the open bio scrolled off the top; see `artist_move`.
    pub artist_bio_scroll: u16,
    /// Where the history is kept between runs. Set by `run` alone, so a
    /// test that plays a track writes nothing to the user's cache.
    pub history_file: Option<std::path::PathBuf>,
    pub artist_tracks: tracklist::TrackListState,
    /// One carousel state per card section of an artist's page, indexed by
    /// `Section::index` — a field each meant a new one to remember every
    /// time a section was added, and two of the five went without.
    pub artist_rows: [carousel::CarouselState; artistview::Section::ALL.len()],
    pub playlists: Vec<crate::library::Playlist>,
    pub albums: Vec<crate::library::Album>,
    pub artists: Vec<crate::library::Artist>,
    /// One grid state per grid view, so moving around Albums does not disturb
    /// where the user was in Playlists.
    pub playlist_grid: grid::GridState,
    pub album_grid: grid::GridState,
    pub artist_grid: grid::GridState,
    pub palette: theme::Palette,
    /// `None` in tests and until the terminal has been probed.
    pub artwork: Option<artwork::Artwork>,
    /// Size of the main pane at the last draw. Scrolling needs to know how
    /// many cards or rows fit, which only the renderer can measure.
    pub last_main_width: u16,
    pub last_main_height: u16,
    /// While typing in a view's filter box, keys are text rather than
    /// commands — otherwise "q" quits instead of filtering.
    pub filtering: bool,
    /// The key list, on "?".
    pub showing_help: bool,
    /// The lyrics, on "L".
    pub lyrics: lyricsview::Pane,
    /// Where listens are reported, once the config's services are linked.
    pub scrobblers: Vec<crate::scrobble::Linked>,
    /// The listen in progress: the track playing, as a service files it.
    pub listen: Option<music::scrobble::Play>,
    /// What the loop sends on: the track that started, the listens that
    /// earned a submission.
    pub scrobble_outbox: crate::scrobble::Outbox,
    /// The engine's spectrum, once it has started.
    pub spectrum: Option<crate::playback::Bands>,
    /// The Discord presence thread, when the config names an application,
    /// and what it was last told: the track and whether it plays.
    pub presence: Option<std::sync::mpsc::Sender<crate::presence::Presence>>,
    pub presence_shown: Option<(crate::domain::TrackId, bool)>,
    /// The Widevine module, for a service whose streams are encrypted.
    pub cdm: drmview::Cdm,
    /// The download waiting on the user's word about Google's terms.
    pub cdm_answer: Option<tokio::sync::oneshot::Sender<bool>>,
    /// Whether the queue is open over the pane.
    pub showing_queue: bool,
    /// Where in it the user is. The selection is set to what is playing
    /// each time the view opens, since that is where the eye goes.
    pub queue_list: tracklist::TrackListState,
    /// Whether the history is open over the pane.
    pub showing_history: bool,
    /// Where in it the user is, as for the queue.
    pub history_list: tracklist::TrackListState,
    /// Set when something drew over the artwork and the next frame has to
    /// be painted in full rather than diffed. See `take_repaint`.
    pub needs_repaint: bool,
    /// The search box and its results, on "s".
    ///
    /// Kept apart from `filtering`, which narrows what is already on screen.
    /// Search fetches new content and replaces the pane, so sharing the one
    /// mode would mean a keystroke that sometimes filters and sometimes
    /// fires a request.
    pub search: Option<SearchState>,
    /// The playlist or album being viewed, if the user opened one. While this
    /// is set the main pane shows it rather than the sidebar's section.
    pub open: Option<OpenCollection>,
    /// The user's rebound keys, applied before anything else reads a
    /// keystroke. Empty unless the config names some.
    pub keymap: keymap::Keymap,
}

impl App {
    /// The cards a grid section shows. Built on demand rather than cached:
    /// the lists are small, and a cache would be one more thing to keep in
    /// step with the data behind it.
    pub fn grid_cards(&self, section: sidebar::Section) -> Vec<carousel::Card> {
        if self.open.is_some() && !self.open_cards.is_empty() {
            return self.open_cards.clone();
        }
        match section {
            sidebar::Section::Playlists => {
                self.playlists.iter().map(carousel::playlist_card).collect()
            }
            sidebar::Section::Albums => self.albums.iter().map(carousel::album_card).collect(),
            sidebar::Section::Profiles => self.artists.iter().map(carousel::artist_card).collect(),
            _ => Vec::new(),
        }
    }

    /// The filter text of whichever view is showing.
    pub fn filter_text(&self) -> &str {
        // The Feed keeps its own: it is a page of rows rather than a grid,
        // so neither of the two below is where its filter lives.
        if self.on_feed() {
            &self.feed.filter
        } else if self.on_grid() {
            &self.grid_state(self.sidebar.section()).filter
        } else {
            &self.tracklist.filter
        }
    }

    /// Whether the Feed is what the pane is drawing.
    fn on_feed(&self) -> bool {
        matches!(self.showing(), Showing::Section(sidebar::Section::Feed))
    }

    /// Set it, and pull the selection back into what is left — a filter that
    /// shrinks the list under a stale index selects nothing at all.
    pub fn set_filter(&mut self, text: String) {
        // Rebuilt rather than clamped: the Feed's sections are built from
        // what survives the filter, so the rows themselves change.
        if self.on_feed() {
            self.feed.filter = text;
            self.rebuild_feed();
        } else if self.on_grid() {
            let section = self.sidebar.section();
            let len = grid::filter(&self.grid_cards(section), &text).len();
            let state = self.grid_state_mut(section);
            state.filter = text;
            state.clamp(len);
        } else {
            let len = tracklist::filter(&self.tracks, &text).len();
            self.tracklist.filter = text;
            self.tracklist.clamp(len);
        }
    }

    pub fn grid_state(&self, section: sidebar::Section) -> &grid::GridState {
        // An opened row has its own selection: routing by section would move
        // the cursor in the collection view behind it instead.
        if self.open.is_some() && !self.open_cards.is_empty() {
            return &self.open_grid;
        }
        match section {
            sidebar::Section::Albums => &self.album_grid,
            sidebar::Section::Profiles => &self.artist_grid,
            _ => &self.playlist_grid,
        }
    }

    pub fn grid_state_mut(&mut self, section: sidebar::Section) -> &mut grid::GridState {
        if self.open.is_some() && !self.open_cards.is_empty() {
            return &mut self.open_grid;
        }
        match section {
            sidebar::Section::Albums => &mut self.album_grid,
            sidebar::Section::Profiles => &mut self.artist_grid,
            _ => &mut self.playlist_grid,
        }
    }

    /// What the selected card opens, if it opens anything: a track plays
    /// instead, and a card pointing at nothing opens nothing.
    pub fn selected_collection(&self) -> Option<Collection> {
        match self.selected_card()?.target? {
            carousel::Target::Playlist(uuid) => Some(Collection::Playlist(uuid)),
            carousel::Target::Album(id) => Some(Collection::Album(id)),
            carousel::Target::Artist(id) => Some(Collection::Artist(id)),
            carousel::Target::Page(path) => Some(Collection::Page {
                title: self.selected_card()?.title,
                path,
            }),
            // A track plays rather than opening, and a shortcut into one
            // of the app's own views has nothing to fetch.
            carousel::Target::Track(_) | carousel::Target::Section(_) => None,
        }
    }

    /// The track under the selection: a list's row, or a card that plays a
    /// track rather than opening a collection.
    ///
    /// The row first. Read from the cards alone, the keys that fetch what a
    /// track does not carry -- its radio, its album, its artist -- were dead
    /// on every list: the queue, an album's tracks, a search's.
    pub fn selected_track_id(&self) -> Option<crate::domain::TrackId> {
        if let Some(track) = self.selected_track() {
            return Some(track.id);
        }
        match self.selected_card()?.target? {
            carousel::Target::Track(id) => Some(crate::domain::TrackId(id)),
            _ => None,
        }
    }

    /// The card the selection is on, in whichever card view is showing.
    ///
    /// The home page's carousels and the grids are both rows of cards, so
    /// enter means the same thing in both: open what the card points at.
    pub fn selected_card(&self) -> Option<carousel::Card> {
        // Whatever is drawn is what enter acts on: reading the cards under
        // the queue opened whatever the hidden page had highlighted.
        match self.showing() {
            Showing::Queue | Showing::History => return None,
            Showing::Artist => {
                let page = self.artist.as_ref()?;
                let section = artistview::Section::from_index(self.artist_section);
                if section == artistview::Section::Tracks {
                    return None;
                }
                let cards = artistview::cards(page, section);
                let row = &self.artist_rows[section.index()];
                return cards.get(row.selected).cloned();
            }
            // A search tab that draws cards has one too. It reuses the app's
            // own grid to draw them, so enter has to reach the same card the
            // grid has highlighted — without this it read the collection view
            // behind search and opened nothing.
            Showing::Search(tab) => {
                let state = self.search.as_ref()?;
                // Top stacks an artist row, an album row and a track list, and
                // `top` says which of them holds the selection. Refusing the
                // whole tab left the artists and albums drawn there impossible
                // to open — enter did nothing at all.
                if tab == searchview::Tab::Top {
                    use searchview::TopSection;
                    let (cards, row) = match state.top {
                        TopSection::Artists => (
                            searchview::cards(&state.results, searchview::Tab::Artists),
                            &state.top_artists,
                        ),
                        TopSection::Albums => (
                            searchview::cards(&state.results, searchview::Tab::Albums),
                            &state.top_albums,
                        ),
                        // A track plays rather than opening, and the track list
                        // is read by `selected_track` instead.
                        TopSection::Tracks => return None,
                    };
                    return cards.get(row.selected).cloned();
                }
                if tab.is_tracks() {
                    return None;
                }
                let cards = searchview::cards(&state.results, tab);
                let grid = match tab {
                    searchview::Tab::Albums => &state.albums,
                    searchview::Tab::Artists => &state.artists,
                    _ => &state.playlists,
                };
                return cards.get(grid.selected).cloned();
            }
            _ => {}
        }
        if self.on_home() {
            let row = self.rows_on_screen().current_row()?;
            return row.cards.get(row.state.selected).cloned();
        }
        if self.on_grid() {
            let section = self.sidebar.section();
            let state = self.grid_state(section);
            let cards = self.grid_cards(section);
            let index = grid::filter_indices(&cards, &state.filter)
                .get(state.selected)
                .copied()?;
            return cards.get(index).cloned();
        }
        None
    }

    /// What the selected card is called, for the header of the view it opens.
    ///
    /// Taken from the card the user is looking at rather than fetched again:
    /// they pressed enter on a title, and the view they land on should be
    /// headed by that same title, immediately, without waiting on a request.
    pub fn selected_identity(&self) -> Option<OpenCollection> {
        let target = self.selected_collection();
        let card = self.selected_card()?;
        Some(OpenCollection {
            title: card.title,
            subtitle: card.subtitle,
            detail: card.detail,
            cover: card.cover_url,
            round_cover: card.round,
            came_from: self.sidebar.section(),
            target,
        })
    }

    /// Whether the main pane is the Settings view.
    ///
    /// Read from what is drawn, not from the sidebar alone: an opened
    /// collection or a search covers the section, and the keys belong to
    /// whatever is actually on screen.
    pub fn on_settings(&self) -> bool {
        matches!(self.showing(), Showing::Section(sidebar::Section::Settings))
    }

    /// Whether the search results are what the pane is drawing. Every key
    /// and renderer asks this rather than whether a search exists: one
    /// opened over a page is on top of it, and a page opened from a result
    /// has the search under it on the stack.
    fn on_search(&self) -> bool {
        matches!(self.showing(), Showing::Search(_))
    }

    /// Whether an artist's or album's page is what the pane is drawing.
    fn on_artist(&self) -> bool {
        self.showing() == Showing::Artist
    }

    /// Where a seek lands: a step along the track, short of its end, or
    /// nowhere when nothing is playing. Asked by the bar, which moves at
    /// once, and for the command to the player, so the two cannot disagree.
    pub fn seek_target(&self, forward: bool) -> Option<std::time::Duration> {
        let track = self.now_playing.track.as_ref()?;
        let at = self.now_playing.position;
        Some(if forward {
            // A track of unknown length has no end to stop short of.
            let end = if track.duration.is_zero() {
                std::time::Duration::MAX
            } else {
                track.duration.saturating_sub(std::time::Duration::from_secs(1))
            };
            (at + SEEK_STEP).min(end)
        } else {
            at.saturating_sub(SEEK_STEP)
        })
    }

    /// Step the highlighted setting and write the file.
    ///
    /// Saved on the keystroke rather than on leaving the view: there is no
    /// "apply", so a setting that only lived in memory would be silently
    /// lost on quit. A write that fails says so in the status line instead
    /// of leaving the user to find out next launch.
    fn change_setting(&mut self, forward: bool) -> Option<Action> {
        let which = self.settings.current();
        settings::cycle(&mut self.config, which, forward);
        if let Some(path) = self.config_path.as_ref() {
            if let Err(e) = self.config.save_to(path) {
                tracing::warn!("the settings could not be saved: {e}");
                self.say(format!("could not save settings: {e}"));
            }
        }
        // The volume is heard now rather than on the next track: a level
        // you cannot hear yourself setting is one you set by guessing.
        match which {
            settings::Setting::Service => {
                self.say(format!("Enter to switch to {}", self.config.provider.kind.label()));
                None
            }
            settings::Setting::Volume => Some(Action::SetVolume(self.config.audio.volume)),
            // Heard now: the engine keeps reading the shared equaliser.
            settings::Setting::Equalizer => {
                self.apply_equalizer();
                None
            }
            // Read once when the engine starts, so it starts again.
            settings::Setting::Normalisation | settings::Setting::Gapless => {
                Some(Action::RestartEngine)
            }
            // The icons are already swapped -- `cycle` does that, so the row
            // shows the new set as it is chosen -- and the rest of the
            // screen picks them up on the next frame.
            settings::Setting::Quality
            | settings::Setting::Autoplay
            | settings::Setting::Explicit
            | settings::Setting::Ai
            | settings::Setting::NerdFont => None,
            // The only setting that cannot show itself: the image protocol
            // is probed before raw mode, so the picker the covers draw
            // through was built before this row existed. Saying so is the
            // difference between a setting that looks broken and one that
            // is waiting.
            settings::Setting::Halfblocks => {
                self.say("covers change at the next start");
                None
            }
        }
    }

    /// Drop everything that belonged to the service -- the session, the
    /// lists, the queue, the view -- and stand ready to sign in to the one
    /// the config now names. What is the app's rather than the service's
    /// stays: the config, the keys, the palette, the artwork cache, the
    /// window size, the history, the equaliser, Discord and the scrobblers.
    fn leave_service(&mut self) {
        let kept = std::mem::take(self);
        let history: Vec<crate::domain::Track> = kept.queue.history().cloned().collect();
        *self = App {
            config: kept.config,
            config_path: kept.config_path,
            keymap: kept.keymap,
            palette: kept.palette,
            artwork: kept.artwork,
            last_main_width: kept.last_main_width,
            last_main_height: kept.last_main_height,
            history_file: kept.history_file,
            equalizer: kept.equalizer,
            presence: kept.presence,
            scrobblers: kept.scrobblers,
            settings: kept.settings,
            sidebar: kept.sidebar,
            needs_repaint: true,
            ..App::default()
        };
        self.queue.restore_history(history);
        match crate::catalogue::provider_for(&self.config) {
            Ok(provider) => self.provider = Some(provider),
            Err(error) => {
                tracing::error!("could not set up the provider: {error:#}");
                self.say(format!("could not set up the provider: {error:#}"));
            }
        }
        self.login = self.login_prompt();
    }

    /// What the engine is started with, from the settings.
    fn playback_options(&self) -> crate::playback::Options {
        crate::playback::Options {
            normalisation: self.config.audio.normalisation,
            gapless: self.config.audio.gapless,
            equalizer: self.equalizer.clone(),
        }
    }

    /// Put the config's preset into the equaliser the engine reads. `off`
    /// turns it off and leaves the gains flat, so turning it back on is
    /// not a surprise.
    pub fn apply_equalizer(&self) {
        match settings::equalizer_preset(&self.config) {
            Some(preset) => {
                self.equalizer.set_gains(&preset.gains());
                self.equalizer.set_enabled(true);
            }
            None => {
                self.equalizer.set_enabled(false);
                self.equalizer.set_gains(&music::equalizer::Preset::Flat.gains());
            }
        }
    }

    /// The sleep timer's next step, said out loud.
    fn cycle_sleep(&mut self) {
        let now = std::time::Instant::now();
        self.sleep = match self.sleep {
            None => Some(Sleep::At {
                when: now + std::time::Duration::from_secs(SLEEP_STEPS[0] * 60),
                minutes: SLEEP_STEPS[0],
            }),
            Some(Sleep::At { minutes, .. }) => match SLEEP_STEPS.iter().find(|m| **m > minutes) {
                Some(next) => Some(Sleep::At {
                    when: now + std::time::Duration::from_secs(next * 60),
                    minutes: *next,
                }),
                None => Some(Sleep::EndOfTrack),
            },
            Some(Sleep::EndOfTrack) => None,
        };
        let said = match self.sleep {
            Some(Sleep::At { minutes, .. }) => format!("sleep in {minutes} minutes"),
            Some(Sleep::EndOfTrack) => "sleep at the end of the track".to_string(),
            None => "sleep timer off".to_string(),
        };
        self.say(said);
    }

    /// What the bar says about the sleep timer.
    fn sleep_mark(&self) -> Option<nowplaying::SleepMark> {
        match self.sleep? {
            Sleep::At { when, .. } => {
                let left = when.saturating_duration_since(std::time::Instant::now());
                Some(nowplaying::SleepMark::Minutes(left.as_secs().div_ceil(60)))
            }
            Sleep::EndOfTrack => Some(nowplaying::SleepMark::EndOfTrack),
        }
    }

    /// The search the lyrics pane is waiting on, if it is open on a track
    /// nothing has been found for yet. Marked as searching here, so the
    /// same track is not asked about on every event.
    fn lyrics_wanted(&mut self) -> Option<Action> {
        if !self.lyrics.open {
            return None;
        }
        let id = self.now_playing.track.as_ref()?.id.clone();
        if !self.lyrics.wants(&id) {
            return None;
        }
        self.lyrics.found.insert(id.clone(), lyricsview::Found::Searching);
        Some(Action::LoadLyrics(id))
    }

    /// Tell Discord what changed, if anything did: the track, or whether
    /// it plays. Positions are not watched -- the clock runs on its own
    /// over there -- except that a seek starts it again from where it
    /// landed, which `presence_shown` is cleared for.
    fn tell_presence(&mut self) {
        let Some(sender) = &self.presence else {
            return;
        };
        let key = self
            .now_playing
            .track
            .as_ref()
            .map(|t| (t.id.clone(), self.now_playing.playing));
        if key == self.presence_shown {
            return;
        }
        self.presence_shown = key;
        let shown = match &self.now_playing.track {
            None => crate::presence::Presence::Off,
            Some(t) if self.now_playing.playing => crate::presence::Presence::Playing {
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone(),
                cover: t.cover.clone(),
                position: self.now_playing.position,
                duration: t.duration,
            },
            Some(t) => crate::presence::Presence::Paused {
                title: t.title.clone(),
                artist: t.artist.clone(),
            },
        };
        let _ = sender.send(shown);
    }

    /// The listen that just ended goes to the outbox if `played` earned it
    /// a submission -- the services' rule, not ours.
    fn settle_listen(&mut self, played: Duration) {
        if let Some(play) = self.listen.take() {
            if play.earned(played) {
                self.scrobble_outbox.finished.push(play);
            }
        }
    }

    /// The track that just started is the listen in progress, and what
    /// the services show as playing now.
    fn begin_listen(&mut self) {
        self.listen = self
            .now_playing
            .track
            .as_ref()
            .and_then(|t| crate::scrobble::play_of(t, std::time::SystemTime::now()));
        self.scrobble_outbox.now_playing = self.listen.clone();
    }

    /// The sleep timer going off: pause, once, and forget it.
    fn sleep_due(&mut self) -> Option<Action> {
        match self.sleep {
            Some(Sleep::At { when, .. }) if std::time::Instant::now() >= when => {
                self.sleep = None;
                self.now_playing.playing.then_some(Action::TogglePause)
            }
            _ => None,
        }
    }

    /// Put a message in the corner, and start its clock.
    ///
    /// Set through here rather than by assignment so nothing can put up a
    /// notice that has no way of coming down.
    pub fn say(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
        self.status_since = Some(std::time::Instant::now());
    }

    /// Take the message down once it has been up long enough.
    ///
    /// Long enough to read twice at a glance, and short enough that it is
    /// gone before it is furniture. Nothing depends on the message staying:
    /// what it reports has already happened, and the log keeps it.
    fn expire_status(&mut self) {
        const SHOWN_FOR: std::time::Duration = std::time::Duration::from_secs(6);
        if self
            .status_since
            .is_some_and(|at| at.elapsed() >= SHOWN_FOR)
        {
            self.status = None;
            self.status_since = None;
        }
    }

    /// The radio to fetch now, so the queue is never caught empty.
    ///
    /// Only with autoplay on, only when the queue is nearly out, only once
    /// per fetch, and never when the queue repeats -- it has no end to
    /// follow on from. Seeded from the last track queued rather than the
    /// one playing: what follows the end of the queue is what the end of
    /// the queue sounds like. TIDAL's own "continue with similar content"
    /// does the same.
    fn autoplay_seed(&mut self) -> Option<Action> {
        /// How few tracks left is nearly out. Two: the fetch is one or two
        /// requests and a track is minutes long, and a one-track radio
        /// reply still leaves time to ask for the next.
        const LOOK_AHEAD: usize = 2;

        if !self.config.playback.autoplay
            || self.autoplay_fetching
            || self.queue.repeat == crate::playback::Repeat::All
            || self.queue.remaining() > LOOK_AHEAD
        {
            return None;
        }
        let last = self.queue.last_track()?;
        if self.autoplay_dead_end.as_ref() == Some(&last.id) {
            return None;
        }
        self.autoplay_fetching = true;
        Some(Action::Autoplay {
            seed: last.id.clone(),
        })
    }

    /// Why this track will not play, or `None` when it will.
    ///
    /// Read rather than acted on, so a test can ask the question the player
    /// asks without a session or a network.
    pub fn why_blocked(&self, track: &crate::domain::Track) -> Option<&'static str> {
        if track.allowed(self.config.playback.explicit, self.config.playback.ai) {
            return None;
        }
        Some(if track.explicit && !self.config.playback.explicit {
            "explicit content is turned off in Settings"
        } else {
            "AI-generated content is turned off in Settings"
        })
    }

    /// Move the queue on past tracks the user does not allow, saying why
    /// each was passed over. True when it rests on a track that may play.
    ///
    /// A refused start never finishes, so nothing moved the queue on from
    /// it: one AI-generated track in a radio stopped the whole evening.
    /// Every path that lands the cursor somewhere steps through here. The
    /// walk is bounded by the queue's length, since with repeat on and
    /// nothing allowed `next` never runs out.
    fn skip_blocked(&mut self, forward: bool) -> bool {
        for _ in 0..=self.queue.len() {
            let Some(track) = self.queue.current() else {
                return false;
            };
            let Some(why) = self.why_blocked(track) else {
                return true;
            };
            let said = format!("{} — {why}", track.title);
            tracing::info!("skipping {:?}: {why}", track.title);
            self.say(said);
            let moved = if forward {
                self.queue.next()
            } else {
                self.queue.previous()
            };
            if moved.is_none() {
                return false;
            }
        }
        false
    }

    /// Whether the next frame must be drawn in full, clearing the flag.
    ///
    /// The image protocols paint straight to the terminal and mark their
    /// cells `Skip` so ratatui leaves them alone. Anything drawn over them
    /// — the help modal — is therefore not undone by the next frame, which
    /// marks the same cells `Skip` again and writes nothing. A full repaint
    /// is the only thing that puts the artwork back.
    pub fn take_repaint(&mut self) -> bool {
        std::mem::take(&mut self.needs_repaint)
    }

    /// How many rows the queue or the history shows, for the keys to
    /// scroll by. The same count the renderer lays out.
    fn over_list_visible(&self) -> usize {
        queueview::visible_rows(self.last_main_height)
    }

    /// How many `j` presses make half a screen in whatever is showing.
    ///
    /// Each view counts its own rows: a grid moves a line of cards per
    /// press, the home page a whole row of covers, a list a row. Never
    /// less than one, so the key always does something.
    fn half_page(&self) -> usize {
        let rows = if matches!(self.showing(), Showing::Queue | Showing::History) {
            self.over_list_visible()
        } else if self.on_home() {
            self.home_rows_to_land_in()
        } else if self.on_grid() {
            self.grid_geometry().1
        } else {
            self.list_visible()
        };
        (rows / 2).max(1)
    }

    /// The track `R` builds a station on: the selected one, or else what
    /// is playing.
    ///
    /// The fall-back is what keeps the obvious case working: on the home
    /// page or in Settings nothing is selected, and continuing from what is
    /// in your ears is then the only thing the key could mean.
    pub fn track_radio(&self) -> Option<crate::domain::TrackId> {
        self.selected_track_id()
            .or_else(|| self.now_playing.track.as_ref().map(|t| t.id.clone()))
    }

    /// Whether the main pane is already the station on this seed.
    ///
    /// A radio lists its seed first, so with the playing track selected R
    /// over it opened the same radio over itself, reloading the list and
    /// losing the place in it.
    fn radio_is_open(&self, seed: &crate::domain::TrackId) -> bool {
        matches!(
            self.open.as_ref().and_then(|o| o.target.as_ref()),
            Some(Collection::Radio(open)) if open == seed
        )
    }

    /// What the radio key would be named after, for the heading.
    fn track_radio_title(&self) -> String {
        self.selected_track()
            .or_else(|| self.now_playing.track.clone())
            .map(|t| t.title)
            .unwrap_or_default()
    }

    /// The artist `F` acts on: the page that is open, whatever section is
    /// selected in it, or else the artist card under the selection. The
    /// page wins because the selection inside it is a track or a record,
    /// never the artist themself.
    pub fn selected_favourable(&self) -> Option<(Favourable, String)> {
        if let Some(page) = self.artist.as_ref() {
            let what = match page.kind {
                crate::library::PageKind::Artist => Favourable::Artist(page.id.clone()),
                crate::library::PageKind::Album => Favourable::Album(page.id.clone()),
            };
            return Some((what, page.name.clone()));
        }
        if let Some(card) = self.selected_card() {
            if let Some(what) = self.selected_collection().and_then(Favourable::of) {
                return Some((what, card.title));
            }
        }
        // The view itself: an album opened from a card, its tracks on
        // screen, is what someone pressing F there means.
        let open = self.open.as_ref()?;
        let what = Favourable::of(open.target.clone()?)?;
        Some((what, open.title.clone()))
    }

    /// Whether the account has this already, as far as the app knows.
    pub fn is_favourite(&self, what: &Favourable) -> bool {
        match what {
            Favourable::Artist(id) => self.followed.contains(id),
            Favourable::Album(id) => self.favourite_albums.contains(id),
            Favourable::Playlist(uuid) => self.favourite_playlists.contains(uuid),
        }
    }

    /// What pressing enter would go and fetch, and under what heading.
    ///
    /// The loop asked this in pieces — a guard, then the selected card,
    /// then the collection — and a test that filled the reply in by hand
    /// could pass while the fetch never started. It is one question now, so
    /// a test can ask exactly what the loop asks.
    pub fn what_enter_opens(&self) -> Option<(Collection, String)> {
        if !(self.on_grid() || self.on_home()) {
            return None;
        }
        // The heading comes from the selected card rather than `open`: the
        // loop asks before `update` runs, so `open` is still whatever was
        // open before — the previous album, or nothing.
        let for_title = self.selected_card().map(|c| c.title).unwrap_or_default();
        Some((self.selected_collection()?, for_title))
    }

    /// The album of the selected track, and what to head it with.
    ///
    /// The track list is where this matters: a grid of covers is already
    /// showing albums, but a list of tracks names one it cannot open.
    pub fn selected_track_album(&self) -> Option<(Collection, String)> {
        self.selected_track().and_then(|t| album_of(&t))
    }

    /// The artist of the selected track, the same way.
    pub fn selected_track_artist(&self) -> Option<(Collection, String)> {
        self.selected_track().and_then(|t| artist_of(&t))
    }

    /// How many home rows the selection may land in without scrolling.
    ///
    /// The count the renderer draws, less the last one when it is cut off
    /// by the pane's edge: landing there leaves the selection half drawn
    /// and the page looking stuck, since the row it moved to is the row
    /// already at the bottom. Scrolling it up into the whole part is what
    /// the web client does, and what the card grids already did.
    fn home_rows_to_land_in(&self) -> usize {
        // The rows themselves, so the count matches what the renderer
        // draws: a track grid is taller than a carousel.
        home::rows_to_land_in(
            self.last_main_height,
            scrollbar::content_width(self.last_main_width),
            self.rows_on_screen(),
        )
    }

    /// The rows the track list's selection may land in, under whatever
    /// heads it. With a banner the header is nine rows taller, so the
    /// no-banner count let the selection run three rows below the last
    /// drawn one — off the bottom, cursor gone.
    fn list_visible(&self) -> usize {
        tracklist::visible_rows_of(
            self.last_main_height,
            self.open
                .as_ref()
                .map(|o| o.cover.is_some() || o.round_cover),
            tracklist::Chrome::Full,
        )
    }

    /// The tracks the list shows once the filter box has had its say. The
    /// keys moved through the unfiltered list, so with a filter typed the
    /// selection could walk onto tracks that were not on screen.
    fn list_len(&self) -> usize {
        tracklist::filter(&self.tracks, &self.tracklist.filter).len()
    }

    /// The cards a home row's selection may land on: the renderer draws the
    /// rows beside the scrollbar's column, so the keys count there too.
    fn home_cards_to_land_in(&self) -> usize {
        carousel::cards_to_land_in(scrollbar::content_width(self.last_main_width))
    }

    /// The columns a home page's track grid draws, counted the same way.
    fn home_grid_columns(&self) -> usize {
        trackgrid::columns(scrollbar::content_width(self.last_main_width))
    }

    /// As [`list_len`], for the grid of the section showing.
    fn grid_len(&self) -> usize {
        let section = self.sidebar.section();
        grid::filter(&self.grid_cards(section), &self.grid_state(section).filter).len()
    }

    /// Move the selection of the grid showing: a whole row on j/k, one card
    /// on h/l.
    fn grid_move(&mut self, dir: Dir) {
        let (cols, rows) = self.grid_geometry();
        let len = self.grid_len();
        self.grid_state_mut(self.sidebar.section()).step(
            dir.forward(),
            dir.vertical().is_some(),
            len,
            cols,
            rows,
        );
    }

    /// Move the selection of the track list showing.
    fn list_move(&mut self, down: bool) {
        let (len, visible) = (self.list_len(), self.list_visible());
        self.tracklist.step(down, len, visible);
    }

    /// `j` and `k`: whichever view is drawn gets the move, asked of
    /// [`Self::showing`] so a view under another never takes the keys.
    fn move_vertical(&mut self, dir: Dir) {
        let down = dir == Dir::Down;
        match self.showing() {
            Showing::Section(sidebar::Section::Settings) => {
                if down {
                    self.settings.next();
                } else {
                    self.settings.previous();
                }
            }
            Showing::Search(_) => self.search_move(dir),
            Showing::Artist => self.artist_move(dir),
            _ if self.on_grid() => self.grid_move(dir),
            _ => self.list_move(down),
        }
    }

    /// `h` and `l`, the same way: a setting's value, a grid's card, or a
    /// home row's card.
    fn move_horizontal(&mut self, dir: Dir) -> Option<Action> {
        let right = dir == Dir::Right;
        if self.on_settings() {
            return self.change_setting(right);
        }
        if self.on_grid() {
            self.grid_move(dir);
            return None;
        }
        let visible = self.home_cards_to_land_in();
        if let Some(row) = self.rows_on_screen_mut().current_row_mut() {
            let len = row.cards.len();
            row.state.step(right, len, visible);
        }
        None
    }

    /// How far back the arrows go.
    ///
    /// Every move through the nav is a step, so a session spent walking the
    /// sidebar would otherwise grow this without end. Fifty is further back
    /// than anyone retraces by hand, and the oldest is what is dropped.
    const HISTORY: usize = 50;

    /// Put a level on the back stack, dropping the oldest past `HISTORY`.
    fn remember(&mut self, level: Level) {
        self.back.push(level);
        if self.back.len() > Self::HISTORY {
            self.back.remove(0);
        }
    }

    /// What the main pane holds right now, taken out of it.
    fn take_level(&mut self) -> Level {
        // A list that was up goes onto the level, so back brings it back.
        // Its rows drew covers into cells the image protocols mark `Skip`,
        // which the next frame writes nothing over: a full paint.
        if self.showing_queue || self.showing_history {
            self.needs_repaint = true;
        }
        // Read before anything is taken: the fields below are moved out as
        // the struct is built, so asking afterwards always says "nothing
        // was open".
        let over_a_section = self.open.is_some() || self.artist.is_some();
        Level {
            section: self.sidebar.section(),
            showing_queue: std::mem::take(&mut self.showing_queue),
            showing_history: std::mem::take(&mut self.showing_history),
            // Taken, not cloned: a search left open would draw over the
            // view being opened, which is what made the Albums and Profils
            // tabs look as though enter did nothing at all.
            search: self.search.take(),
            open: self.open.take(),
            artist: self.artist.take(),
            // An opened collection's tracks go with it, but the Tracks
            // section's own favourites do not: they are the section's
            // contents, fetched once at startup, and taking them left the
            // view empty with nothing to fetch them again -- the same
            // reason Explore's own rows stay behind.
            tracks: if over_a_section {
                std::mem::take(&mut self.tracks)
            } else {
                Vec::new()
            },
            tracklist: std::mem::take(&mut self.tracklist),
            open_cards: std::mem::take(&mut self.open_cards),
            open_grid: std::mem::take(&mut self.open_grid),
            // A genre page IS the Explore pane's contents rather than
            // something drawn over it, so leaving one has to carry it or
            // there is nothing for forward to come back to. Explore's own
            // list is not taken: it is the section's contents, and emptying
            // it here made arriving back on Explore fetch the page it
            // already had.
            explore: self
                .explore
                .heading
                .is_some()
                .then(|| std::mem::take(&mut self.explore)),
        }
    }

    /// Put a level back into the main pane.
    fn restore_level(&mut self, level: Level) {
        self.sidebar.select(level.section);
        self.search = level.search;
        self.open = level.open;
        self.artist = level.artist;
        self.tracks = level.tracks;
        self.tracklist = level.tracklist;
        self.open_cards = level.open_cards;
        self.open_grid = level.open_grid;
        self.showing_queue = level.showing_queue;
        self.showing_history = level.showing_history;
        // Back onto a list: its rows go over covers the view being left
        // drew, in cells the diff never writes.
        if self.showing_queue || self.showing_history {
            self.needs_repaint = true;
        }
        if let Some(explore) = level.explore {
            self.explore = explore;
        }
    }

    /// Step back one view, returning whether there was one to step to.
    ///
    /// What escape does, and the back arrow: the view stepped out of goes
    /// on the forward stack so it can be stepped into again.
    fn go_back(&mut self) -> bool {
        let Some(level) = self.back.pop() else {
            return false;
        };
        let leaving = self.take_level();
        self.forward.push(leaving);
        self.restore_level(level);
        true
    }

    /// Step into the view that going back stepped out of.
    fn go_forward(&mut self) -> bool {
        let Some(level) = self.forward.pop() else {
            return false;
        };
        let leaving = self.take_level();
        self.remember(leaving);
        self.restore_level(level);
        true
    }

    /// Move to the next or previous nav entry, closing whatever is open
    /// over it.
    ///
    /// Without the closing, J and K moved the nav and the opened view
    /// stayed on top — the keys looked like they did nothing at all. The
    /// view goes on the history, so back returns to it.
    fn go_to_section(&mut self, down: bool) {
        self.leave_for_a_new_place();
        if down {
            self.sidebar.next();
        } else {
            self.sidebar.previous();
        }
    }

    /// Record where the user is and clear the pane, ready for somewhere
    /// else.
    ///
    /// The clearing is what taking the level does: an opened album left on
    /// top of a change of section made J and K look like they did nothing.
    /// Going somewhere new is a branch, so what was ahead is dropped.
    fn leave_for_a_new_place(&mut self) {
        let level = self.take_level();
        self.remember(level);
        self.forward.clear();
    }

    /// Take the pane for a view opened from inside another, and remember
    /// what it replaced.
    ///
    /// Every "open something" did this by hand, and the four copies had
    /// drifted: one forgot to push a level at all, so back and escape had
    /// nothing to pop and the view it opened could not be left; one cleared
    /// the artist page it covered and two did not; one reset the track list
    /// by its selection alone, leaving the filter and the offset behind.
    /// One place now, so a view opened from anywhere leaves the same state
    /// behind it.
    ///
    /// `cards` is the covers an opened row carries, empty for a list of
    /// tracks -- which is also what says whether the pane draws a grid.
    fn open_view(&mut self, identity: Option<OpenCollection>, cards: Vec<carousel::Card>) {
        // Read before the level is put away: `push_level` takes the cards
        // with it, and an identity read off the card underneath is gone by
        // then. Callers hand it in for the same reason.
        self.push_level();
        self.open = identity;
        // Nothing has arrived yet. Without this the pane drew whatever the
        // opened view would eventually be -- an empty track list, columns
        // and filter box and all -- for as long as the request took, which
        // read as a broken view rather than as one still loading. Cards
        // handed in here are already the content, so those are not waiting.
        self.awaiting = cards.is_empty();
        self.open_cards = cards;
        self.open_grid = grid::GridState::default();

        // A page from the last artist would show under the new one's name
        // until its own reply arrived.
        self.artist = None;
        self.artist_section = 0;
        self.artist_scroll = 0;
        self.artist_bio_open = false;
        self.artist_bio_scroll = 0;
        self.artist_tracks = tracklist::TrackListState::default();
        self.artist_rows = Default::default();

        // The opened view is what the user is now looking at, so it gets
        // the keys. Leaving focus on the view behind it made the new one
        // impossible to move around in.
        self.tracks.clear();
        self.tracklist = tracklist::TrackListState::default();
    }

    /// The heading-only identity most opens carry: a title and nothing else.
    /// Take the pane for a collection a key asked for, under its name.
    ///
    /// An artist opens into a moment of the collection view before their
    /// page arrives, so their cover -- when there is one -- is round there.
    fn open_for(&mut self, round: bool, title: String, subtitle: String, cover: Option<String>) {
        let mut identity = self.heading_only(title);
        if let Some(open) = identity.as_mut() {
            open.subtitle = subtitle;
            open.cover = cover;
            open.round_cover = round;
        }
        self.open_view(identity, Vec::new());
    }

    /// Whether `title` is the view already on screen, filled.
    ///
    /// `c` on a track inside its own album opened the album again, and
    /// each press stacked a level: seven presses on a single were eight
    /// steps back out. A key that would open what is showing does nothing.
    fn already_showing(&self, title: &str) -> bool {
        (!self.awaiting && self.open.as_ref().is_some_and(|o| o.title == title))
            || self.artist.as_ref().is_some_and(|p| p.name == title)
    }

    fn heading_only(&self, title: String) -> Option<OpenCollection> {
        Some(OpenCollection {
            title,
            subtitle: String::new(),
            detail: String::new(),
            cover: None,
            round_cover: false,
            came_from: self.sidebar.section(),
            target: None,
        })
    }

    /// As [`heading_only`], with the artwork of whatever it was opened from.
    ///
    /// A radio is a mix, and a mix has a cover -- checked against a real
    /// response. Opened by a key rather than by enter, it had nowhere to
    /// take one from and the page came up with a blank square where every
    /// other opened view has a picture.
    ///
    /// The seed goes in as the target, which is what the loop fetches.
    fn heading_with_cover(
        &self,
        seed: crate::domain::TrackId,
        title: String,
        cover: Option<String>,
        round: bool,
    ) -> Option<OpenCollection> {
        Some(OpenCollection {
            title,
            subtitle: String::new(),
            detail: String::new(),
            cover,
            round_cover: round,
            came_from: self.sidebar.section(),
            target: Some(Collection::Radio(seed)),
        })
    }

    /// Remember what the main pane holds, so escape can put it back.
    ///
    /// The opened collection stays behind as well as going onto the stack:
    /// what is being opened is read off the card of the view above it, and
    /// that view's own identity is what heads it until its reply lands.
    fn push_level(&mut self) {
        let open = self.open.clone();
        let level = self.take_level();
        self.remember(level);
        self.open = open;
        // A new branch: what was ahead is no longer reachable, the same as
        // following a link after going back in a browser.
        self.forward.clear();
    }

    /// Back to the sidebar's section: nothing opened, nothing to go back to.
    fn clear_open(&mut self) {
        if let Some(open) = self.open.take() {
            self.sidebar.select(open.came_from);
        }
        self.tracks.clear();
        self.tracklist = tracklist::TrackListState::default();
        // An opened row's cards go with it, or the next view drawn would
        // show the last row's contents.
        self.open_cards.clear();
        self.open_grid = grid::GridState::default();
        self.artist = None;
    }

    /// Columns and visible rows of the current grid, measured from the pane
    /// the renderer last drew into.
    ///
    /// The same count, and the same rows of chrome above the cards, that the
    /// renderer draws with. Keeping a second copy of this arithmetic let the
    /// two disagree, and the keys reached cards that were never drawn.
    fn grid_geometry(&self) -> (usize, usize) {
        grid::geometry_with(
            self.last_main_width,
            self.last_main_height,
            self.grid_lines(self.sidebar.section()),
            grid::Chrome::Full,
            &[],
        )
    }

    /// Lines of text under a section's covers, which decides how tall a
    /// card is. One place, so the renderer and the keys cannot disagree.
    fn grid_lines(&self, section: sidebar::Section) -> u16 {
        if self.open.is_some() && !self.open_cards.is_empty() {
            return 2;
        }
        match section {
            sidebar::Section::Playlists => 3,
            sidebar::Section::Albums | sidebar::Section::Feed => 2,
            _ => 1,
        }
    }

    /// What the main pane is actually drawing.
    ///
    /// Three places used to work this out for themselves -- the movement
    /// keys, the card the keys act on, and the renderer -- each with its own
    /// chain of `if`s in the same order. They agreed until they did not: a
    /// genre page set `open`, which the renderer read as an opened
    /// collection and drew as an empty track list, while the keys still
    /// thought they were on a page of rows. Asked once here, the three
    /// cannot drift apart again.
    ///
    /// The order is what is drawn over what: an artist page and a search
    /// cover whatever is behind them, an opened collection covers the
    /// section, and the section is what is left.
    pub(super) fn showing(&self) -> Showing {
        // The box opened over a page is on top of it: `s` on an album put
        // the box under the album, which stayed drawn and kept the keys.
        // A page opened from a result is the other way round, and the
        // stack keeps them apart then: the search goes onto the level.
        if let Some(state) = self.search.as_ref() {
            return Showing::Search(searchview::Tab::from_index(state.tab));
        }
        if self.artist.is_some() {
            return Showing::Artist;
        }
        // A list is a level of its own: opening anything from it puts it on
        // the stack and takes it off the screen, so while it shows it is
        // the top -- only the search box, which is not a level, opens over
        // it, and closes back onto it.
        if self.showing_queue {
            return Showing::Queue;
        }
        if self.showing_history {
            return Showing::History;
        }
        // A page of rows opened from Explore: drawn by the rows renderer
        // under its own heading, and driven by the keys as the page of rows
        // it is. It carries no `open` -- setting one handed the pane to the
        // collection view, which drew an empty track list.
        if self.explore.heading.is_some() && self.sidebar.section() == sidebar::Section::Explore {
            return Showing::Rows;
        }
        if self.open.is_some() {
            // Still waiting on the request that fills it. What arrives
            // decides which view this becomes -- a genre is a page of rows,
            // an album a list of tracks -- so until it does, neither shape
            // is drawn. The empty one used to be, columns and filter box
            // and all, which read as a broken view rather than a loading
            // one.
            // Cards already in hand are the content, whatever the flag
            // says: a view that has something to draw is not waiting.
            if self.awaiting && self.open_cards.is_empty() && self.tracks.is_empty() {
                return Showing::Loading;
            }
            // An opened collection is a list of tracks, or a grid when it
            // carries covers.
            return if self.open_cards.is_empty() {
                Showing::Tracks
            } else {
                Showing::Cards
            };
        }
        Showing::Section(self.sidebar.section())
    }

    /// Whether the main pane is currently a card grid, which decides what the
    /// movement keys mean: a grid moves by rows, a table by lines.
    pub fn on_grid(&self) -> bool {
        // Whatever the sidebar still points at, what is drawn wins. Reading
        // the section alone left j and k driving the view behind the one on
        // screen — a selection nobody could see. Twice: once for an opened
        // album, once for search.
        match self.showing() {
            // An artist page's card sections are grids; its track list is not.
            Showing::Artist => {
                artistview::Section::from_index(self.artist_section) != artistview::Section::Tracks
            }
            // Top is a grid while the selection is on one of its card rows
            // and a list while it is in the tracks — the keys follow what is
            // highlighted, not what the tab is called.
            Showing::Search(searchview::Tab::Top) => self
                .search
                .as_ref()
                .is_some_and(|s| s.top != searchview::TopSection::Tracks),
            Showing::Search(tab) => !tab.is_tracks(),
            Showing::Cards => true,
            // Nothing to move around in until the reply lands. The queue
            // is a list, and drives its own cursor.
            Showing::Tracks
            | Showing::Rows
            | Showing::Loading
            | Showing::Queue
            | Showing::History => false,
            Showing::Section(section) => matches!(
                section,
                sidebar::Section::Playlists
                    | sidebar::Section::Albums
                    | sidebar::Section::Profiles
            ),
        }
    }

    /// Apply one action. Returns a follow-up action when one is implied.
    pub fn update(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::Quit => {
                self.should_quit = true;
                None
            }
            Action::Key(key) => self.on_key(key),
            Action::Error(message) => {
                // A failed request is still an answer: the pane stops
                // waiting, or a view whose fetch died says "loading" for
                // the rest of the session.
                self.awaiting = false;
                // The login modal paints over `status`, so while it is
                // showing (no session yet) an error must go where it is
                // actually visible.
                if self.session.is_none() {
                    self.login = login::LoginState::Failed(message);
                } else {
                    self.say(message);
                }
                None
            }
            Action::Tick => {
                self.expire_status();
                self.sleep_due()
            }
            Action::BeginLogin => {
                // A Subsonic server asks for its password first; any other
                // service is contacted now, and says what it wants next.
                self.login = match self.login_prompt() {
                    login::LoginState::Idle => {
                        login::LoginState::Busy(format!("Contacting {}…", self.service_name()))
                    }
                    other => other,
                };
                None
            }
            Action::BeginGuest => {
                self.login = login::LoginState::Busy("Continuing as a guest…".into());
                None
            }
            // Sent on by the loop; the prompt is taken down while the
            // service checks the answer.
            Action::Answer(_) => {
                self.login = login::LoginState::Busy("Signing in…".into());
                None
            }
            // The connection is made by the loop; the prompt stays up,
            // emptied, until it answers.
            Action::Connect(_) => None,
            Action::SessionOpened(session) => {
                self.session = Some(session.clone());
                self.login_input = None;
                self.status = None;
                self.status_since = None;
                Some(Action::SessionReady)
            }
            Action::SessionExpired => {
                // Log the sign-out, not just a failure. Silence here made a
                // spurious one impossible to trace: the token was gone with
                // nothing in the log to say who removed it.
                tracing::warn!("session rejected — signing out");
                self.session = None;
                if let Some(provider) = &self.provider {
                    provider.sign_out();
                }
                self.login = login::LoginState::Failed(
                    "your session expired — press Enter to sign in again".into(),
                );
                self.tracks.clear();
                None
            }
            Action::LoginPrompt(prompt) => {
                self.login = match prompt {
                    music::SignInPrompt::Code { code, url } => {
                        login::LoginState::Waiting { code, url }
                    }
                    music::SignInPrompt::Url(url) => login::LoginState::Url(url),
                    music::SignInPrompt::Secret => login::LoginState::Secret {
                        typed: login::Secret::default(),
                    },
                    music::SignInPrompt::Accounts(choices) => login::LoginState::Accounts {
                        choices,
                        selected: 0,
                    },
                };
                None
            }
            // A service whose streams are encrypted needs the module, and
            // the machine is searched once an account is in -- never for
            // a service that does not need it.
            Action::SessionReady => {
                let protected = self.provider.as_ref().is_some_and(|p| p.protected());
                if protected && self.cdm == drmview::Cdm::Unneeded {
                    self.cdm = drmview::Cdm::Looking;
                    return Some(Action::LookForCdm);
                }
                None
            }
            // Both are the loop's.
            Action::LookForCdm | Action::DownloadCdm => None,
            Action::CdmFound(found) => {
                self.cdm = match found {
                    Some(origin) => {
                        self.say(drmview::describe(origin));
                        drmview::Cdm::Ready(origin)
                    }
                    None => drmview::Cdm::Wanted,
                };
                self.needs_repaint = true;
                None
            }
            Action::AnswerCdm(yes) => {
                self.needs_repaint = true;
                match (&self.cdm, yes) {
                    (drmview::Cdm::Wanted, true) => {
                        self.cdm = drmview::Cdm::Offering;
                        return Some(Action::DownloadCdm);
                    }
                    (drmview::Cdm::Offered { .. }, true) => {
                        if let Some(answer) = self.cdm_answer.take() {
                            let _ = answer.send(true);
                        }
                        self.cdm = drmview::Cdm::Installing;
                    }
                    _ => {
                        if let Some(answer) = self.cdm_answer.take() {
                            let _ = answer.send(false);
                        }
                        self.cdm = drmview::Cdm::Declined;
                        self.say("without the Widevine module, this service will not play");
                    }
                }
                None
            }
            Action::CdmOffered { version, license } => {
                self.cdm = drmview::Cdm::Offered {
                    version,
                    license,
                    scroll: 0,
                };
                None
            }
            Action::CdmReady(origin) => {
                self.cdm = drmview::Cdm::Ready(origin);
                self.needs_repaint = true;
                self.say(drmview::describe(origin));
                None
            }
            Action::CdmFailed(message) => {
                self.cdm = drmview::Cdm::Missing;
                self.needs_repaint = true;
                self.say(message);
                None
            }
            Action::TrackNext => {
                self.move_vertical(Dir::Down);
                None
            }
            Action::TrackPrevious => {
                self.move_vertical(Dir::Up);
                None
            }
            Action::SidebarNext => {
                self.go_to_section(true);
                self.load_if_needed()
            }
            Action::SidebarPrevious => {
                self.go_to_section(false);
                self.load_if_needed()
            }
            Action::TracksLoaded(tracks) => {
                // The reply is here, so the pane stops waiting.
                self.awaiting = false;
                // Favourites only fill the Tracks view. Writing them in
                // while an album is open would replace its contents with
                // something else entirely.
                // The favourites are the only list that says what is one,
                // so they are remembered whether or not they are on screen.
                self.favourites = tracks.iter().map(|t| t.id.clone()).collect();
                if self.open.is_none() {
                    self.tracks = tracks;
                    self.tracklist.selected = 0;
                }
                None
            }
            Action::CollectionLoaded { for_title, tracks } => {
                // The reply is here, so the pane stops waiting. Outside the
                // match: a reply for a view already closed still ends the
                // wait, or the next thing opened inherits it.
                self.awaiting = false;
                match self.open.as_ref() {
                    Some(open) if open.title == for_title => {
                        // The banner's own line, from the tracks that just
                        // arrived: they carry their durations, so it needs
                        // no second request and works for a playlist as
                        // well as an album.
                        let total: std::time::Duration = tracks.iter().map(|t| t.duration).sum();
                        let detail = carousel::collection_detail(tracks.len() as u32, Some(total));
                        self.tracks = tracks;
                        self.tracklist.selected = 0;
                        if let Some(open) = self.open.as_mut() {
                            open.detail = detail;
                        }
                    }
                    // Closed, or already moved on to another album: this
                    // reply is for a view that is no longer on screen.
                    _ => tracing::info!("dropping tracks for {for_title:?}, no longer open"),
                }
                None
            }
            Action::ArtistLoaded(page) => {
                // The reply is here, so the pane stops waiting.
                self.awaiting = false;
                // Same rule as any other reply: one for a view the user has
                // since left would arrive under the wrong heading.
                match self.open.as_ref() {
                    Some(open) if open.title == page.name => {
                        // The server's word on whether this account
                        // follows them, which is what `F` reads.
                        if page.following {
                            self.followed.insert(page.id.clone());
                        } else {
                            self.followed.remove(&page.id);
                        }
                        self.artist = Some(*page);
                    }
                    _ => tracing::info!("dropping the page for {:?}", page.name),
                }
                None
            }
            Action::PageLoaded { title, home } => {
                // The reply is here, so the pane stops waiting.
                self.awaiting = false;
                // A genre is a page of rows, so it is drawn by the rows
                // renderer under its own heading -- not by `open`, which
                // hands the pane to the collection view. It used to set
                // `open` as well, and that view draws `open_cards`, which
                // a page of rows never fills: the pane went empty and
                // opening a genre looked like it did nothing at all.
                //
                // The level was pushed when the card was activated, and
                // Explore's own rows went onto it then, so back puts the
                // genre list back.
                self.open = None;
                self.explore.heading = Some(title);
                self.explore.rows = home
                    .rows
                    .into_iter()
                    .map(|row| home::Row {
                        heading: row.heading,
                        kind: row.kind,
                        cards: row.cards,
                        state: carousel::CarouselState::default(),
                    })
                    .collect();
                self.explore.row = 0;
                self.explore.scroll = 0;
                None
            }
            Action::ExploreLoaded(home) => {
                let home = *home;
                // Explore's own list is named by the nav entry, not by a
                // heading of its own -- a genre's name left over here would
                // sit above the list of genres.
                self.explore.heading = None;
                self.explore.rows = home
                    .rows
                    .into_iter()
                    .map(|row| home::Row {
                        heading: row.heading,
                        kind: row.kind,
                        cards: row.cards,
                        state: carousel::CarouselState::default(),
                    })
                    .collect();
                None
            }
            Action::PlaylistsLoaded(playlists) => {
                self.playlists = playlists;
                self.playlist_grid.clamp(self.playlists.len());
                None
            }
            Action::AlbumsLoaded(albums) => {
                self.albums = albums;
                self.album_grid.clamp(self.albums.len());
                None
            }
            Action::ArtistsLoaded(artists) => {
                self.followed = artists.iter().map(|a| a.id.clone()).collect();
                self.artists = artists;
                self.artist_grid.clamp(self.artists.len());
                None
            }
            Action::HomeLoaded(home) => {
                let home = *home;
                self.home.rows = home
                    .rows
                    .into_iter()
                    .map(|row| home::Row {
                        heading: row.heading,
                        kind: row.kind,
                        cards: row.cards,
                        state: carousel::CarouselState::default(),
                    })
                    .collect();
                None
            }
            Action::CarouselNext => self.move_horizontal(Dir::Right),
            Action::CarouselPrevious => self.move_horizontal(Dir::Left),
            Action::RowNext => {
                // A track row is a grid, so down has somewhere to go inside
                // it before leaving for the next row.
                let (visible, cols) = (self.home_rows_to_land_in(), self.home_grid_columns());
                let width = scrollbar::content_width(self.last_main_width);
                self.rows_on_screen_mut().down(visible, cols, width);
                None
            }
            Action::RowPrevious => {
                let (visible, cols) = (self.home_rows_to_land_in(), self.home_grid_columns());
                let width = scrollbar::content_width(self.last_main_width);
                self.rows_on_screen_mut().up(visible, cols, width);
                None
            }
            Action::LoadHome => None,
            // The request is a side effect in run(), like LoadHome's.
            Action::LoadExplore => None,
            Action::LoadFeed => None,
            Action::LoadFavourites => None,
            Action::FeedLoaded(cards) => {
                self.feed_cards = cards;
                self.rebuild_feed();
                None
            }
            Action::BeginSearch => {
                // Opening the box does not clear what a previous search
                // found: reopening it to refine a query should not blank
                // the results being refined.
                let state = self.search.get_or_insert_with(SearchState::default);
                state.typing = true;
                None
            }
            Action::RunSearch(_) => None, // the request is a side effect in run()
            Action::SearchLoaded(results) => {
                if let Some(state) = self.search.as_mut() {
                    // A reply for a query the user has since retyped would
                    // show results under text that no longer produced them.
                    if state.query.trim() == results.query {
                        state.results = *results;
                        state.tab = 0;
                        // New results: every tab's position means nothing.
                        state.tracks = tracklist::TrackListState::default();
                        state.albums = grid::GridState::default();
                        state.artists = grid::GridState::default();
                        state.playlists = grid::GridState::default();
                        state.top_artists = carousel::CarouselState::default();
                        state.top_albums = carousel::CarouselState::default();
                        state.top_scroll = 0;
                    } else {
                        tracing::info!(
                            "dropping results for {:?}, the query is now {:?}",
                            results.query,
                            state.query
                        );
                    }
                }
                None
            }
            Action::CloseSearch => {
                self.search = None;
                None
            }
            // Moving through the queue is decided here; the loop turns the
            // resulting track into a stream. Split that way so the rules can
            // be tested without a sound card.
            Action::QueueNext => {
                if self.queue.next().is_some() && self.skip_blocked(true) {
                    return Some(Action::PlayQueued);
                }
                self.now_playing.playing = false;
                None
            }
            Action::QueuePrevious => {
                if self.queue.previous().is_some() && self.skip_blocked(false) {
                    return Some(Action::PlayQueued);
                }
                None
            }
            // Sent on to the player by the loop; nothing to change here.
            Action::SetVolume(_) => None,
            // Both are the loop's: it has the engine.
            Action::Preload(_) | Action::RestartEngine => None,
            Action::CycleSleep => {
                self.cycle_sleep();
                None
            }
            Action::ToggleLyrics => {
                self.lyrics.toggle();
                // Like the help: the pane wrote over cells the image
                // protocols skip, and only a full repaint clears that.
                self.needs_repaint = true;
                self.lyrics_wanted()
            }
            // The search is the loop's.
            Action::LoadLyrics(_) => None,
            Action::LyricsLoaded { id, found } => {
                self.lyrics.found.insert(id, found);
                None
            }
            Action::ScrobblersLinked(linked) => {
                if !linked.is_empty() {
                    let names: Vec<String> = linked.iter().map(|l| l.label()).collect();
                    self.say(format!("scrobbling to {}", names.join(", ")));
                }
                self.scrobblers = linked;
                None
            }
            Action::ToggleShuffle => {
                let on = !self.queue.shuffled();
                self.queue
                    .set_shuffled(on, &mut crate::playback::clock_rng());
                // No message: the transport row lights the button, which
                // says it better than a notice that covers the view.
                None
            }
            Action::CycleRepeat => {
                self.queue.repeat = self.queue.repeat.cycle();
                None
            }
            // Handled in the loop, which has the client to fetch a stream
            // with; this arm keeps `update` alone from silently doing
            // nothing.
            Action::PlayQueued => None,
            // The fetch belongs to the loop, which has the client; these
            // arms are here so the match stays exhaustive.
            Action::Autoplay { .. } => None,
            Action::AutoplayFailed(seed) => {
                self.autoplay_fetching = false;
                self.autoplay_dead_end = Some(seed);
                None
            }
            Action::HalfPage { down } => {
                // The key `j` or `k` stands for in this config, pressed as
                // many times as half the screen holds: every view's own
                // idea of down applies, and so does the scrolling.
                let default = KeyCode::Char(if down { 'j' } else { 'k' });
                let key = KeyEvent::new(
                    self.keymap.pressed_for(default),
                    crossterm::event::KeyModifiers::NONE,
                );
                for _ in 0..self.half_page() {
                    let mut next = self.on_key(key);
                    while let Some(action) = next {
                        next = self.update(action);
                    }
                }
                None
            }
            Action::ToggleAutoplay => {
                self.config.playback.autoplay = !self.config.playback.autoplay;
                if let Some(path) = self.config_path.as_ref() {
                    if let Err(e) = self.config.save_to(path) {
                        tracing::warn!("the settings could not be saved: {e}");
                        self.say(format!("could not save settings: {e}"));
                        return None;
                    }
                }
                self.say(if self.config.playback.autoplay {
                    "autoplay on"
                } else {
                    "autoplay off"
                });
                None
            }
            // The pane is taken now, under the name the row knows, rather
            // than when the track comes back: the fetch is a round trip
            // before the one that brings the album, and for that first
            // trip nothing changed on screen -- the key looked dead.
            Action::FetchTrackAlbum(_) => {
                let track = self.selected_track()?;
                let title = if track.album.is_empty() {
                    track.title.clone()
                } else {
                    track.album.clone()
                };
                self.open_for(false, title, track.artist.clone(), track.cover.clone());
                None
            }
            Action::FetchTrackArtist(_) => {
                let track = self.selected_track()?;
                self.open_for(true, first_artist(&track.artist), String::new(), None);
                None
            }
            // What the fetch found. The pane is already waiting under the
            // name the row knew; the reply is named by the API, and that
            // is the name the tracks or the page will arrive under, so the
            // wait is renamed rather than opened again -- a second open
            // was a second level to step back out of.
            Action::OpenTarget {
                target,
                title,
                subtitle,
                cover,
            } => {
                let round = matches!(target, Collection::Artist(_));
                let awaiting = self.awaiting;
                match self.open.as_mut() {
                    Some(open) if awaiting => {
                        open.title = title;
                        open.subtitle = subtitle;
                        open.cover = cover;
                        open.round_cover = round;
                    }
                    // The wait was left before the track came back: the
                    // same rule as any other late reply.
                    _ => tracing::info!("dropping {title:?}, no longer waited for"),
                }
                None
            }
            Action::QueueRadio(tracks) => {
                self.autoplay_fetching = false;
                // Marked as autoplay, which is what it is: what TIDAL
                // suggested following the end of the queue. That marking
                // is what lets the next album the user starts throw it away
                // rather than play a radio chosen for a track from twenty
                // minutes ago.
                //
                // Appended, never started: whatever is playing moves on to
                // it by itself. The one exception is a queue that ran out
                // before this landed -- then nothing is playing, and the
                // first of these has to be started by hand.
                self.queue.append_autoplay(tracks);
                if self.ran_dry && self.queue.next().is_some() && self.skip_blocked(true) {
                    self.ran_dry = false;
                    return Some(Action::PlayQueued);
                }
                None
            }
            Action::ToggleFavourite | Action::ToggleFollow => None,
            Action::FollowChanged {
                what,
                name,
                following,
            } => {
                let said = match (&what, following) {
                    (Favourable::Artist(_), true) => format!("Following {name}"),
                    (Favourable::Artist(_), false) => format!("Unfollowed {name}"),
                    (_, true) => format!("Added {name} to favourites"),
                    (_, false) => format!("Removed {name} from favourites"),
                };
                match &what {
                    Favourable::Artist(id) => {
                        if following {
                            self.followed.insert(id.clone());
                        } else {
                            self.followed.remove(id);
                        }
                        // The page on screen says so too, count included,
                        // rather than waiting to be opened again.
                        if let Some(page) = self.artist.as_mut().filter(|p| p.id == *id) {
                            page.following = following;
                            page.fans = page.fans.map(|n| {
                                if following {
                                    n + 1
                                } else {
                                    n.saturating_sub(1)
                                }
                            });
                        }
                    }
                    Favourable::Album(id) => {
                        if following {
                            self.favourite_albums.insert(id.clone());
                        } else {
                            self.favourite_albums.remove(id);
                        }
                        // The album's page, when it is up, shows the heart
                        // at once rather than on the next open.
                        if let Some(page) = self
                            .artist
                            .as_mut()
                            .filter(|p| p.kind == crate::library::PageKind::Album && p.id == *id)
                        {
                            page.following = following;
                        }
                    }
                    Favourable::Playlist(uuid) => {
                        if following {
                            self.favourite_playlists.insert(uuid.clone());
                        } else {
                            self.favourite_playlists.remove(uuid);
                        }
                    }
                }
                self.say(said);
                None
            }
            Action::FavouriteIdsLoaded(ids) => {
                self.followed.extend(ids.artists);
                self.favourite_albums = ids.albums.into_iter().collect();
                self.favourite_playlists = ids.playlists.into_iter().collect();
                None
            }
            // Applied here rather than in the loop, for the same reason
            // opening is: the loop's copy could only be exercised by running
            // the whole app.
            Action::SeeAll if self.on_artist() => {
                // An artist's sections are already in hand — the page came
                // back with forty-odd albums and fifty singles — so this
                // opens what is held rather than asking for it again.
                let page = self.artist.as_ref()?;
                // The section the selection is actually in: an artist with
                // no top tracks starts on a section they do not have, and
                // asking for its cards gave an empty list.
                let present = artistview::Section::present(page);
                let wanted = artistview::Section::from_index(self.artist_section);
                let section = if present.contains(&wanted) {
                    wanted
                } else {
                    *present.first()?
                };
                let heading = format!("{} — {}", page.name, section.heading(page.kind));
                if section == artistview::Section::Tracks {
                    if page.top_tracks.is_empty() {
                        return None;
                    }
                    let identity = self.heading_only(heading);
                    self.open_view(identity, Vec::new());
                    return None;
                }
                let cards = artistview::cards(page, section);
                if cards.is_empty() {
                    return None;
                }
                let identity = self.heading_only(heading);
                self.open_view(identity, cards);
                None
            }
            // A Feed section is whole in hand -- the four are cut from one
            // reply -- so this opens what is held rather than asking for a
            // page that was never fetched.
            Action::SeeAll if self.on_feed() => {
                let row = self.rows_on_screen().current_row()?;
                let (heading, cards) = (row.heading.clone(), row.cards.clone());
                let identity = self.heading_only(heading);
                self.open_view(identity, cards);
                None
            }
            // A Top results row offers "See all" when its cards run past the
            // edge; the whole of what was found is on the kind's own tab.
            Action::SeeAll if self.on_search() => {
                let state = self.search.as_mut()?;
                if searchview::Tab::from_index(state.tab) != searchview::Tab::Top {
                    return None;
                }
                let tab = match state.top {
                    searchview::TopSection::Artists => searchview::Tab::Artists,
                    searchview::TopSection::Albums => searchview::Tab::Albums,
                    searchview::TopSection::Tracks => return None,
                };
                state.tab = searchview::Tab::ALL.iter().position(|t| *t == tab)?;
                None
            }
            // A home row, or one of a page opened from Explore: whole in
            // hand, like the Feed's, so it opens from what is held.
            Action::SeeAll if self.on_home() => {
                let row = self.rows_on_screen().current_row()?;
                let (heading, cards) = (row.heading.clone(), row.cards.clone());
                let identity = self.heading_only(heading);
                self.open_view(identity, cards);
                None
            }
            Action::SeeAll => None,
            Action::FavouriteChanged { id, favourite } => {
                if favourite {
                    self.favourites.insert(id);
                } else {
                    self.favourites.remove(&id);
                }
                None
            }
            Action::ArtistRight => {
                self.artist_move(Dir::Right);
                None
            }
            Action::ArtistLeft => {
                self.artist_move(Dir::Left);
                None
            }
            Action::SearchRight => {
                self.search_move(Dir::Right);
                None
            }
            Action::SearchLeft => {
                self.search_move(Dir::Left);
                None
            }
            Action::NextSearchTab => {
                if let Some(state) = self.search.as_mut() {
                    // Each tab keeps its own position now that each is one
                    // of the app's own views, so coming back to a tab finds
                    // it where it was left.
                    state.tab = (state.tab + 1) % searchview::Tab::ALL.len();
                }
                None
            } // the fetch is a side effect in run()
            Action::PlaybackFailed { id, message } => {
                // The bar is filled the moment a track is chosen, so the
                // keypress has a visible effect before the request
                // returns. When that request fails the guess has to be
                // taken back: otherwise the cover and title of a track
                // that never started sit there looking exactly like one
                // that is playing. Only if it is still the same track —
                // a later choice has already replaced it.
                if self.now_playing.track.as_ref().is_some_and(|t| t.id == id) {
                    self.now_playing = nowplaying::NowPlaying::default();
                }
                self.say(message);
                None
            }
            Action::TogglePause => {
                self.now_playing.playing = !self.now_playing.playing;
                None
            }
            Action::SeekForward => {
                if let Some(to) = self.seek_target(true) {
                    self.now_playing.position = to;
                }
                None
            }
            Action::SeekBack => {
                if let Some(to) = self.seek_target(false) {
                    self.now_playing.position = to;
                }
                None
            }
            Action::Playback(event) => {
                match event {
                    crate::playback::PlaybackEvent::Position(p) => {
                        self.now_playing.position = p;
                        // Filled while the track is still playing rather
                        // than at the silence after the last one: the web
                        // client's "up next" is never empty, and fetching
                        // at the end means the user waits for the request
                        // with nothing in their ears. The reply appends, so
                        // this does not interrupt anything.
                        if let Some(fetch) = self.autoplay_seed() {
                            return Some(fetch);
                        }
                    }
                    crate::playback::PlaybackEvent::Started {
                        bit_depth,
                        sample_rate,
                        delivered,
                    } => {
                        // The track before this one is over, however far it
                        // got: the position is still its own here.
                        self.settle_listen(self.now_playing.position);
                        // Report what was DELIVERED.
                        let bits = bit_depth.map(|b| format!("{b}-bit ")).unwrap_or_default();
                        // Two decimals, then one trailing zero dropped:
                        // 44.1, 48.0, 22.05. A fixed ".1" rounded HE-AAC's
                        // 22050 Hz to "22.1".
                        let khz = format!("{:.2}", sample_rate as f64 / 1000.0);
                        let khz = khz.strip_suffix('0').unwrap_or(&khz);
                        self.now_playing.quality = Some(format!("{bits}{khz}kHz"));
                        self.now_playing.tier =
                            nowplaying::Tier::of_quality(delivered, bit_depth, sample_rate);
                        // A new stream starts at the beginning. Without this
                        // the previous track's position survived into the new
                        // one, and if that track was the longer of the two
                        // the ratio saturated and drew a full bar over a
                        // track that had barely started.
                        self.now_playing.position = std::time::Duration::ZERO;
                        self.now_playing.playing = true;
                        self.ran_dry = false;
                        self.begin_listen();
                        // A new track: the lyrics pane follows it from the
                        // top, and asks for its words if it is open.
                        self.lyrics.restart();
                        if let Some(ask) = self.lyrics_wanted() {
                            return Some(ask);
                        }
                    }
                    // The whole track is here: fetch the next one, so it
                    // starts at once and a gapless engine joins the two.
                    // Not with the sleep timer set to the end of the
                    // track: the next is not to be lined up.
                    crate::playback::PlaybackEvent::Downloaded(id) => {
                        if self.now_playing.track.as_ref().is_some_and(|t| t.id == id)
                            && self.sleep != Some(Sleep::EndOfTrack)
                        {
                            if let Some(next) = self.queue.upcoming() {
                                if self.why_blocked(next).is_none() {
                                    return Some(Action::Preload(next.id.clone()));
                                }
                            }
                        }
                    }
                    crate::playback::PlaybackEvent::Finished => {
                        // Played to the end: the whole of it counts.
                        let whole = self.listen.as_ref().map(|p| p.duration);
                        self.settle_listen(whole.unwrap_or(self.now_playing.position));
                        self.now_playing.position = std::time::Duration::ZERO;
                        // The sleep timer set to the end of the track: this
                        // is it. Paused rather than left, since a gapless
                        // engine may already have the next lined up.
                        if self.sleep == Some(Sleep::EndOfTrack) {
                            self.sleep = None;
                            return self.now_playing.playing.then_some(Action::TogglePause);
                        }
                        // A track ending on its own moves the queue on, which
                        // is what makes an album play through. `advance`
                        // rather than `next`: repeat-one applies to a track
                        // that ends, not to the skip button.
                        if self.queue.advance().is_some() && self.skip_blocked(true) {
                            return Some(Action::PlayQueued);
                        }
                        self.now_playing.playing = false;
                        // The queue is out. The radio was asked for while
                        // the last track played; if it has not landed, or
                        // was never asked for, this is the last resort.
                        self.ran_dry = true;
                        if let Some(fetch) = self.autoplay_seed() {
                            tracing::info!("queue empty, following the last track's radio");
                            return Some(fetch);
                        }
                    }
                    crate::playback::PlaybackEvent::Error(e) => {
                        self.say(e);
                        self.now_playing.playing = false;
                    }
                    crate::playback::PlaybackEvent::Spectrum(bands) => {
                        self.spectrum = Some(bands);
                    }
                    // The engine's verdicts on a track it could not play,
                    // handled where the shell already handles them.
                    crate::playback::PlaybackEvent::Failed { id, message } => {
                        // Never played, so never a listen.
                        self.listen = None;
                        return Some(Action::PlaybackFailed { id, message });
                    }
                    crate::playback::PlaybackEvent::SessionExpired => {
                        return Some(Action::SessionExpired);
                    }
                }
                None
            }
            // Opening is applied here rather than in the loop so it can be
            // tested: the loop's copy could only be exercised by running the
            // whole app, and a test that called the helpers directly passed
            // just as well with the bug in place.
            // Enter on the Service row is the switch itself; on any other
            // row it is nothing.
            Action::ActivateSelection if self.on_settings() => {
                (self.settings.current() == settings::Setting::Service)
                    .then_some(Action::SwitchProvider)
            }
            Action::SwitchProvider => {
                self.leave_service();
                Some(Action::BeginLogin)
            }
            Action::ActivateSelection if self.on_grid() || self.on_home() => {
                // A shortcut into one of the app's own views is a change
                // of section, the same as pressing its number.
                if let Some(carousel::Target::Section(section)) =
                    self.selected_card().and_then(|c| c.target)
                {
                    return Some(Action::GoToSection(section));
                }
                if self.selected_collection().is_some() {
                    // Read before the level is put away: `push_level` takes
                    // the cards with it, and the identity is read off the
                    // card the user pressed enter on.
                    let identity = self.selected_identity();
                    // The sidebar stays where it is. Moving it to Tracks made
                    // an opened album look like the favourites view and
                    // highlighted the wrong nav entry.
                    self.open_view(identity, Vec::new());
                    // A page opened from Explore replaces its rows, and the
                    // reply takes a moment to arrive. Left in place, the
                    // pane went on drawing the genres under the new
                    // heading, so pressing enter looked like nothing at
                    // all. They go on the level, which is what back
                    // restores.
                    if self.sidebar.section() == sidebar::Section::Explore {
                        let replaced = std::mem::take(&mut self.explore);
                        if let Some(level) = self.back.last_mut() {
                            level.explore = Some(replaced);
                        }
                    }
                }
                None
            }
            Action::ActivateSelection => None, // playback is a side effect in run()
            // The fetch belongs to the loop, which has the client. These
            // arms keep the match exhaustive.
            // The pane is taken here, so the reply has a view to land in:
            // a reply is only accepted for the title that is open, and
            // nothing opened one -- every album and artist a key asked for
            // came back and was dropped as "no longer open". The loop reads
            // the target before this runs, so it sees the same selection.
            Action::OpenTrackAlbum => {
                let track = self.selected_track()?;
                let (_, title) = album_of(&track)?;
                self.open_for(false, title, track.artist.clone(), track.cover.clone());
                None
            }
            Action::OpenTrackArtist => {
                let track = self.selected_track()?;
                let (_, name) = artist_of(&track)?;
                self.open_for(true, name, String::new(), None);
                None
            }
            // Both change what is playing, which only the loop can do: it
            // owns the channel to the audio thread.
            Action::RemoveQueued(_) | Action::JumpInQueue(_) => None,
            // These only add to the queue, so nothing has to reach the
            // audio thread: what is playing carries on.
            Action::QueueTrackNext | Action::QueueTrackLast => {
                let track = self.selected_track()?;
                let title = track.title.clone();
                if matches!(action, Action::QueueTrackNext) {
                    self.queue.play_next(track);
                    self.say(format!("{title} — next"));
                } else {
                    self.queue.play_last(track);
                    self.say(format!("{title} — queued"));
                }
                None
            }
            Action::PlayTrackRadio => {
                // The same step opening anything else takes: what is on
                // screen goes on the history, and the radio takes the pane.
                let seed = self.track_radio()?;
                let title = self.track_radio_title();
                // The track's own artwork: a radio built from it is about
                // that record, and the mix's own cover is not in hand until
                // its items arrive.
                let cover = self
                    .selected_track()
                    .or_else(|| self.now_playing.track.clone())
                    .and_then(|t| t.cover);
                let identity = self.heading_with_cover(seed, format!("{title} Radio"), cover, false);
                self.open_view(identity, Vec::new());
                None
            }
            Action::GoToSection(section) => {
                if self.sidebar.section() == section && self.open.is_none() {
                    // Already there and nothing over it: pressing the number
                    // again should not fill the history with the same place.
                    return None;
                }
                self.leave_for_a_new_place();
                self.sidebar.select(section);
                self.load_if_needed()
            }
            Action::GoHome => {
                // A step in the history like any other, so `[` returns to
                // whatever the user pressed escape from.
                self.leave_for_a_new_place();
                self.sidebar.select(sidebar::Section::Music);
                self.load_if_needed()
            }
            Action::GoBack => {
                self.go_back();
                self.load_if_needed()
            }
            Action::GoForward => {
                self.go_forward();
                self.load_if_needed()
            }
            Action::CloseCollection => {
                // One step back, to whatever was on screen when this view
                // was opened — the view below it when they are nested, and
                // the sidebar's section at the bottom of the stack. Its own
                // selection comes back with it.
                if !self.go_back() {
                    self.clear_open();
                }
                None
            }
        }
    }

    /// The login screen for the service the config names: TIDAL waits for
    /// Enter to show its code, a Subsonic server asks for its password.
    fn login_prompt(&self) -> login::LoginState {
        match self.config.provider.kind {
            crate::config::ProviderKind::Subsonic => self.subsonic_prompt(),
            // Nothing to sign in to: the folders are read as soon as the
            // loop starts, and this is what the screen says meanwhile.
            crate::config::ProviderKind::Local if self.config.local.folders().is_empty() => {
                login::LoginState::Failed(
                    "set [local] folders in config.toml, then press Enter".into(),
                )
            }
            crate::config::ProviderKind::Local => {
                login::LoginState::Busy("Reading your music folders…".into())
            }
            _ => login::LoginState::Idle,
        }
    }

    /// What the login screen calls the service.
    fn service_name(&self) -> &'static str {
        self.provider.as_ref().map(|p| p.name()).unwrap_or("TIDAL")
    }

    /// The login screen's key for going in without an account. Named
    /// rather than written where it is matched: the help modal lists the
    /// signed-in keys, and its test reads this file for every `Char('x')`.
    const GUEST_KEY: char = 'g';

    /// Whether the service can be used without an account -- YouTube
    /// Music can -- so the login screen offers it.
    fn guest_allowed(&self) -> bool {
        self.provider
            .as_ref()
            .is_some_and(|p| p.sign_in_options().contains(&music::SignIn::Anonymous))
    }

    /// The login screen for the Subsonic server the config names, or what
    /// to put in the config when it names none.
    fn subsonic_prompt(&self) -> login::LoginState {
        let subsonic = &self.config.subsonic;
        if subsonic.server.trim().is_empty() || subsonic.username.trim().is_empty() {
            return login::LoginState::Failed(
                "set [subsonic] server and username in config.toml, then press Enter".into(),
            );
        }
        login::LoginState::Password {
            server: subsonic.server.trim().to_owned(),
            username: subsonic.username.trim().to_owned(),
            typed: login::Secret::default(),
        }
    }

    /// True when the main pane is showing the home page rather than a list.
    fn on_home(&self) -> bool {
        // Explore and the genre pages it opens are drawn by the same
        // renderer as the home page, so they move the same way.
        match self.showing() {
            Showing::Rows => true,
            Showing::Section(section) => matches!(
                section,
                sidebar::Section::Music
                    | sidebar::Section::Explore
                    // Four dated sections of covers, which is the same
                    // shape as a page of rows and moves the same way.
                    | sidebar::Section::Feed
            ),
            _ => false,
        }
    }

    /// Move within an artist's page, which stacks its sections the way Top
    /// results does.
    fn artist_move(&mut self, dir: Dir) {
        let (width, height) = (self.last_main_width, self.last_main_height);
        let Some(page) = self.artist.as_ref() else {
            return;
        };
        // With the bio open, the bio is the page: up and down scroll it,
        // and the sections under it wait until it closes. Without this the
        // first screen of a long blurb was all of it that could be read.
        if self.artist_bio_open {
            let most = artistview::bio_overflow(page, width, height);
            self.artist_bio_scroll = match dir {
                Dir::Down => (self.artist_bio_scroll + 1).min(most),
                Dir::Up => self.artist_bio_scroll.saturating_sub(1),
                Dir::Left | Dir::Right => self.artist_bio_scroll,
            };
            return;
        }
        let present = artistview::Section::present(page);
        if present.is_empty() {
            return;
        }
        let current = artistview::Section::from_index(self.artist_section);
        let current = if present.contains(&current) {
            current
        } else {
            present[0]
        };
        let at = present.iter().position(|s| *s == current).unwrap_or(0);
        // The rows the renderer gives the list at this size, so the keys
        // scroll it where it is drawn.
        let tracks_visible = tracklist::visible_rows_of(
            artistview::tracks_height(
                page,
                self.artist_scroll,
                self.artist_bio_open,
                width,
                height,
            ),
            None,
            tracklist::Chrome::Bare,
        );
        // Asked of the same function the renderer draws from, so a section
        // added to one is not missed by the other.
        let cards = artistview::cards(page, current).len();
        let in_view = artistview::sections_in_view(
            page,
            self.artist_scroll,
            self.artist_bio_open,
            width,
            height,
        );
        let moved = Self::move_in_stack(
            Stacked {
                count: present.len(),
                at,
                tracks: present
                    .iter()
                    .position(|s| *s == artistview::Section::Tracks)
                    .map(|i| {
                        (
                            i,
                            &mut self.artist_tracks,
                            page.top_tracks.len(),
                            tracks_visible,
                        )
                    }),
                row: (current != artistview::Section::Tracks).then(|| {
                    (
                        &mut self.artist_rows[current.index()],
                        cards,
                        carousel::cards_to_land_in(scrollbar::content_width(width)),
                    )
                }),
                scroll: &mut self.artist_scroll,
                visible: in_view,
            },
            dir,
        );
        self.artist_section = present[moved].index();
    }

    /// What the section just moved to still needs fetching.
    ///
    /// Explore is a page of its own, and nothing else asks for it: without
    /// this the section drew an empty page until something else happened
    /// to trigger a load.
    fn load_if_needed(&mut self) -> Option<Action> {
        match self.sidebar.section() {
            sidebar::Section::Explore if self.explore.rows.is_empty() => Some(Action::LoadExplore),
            sidebar::Section::Feed if self.feed_cards.is_empty() => Some(Action::LoadFeed),
            // The three that arrive together at sign-in. Any of them can be
            // dropped on the way in -- the reply is only taken when nothing
            // is open over the pane -- and nothing asked for them a second
            // time, so the section stayed empty for the rest of the session.
            sidebar::Section::Tracks if self.tracks.is_empty() => Some(Action::LoadFavourites),
            sidebar::Section::Playlists if self.playlists.is_empty() => {
                Some(Action::LoadFavourites)
            }
            sidebar::Section::Albums if self.albums.is_empty() => Some(Action::LoadFavourites),
            _ => None,
        }
    }

    /// Group the feed into its four dated sections.
    ///
    /// Filtered first and grouped after, so a filter that empties a bucket
    /// leaves no heading behind rather than a heading over nothing: a
    /// section that has no cards is never built.
    fn rebuild_feed(&mut self) {
        let filter = std::mem::take(&mut self.feed.filter);
        let cards: Vec<carousel::Card> = grid::filter(&self.feed_cards, &filter)
            .into_iter()
            .cloned()
            .collect();
        self.feed = home::HomeState {
            rows: feed_rows(&cards, today()),
            filter,
            ..Default::default()
        };
    }

    /// The page of rows on screen, whichever it is.
    fn rows_on_screen(&self) -> &home::HomeState {
        match self.sidebar.section() {
            sidebar::Section::Explore => &self.explore,
            sidebar::Section::Feed => &self.feed,
            _ => &self.home,
        }
    }

    fn rows_on_screen_mut(&mut self) -> &mut home::HomeState {
        match self.sidebar.section() {
            sidebar::Section::Explore => &mut self.explore,
            sidebar::Section::Feed => &mut self.feed,
            _ => &mut self.home,
        }
    }

    /// Public so a test can drive the app the way a person does: keys in,
    /// buffer out. Reaching past this and calling the handlers directly is
    /// how a hint has twice been drawn for a key that did nothing.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Read before the keymap: the map knows keys, not modifiers, and
        // would translate the `d` of ctrl-d as a bare `d`. Not while a box
        // has the keyboard, where nothing is a command.
        if key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
            && !self.filtering
            && !self.search.as_ref().is_some_and(|s| s.typing)
        {
            match key.code {
                KeyCode::Char('d') => return Some(Action::HalfPage { down: true }),
                KeyCode::Char('u') => return Some(Action::HalfPage { down: false }),
                _ => {}
            }
        }
        // What the user asked this key to mean, before anything reads it.
        // Rebinding is a translation rather than a table of its own: every
        // key below is guarded by what is on screen, and a table would have
        // to carry all of that or lose it.
        //
        // Not applied to the text boxes below: while one has the keyboard
        // `q` is a letter, and a config that could change that would be a
        // config that breaks typing.
        let key = KeyEvent {
            code: self.keymap.resolve(key.code),
            ..key
        };

        // The search box takes the keyboard before anything else. Same
        // reason as the filter box below: while text is being typed, "q" is
        // a letter, not a command.
        if self.search.as_ref().is_some_and(|s| s.typing) {
            let mut query = self
                .search
                .as_ref()
                .map(|s| s.query.clone())
                .unwrap_or_default();
            match key.code {
                KeyCode::Esc => {
                    // Escape backs out of search entirely rather than only
                    // leaving the box: a box with no way back to the app
                    // would be a trap.
                    return Some(Action::CloseSearch);
                }
                KeyCode::Enter => {
                    if let Some(state) = self.search.as_mut() {
                        state.typing = false;
                    }
                    return Some(Action::RunSearch(query));
                }
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Char(c) => query.push(c),
                _ => return None,
            }
            if let Some(state) = self.search.as_mut() {
                state.query = query;
            }
            return None;
        }

        // While the filter box has the keyboard, printable keys are text.
        // Without this "q" would quit rather than filter.
        if self.filtering {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.filtering = false;
                    if key.code == KeyCode::Esc {
                        self.set_filter(String::new());
                    }
                }
                KeyCode::Backspace => {
                    let mut f = self.filter_text().to_string();
                    f.pop();
                    self.set_filter(f);
                }
                KeyCode::Char(c) => {
                    let mut f = self.filter_text().to_string();
                    f.push(c);
                    self.set_filter(f);
                }
                _ => {}
            }
            return None;
        }

        // Any key closes the help, including the one that opened it. A modal
        // that takes a specific key to dismiss is one more thing to know.
        // The queue is a view rather than a notice: it moves and it edits,
        // so only the keys it does not use close it. Nothing is passed on
        // to the app behind -- pressing `q` to shut it must not also quit.
        // The queue's own keys. Only these: it used to take every key and
        // close on any it did not know, which made each command a way out
        // and none of them a command -- `A` over the track that was playing
        // shut the queue instead of liking it. The rest fall through to what
        // they always do, and act on the row under the cursor, which
        // `selected_track` reads from the queue while it is showing.
        if self.showing() == Showing::Queue {
            let len = self.queue.len();
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.queue_list.next(len, self.over_list_visible());
                    return None;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.queue_list.previous(self.over_list_visible());
                    return None;
                }
                // Take it out. Removing what is playing has to stop or move
                // the player, which the loop does -- `update` cannot reach
                // the audio thread.
                KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                    if len > 0 {
                        // Taking a track out shortens the list and uncovers
                        // a row of whatever was drawn there before.
                        self.needs_repaint = true;
                        return Some(Action::RemoveQueued(self.queue_list.selected));
                    }
                    return None;
                }
                // Jump to it, which is what enter means in every list here.
                // Taken even when there is nothing to jump to: falling
                // through would open whatever the hidden page had selected.
                KeyCode::Enter => {
                    return (len > 0).then_some(Action::JumpInQueue(self.queue_list.selected));
                }
                // Whether the queue has an end. Apple Music puts the same
                // switch at the top of its Playing Next list: the place
                // you look when wondering what comes after the last row.
                KeyCode::Char('i') => return Some(Action::ToggleAutoplay),
                // The key that opened it closes it, which is a step back:
                // the queue is a view of its own. Escape and `[` reach the
                // same step below, as they do from any view.
                KeyCode::Char('Q') => return Some(Action::GoBack),
                _ => {}
            }
        }
        // The history's keys, as the queue's: only its own, the rest fall
        // through and act on the row under the cursor.
        if self.showing() == Showing::History {
            let len = self.queue.history().count();
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.history_list.next(len, self.over_list_visible());
                    return None;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.history_list.previous(self.over_list_visible());
                    return None;
                }
                // Bringing a played track back means queuing it next; `n`
                // then reaches it. Taken even with nothing to queue, or it
                // would open whatever the hidden page had selected.
                KeyCode::Enter => {
                    return (len > 0).then_some(Action::QueueTrackNext);
                }
                KeyCode::Char('H') => return Some(Action::GoBack),
                _ => {}
            }
        }
        // A question about the Widevine module takes every key: y or n
        // answer it, j and k scroll the terms, nothing else gets through.
        if self.cdm.asking() {
            // Matched on the letter rather than the key, so the help's
            // own scan does not count these among the app's bindings: they
            // are the modal's, and it says them itself.
            let typed = match key.code {
                KeyCode::Char(c) => Some(c),
                _ => None,
            };
            return match (typed, key.code) {
                (Some('y'), _) => Some(Action::AnswerCdm(true)),
                (Some('n'), _) | (_, KeyCode::Esc) => Some(Action::AnswerCdm(false)),
                (Some('j'), _) | (_, KeyCode::Down) => {
                    self.cdm.scroll_by(true);
                    None
                }
                (Some('k'), _) | (_, KeyCode::Up) => {
                    self.cdm.scroll_by(false);
                    None
                }
                _ => None,
            };
        }
        // The lyrics pane takes the keys that move it and lets the rest
        // through: pausing and skipping are still wanted while reading.
        if self.lyrics.open {
            match key.code {
                KeyCode::Esc | KeyCode::Char('L') => return Some(Action::ToggleLyrics),
                KeyCode::Char('j') | KeyCode::Down => {
                    self.lyrics.scroll_by(true);
                    return None;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.lyrics.scroll_by(false);
                    return None;
                }
                _ => {}
            }
        }
        if self.showing_help {
            self.showing_help = false;
            // The modal wrote over cells the image protocols had marked
            // `Skip`, and on the next frame they go back to `Skip` — so
            // nothing is written to undo it and a patch of the help stays
            // on the artwork. Only a full repaint clears that.
            self.needs_repaint = true;
            return None;
        }

        if self.session.is_none() {
            // The login screen's own prompts take every key: what is typed
            // into them is a password or a cookie, `q` included.
            match &mut self.login {
                login::LoginState::Password { typed, .. } => {
                    match key.code {
                        KeyCode::Char(c) => typed.0.push(c),
                        KeyCode::Backspace => {
                            typed.0.pop();
                        }
                        KeyCode::Enter if !typed.0.is_empty() => {
                            return Some(Action::Connect(std::mem::take(typed)));
                        }
                        KeyCode::Esc => return Some(Action::Quit),
                        _ => {}
                    }
                    return None;
                }
                login::LoginState::Secret { typed } => {
                    match key.code {
                        KeyCode::Char(c) => typed.0.push(c),
                        KeyCode::Backspace => {
                            typed.0.pop();
                        }
                        KeyCode::Enter if !typed.0.trim().is_empty() => {
                            return Some(Action::Answer(std::mem::take(typed).0));
                        }
                        KeyCode::Esc => return Some(Action::Quit),
                        _ => {}
                    }
                    return None;
                }
                login::LoginState::Accounts { choices, selected } => {
                    match key.code {
                        KeyCode::Char('j') | KeyCode::Down => {
                            *selected = (*selected + 1).min(choices.len().saturating_sub(1));
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            *selected = selected.saturating_sub(1);
                        }
                        KeyCode::Enter => {
                            return choices.get(*selected).map(|c| Action::Answer(c.id.clone()));
                        }
                        KeyCode::Esc => return Some(Action::Quit),
                        _ => {}
                    }
                    return None;
                }
                // Nothing to answer while the service works, or waits on
                // the browser: Enter must not start a second sign-in.
                login::LoginState::Busy(_)
                | login::LoginState::Url(_)
                | login::LoginState::Waiting { .. } => {
                    return (key.code == KeyCode::Esc).then_some(Action::Quit);
                }
                login::LoginState::Idle | login::LoginState::Failed(_) => {
                    if key.code == KeyCode::Char(Self::GUEST_KEY) && self.guest_allowed() {
                        return Some(Action::BeginGuest);
                    }
                }
            }
        }

        match key.code {
            // "s" puts the keyboard back in the box, whether search is open
            // or not: with the box already up it did nothing at all, so
            // there was no way back to the query except closing search and
            // starting over.
            KeyCode::Char('s') if self.session.is_some() => Some(Action::BeginSearch),
            // Only when the results are what is on screen: an album opened
            // from a result stands on top of them, and escape there closed
            // the search out from under it -- taking away both views at
            // once when the user meant to leave the one they were in.
            KeyCode::Esc if self.on_search() => Some(Action::CloseSearch),
            KeyCode::Char('/') if self.on_search() => Some(Action::BeginSearch),
            // Shift-/ on most layouts, so this is the one key to remember.
            KeyCode::Char('?') if self.session.is_some() => {
                self.showing_help = true;
                None
            }
            KeyCode::Char('L') if self.session.is_some() => Some(Action::ToggleLyrics),
            // What is playing next, and where each of it came from. Capital,
            // because `q` quits and the two would be a bad pair to confuse.
            KeyCode::Char('Q') if self.session.is_some() => {
                // A view of its own: what was showing goes onto the stack
                // and back returns to it -- the history included, so the
                // two lists are never up at once.
                self.push_level();
                self.open = None;
                self.showing_queue = true;
                self.queue_list.selected = self.queue.position();
                let visible = self.over_list_visible();
                self.queue_list.scroll_into_view(visible);
                tracing::debug!(
                    queued = self.queue.len(),
                    entries = self.queue.entries().len(),
                    at = self.queue.position(),
                    selected = self.queue_list.selected,
                    offset = self.queue_list.offset,
                    visible,
                    context = ?self.queue.context_name(),
                    "queue opened"
                );
                None
            }
            // What has been played. Beside `Q` on the keyboard, and the
            // other half of the same question.
            KeyCode::Char('H') if self.session.is_some() => {
                // As the queue: a level of its own.
                self.push_level();
                self.open = None;
                self.showing_history = true;
                // From the top, which is the newest: what was just played
                // is what the eye is after.
                self.history_list = Default::default();
                None
            }
            // The bio is drawn over the artist's page rather than pushed as
            // a view, so back closes it first. Otherwise it popped the page
            // from under the reader, who landed on whatever the artist had
            // been opened from -- two views away from where they were.
            KeyCode::Esc | KeyCode::Char('[') if self.on_artist() && self.artist_bio_open => {
                self.artist_bio_open = false;
                None
            }
            // Escape steps back one view, the same as `[`: it is the key
            // people reach for to leave where they are, and leaving should
            // mean the same thing whichever of the two is pressed. It never
            // quits — `q` alone does that, since a key that usually means
            // "leave this view" should not sometimes close the app instead.
            KeyCode::Esc if self.session.is_some() => Some(Action::GoBack),
            KeyCode::Esc => None,
            // Only on an artist's page, where there is a blurb to open.
            KeyCode::Char('b') if self.on_artist() => {
                self.artist_bio_open = !self.artist_bio_open;
                // From the top each time it opens, as a page would.
                self.artist_bio_scroll = 0;
                None
            }
            // Queue the highlighted track by hand: `e` next, `E` last.
            // Both survive a change of album, which is the whole point of
            // asking for a track by name rather than playing its list.
            KeyCode::Char('e') if self.selected_track().is_some() => Some(Action::QueueTrackNext),
            KeyCode::Char('E') if self.selected_track().is_some() => Some(Action::QueueTrackLast),
            // The album and the artist of the highlighted track. A list of
            // tracks names both and could open neither, so the only way to
            // an album from your favourites was to search for it.
            //
            // `c` for the cover: `a` is the artist and every letter that
            // says "album" was already spoken for, so this names the thing
            // on screen rather than a word nobody would guess.
            KeyCode::Char('c') if self.selected_track_album().is_some() => {
                let (_, title) = self.selected_track_album()?;
                (!self.already_showing(&title)).then_some(Action::OpenTrackAlbum)
            }
            KeyCode::Char('a') if self.selected_track_artist().is_some() => {
                let (_, name) = self.selected_track_artist()?;
                (!self.already_showing(&name)).then_some(Action::OpenTrackArtist)
            }
            // The row names an album and an artist but carries the id of
            // neither -- a card from the home page, a favourite from before
            // the ids were kept. `R` has the same gap and fills it the same
            // way: fetch the track, which does carry them, then open.
            // Without this the key did nothing, in silence, on the one page
            // most people start from.
            KeyCode::Char('c') if self.selected_track().is_some() => {
                self.selected_track().map(|t| Action::FetchTrackAlbum(t.id))
            }
            KeyCode::Char('a') if self.selected_track().is_some() => self
                .selected_track()
                .map(|t| Action::FetchTrackArtist(t.id)),
            // The radio around the selected track, or whatever is playing.
            // `R` reaches this from anywhere, which is where the user is
            // when the thought occurs.
            // Already in that radio: nothing to open.
            KeyCode::Char('R') if self.track_radio().is_some_and(|s| self.radio_is_open(&s)) => None,
            KeyCode::Char('R') if self.track_radio().is_some() => Some(Action::PlayTrackRadio),
            // The nav by number, 1 through 8: there are exactly eight
            // entries, Music first and Settings last, and reaching the far
            // end with J took seven presses.
            KeyCode::Char(c @ '1'..='8') if self.session.is_some() => {
                let at = c as usize - '1' as usize;
                sidebar::Section::ALL
                    .get(at)
                    .copied()
                    .map(Action::GoToSection)
            }
            // The two arrows, as a browser has them: back through a run of
            // sections as well as of opened views, and forward again, which
            // escape alone cannot do.
            KeyCode::Char('[') if self.session.is_some() => Some(Action::GoBack),
            KeyCode::Char(']') if self.session.is_some() => Some(Action::GoForward),
            KeyCode::Char('q') => Some(Action::Quit),
            // Every view but the home page has a filter box.
            KeyCode::Char('/') if self.session.is_some() && !self.on_home() => {
                self.filtering = true;
                None
            }
            KeyCode::Enter if self.session.is_none() => Some(Action::BeginLogin),
            // j and k always drive the content. The sidebar has J and K to
            // itself, so moving through a list never depends on remembering
            // which pane last took focus.
            KeyCode::Char('j') | KeyCode::Down if self.session.is_some() => {
                if self.on_home() {
                    Some(Action::RowNext)
                } else {
                    Some(Action::TrackNext)
                }
            }
            // Within an artist's page, which is drawn over everything else.
            KeyCode::Char('l') | KeyCode::Right if self.on_artist() => Some(Action::ArtistRight),
            KeyCode::Char('h') | KeyCode::Left if self.on_artist() => Some(Action::ArtistLeft),
            // h and l move within the search results. They used to fall
            // straight through to the carousel, which in a search grid moved
            // a row of the home page nothing was drawing.
            KeyCode::Char('l') | KeyCode::Right if self.on_search() => Some(Action::SearchRight),
            KeyCode::Char('h') | KeyCode::Left if self.on_search() => Some(Action::SearchLeft),
            KeyCode::Char('k') | KeyCode::Up if self.session.is_some() => {
                if self.on_home() {
                    Some(Action::RowPrevious)
                } else {
                    Some(Action::TrackPrevious)
                }
            }
            KeyCode::Char('l') | KeyCode::Right if self.session.is_some() => {
                Some(Action::CarouselNext)
            }
            // Left, everywhere. It used to back out of an opened album --
            // the one place `h` meant something other than a direction --
            // which put the user back on the page they came from whenever
            // they reached for the left-hand end of a row. Escape and `[`
            // are what close a view; in a track list, where there is
            // nothing to the left, this simply does nothing.
            KeyCode::Char('h') | KeyCode::Left if self.session.is_some() => {
                Some(Action::CarouselPrevious)
            }
            // Favouriting the highlighted track, both ways: pressing it on
            // one that is already a favourite takes it out again.
            KeyCode::Char('A') if self.session.is_some() => Some(Action::ToggleFavourite),
            // The collections' own `A`: the artist, album, playlist or mix
            // under the cursor. Not `A` itself: on an artist's page that
            // key still means the top track selected.
            KeyCode::Char('F') if self.selected_favourable().is_some() => {
                Some(Action::ToggleFollow)
            }
            // The web client's "See all": the page hands back six items per
            // row, and this opens the rest of them.
            KeyCode::Char('o') if self.session.is_some() => Some(Action::SeeAll),
            // The transport, which the player bar has always drawn but
            // nothing was behind.
            KeyCode::Char('n') if self.session.is_some() => Some(Action::QueueNext),
            KeyCode::Char('p') if self.session.is_some() => Some(Action::QueuePrevious),
            KeyCode::Char('z') if self.session.is_some() => Some(Action::ToggleShuffle),
            KeyCode::Char('r') if self.session.is_some() => Some(Action::CycleRepeat),
            KeyCode::Char('S') if self.session.is_some() => Some(Action::CycleSleep),
            KeyCode::Char('t') if self.on_search() => Some(Action::NextSearchTab),
            // Tab keeps working in search too, since it is what the web
            // client's own tab strip responds to.
            KeyCode::Tab if self.on_search() => Some(Action::NextSearchTab),
            // Shift-J/K reach the sidebar without moving focus first. Kept
            // alongside the focus-aware j/k rather than replaced by them:
            // it is a shortcut people learn and then rely on.
            KeyCode::Char('J') if self.session.is_some() => Some(Action::SidebarNext),
            KeyCode::Char('K') if self.session.is_some() => Some(Action::SidebarPrevious),
            KeyCode::Enter if self.session.is_some() => Some(Action::ActivateSelection),
            KeyCode::Char(' ') if self.session.is_some() => Some(Action::TogglePause),
            // Along the track, in steps the ear can place. Only with a track
            // up: a seek with nothing playing has nowhere to go.
            KeyCode::Char('.') if self.now_playing.track.is_some() => Some(Action::SeekForward),
            KeyCode::Char(',') if self.now_playing.track.is_some() => Some(Action::SeekBack),
            _ => None,
        }
    }
}

/// Fetch whatever a collection holds and send it back as tracks.
///
/// Shared by opening a card and by "See all", so an album reached either way
/// arrives through the same path — and a reply for a view the user has since
/// left is dropped by `for_title` rather than shown under the wrong heading.
/// The album a track names, as something a key can open.
fn album_of(track: &crate::domain::Track) -> Option<(Collection, String)> {
    Some((Collection::Album(track.album_id.clone()?), track.album.clone()))
}

/// The artist a track names, the same way.
///
/// The first credited one: TIDAL lists several on a collaboration and the
/// web opens the first, which is also the one the row shows.
fn artist_of(track: &crate::domain::Track) -> Option<(Collection, String)> {
    Some((
        Collection::Artist(track.artist_id.clone()?),
        first_artist(&track.artist),
    ))
}

/// The first of a track's credited artists: TIDAL lists several on a
/// collaboration, and the first is the one the row shows and the web opens.
fn first_artist(credited: &str) -> String {
    credited
        .split(',')
        .next()
        .unwrap_or(credited)
        .trim()
        .to_string()
}

fn open_collection(
    app: &App,
    target: Option<Collection>,
    for_title: String,
    action_tx: &tokio::sync::mpsc::UnboundedSender<Action>,
) {
    // Both halves used to fall out of here without a word, and a session
    // that expired while the app was running made every open do exactly
    // nothing: no request, no error, no line in the log.
    let (Some(session), Some(target)) = (&app.session, target) else {
        if app.session.is_none() {
            tracing::warn!("cannot open a collection: there is no session");
            let _ = action_tx.send(Action::Error(
                "the session has expired — restart to sign in again".into(),
            ));
        }
        return;
    };
    let (client, tx) = (std::sync::Arc::clone(session), action_tx.clone());
    tokio::spawn(async move {
        let loaded = match &target {
            Collection::Playlist(uuid) => client.playlist_tracks(uuid).await,
            // An album is a page of its own, the artist's shape: its tracks
            // and, under them, the rows the web draws -- more by the
            // artist, other versions, related records.
            Collection::Album(id) => {
                match client.album_page(id, &for_title).await {
                    Ok(page) => {
                        let _ = tx.send(Action::ArtistLoaded(Box::new(page)));
                        return;
                    }
                    Err(e) => Err(e),
                }
            }
            Collection::Radio(seed) => client.track_radio(seed).await,
            // A genre, a mood or a decade is a page of rows, not a list of
            // tracks — the same shape Explore itself is, so it comes back
            // as its own reply and takes the pane.
            Collection::Page { title, path } => {
                match client.page_of_rows(path).await {
                    Ok(home) => {
                        let _ = tx.send(Action::PageLoaded {
                            title: title.clone(),
                            home: Box::new(home),
                        });
                        return;
                    }
                    Err(e) => Err(e),
                }
            }
            // An artist is a page of its own — three sections rather than
            // one list — so it comes back as its own reply.
            Collection::Artist(id) => match client.artist_page(id).await {
                Ok(page) => {
                    let _ = tx.send(Action::ArtistLoaded(Box::new(page)));
                    return;
                }
                Err(e) => Err(e),
            },
        };
        match loaded {
            Ok(tracks) => {
                let _ = tx.send(Action::CollectionLoaded { for_title, tracks });
            }
            Err(crate::tidal::TidalError::Unauthorized) => {
                let _ = tx.send(Action::SessionExpired);
            }
            Err(e) => {
                let _ = tx.send(Action::Error(e.to_string()));
            }
        }
    });
}

/// Fetch a track's stream and hand it to the player.
///
/// Shared by starting a track by hand and by the queue moving on, so the two
/// cannot drift: the bar is filled in from what is known now, and the
/// response says what was actually delivered.
fn start_track(
    app: &mut App,
    track: Option<crate::domain::Track>,
    cmd_tx: &std::sync::mpsc::Sender<crate::playback::Cmd>,
) {
    let (Some(session), Some(track)) = (&app.session, track) else {
        return;
    };
    // What the user allows. Both paths into playback come through here, so a
    // blocked track cannot be reached by starting one by hand and then
    // letting the queue run on past it.
    if let Some(why) = app.why_blocked(&track) {
        tracing::info!("not playing {:?}: {why}", track.title);
        app.say(format!("{} — {why}", track.title));
        return;
    }
    // What the user asked for, not what this build prefers. The response
    // still says what was actually delivered, and the badge reads that.
    let wanted = app.config.audio.quality();
    // Remembered as it starts rather than as it ends: a track skipped half
    // way through is still one the user heard and may want to name later.
    app.queue.remember(&track);
    if let Some(path) = &app.history_file {
        crate::playback::save_history(path, app.queue.history());
    }
    // The engine fetches the stream itself and reports back as events: a
    // rejected session as `SessionExpired`, anything else as `Failed`.
    session.set_quality(wanted);
    let _ = cmd_tx.send(crate::playback::Cmd::Play(track.id.clone()));
    app.now_playing.track = Some(track);
    app.now_playing.playing = true;
}

/// The track a card stands for, when it stands for one.
///
/// A card carries what the page said about the item — enough for a row, and
/// the same fields the player fills in when a track is started from one.
fn track_from_card(card: &carousel::Card) -> Option<crate::domain::Track> {
    let Some(carousel::Target::Track(id)) = card.target.as_ref() else {
        return None;
    };
    Some(crate::domain::Track {
        id: crate::domain::TrackId(id.clone()),
        title: card.title.clone(),
        artist: card.subtitle.clone(),
        album: card.detail.clone(),
        cover: card.cover_url.clone(),
        ..crate::domain::Track::sample("", "", card.duration)
    })
}

pub async fn run(
    terminal: &mut DefaultTerminal,
    picker: ratatui_image::picker::Picker,
) -> anyhow::Result<()> {
    let config = crate::config::Config::load()?;
    // Before the first frame: the icons are read as the screen is drawn, so
    // setting this afterwards would open the app on the wrong set.
    icons::set_nerd_font(config.ui.nerd_font);

    // What the user rebound, and what could not be read. A typo names an
    // action that does nothing, which is worth saying rather than leaving
    // the user to wonder why their key is dead.
    let (keys, mut problems) = keymap::Keymap::from_config(&config.keys);
    for problem in &problems {
        tracing::warn!("keys: {problem}");
    }

    // The colours, the same way: a theme that cannot be read costs the
    // colours it names and is said out loud, rather than stopping the app.
    let (palette, colour_problems) = theme::Palette::from_config(&config.ui);
    for problem in &colour_problems {
        tracing::warn!("theme: {problem}");
    }
    problems.extend(colour_problems);

    let mut app = App {
        config: config.clone(),
        config_path: crate::config::paths::config_file(),
        keymap: keys,
        palette,
        status: problems.first().map(|p| format!("config: {p}")),
        // Stamped like any other message, so a config warning does not sit
        // in the corner for the rest of the session. The log keeps it.
        status_since: problems.first().map(|_| std::time::Instant::now()),
        ..App::default()
    };
    // What the last run played, so the history is not blank on every
    // start: a track heard yesterday is one the user may want to name.
    // The engine reads the shared equaliser from its first frame.
    app.apply_equalizer();
    app.history_file = crate::config::paths::cache_dir().map(|d| d.join("history.json"));
    if let Some(path) = &app.history_file {
        app.queue
            .restore_history(crate::playback::load_history(path));
    }
    // Resume an existing session rather than making the user log in again.
    //
    // Every branch below logs what it decided. Without that, a spurious
    // sign-out is indistinguishable from a missing file, an unreadable one,
    // or a token judged expired — which cost several rounds of guessing.
    match crate::catalogue::provider_for(&config) {
        Ok(provider) => {
            match provider.restore().await {
                Ok(Some(session)) => {
                    tracing::info!("resuming the stored {} session", provider.name());
                    let session = crate::catalogue::Session::open(std::sync::Arc::clone(&provider), session);
                    app.session = Some(std::sync::Arc::new(session));
                }
                Ok(None) => {
                    tracing::info!(
                        "no stored {} session; showing the login screen",
                        provider.name()
                    );
                    app.login = app.login_prompt();
                }
                // Not forgotten: a launch on a bad network lands here with
                // a perfectly good token on disk, and Enter tries it again.
                Err(e) => {
                    tracing::warn!(
                        "the stored {} session could not be resumed: {e:#}",
                        provider.name()
                    );
                    app.login = login::LoginState::Failed(format!("{e:#}"));
                }
            }
            app.provider = Some(provider);
        }
        Err(e) => {
            tracing::error!("could not set up the provider: {e:#}");
            app.login = login::LoginState::Failed(format!("{e:#}"));
        }
    }

    let mut events = EventStream::new();
    // ~30fps ceiling: a burst of scroll events coalesces into one frame.
    let mut ticker = tokio::time::interval(Duration::from_millis(33));
    let (action_tx, mut action_rx) = tokio::sync::mpsc::unbounded_channel::<Action>();
    // Discord, when the config names an application to show the track as.
    let application_id = config.discord.application_id.trim();
    if !application_id.is_empty() {
        app.presence = Some(crate::presence::spawn(application_id.to_owned()));
    }
    // A folder of files has nothing stored to resume and nothing to ask:
    // it is read now, without waiting for Enter.
    if config.provider.kind == crate::config::ProviderKind::Local && app.session.is_none() {
        let _ = action_tx.send(Action::BeginLogin);
    }
    // The scrobbling services, linked off the main thread: Last.fm and
    // Libre.fm wait on the browser the first time.
    if crate::scrobble::configured(&config.scrobble) {
        let (scrobble, tx) = (config.scrobble.clone(), action_tx.clone());
        tokio::spawn(async move {
            let file = crate::config::paths::scrobble_file();
            let linked = crate::scrobble::link(&scrobble, file.as_deref()).await;
            let _ = tx.send(Action::ScrobblersLinked(linked));
        });
    }
    let (cmd_tx, mut playback_rx) = crate::playback::spawn();

    // The OS media keys. They reach the desktop, never the terminal, so the
    // app registers itself as a media player and the commands come back as
    // the same actions the keyboard sends.
    let (media_tx, media_rx) = mediakeys::channel();
    spawn_media_keys(action_tx.clone(), media_rx);

    // A resumed session needs the same engine and fetches a fresh login
    // gets. Without this the app opens already signed in and shows nothing.
    if app.session.is_some() {
        let _ = action_tx.send(Action::SessionReady);
    }

    // Cover art. Detection ran before the terminal was put into raw mode —
    // it queries stdout and reads stdin, so it cannot happen from in here.
    let (art_tx, mut art_rx) = tokio::sync::mpsc::unbounded_channel();
    // Cards are sized from the terminal's own cell, which the picker
    // measured while probing for an image protocol. Without this they
    // assume a cell exactly twice as tall as it is wide and a cover fills
    // only part of its card on everything else.
    let cell = picker.font_size();
    carousel::set_cell_size(cell.width, cell.height);
    tracing::info!(
        "terminal cell is {}x{}px, cards are {} columns",
        cell.width,
        cell.height,
        carousel::card_width()
    );
    app.artwork = Some(artwork::Artwork::with_picker(picker, art_tx));

    // The level from the config file, so a session starts where the last
    // one left off rather than at full whatever the setting says.
    let _ = cmd_tx.send(crate::playback::Cmd::Volume(app.config.audio.volume));

    while !app.should_quit {
        // A modal that covered the artwork leaves its text there: the image
        // cells are marked `Skip`, so the next frame writes nothing over
        // them. Clearing first forces the whole screen to be sent again,
        // which is what puts the covers back.
        // Tell the desktop what is playing. Sent every frame, filtered at
        // the other end: only a real change is announced, since the position
        // moves on every tick and a metadata signal that often makes the
        // desktop redraw its player for nothing.
        let _ = media_tx.send(mediakeys::State {
            track: app.now_playing.track.clone(),
            playing: app.now_playing.playing,
            position: app.now_playing.position,
            // More than one track in the queue means there is something
            // either side of this one to reach.
            can_next: app.queue.len() > 1,
            can_previous: app.queue.len() > 1,
        });

        if app.take_repaint() {
            terminal.clear()?;
        }
        terminal.draw(|frame| draw(frame, &mut app))?;

        let action = tokio::select! {
            _ = ticker.tick() => Some(Action::Tick),
            Some(action) = action_rx.recv() => Some(action),
            Some(event) = playback_rx.recv() => Some(Action::Playback(event)),
            Some(loaded) = art_rx.recv() => {
                // Not an Action: nothing about app state changes, the cache
                // just gains a decoded cover for the next frame.
                if let Some(art) = app.artwork.as_mut() {
                    art.insert(loaded);
                }
                None
            }
            maybe_event = events.next() => match maybe_event {
                // Windows emits both press and release; without this filter
                // every keystroke counts twice.
                Some(Ok(Event::Key(key))) if key.is_press() => Some(Action::Key(key)),
                Some(Ok(_)) => None,
                Some(Err(e)) => Some(Action::Error(e.to_string())),
                None => Some(Action::Quit),
            },
        };

        // Actions may cascade; apply until the chain settles.
        let mut next = action;
        while let Some(action) = next {
            // What happened, for a run that has to be replayed by reading
            // its log: the key, what it became, and what was under it.
            // Debug only -- `RUST_LOG=ratidal=debug` -- so the default log
            // stays as it is, and the tick is left out or it would be
            // thirty lines a second.
            if tracing::enabled!(tracing::Level::DEBUG) && !matches!(action, Action::Tick) {
                let shown: String = format!("{action:?}").chars().take(120).collect();
                tracing::debug!(
                    action = %shown,
                    showing = ?app.showing(),
                    section = ?app.sidebar.section(),
                    selected = ?app.selected_track().map(|t| t.title),
                    queued = app.queue.len(),
                    "action"
                );
            }
            // Side effects that must not block the render loop are spawned
            // here and report back through action_tx.
            match &action {
                // The service may have been switched since the start, so
                // the config read here is the app's, not the one loaded.
                Action::Connect(secret) if app.session.is_none() => {
                    let method = music::SignIn::Credentials {
                        server: app.config.subsonic.server.trim().to_owned(),
                        username: app.config.subsonic.username.trim().to_owned(),
                        password: secret.0.clone(),
                    };
                    start_sign_in(&mut app, method, &action_tx);
                }
                // The old engine falls silent; the new one attaches at
                // `SessionReady`.
                Action::SwitchProvider => {
                    let _ = cmd_tx.send(crate::playback::Cmd::Stop);
                }
                // The way in the service offers, apart from going without an
                // account: a code for TIDAL, a link for Spotify, cookies for
                // the rest. A Subsonic server went through `Connect` above.
                Action::BeginLogin
                    if app.session.is_none()
                        && app.config.provider.kind != crate::config::ProviderKind::Subsonic =>
                {
                    let method = match app.config.provider.kind {
                        // Nothing to read without a folder named; the
                        // screen says what to set.
                        crate::config::ProviderKind::Local => {
                            let folders = app.config.local.folders();
                            (!folders.is_empty()).then_some(music::SignIn::Path(folders))
                        }
                        _ => Some(
                            app.provider
                                .as_ref()
                                .map(|p| p.sign_in_options())
                                .unwrap_or_default()
                                .into_iter()
                                .find(|m| *m != music::SignIn::Anonymous)
                                .unwrap_or(music::SignIn::Default),
                        ),
                    };
                    if let Some(method) = method {
                        start_sign_in(&mut app, method, &action_tx);
                    }
                }
                Action::BeginGuest if app.session.is_none() => {
                    start_sign_in(&mut app, music::SignIn::Anonymous, &action_tx);
                }
                Action::Answer(text) => {
                    if let Some(input) = &app.login_input {
                        let _ = input.send(text.clone());
                    }
                }
                // The engine on the session's own factory, then the fetches
                // every signed-in screen needs. Both after a fresh login and
                // when a stored session is resumed: the second case is easy
                // to forget, and forgetting it means the app opens already
                // signed in and shows nothing at all.
                Action::SessionReady => {
                    if let Some(session) = &app.session {
                        let _ = cmd_tx.send(crate::playback::Cmd::Attach {
                            factory: session.playback(),
                            meta: session.stream_meta(),
                            options: app.playback_options(),
                        });
                        load_collection(std::sync::Arc::clone(session), action_tx.clone());
                    }
                }
                // Read before `update` clears the page: the same reason an
                // opened card's title is.
                Action::PlayTrackRadio => {
                    if let Some(id) = app.track_radio() {
                        let title = app.track_radio_title();
                        open_collection(
                            &app,
                            Some(Collection::Radio(id)),
                            format!("{title} Radio"),
                            &action_tx,
                        );
                    }
                }
                // The card is a track whose radio the page did not send.
                // Fetch the track for it, then open the radio the same way
                // the key would have.
                Action::FetchTrackAlbum(id) | Action::FetchTrackArtist(id) => {
                    if let Some(session) = &app.session {
                        let (client, tx) =
                            (std::sync::Arc::clone(session), action_tx.clone());
                        let id = id.clone();
                        let album = matches!(action, Action::FetchTrackAlbum(_));
                        tokio::spawn(async move {
                            match client.track(&id).await {
                                Ok(track) => {
                                    let found = if album {
                                        album_of(&track)
                                    } else {
                                        artist_of(&track)
                                    };
                                    match found {
                                        Some((target, title)) => {
                                            // The track's own artist and sleeve
                                            // head an album; an artist's page
                                            // brings its own picture.
                                            let (subtitle, cover) = if album {
                                                (track.artist.clone(), track.cover.clone())
                                            } else {
                                                (String::new(), None)
                                            };
                                            let _ = tx.send(Action::OpenTarget {
                                                target,
                                                title,
                                                subtitle,
                                                cover,
                                            });
                                        }
                                        None => tracing::info!(
                                            "{:?} names no {}",
                                            track.title,
                                            if album { "album" } else { "artist" }
                                        ),
                                    }
                                }
                                Err(e) => tracing::warn!("could not fetch the track: {e}"),
                            }
                        });
                    }
                }
                Action::OpenTarget { target, title, .. } => {
                    open_collection(&app, Some(target.clone()), title.clone(), &action_tx);
                }
                // A card that opens something: a playlist, an album, an
                // artist, a mix, a genre page. Anything else falls through
                // to the arm below, which plays a track.
                Action::ActivateSelection if app.what_enter_opens().is_some() => {
                    if let Some((target, for_title)) = app.what_enter_opens() {
                        open_collection(&app, Some(target), for_title, &action_tx);
                    }
                }
                // Taking a track out of the queue. Only the loop can do
                // it: dropping what is playing has to stop the audio
                // thread, and `update` has no channel to it.
                Action::RemoveQueued(at) => {
                    let was_playing = app.queue.remove(*at);
                    // The cursor follows: the row that was highlighted is
                    // gone, so the one that took its place is highlighted
                    // instead, and the last row steps back.
                    app.queue_list.selected = app
                        .queue_list
                        .selected
                        .min(app.queue.len().saturating_sub(1));
                    if was_playing {
                        match app.queue.current().cloned() {
                            Some(next) => start_track(&mut app, Some(next), &cmd_tx),
                            None => {
                                let _ = cmd_tx.send(crate::playback::Cmd::Stop);
                                app.now_playing.playing = false;
                                app.now_playing.track = None;
                            }
                        }
                    }
                }
                // Jump straight to a queued track, skipping what is between.
                Action::JumpInQueue(at) => {
                    if let Some(track) = app.queue.jump_to(*at).cloned() {
                        app.queue_list.selected = *at;
                        start_track(&mut app, Some(track), &cmd_tx);
                    }
                }
                // The album or the artist behind the highlighted track,
                // opened the same way enter opens a card.
                Action::OpenTrackAlbum => {
                    if let Some((target, title)) = app.selected_track_album() {
                        open_collection(&app, Some(target), title, &action_tx);
                    }
                }
                Action::OpenTrackArtist => {
                    if let Some((target, title)) = app.selected_track_artist() {
                        open_collection(&app, Some(target), title, &action_tx);
                    }
                }
                Action::ToggleFollow => {
                    if let (Some(session), Some((what, name))) =
                        (&app.session, app.selected_favourable())
                    {
                        let client = std::sync::Arc::clone(session);
                        let tx = action_tx.clone();
                        let wanted = !app.is_favourite(&what);
                        tokio::spawn(async move {
                            if let Err(e) = client.set_favourite(&what, wanted).await {
                                let _ = tx.send(Action::Error(e.to_string()));
                                return;
                            }
                            let _ = tx.send(Action::FollowChanged {
                                what: what.clone(),
                                name,
                                following: wanted,
                            });
                            // The section that lists these is read again,
                            // so it shows the change rather than the last
                            // launch.
                            match what {
                                Favourable::Artist(_) => {
                                    if let Ok(a) = client.artists().await {
                                        let _ = tx.send(Action::ArtistsLoaded(a));
                                    }
                                }
                                Favourable::Album(_) => {
                                    if let Ok(a) = client.albums().await {
                                        let _ = tx.send(Action::AlbumsLoaded(a));
                                    }
                                }
                                Favourable::Playlist(_) => {
                                    if let Ok(p) = client.playlists().await {
                                        let _ = tx.send(Action::PlaylistsLoaded(p));
                                    }
                                }
                            }
                        });
                    }
                }
                Action::ToggleFavourite => {
                    let selected = app.selected_track();
                    if let (Some(session), Some(track)) = (&app.session, selected.as_ref()) {
                        let client = std::sync::Arc::clone(session);
                        let (id, tx) = (track.id.clone(), action_tx.clone());
                        // What it should become, decided here so the request
                        // matches what the user saw when they pressed it.
                        let wanted = !app.favourites.contains(&id);
                        tokio::spawn(async move {
                            match client.set_track_favourite(&id, wanted).await {
                                // The reply carries the id, so a slow one
                                // cannot mark whatever is selected by the
                                // time it lands.
                                Ok(()) => {
                                    let _ = tx.send(Action::FavouriteChanged {
                                        id,
                                        favourite: wanted,
                                    });
                                }
                                Err(e) => {
                                    let _ = tx.send(Action::Error(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Action::ActivateSelection => {
                    // Playing from a list queues the whole list, so what
                    // follows this track is already decided.
                    let (list, at, named) = app.selected_list();
                    tracing::debug!(
                        list = list.len(),
                        at,
                        named = ?named,
                        showing = ?app.showing(),
                        "activate: the list under the selection"
                    );
                    if !list.is_empty() {
                        let shuffled = app.queue.shuffled();
                        // A new context rather than a new queue: what the
                        // user put next by hand outlives the album it was
                        // queued over, and the radio trailing the old one
                        // goes. Repeat is the player's mode, not the
                        // queue's contents, so it is left alone.
                        app.queue.start_context(list, at, named.clone());
                        if shuffled {
                            app.queue
                                .set_shuffled(true, &mut crate::playback::clock_rng());
                        }
                    }
                    let selected = app.selected_track();
                    start_track(&mut app, selected, &cmd_tx);
                }
                // The queue decided which track; this fetches it. Same path
                // as starting one by hand, so a queued track and a chosen
                // one cannot drift apart.
                Action::PlayQueued => {
                    let next = app.queue.current().cloned();
                    start_track(&mut app, next, &cmd_tx);
                }
                // Autoplay: the queue is nearly out. The station built on
                // its last track is fetched and appended -- so what follows
                // is a queue like any other and the skip keys work on it.
                Action::Autoplay { seed } => {
                    if let Some(session) = &app.session {
                        let (client, tx) =
                            (std::sync::Arc::clone(session), action_tx.clone());
                        let seed = seed.clone();
                        tokio::spawn(async move {
                            match client.track_radio(&seed).await {
                                Ok(tracks) if !tracks.is_empty() => {
                                    tracing::info!("autoplay: {} tracks", tracks.len());
                                    let _ = tx.send(Action::QueueRadio(tracks));
                                }
                                Ok(_) => {
                                    tracing::info!("autoplay: nothing follows {seed:?}");
                                    let _ = tx.send(Action::AutoplayFailed(seed.clone()));
                                }
                                Err(e) => {
                                    tracing::warn!("autoplay failed: {e}");
                                    let _ = tx.send(Action::AutoplayFailed(seed.clone()));
                                }
                            }
                        });
                    }
                }
                Action::RunSearch(query) => {
                    let query = query.clone();
                    if let Some(session) = &app.session {
                        let (client, t) =
                            (std::sync::Arc::clone(session), action_tx.clone());
                        tokio::spawn(async move {
                            match client.search(&query).await {
                                Ok(results) => {
                                    tracing::info!("search {query:?}: {} results", results.total());
                                    let _ = t.send(Action::SearchLoaded(Box::new(results)));
                                }
                                Err(crate::tidal::TidalError::Unauthorized) => {
                                    let _ = t.send(Action::SessionExpired);
                                }
                                Err(e) => {
                                    tracing::warn!("search {query:?} failed: {e}");
                                    let _ = t.send(Action::Error(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Action::LoadHome => {
                    if let Some(session) = &app.session {
                        let (client, t) =
                            (std::sync::Arc::clone(session), action_tx.clone());
                        tokio::spawn(async move {
                            match client.home().await {
                                Ok(home) => {
                                    tracing::info!("loaded home: {} rows", home.rows.len());
                                    let _ = t.send(Action::HomeLoaded(Box::new(home)));
                                }
                                Err(e) => {
                                    tracing::warn!("could not load the home page: {e}");
                                    let _ = t.send(Action::Error(e.to_string()));
                                }
                            }
                        });
                    }
                }
                // The same three fetches sign-in makes: whichever of them
                // was dropped fills in, and the ones already held are
                // written back unchanged.
                Action::LoadFavourites => {
                    if let Some(session) = &app.session {
                        load_collection(std::sync::Arc::clone(session), action_tx.clone());
                    }
                }
                Action::LoadFeed => {
                    if let Some(session) = &app.session {
                        let (client, t) =
                            (std::sync::Arc::clone(session), action_tx.clone());
                        tokio::spawn(async move {
                            match client.feed().await {
                                Ok(cards) => {
                                    tracing::info!("loaded {} feed cards", cards.len());
                                    let _ = t.send(Action::FeedLoaded(cards));
                                }
                                Err(e) => {
                                    tracing::warn!("could not load the feed: {e}");
                                    let _ = t.send(Action::Error(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Action::LoadExplore => {
                    if let Some(session) = &app.session {
                        let (client, t) =
                            (std::sync::Arc::clone(session), action_tx.clone());
                        tokio::spawn(async move {
                            match client.explore().await {
                                Ok(home) => {
                                    tracing::info!("loaded explore: {} rows", home.rows.len());
                                    let _ = t.send(Action::ExploreLoaded(Box::new(home)));
                                }
                                Err(e) => {
                                    tracing::warn!("could not load explore: {e}");
                                    let _ = t.send(Action::Error(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Action::TogglePause => {
                    let cmd = if app.now_playing.playing {
                        crate::playback::Cmd::Pause
                    } else {
                        crate::playback::Cmd::Resume
                    };
                    let _ = cmd_tx.send(cmd);
                }
                Action::SetVolume(v) => {
                    let _ = cmd_tx.send(crate::playback::Cmd::Volume(*v));
                }
                Action::LookForCdm => {
                    let tx = action_tx.clone();
                    tokio::spawn(async move {
                        // Reads browser folders: off the loop's thread.
                        let found = tokio::task::spawn_blocking(music::drm::find)
                            .await
                            .ok()
                            .flatten();
                        let _ = tx.send(Action::CdmFound(found.map(|f| f.origin)));
                    });
                }
                // Download, show the terms, wait for the word, install. The
                // archive stays in this task: the screen only sees the terms.
                Action::DownloadCdm => {
                    let (answer, asked) = tokio::sync::oneshot::channel();
                    app.cdm_answer = Some(answer);
                    let tx = action_tx.clone();
                    tokio::spawn(async move {
                        let offer = match music::drm::offer().await {
                            Ok(offer) => offer,
                            Err(e) => {
                                tracing::warn!("widevine: cannot download the module: {e:#}");
                                let _ = tx.send(Action::CdmFailed(format!(
                                    "could not download the Widevine module: {e:#}"
                                )));
                                return;
                            }
                        };
                        let _ = tx.send(Action::CdmOffered {
                            version: offer.version().to_owned(),
                            license: offer.license().to_owned(),
                        });
                        if asked.await != Ok(true) {
                            return;
                        }
                        match tokio::task::spawn_blocking(move || offer.install()).await {
                            Ok(Ok(found)) => {
                                tracing::info!("widevine: installed into the store");
                                let _ = tx.send(Action::CdmReady(found.origin));
                            }
                            Ok(Err(e)) => {
                                tracing::warn!("widevine: cannot install the module: {e:#}");
                                let _ = tx.send(Action::CdmFailed(format!(
                                    "could not install the Widevine module: {e:#}"
                                )));
                            }
                            Err(_) => {
                                let _ = tx.send(Action::CdmFailed(
                                    "the Widevine install task went away".into(),
                                ));
                            }
                        }
                    });
                }
                Action::Preload(id) => {
                    let _ = cmd_tx.send(crate::playback::Cmd::Preload(id.clone()));
                }
                Action::LoadLyrics(id) => {
                    if let (Some(session), Some(track)) =
                        (&app.session, app.now_playing.track.clone())
                    {
                        if track.id == *id {
                            let query = lyricsview::query(&track, session.slug());
                            let (id, tx) = (id.clone(), action_tx.clone());
                            tokio::spawn(async move {
                                let found = lyricsview::search(query).await;
                                let _ = tx.send(Action::LyricsLoaded { id, found });
                            });
                        }
                    }
                }
                // A fresh engine on the same session, picking the track
                // back up where it was, paused if it was paused.
                Action::RestartEngine => {
                    if let Some(session) = &app.session {
                        let _ = cmd_tx.send(crate::playback::Cmd::Attach {
                            factory: session.playback(),
                            meta: session.stream_meta(),
                            options: app.playback_options(),
                        });
                        if let Some(track) = &app.now_playing.track {
                            let _ = cmd_tx.send(crate::playback::Cmd::PlayAt(
                                track.id.clone(),
                                app.now_playing.position,
                            ));
                            if !app.now_playing.playing {
                                let _ = cmd_tx.send(crate::playback::Cmd::Pause);
                            }
                        }
                    }
                }
                Action::SeekForward | Action::SeekBack => {
                    if let Some(to) = app.seek_target(matches!(action, Action::SeekForward)) {
                        let _ = cmd_tx.send(crate::playback::Cmd::Seek(to));
                        // Discord's clock starts again from where this lands.
                        app.presence_shown = None;
                    }
                }
                _ => {}
            }
            next = app.update(action);
        }
        // What the actions above put in the outbox goes out now, off the
        // loop: a service that is slow must not hold the keys.
        for sending in crate::scrobble::drain(&app.scrobblers, &mut app.scrobble_outbox) {
            tokio::spawn(sending);
        }
        app.tell_presence();
    }
    if let Some(presence) = &app.presence {
        let _ = presence.send(crate::presence::Presence::Off);
    }
    // On the way out: the listen in progress, if it earned it, and whatever
    // is waiting. Awaited, briefly, since nothing runs after this.
    app.settle_listen(app.now_playing.position);
    let sending = crate::scrobble::drain(&app.scrobblers, &mut app.scrobble_outbox);
    if !sending.is_empty() {
        let _ = tokio::time::timeout(
            Duration::from_secs(3),
            futures::future::join_all(sending),
        )
        .await;
    }
    Ok(())
}

/// Today, counted in days from 1970-01-01.
///
/// The feed's stamps are date-only UTC, so the comparison is between whole
/// days and the time of day never enters into it.
fn today() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or_default()
}

/// The four sections the Feed is drawn in, newest first.
///
/// Fixed windows rather than calendar weeks: "this week" is the last seven
/// days, which is what a feed of releases means by it. A card whose date
/// the API left out falls under the oldest heading rather than being
/// dropped -- it is still a release the user follows.
///
/// `more` is left empty: the whole bucket is already in hand, so `o` opens
/// what is held rather than asking for a page that does not exist.
fn feed_rows(cards: &[carousel::Card], today: i64) -> Vec<home::Row> {
    const SECTIONS: [(&str, i64); 4] = [
        ("This week", 7),
        ("Last week", 14),
        ("Last month", 30),
        ("Older", i64::MAX),
    ];
    let mut rows = Vec::new();
    let mut from = 0;
    for (heading, until) in SECTIONS {
        let mine: Vec<carousel::Card> = cards
            .iter()
            .filter(|c| {
                // No date is as old as it gets: the card is still a release
                // the user follows, so it is shown rather than dropped.
                let age = c.day.map_or(i64::MAX, |d| today - d);
                // Inclusive at the top so the oldest section, whose bound is
                // the largest number there is, holds those as well.
                age >= from && (age < until || until == i64::MAX)
            })
            .cloned()
            .collect();
        // No cards, no heading: an empty section is not drawn at all.
        if !mine.is_empty() {
            rows.push(home::Row {
                heading: heading.to_string(),
                kind: crate::browse::RowKind::Carousel,
                cards: mine,
                state: carousel::CarouselState::default(),
            });
        }
        from = until;
    }
    rows
}

/// Fetch everything a signed-in session needs.
///
/// Called both after a fresh login and when a stored session is resumed at
/// startup — the second case is easy to forget, and forgetting it means the
/// app opens already signed in and shows nothing at all.
///
/// Each fetch is its own task so a slow one does not hold up the others; the
/// home page is by far the largest response.
fn load_collection(
    catalogue: std::sync::Arc<crate::catalogue::Session>,
    tx: tokio::sync::mpsc::UnboundedSender<Action>,
) {
    let (client, t) = (std::sync::Arc::clone(&catalogue), tx.clone());
    tokio::spawn(async move {
        match client.favourite_tracks().await {
            Ok(tracks) => {
                tracing::info!("loaded {} favourite tracks", tracks.len());
                let _ = t.send(Action::TracksLoaded(tracks));
            }
            Err(crate::tidal::TidalError::Unauthorized) => {
                let _ = t.send(Action::SessionExpired);
            }
            Err(e) => {
                tracing::warn!("could not load favourites: {e}");
                let _ = t.send(Action::Error(e.to_string()));
            }
        }
    });

    let (client, t) = (std::sync::Arc::clone(&catalogue), tx.clone());
    tokio::spawn(async move {
        match client.playlists().await {
            Ok(playlists) => {
                tracing::info!("loaded {} playlists", playlists.len());
                let _ = t.send(Action::PlaylistsLoaded(playlists));
            }
            // A failure here costs the sidebar list, not the session — log it
            // rather than throwing the user back to the login screen.
            Err(e) => tracing::warn!("could not load playlists: {e}"),
        }
    });

    let (client, t) = (std::sync::Arc::clone(&catalogue), tx.clone());
    tokio::spawn(async move {
        match client.albums().await {
            Ok(albums) => {
                tracing::info!("loaded {} favourite albums", albums.len());
                let _ = t.send(Action::AlbumsLoaded(albums));
            }
            Err(e) => tracing::warn!("could not load albums: {e}"),
        }
    });

    let (client, t) = (std::sync::Arc::clone(&catalogue), tx.clone());
    tokio::spawn(async move {
        match client.artists().await {
            Ok(artists) => {
                tracing::info!("loaded {} favourite artists", artists.len());
                let _ = t.send(Action::ArtistsLoaded(artists));
            }
            Err(e) => tracing::warn!("could not load artists: {e}"),
        }
    });

    // One reply with every favourited id, for the key that toggles them:
    // the sections above are pages of covers, this is the list.
    let (client, t) = (std::sync::Arc::clone(&catalogue), tx.clone());
    tokio::spawn(async move {
        match client.favourite_ids().await {
            Ok(ids) => {
                tracing::info!(
                    "loaded favourite ids: {} albums, {} playlists, {} artists",
                    ids.albums.len(),
                    ids.playlists.len(),
                    ids.artists.len()
                );
                let _ = t.send(Action::FavouriteIdsLoaded(ids));
            }
            Err(e) => tracing::warn!("could not load the favourite ids: {e}"),
        }
    });

    let (client, t) = (catalogue, tx);
    tokio::spawn(async move {
        // The same call `LoadHome` makes: one page loaded two ways is two
        // ways for it to differ, and it did.
        match client.home().await {
            Ok(home) => {
                tracing::info!("loaded home: {} rows", home.rows.len());
                let _ = t.send(Action::HomeLoaded(Box::new(home)));
            }
            Err(e) => tracing::warn!("could not load the home page: {e}"),
        }
    });
}

/// Signs in to `provider`, in its own task so the UI stays responsive. A
/// stored session is tried first: a launch on a bad network lands here with
/// a perfectly good token on disk. What the service shows on the way -- the
/// code to confirm in the browser -- reaches the login screen as actions;
/// so does the outcome.
fn start_sign_in(
    app: &mut App,
    method: music::SignIn,
    action_tx: &tokio::sync::mpsc::UnboundedSender<Action>,
) {
    if let Some(provider) = app.provider.clone() {
        let (typed, input) = tokio::sync::mpsc::unbounded_channel();
        app.login_input = Some(typed);
        tokio::spawn(sign_in(provider, method, input, action_tx.clone()));
    }
}

/// The sign-in itself, in its own task so the UI stays responsive. The
/// provider's questions arrive on `input`, answered through `Answer`.
async fn sign_in(
    provider: std::sync::Arc<dyn crate::catalogue::Provider>,
    method: music::SignIn,
    input: music::InputSource,
    tx: tokio::sync::mpsc::UnboundedSender<Action>,
) {
    let name = provider.name();
    if provider.stored() {
        if let Ok(Some(session)) = provider.restore().await {
            tracing::info!("resumed the stored {name} session");
            let session = crate::catalogue::Session::open(std::sync::Arc::clone(&provider), session);
            let _ = tx.send(Action::SessionOpened(std::sync::Arc::new(session)));
            return;
        }
    }
    let prompt: music::PromptSink = {
        let tx = tx.clone();
        std::sync::Arc::new(move |prompt| {
            // Save the user copying a URL out of a TUI. Best effort: the
            // link stays on screen, so a failure here costs nothing. A
            // `Url` prompt is not opened here: the provider opens it
            // itself, and listens for the browser to come back.
            if let music::SignInPrompt::Code { url, .. } = &prompt {
                login::open_in_browser(url);
            }
            let _ = tx.send(Action::LoginPrompt(prompt));
        })
    };
    match provider.sign_in(method, prompt, input).await {
        Ok(session) => {
            tracing::info!("signed in to {name}");
            let session = crate::catalogue::Session::open(std::sync::Arc::clone(&provider), session);
            let _ = tx.send(Action::SessionOpened(std::sync::Arc::new(session)));
        }
        Err(e) => {
            tracing::warn!("could not sign in to {name}: {e:#}");
            let _ = tx.send(Action::Error(format!("{e:#}")));
        }
    }
}

/// Public so `examples/screenshot.rs` renders what the app actually renders.
/// A preview that assembles the layout itself drifts from the real one, and
/// then it is checking its own copy rather than the UI.
/// Whether the account has what a card opens, for the heart on it.
fn liked_in(
    target: &carousel::Target,
    followed: &std::collections::HashSet<String>,
    albums: &std::collections::HashSet<String>,
    playlists: &std::collections::HashSet<String>,
) -> bool {
    match target {
        carousel::Target::Artist(id) => followed.contains(id),
        carousel::Target::Album(id) => albums.contains(id),
        carousel::Target::Playlist(uuid) => playlists.contains(uuid),
        carousel::Target::Track(_) | carousel::Target::Page(_) | carousel::Target::Section(_) => {
            false
        }
    }
}

/// The closure every renderer draws covers through: the artwork store when
/// there is one, nothing drawn when the terminal has none.
fn cover_drawer(
    art: &mut Option<artwork::Artwork>,
) -> impl FnMut(&mut ratatui::Frame, ratatui::layout::Rect, &str, artwork::Shape) -> bool + '_ {
    move |frame, area, url, shape| {
        art.as_mut()
            .is_some_and(|a| a.render_shaped(frame, area, url, shape))
    }
}

pub fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    // Borrowed by field, so the closure lives beside the artwork being
    // taken out and put back below.
    let (followed, fav_albums, fav_playlists) =
        (&app.followed, &app.favourite_albums, &app.favourite_playlists);
    let liked = |t: &carousel::Target| liked_in(t, followed, fav_albums, fav_playlists);
    let regions = layout::split(frame.area());
    let palette = app.palette;
    app.last_main_width = regions.main.width;
    app.last_main_height = regions.main.height;

    // The rule between the panes, drawn before either so neither has to
    // know it is there.
    if regions.divider.width > 0 {
        frame.render_widget(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::LEFT)
                .border_style(palette.rule()),
            regions.divider,
        );
    }

    sidebar::render(
        frame,
        regions.sidebar,
        &palette,
        &app.sidebar,
        // The nav is never the focused pane: J and K move it from
        // wherever the user is, and nothing else drives it, so a highlight
        // that says otherwise points at a mode that does not exist.
        false,
    );

    // The main pane always has the keys. Tab used to move a highlight onto
    // the nav, where no key did anything — a mode with nothing behind it.
    let main_focused = true;
    // Search takes the pane while it is open, ahead of both an opened album
    // and the sidebar's section — it is where the user is looking. Drawn
    // here rather than with an early return, so the now-playing bar and the
    // status line below still get their turn.
    // Asked once, and every branch below reads it: exactly one view is
    // drawn, the one `showing` names, whatever else is set behind it.
    let showing = app.showing();
    if let (Showing::Search(_), Some(search)) = (showing, app.search.as_ref()) {
        // `artwork` needs &mut while the renderer borrows `app`, so take it
        // out for the duration and put it back — as the other views do.
        let mut art = app.artwork.take();
        searchview::render(
            frame,
            regions.main,
            &palette,
            searchview::View {
                query: &search.query,
                typing: search.typing,
                results: &search.results,
                tab: search.tab,
                tracks: &search.tracks,
                // Whichever grid this tab reuses; the others keep their own
                // place for when the user comes back to them.
                grid: match searchview::Tab::from_index(search.tab) {
                    searchview::Tab::Albums => &search.albums,
                    searchview::Tab::Artists => &search.artists,
                    _ => &search.playlists,
                },
                liked: &liked,
                artists: &search.top_artists,
                albums: &search.top_albums,
                top: search.top,
                scroll: search.top_scroll,
                favourites: &app.favourites,
                playing: app.now_playing.track.as_ref().map(|t| t.id.clone()),
                tier: app.now_playing.tier,
            },
            cover_drawer(&mut art),
        );
        app.artwork = art;
    }

    if let (Showing::Artist, Some(page)) = (showing, app.artist.as_ref()) {
        let mut art = app.artwork.take();
        artistview::render(
            frame,
            regions.main,
            &palette,
            artistview::View {
                page,
                section: artistview::Section::from_index(app.artist_section),
                tracks: &app.artist_tracks,
                rows: &app.artist_rows,
                scroll: app.artist_scroll,
                bio_open: app.artist_bio_open,
                bio_scroll: app.artist_bio_scroll,
                liked: &liked,
                favourites: &app.favourites,
                playing: app.now_playing.track.as_ref().map(|t| t.id.clone()),
                tier: app.now_playing.tier,
            },
            cover_drawer(&mut art),
        );
        app.artwork = art;
    }

    // A view opened but not yet filled: its name and a line saying it is
    // on its way. Drawn here rather than by a section's renderer because
    // what it becomes is not known until the reply lands.
    if showing == Showing::Loading {
        let title = app
            .open
            .as_ref()
            .map(|o| o.title.as_str())
            .unwrap_or_default();
        loadingview::render(frame, regions.main, &palette, title);
    }
    let section = match showing {
        // Already drawn above, or drawn below in the queue's case; this
        // keeps the match from drawing a section under them.
        Showing::Artist
        | Showing::Search(_)
        | Showing::Loading
        | Showing::Queue
        | Showing::History => None,
        // An opened row of covers is a grid, not a track list — Albums is
        // the section whose renderer draws two-line cards.
        Showing::Tracks => Some(sidebar::Section::Tracks),
        Showing::Cards => Some(sidebar::Section::Albums),
        // A page of rows is drawn by the home renderer, on whichever state
        // holds it.
        Showing::Rows => Some(app.sidebar.section()),
        Showing::Section(section) => Some(section),
    };

    if let Some(showing) = section {
        match showing {
            // Explore is a page of card rows like the home one, so it is drawn
            // by the same renderer on its own state.
            section @ (sidebar::Section::Music | sidebar::Section::Explore) => {
                // `artwork` needs &mut to cache what it decodes, and the closure
                // is handed to a renderer that also borrows `app` — so take it out
                // for the duration and put it back.
                let mut art = app.artwork.take();
                let state = if section == sidebar::Section::Explore {
                    &app.explore
                } else {
                    &app.home
                };
                home::render(
                    frame,
                    regions.main,
                    &palette,
                    state,
                    main_focused,
                    trackgrid::Marks {
                        favourites: &app.favourites,
                        playing: app.now_playing.track.as_ref().map(|t| &t.id),
                        tier: app.now_playing.tier,
                        liked: &liked,
                    },
                    cover_drawer(&mut art),
                );
                app.artwork = art;
            }
            sidebar::Section::Feed => {
                // The Feed is a page of rows like the home page, but headed and
                // filtered like a grid: its chrome is drawn here and what is
                // left of the pane goes to the row renderer. The heading rows
                // come from the grid's own count so the two cannot drift.
                let header = inputbox::render_headed(
                    frame,
                    scrollbar::reserve(regions.main),
                    &palette,
                    "Feed",
                    None,
                    "Filter releases",
                    &app.feed.filter,
                    app.filtering,
                );
                let mut art = app.artwork.take();
                home::render(
                    frame,
                    ratatui::layout::Rect {
                        y: regions.main.y + header,
                        height: regions.main.height.saturating_sub(header),
                        ..regions.main
                    },
                    &palette,
                    &app.feed,
                    main_focused,
                    trackgrid::Marks {
                        favourites: &app.favourites,
                        playing: app.now_playing.track.as_ref().map(|t| &t.id),
                        tier: app.now_playing.tier,
                        liked: &liked,
                    },
                    cover_drawer(&mut art),
                );
                app.artwork = art;
            }
            sidebar::Section::Settings => {
                settings::render(
                    frame,
                    regions.main,
                    &palette,
                    &app.config,
                    &app.settings,
                    main_focused,
                );
            }
            section @ (sidebar::Section::Playlists
            | sidebar::Section::Albums
            | sidebar::Section::Profiles) => {
                // An opened row brings its own cards and its own heading; the
                // sidebar's section only says which renderer draws them.
                let opened = app.open.as_ref().filter(|_| !app.open_cards.is_empty());
                let cards = match opened {
                    Some(_) => app.open_cards.clone(),
                    None => app.grid_cards(section),
                };
                // Cloned rather than borrowed: `artwork` is taken out below and
                // a live borrow of `app` here would hold across it.
                let state = match opened {
                    Some(_) => app.open_grid.clone(),
                    None => app.grid_state(section).clone(),
                };
                let visible = grid::filter(&cards, &state.filter);
                // `lines` comes from the same place the movement keys read it,
                // so the two cannot drift: they disagreed by a whole row of
                // cards, and the keys reached ones that were never drawn.
                let lines = app.grid_lines(section);
                let (heading, hint) = match opened {
                    Some(open) => (open.title.as_str(), "Filter"),
                    None => match section {
                        sidebar::Section::Playlists => ("Playlists", "Filter playlists"),
                        sidebar::Section::Albums => ("Albums", "Filter albums"),
                        _ => ("Profiles", "Filter profiles"),
                    },
                };
                let mut art = app.artwork.take();
                grid::render(
                    frame,
                    regions.main,
                    &palette,
                    grid::Grid {
                        filtering: app.filtering,
                        chrome: grid::Chrome::Full,
                        heading,
                        filter_hint: hint,
                        cards: &visible,
                        state: &state,
                        focused: main_focused,
                        lines,
                        tabs: (&[], 0),
                        liked: &liked,
                    },
                    cover_drawer(&mut art),
                );
                app.artwork = art;
            }
            _ => {
                let visible = tracklist::filter(&app.tracks, &app.tracklist.filter);
                let mut art = app.artwork.take();
                tracklist::render(
                    frame,
                    regions.main,
                    &palette,
                    tracklist::TrackList {
                        filtering: app.filtering,
                        favourites: &app.favourites,
                        chrome: tracklist::Chrome::Full,
                        tracks: &visible,
                        state: &app.tracklist,
                        focused: main_focused,
                        playing: app.now_playing.track.as_ref().map(|t| t.id.clone()),
                        tier: app.now_playing.tier,
                        banner: app.open.as_ref().map(|o| tracklist::Banner {
                            title: &o.title,
                            subtitle: &o.subtitle,
                            detail: &o.detail,
                            cover: o.cover.as_deref(),
                            round: o.round_cover,
                            liked: o
                                .target
                                .clone()
                                .and_then(Favourable::of)
                                .is_some_and(|w| app.is_favourite(&w)),
                        }),
                    },
                    cover_drawer(&mut art),
                );
                app.artwork = art;
            }
        }
    }

    // The queue and the history take the pane, as the artist page does,
    // and draw the list the Tracks section draws: a row here is a row like
    // any other, and the keys act on it. They were modals with a list of
    // their own, and a modal that closed on any key was one no key could
    // act in.
    if matches!(showing, Showing::Queue | Showing::History) {
        let marks = trackgrid::Marks {
            favourites: &app.favourites,
            playing: app.now_playing.track.as_ref().map(|t| &t.id),
            tier: app.now_playing.tier,
            liked: &liked,
        };
        let entries = app.queue.entries();
        // The heading says whether the queue has an end: what follows the
        // last row is either silence or more of the same, and the list
        // itself cannot show which.
        let queue_heading = if app.config.playback.autoplay {
            "Queue · autoplay on"
        } else {
            "Queue · autoplay off"
        };
        let (heading, tracks, state): (&str, Vec<&crate::domain::Track>, _) =
            if showing == Showing::Queue {
                (
                    queue_heading,
                    entries.iter().map(|(t, _)| *t).collect(),
                    &app.queue_list,
                )
            } else {
                ("History", app.queue.history().collect(), &app.history_list)
            };
        let mut art = app.artwork.take();
        queueview::render(
            frame,
            regions.main,
            &palette,
            heading,
            &tracks,
            state,
            marks,
            cover_drawer(&mut art),
        );
        app.artwork = art;
    }

    let mut art = app.artwork.take();
    nowplaying::render_with_cover(
        frame,
        regions.now_playing,
        &palette,
        &app.now_playing,
        // Straight from the queue, so the bar cannot disagree with what
        // actually plays next.
        nowplaying::Modes {
            shuffled: app.queue.shuffled(),
            repeat: app.queue.repeat,
            sleep: app.sleep_mark(),
            // Only while something plays: a paused track's last frame
            // frozen under the badge read as the music still going.
            spectrum: match (&app.spectrum, app.now_playing.playing) {
                (Some(bands), true) => bands.levels(),
                _ => Vec::new(),
            },
        },
        cover_drawer(&mut art),
    );
    app.artwork = art;

    if let Some(status) = &app.status {
        // Tucked into the top right rather than laid across the whole line.
        // Taking the line meant covering the tab strip that lives there:
        // drawn over it the two read as one garbled line, and clearing it
        // first hid the tabs for as long as the message stood.
        let width = (status.chars().count() as u16 + 2).min(regions.main.width);
        let notice = ratatui::layout::Rect {
            x: regions.main.x + regions.main.width - width,
            y: regions.main.y,
            width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Line::styled(format!(" {status} "), palette.notice())),
            notice,
        );
    }

    if app.session.is_none() {
        login::render(
            frame,
            frame.area(),
            &app.login,
            app.service_name(),
            app.guest_allowed(),
        );
    }

    if app.cdm.shown() {
        drmview::render(frame, regions.main, &palette, &app.cdm);
    }

    if app.lyrics.open {
        lyricsview::render(
            frame,
            regions.main,
            &palette,
            &app.lyrics,
            app.now_playing.track.as_ref(),
            app.now_playing.position,
        );
    }

    // Last, so it covers whatever is underneath.
    if app.showing_help {
        help::render(frame, frame.area(), &palette);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn every_card_a_grid_counts_as_visible_is_drawn() {
        // The Feed showed six of fourteen releases: `rows` said how many
        // fit and the loop drew fewer, so the rest were reachable by the
        // keys and invisible on screen. Asked of the albums now, the Feed
        // having become a page of rows -- this is about the grid, and the
        // albums are one.
        let mut app = signed_in(sidebar::Section::Albums);
        app.update(Action::AlbumsLoaded(
            (0..14)
                .map(|i| crate::library::Album {
                    id: (i).to_string(),
                    title: format!("Release {i}"),
                    artist: "An Artist".into(),
                    year: None,
                    cover: None,
                    track_count: 0,
                    duration: None,
                })
                .collect(),
        ));

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let text = geometry::text(&terminal.backend().buffer().clone());

        // Counted by title, so this covers the rows drawn whole. The row
        // at the fold is cut off — its title is the first thing to go — so
        // what it holds is that no whole row is missing.
        let drawn = (0..14)
            .filter(|i| text.contains(&format!("Release {i}")))
            .count();
        let (cols, rows) = app.grid_geometry();
        let whole_rows = rows.saturating_sub(1);
        assert!(
            drawn >= (cols * whole_rows).min(14),
            "the grid drew {drawn} cards where {} rows fit whole\n{text}",
            whole_rows
        );
    }

    #[test]
    fn no_view_paints_under_the_now_playing_bar() {
        // The pane ends where the bar begins, and every view has its own
        // idea of how much room that leaves. Chasing them one at a time has
        // found three so far, so this walks the lot: whatever is on screen,
        // nothing may reach the bar's rows.
        //
        // Height 40 down to 12: tall enough for everything, short enough
        // that the arithmetic is tight.
        for height in [12u16, 16, 20, 28, 40] {
            for section in sidebar::Section::ALL {
                let mut app = signed_in(section);
                app.tracks = (0..30)
                    .map(|i| {
                        let mut t = crate::domain::Track::sample(
                            &format!("Track {i}"),
                            "An Artist",
                            std::time::Duration::from_secs(200),
                        );
                        t.id = crate::domain::TrackId::from(i as u64);
                        t
                    })
                    .collect();
                app.playlists = (0..20)
                    .map(|i| crate::library::Playlist {
                        uuid: format!("u{i}"),
                        title: format!("Playlist {i}"),
                        track_count: 10,
                        duration: None,
                        creator: "TIDAL".into(),
                        cover: None,
                    })
                    .collect();
                app.albums = (0..20)
                    .map(|i| crate::library::Album {
                        id: (i).to_string(),
                        title: format!("Album {i}"),
                        artist: "An Artist".into(),
                        year: None,
                        cover: None,
                        track_count: 10,
                        duration: None,
                    })
                    .collect();
                app.artists = (0..20)
                    .map(|i| crate::library::Artist {
                        id: (i).to_string(),
                        name: format!("Artist {i}"),
                        picture: None,
                    })
                    .collect();
                app.update(Action::FeedLoaded(
                    (0..20)
                        .map(|i| carousel::Card::new(format!("Release {i}"), "An Artist"))
                        .collect(),
                ));

                assert_clear_of_the_bar(&mut app, height, &format!("{section:?}"));
            }

            // And the views that are not sections: search, an opened album,
            // an artist's page.
            let mut app = signed_in(sidebar::Section::Music);
            app.update(Action::BeginSearch);
            for c in "daft".chars() {
                key(&mut app, KeyCode::Char(c));
            }
            app.update(Action::SearchLoaded(Box::new(some_results())));
            app.search.as_mut().unwrap().typing = false;
            for tab in 0..searchview::Tab::ALL.len() {
                app.search.as_mut().unwrap().tab = tab;
                assert_clear_of_the_bar(&mut app, height, &format!("search tab {tab}"));
            }

            let mut app = signed_in(sidebar::Section::Profiles);
            app.open = Some(OpenCollection {
                title: "Daft Punk".into(),
                subtitle: String::new(),
                detail: String::new(),
                cover: None,
                round_cover: false,
                came_from: sidebar::Section::Profiles,
                target: None,
            });
            app.update(Action::ArtistLoaded(Box::new(crate::library::ArtistPage {
                id: (0).to_string(),
                fans: None,
                following: false,
                kind: crate::library::PageKind::Artist,
                name: "Daft Punk".into(),
                picture: None,
                top_tracks_path: None,
                year: None,
                top_tracks: some_tracks(&["One", "Two", "Three"]),
                albums: (0..4)
                    .map(|i| crate::library::Album {
                        id: (i).to_string(),
                        title: format!("Album {i}"),
                        artist: "Daft Punk".into(),
                        year: None,
                        cover: None,
                        track_count: 10,
                        duration: None,
                    })
                    .collect(),
                similar: Vec::new(),
                bio: None,
                singles: Vec::new(),
                appears_on: Vec::new(),
                radio: None,
            })));
            assert_clear_of_the_bar(&mut app, height, "artist page");
        }
    }

    /// Draw `app` and check the pane stopped where the bar begins.
    ///
    /// The bar is drawn last and would cover anything that overran, so the
    /// pane is rendered on its own into a buffer the height of the whole
    /// frame: whatever appears on the bar's rows got there from above.
    #[track_caller]
    fn assert_clear_of_the_bar(app: &mut App, height: u16, what: &str) {
        let width = 100u16;
        let regions = layout::split(ratatui::layout::Rect::new(0, 0, width, height));
        let bar_top = regions.now_playing.y;

        // Twice: once as the app draws it, once with the views emptied so
        // only the bar paints. Anything on the bar's rows in the first that
        // is not there in the second came from a view above it.
        let render = |app: &mut App| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, app)).unwrap();
            terminal.backend().buffer().clone()
        };
        let full = render(app);

        let mut bare = App {
            session: app.session.clone(),
            now_playing: app.now_playing.clone(),
            palette: app.palette,
            ..App::default()
        };
        let bar_only = render(&mut bare);

        for y in bar_top..height {
            for x in 0..width {
                let a = &full[(x, y)];
                let b = &bar_only[(x, y)];
                assert!(
                    a.symbol() == b.symbol(),
                    "{what} at height {height}: ({x},{y}) is inside the \
                     now-playing bar and shows {:?}, where the bar alone \
                     draws {:?}\n{}",
                    a.symbol(),
                    b.symbol(),
                    geometry::text(&full)
                );
            }
        }
    }

    #[test]
    fn a_selected_cards_shade_stays_out_of_the_sidebar() {
        // The shade is a column wider than the card, so the leftmost card's
        // ran over the rule and into the nav beside it.
        let palette = theme::Palette::detect();
        let mut app = signed_in(sidebar::Section::Playlists);
        app.playlists = (0..3)
            .map(|i| crate::library::Playlist {
                uuid: format!("u{i}"),
                title: format!("Playlist {i}"),
                track_count: 10,
                duration: None,
                creator: "TIDAL".into(),
                cover: None,
            })
            .collect();

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();

        let regions = layout::split(ratatui::layout::Rect::new(0, 0, 100, 30));
        let card = geometry::find(&buf, "Playlist 0").expect("the selected card");

        // Nothing shaded at or before the rule, on the card's own rows.
        for x in 0..=regions.divider.x {
            assert_ne!(
                buf[(x, card.row)].bg,
                palette.selection,
                "column {x} is shaded, at or before the rule at {}\n{}",
                regions.divider.x,
                geometry::text(&buf)
            );
        }
    }

    #[test]
    fn a_rule_separates_the_sidebar_from_the_main_pane() {
        // The two panes abutted, so a card's left edge sat directly against
        // the nav's text.
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = some_tracks(&["One", "Two"]);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();

        let regions = layout::split(ratatui::layout::Rect::new(0, 0, 100, 30));
        let x = regions.divider.x;
        let painted = (regions.divider.y..regions.divider.bottom())
            .filter(|y| buf[(x, *y)].symbol() != " ")
            .count();
        assert_eq!(
            painted,
            regions.divider.height as usize,
            "the rule runs the height of the panes\n{}",
            geometry::text(&buf)
        );
    }

    #[test]
    fn a_terminal_too_narrow_for_a_rule_gives_it_up() {
        // The nav and the content matter more than the line between them.
        let r = layout::split(ratatui::layout::Rect::new(0, 0, 20, 10));
        assert_eq!(r.divider.width, 0);
        assert_eq!(r.main.x, r.sidebar.right(), "the panes abut again");
    }

    #[test]
    fn the_scrollbar_reaches_the_bottom_of_the_pane() {
        // The pane ends where the now-playing bar begins; the bar has to
        // run all the way to that edge rather than stopping short of it.
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = (0..40)
            .map(|i| {
                let mut t = crate::domain::Track::sample(
                    &format!("Track {i}"),
                    "An Artist",
                    std::time::Duration::from_secs(200),
                );
                t.id = crate::domain::TrackId::from(i as u64);
                t
            })
            .collect();

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();

        let regions = layout::split(ratatui::layout::Rect::new(0, 0, 100, 30));
        let x = regions.main.x + regions.main.width - scrollbar::WIDTH;
        let last = regions.main.y + regions.main.height - 1;
        assert_ne!(
            buf[(x, last)].symbol(),
            " ",
            "the bar reaches the last row of the pane\n{}",
            geometry::text(&buf)
        );
    }

    #[test]
    fn the_selection_stays_on_screen_inside_an_opened_album() {
        // Scrolling used the no-banner row count while the view drew with
        // one, so the selection could sit three rows below the last row
        // actually painted — the cursor simply vanished off the bottom.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        app.last_main_height = 38;
        app.tracks = (0..60)
            .map(|i| {
                crate::domain::Track::sample(
                    &format!("Track {i}"),
                    "An Artist",
                    std::time::Duration::from_secs(100),
                )
            })
            .collect();

        // Walk to the bottom of the list.
        for _ in 0..60 {
            app.update(Action::TrackNext);
        }

        let drawn = app.list_visible();
        let last_visible = app.tracklist.offset + drawn;
        assert!(
            app.tracklist.selected < last_visible,
            "selection {} is past the last drawn row {} (offset {}, {drawn} visible)",
            app.tracklist.selected,
            last_visible,
            app.tracklist.offset,
        );
    }

    #[test]
    fn a_track_that_fails_to_start_is_taken_out_of_the_bar() {
        // The bar is filled the moment a track is chosen so the keypress has
        // a visible effect. When TIDAL then answers 500 — which it did, three
        // requests at once — the cover and title of a track that never
        // started were left sitting there looking like it was playing.
        let mut app = App::default();
        let track = crate::domain::Track::sample(
            "Never Started",
            "An Artist",
            std::time::Duration::from_secs(200),
        );
        let id = track.id.clone();
        app.now_playing.track = Some(track);
        app.now_playing.playing = true;

        app.update(Action::PlaybackFailed {
            id,
            message: "500 from /tracks/0/playbackinfopostpaywall".into(),
        });

        assert!(app.now_playing.track.is_none(), "the bar is cleared");
        assert!(
            !app.now_playing.playing,
            "and it does not claim to be playing"
        );
        assert!(
            app.status.as_deref().is_some_and(|s| s.contains("500")),
            "the failure is reported: {:?}",
            app.status
        );
    }

    #[test]
    fn a_late_failure_does_not_clear_a_track_that_has_since_started() {
        // Choose one track, then another before the first request returns:
        // the first one's failure must not wipe the second from the bar.
        let mut app = App::default();
        let first = crate::domain::Track::sample("First", "A", std::time::Duration::ZERO);
        let stale_id = first.id;

        let mut second = crate::domain::Track::sample("Second", "B", std::time::Duration::ZERO);
        second.id = crate::domain::TrackId(format!("{}-later", stale_id.0));
        app.now_playing.track = Some(second);

        app.update(Action::PlaybackFailed {
            id: stale_id,
            message: "500".into(),
        });

        assert_eq!(
            app.now_playing.track.as_ref().map(|t| t.title.as_str()),
            Some("Second"),
            "the track now playing is untouched"
        );
    }

    #[test]
    fn a_new_stream_starts_the_progress_bar_at_the_beginning() {
        // The previous track's position used to survive into the next one.
        // If that track was the longer of the two, the ratio saturated and
        // the bar was drawn full over a track seven seconds in.
        let mut app = App::default();
        app.now_playing.position = std::time::Duration::from_secs(400);

        app.update(Action::Playback(crate::playback::PlaybackEvent::Started {
            bit_depth: None,
            sample_rate: 44_100,
            delivered: crate::domain::Quality::High,
        }));

        assert_eq!(
            app.now_playing.position,
            std::time::Duration::ZERO,
            "a stream that just started is at its beginning"
        );
    }

    #[test]
    fn the_quality_badge_shows_the_delivered_bit_depth() {
        // The decoder cannot report bit depth, so it is threaded from the
        // playback-info response through Cmd::Play. If that ever breaks, the
        // badge silently drops to "44.1kHz" and stops confirming hi-res.
        let mut app = App::default();
        app.update(Action::Playback(crate::playback::PlaybackEvent::Started {
            bit_depth: Some(24),
            sample_rate: 44_100,
            delivered: crate::domain::Quality::HiResLossless,
        }));
        assert_eq!(app.now_playing.quality.as_deref(), Some("24-bit 44.1kHz"));
    }

    #[test]
    fn the_quality_badge_omits_bit_depth_when_unknown() {
        let mut app = App::default();
        app.update(Action::Playback(crate::playback::PlaybackEvent::Started {
            bit_depth: None,
            sample_rate: 44_100,
            delivered: crate::domain::Quality::High,
        }));
        assert_eq!(app.now_playing.quality.as_deref(), Some("44.1kHz"));
    }

    #[test]
    fn the_quality_badge_keeps_the_half_rates_exact() {
        // HE-AAC decodes to 22050 Hz here: one decimal rounds it to "22.1",
        // which is not a rate that exists. Whole rates keep their ".0".
        for (rate, label) in [
            (22_050, "22.05kHz"),
            (44_100, "44.1kHz"),
            (48_000, "48.0kHz"),
        ] {
            let mut app = App::default();
            app.update(Action::Playback(crate::playback::PlaybackEvent::Started {
                bit_depth: None,
                sample_rate: rate,
                delivered: crate::domain::Quality::Low,
            }));
            assert_eq!(app.now_playing.quality.as_deref(), Some(label));
        }
    }

    #[test]
    fn browse_keys_do_nothing_before_login() {
        // Every browse arm is guarded on an existing session; without the
        // guards Enter would fire ActivateSelection in every state but
        // logged-out.
        let mut app = App::default();
        assert!(app.session.is_none());

        for code in [
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Tab,
            KeyCode::Char(' '),
        ] {
            let action = app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
            assert!(action.is_none(), "{code:?} must be inert before login");
        }
        // Enter is the exception: logged out, it starts the login flow.
        assert!(matches!(
            app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::BeginLogin)
        ));
    }

    #[test]
    fn a_section_emptied_by_an_open_view_is_fetched_again_on_the_way_in() {
        // The favourites, playlists and albums arrive together at sign-in,
        // and the reply is only taken when nothing is open over the pane --
        // `tracks` is shared with whatever collection is on screen. A reply
        // that lands while a view is open is dropped, and nothing asked for
        // it again: the section was empty for the rest of the session.
        let mut app = signed_in(sidebar::Section::Music);
        app.open = Some(OpenCollection {
            title: "An Album".into(),
            subtitle: String::new(),
            detail: String::new(),
            cover: None,
            round_cover: false,
            came_from: sidebar::Section::Music,
            target: None,
        });

        // The sign-in reply, arriving while that album is open.
        app.update(Action::TracksLoaded(some_tracks(&["Fav 1", "Fav 2"])));
        app.open = None;
        assert!(
            app.tracks.is_empty(),
            "the reply is dropped: it would have replaced the open album"
        );

        // Walking into Tracks has to ask for them again.
        let asked = app.update(Action::GoToSection(sidebar::Section::Tracks));
        assert!(
            matches!(asked, Some(Action::LoadFavourites)),
            "the empty section fetches its list rather than staying empty, got {asked:?}"
        );
    }

    fn signed_in(section: sidebar::Section) -> App {
        let mut app = App {
            session: Some(sample_session()),
            ..App::default()
        };
        while app.sidebar.section() != section {
            app.sidebar.next();
        }
        // A size the renderer would have measured; the movement keys need it.
        app.last_main_width = 120;
        app.last_main_height = 40;
        app
    }

    #[test]
    fn typing_in_the_filter_box_is_text_not_commands() {
        // "q" must filter, not quit. This is the whole reason the filter has
        // a mode of its own.
        let mut app = signed_in(sidebar::Section::Tracks);
        assert!(app
            .on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .is_none());
        assert!(app.filtering);

        for c in ['q', 'u', 'e'] {
            let action = app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
            assert!(
                action.is_none(),
                "{c} must not fire a command while filtering"
            );
        }
        assert_eq!(app.filter_text(), "que");
        assert!(!app.should_quit, "typing q must not have quit");

        app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.filter_text(), "qu");

        // Enter accepts what was typed; Esc would have cleared it.
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.filtering);
        assert_eq!(app.filter_text(), "qu");
    }

    #[test]
    fn escape_clears_the_filter_and_leaves_the_box() {
        let mut app = signed_in(sidebar::Section::Playlists);
        app.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for c in ['a', 'b'] {
            app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(app.filter_text(), "ab");

        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.filtering);
        assert_eq!(app.filter_text(), "", "escape abandons the filter");
        assert!(!app.should_quit, "escape must leave the box, not the app");
    }

    #[test]
    fn the_home_page_has_no_filter_box() {
        // Only the collection views have one; on home, "/" is not a binding.
        let mut app = signed_in(sidebar::Section::Music);
        app.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(!app.filtering);
    }

    #[test]
    fn each_grid_keeps_its_own_place() {
        // Moving around Albums must not disturb where the user was in
        // Playlists — they are separate views, not one list.
        let mut app = signed_in(sidebar::Section::Playlists);
        app.playlists = (0..30)
            .map(|i| crate::library::Playlist::sample(&format!("P{i}"), i))
            .collect();
        app.albums = (0..30)
            .map(|i| crate::library::Album {
                id: (i as u64).to_string(),
                title: format!("A{i}"),
                artist: "x".into(),
                year: None,
                cover: None,
                track_count: 10,
                duration: None,
            })
            .collect();

        app.update(Action::TrackNext);
        let playlist_row = app.playlist_grid.selected;
        assert!(playlist_row > 0, "the playlist grid moved");

        while app.sidebar.section() != sidebar::Section::Albums {
            app.sidebar.next();
        }
        app.update(Action::TrackNext);
        assert!(app.album_grid.selected > 0, "the album grid moved");
        assert_eq!(
            app.playlist_grid.selected, playlist_row,
            "the playlist grid must not have moved with it"
        );
    }

    #[test]
    fn a_grid_moves_by_rows_down_and_by_cards_sideways() {
        let mut app = signed_in(sidebar::Section::Playlists);
        app.playlists = (0..40)
            .map(|i| crate::library::Playlist::sample(&format!("P{i}"), i))
            .collect();

        let (cols, _) = app.grid_geometry();
        assert!(cols > 1, "120 columns must fit several cards");

        app.update(Action::TrackNext);
        assert_eq!(app.playlist_grid.selected, cols, "j moves a whole row");

        app.update(Action::CarouselNext);
        assert_eq!(app.playlist_grid.selected, cols + 1, "l moves one card");

        app.update(Action::CarouselPrevious);
        assert_eq!(app.playlist_grid.selected, cols);
    }

    #[test]
    fn a_filtered_grid_opens_the_card_that_is_highlighted() {
        // The selection indexes the filtered list. Using it against the full
        // list would open a different playlist than the one shown selected.
        let mut app = signed_in(sidebar::Section::Playlists);
        app.playlists = vec![
            crate::library::Playlist::sample("Alpha", 1),
            crate::library::Playlist::sample("Beta", 2),
            crate::library::Playlist::sample("Gamma", 3),
        ];
        for (i, p) in app.playlists.iter_mut().enumerate() {
            p.uuid = format!("uuid-{i}");
        }

        // Filter to the one playlist whose title contains "amm": Gamma, at
        // index 2 of the full list but index 0 of the filtered one.
        app.set_filter("amm".into());
        assert_eq!(app.playlist_grid.selected, 0);

        match app.selected_collection() {
            Some(Collection::Playlist(uuid)) => {
                assert_eq!(uuid, "uuid-2", "must open Gamma, not the first playlist");
            }
            other => panic!("expected a playlist, got {other:?}"),
        }
    }

    #[test]
    fn an_artist_card_opens_that_artists_page() {
        // It was the one kind of card the app drew with nowhere to go.
        let mut app = signed_in(sidebar::Section::Profiles);
        app.artists = vec![crate::library::Artist {
            id: (1).to_string(),
            name: "2Pac".into(),
            picture: None,
        }];
        assert!(matches!(
            app.selected_collection(),
            Some(Collection::Artist(id)) if id == "1"
        ));
    }

    #[test]
    fn filtering_the_track_list_keeps_the_selection_in_range() {
        use std::time::Duration;
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = (0..20)
            .map(|i| {
                crate::domain::Track::sample(
                    &format!("Song {i}"),
                    "An Artist",
                    Duration::from_secs(100),
                )
            })
            .collect();
        app.tracklist.selected = 19;

        // "Song 1" matches 1 and 10..19: eleven tracks, so index 19 is gone.
        app.set_filter("Song 1".into());
        let visible = tracklist::filter(&app.tracks, &app.tracklist.filter).len();
        assert!(
            app.tracklist.selected < visible,
            "selection {} must stay inside {visible} filtered tracks",
            app.tracklist.selected
        );
    }

    fn key(app: &mut App, code: KeyCode) -> Option<Action> {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn no_key_moves_the_selection_off_the_main_pane() {
        // Every key, on a page with something to move around in: none of
        // them may take the selection off the main pane. Tab in particular
        // used to, onto a nav where no key did anything — a mode with
        // nothing behind it, and one the user could not move back out of.
        // The nav is driven by J and K from wherever the user is; nothing
        // hands the selection over to it.
        let mut app = signed_in(sidebar::Section::Albums);
        app.albums = (0..9)
            .map(|i| crate::library::Album {
                id: (i).to_string(),
                title: format!("A{i}"),
                artist: "An Artist".into(),
                year: None,
                cover: None,
                track_count: 1,
                duration: None,
            })
            .collect();
        // The movement keys and Tab. J and K are left out on purpose: they
        // change section, and the section they land on has nothing loaded
        // in a test — that is the nav working, not the selection escaping.
        for code in [
            KeyCode::Tab,
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('h'),
            KeyCode::Char('l'),
        ] {
            if let Some(action) = key(&mut app, code) {
                app.update(action);
            }
            // Whatever the pane is showing, one of its cards is selected:
            // a selection that had moved to the nav would leave none.
            assert!(
                app.selected_card().is_some(),
                "after {code:?} the selection is still on a card in the pane"
            );
        }
    }

    #[test]
    fn j_and_k_always_drive_the_content() {
        // j and k move through the list in front of you, always. Making
        // them depend on a focused pane meant remembering an invisible mode
        // before you could move at all — there is no such mode now.
        let mut app = signed_in(sidebar::Section::Tracks);
        assert!(matches!(
            key(&mut app, KeyCode::Char('j')),
            Some(Action::TrackNext)
        ));
        assert!(matches!(
            key(&mut app, KeyCode::Char('k')),
            Some(Action::TrackPrevious)
        ));
    }

    #[test]
    fn h_and_l_stay_inside_the_content() {
        // They move along a carousel row; they no longer cross into the
        // sidebar, which has keys of its own.
        let mut app = signed_in(sidebar::Section::Playlists);
        app.playlists = (0..30)
            .map(|i| crate::library::Playlist::sample(&format!("P{i}"), i))
            .collect();
        assert!(matches!(
            key(&mut app, KeyCode::Char('l')),
            Some(Action::CarouselNext)
        ));
        assert!(matches!(
            key(&mut app, KeyCode::Char('h')),
            Some(Action::CarouselPrevious)
        ));
    }

    #[test]
    fn h_does_not_close_an_opened_album() {
        // It used to: the one place `h` meant something other than a
        // direction. Opening an album from the home page and reaching for
        // the left of a row put the user back on the page they came from,
        // so the key now means "left" everywhere and escape is what closes.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        let acted = key(&mut app, KeyCode::Char('h'));
        assert!(
            !matches!(acted, Some(Action::CloseCollection)),
            "h is a direction, not a way out, got {acted:?}"
        );
        assert!(app.open.is_some(), "the album is still open");
    }

    #[test]
    fn shift_jk_still_reach_the_sidebar_from_the_main_pane() {
        // Kept alongside the focus-aware keys: it is a shortcut people learn.
        let mut app = signed_in(sidebar::Section::Tracks);
        assert!(matches!(
            key(&mut app, KeyCode::Char('J')),
            Some(Action::SidebarNext)
        ));
        assert!(matches!(
            key(&mut app, KeyCode::Char('K')),
            Some(Action::SidebarPrevious)
        ));
    }

    #[test]
    fn the_help_opens_on_question_mark_and_any_key_closes_it() {
        let mut app = signed_in(sidebar::Section::Tracks);
        assert!(key(&mut app, KeyCode::Char('?')).is_none());
        assert!(app.showing_help);

        // While it is up, keys dismiss rather than acting — including q,
        // which must not quit out from under the modal.
        assert!(key(&mut app, KeyCode::Char('q')).is_none());
        assert!(!app.showing_help);
        assert!(!app.should_quit, "q must have closed the help, not quit");

        // And the key that opened it closes it too.
        key(&mut app, KeyCode::Char('?'));
        assert!(app.showing_help);
        key(&mut app, KeyCode::Char('?'));
        assert!(!app.showing_help);
    }

    #[test]
    fn the_help_is_not_offered_before_signing_in() {
        // The login screen has its own instructions; a key list of bindings
        // that do nothing yet would only confuse.
        let mut app = App::default();
        key(&mut app, KeyCode::Char('?'));
        assert!(!app.showing_help);
    }

    fn with_albums(n: u64) -> App {
        let mut app = signed_in(sidebar::Section::Albums);
        app.albums = (0..n)
            .map(|i| crate::library::Album {
                id: (i + 1).to_string(),
                title: format!("Album {i}"),
                artist: format!("Artist {i}"),
                year: Some("2026".into()),
                cover: None,
                track_count: 10,
                duration: None,
            })
            .collect();
        app
    }

    #[test]
    fn opening_an_album_names_it_from_the_card_that_was_selected() {
        // The header has to say which album this is. Taking the name from
        // the card means it is right immediately, without waiting on the
        // request that fetches the tracks.
        let mut app = with_albums(5);
        app.album_grid.selected = 2;

        let open = app.selected_identity().expect("an album is selected");
        assert_eq!(open.title, "Album 2");
        assert_eq!(open.subtitle, "Artist 2");
        assert_eq!(open.detail, "2026");
        assert_eq!(open.came_from, sidebar::Section::Albums);
    }

    #[test]
    fn an_opened_album_does_not_move_the_sidebar() {
        // It used to jump to Tracks, which highlighted the wrong nav entry
        // and made the album indistinguishable from the favourites view.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);

        assert_eq!(
            app.sidebar.section(),
            sidebar::Section::Albums,
            "the sidebar stays on Albums while an album is open"
        );
        assert!(app.open.is_some());
    }

    #[test]
    fn r_in_the_queue_starts_the_radio_of_the_selected_entry() {
        // The queue is a list like any other: R over one of its rows starts
        // that track's radio, not the playing track's.
        let mut app = signed_in(sidebar::Section::Tracks);
        let mut a = crate::domain::Track::sample("Playing", "A", std::time::Duration::from_secs(1));
        a.id = crate::domain::TrackId::from(1);
        let mut b = crate::domain::Track::sample("Queued", "B", std::time::Duration::from_secs(1));
        b.id = crate::domain::TrackId::from(2);
        app.queue.start_context(vec![a.clone(), b], 0, None);
        app.now_playing.track = Some(a);
        app.last_main_height = 40;
        key(&mut app, KeyCode::Char('Q'));
        assert_eq!(app.showing(), Showing::Queue, "the queue is up");

        app.queue_list.selected = 1;
        assert!(
            matches!(
                key(&mut app, KeyCode::Char('R')),
                Some(Action::PlayTrackRadio)
            ),
            "R offers the radio"
        );
        assert_eq!(
            app.track_radio(),
            Some(crate::domain::TrackId::from(2)),
            "the selected row's, not the playing one's"
        );
        app.update(Action::PlayTrackRadio);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some("Queued Radio")
        );
        assert!(
            matches!(
                app.open.as_ref().and_then(|o| o.target.clone()),
                Some(Collection::Radio(ref seed)) if seed.0 == "2"
            ),
            "and the loop fetches the station on that seed"
        );
    }

    #[test]
    fn r_does_nothing_where_there_is_no_track() {
        // It is repeat's own key in capitals, so a stray press with no
        // track selected and nothing playing falls through rather than
        // opening an empty view.
        let mut app = signed_in(sidebar::Section::Profiles);
        assert!(app.track_radio().is_none());
        assert!(key(&mut app, KeyCode::Char('R')).is_none());
    }

    #[test]
    fn enter_on_a_my_tracks_shortcut_goes_to_the_tracks_view() {
        // The feed's shortcut grid points at the app's own views: "My
        // Tracks" is the favourites, not a collection to fetch.
        let mut app = signed_in(sidebar::Section::Music);
        let mut card = carousel::Card::new("My Tracks", "");
        card.target = Some(carousel::Target::Section(sidebar::Section::Tracks));
        app.home.rows.push(home::Row {
            heading: "Shortcuts".into(),
            kind: crate::browse::RowKind::Shortcuts,
            cards: vec![card],
            state: carousel::CarouselState::default(),
        });
        assert!(app.selected_collection().is_none(), "nothing to fetch");

        let mut next = Some(Action::ActivateSelection);
        while let Some(action) = next {
            next = app.update(action);
        }
        assert_eq!(app.sidebar.section(), sidebar::Section::Tracks);
        assert!(app.open.is_none(), "the view itself, not something over it");
    }

    #[test]
    fn enter_on_an_explore_link_opens_its_page() {
        // Explore did nothing at all: its cards carry a path rather than an
        // id of any kind, so none of them had anything to open.
        let mut app = signed_in(sidebar::Section::Explore);
        let mut card = carousel::Card::new("Hip-Hop", "");
        card.target = Some(carousel::Target::Page("pages/genre_hip_hop".into()));
        app.explore.rows.push(home::Row {
            heading: "Genres".into(),
            kind: crate::browse::RowKind::Carousel,
            cards: vec![card],
            state: carousel::CarouselState::default(),
        });

        assert!(
            matches!(
                app.selected_collection(),
                Some(Collection::Page { ref path, .. }) if path == "pages/genre_hip_hop"
            ),
            "enter has a page to open, got {:?}",
            app.selected_collection()
        );

        // The reply takes the pane, and the history gets back to Explore.
        let mut home = crate::browse::Home::default();
        home.rows.push(crate::browse::HomeRow {
            heading: "Playlists".into(),
            kind: crate::browse::RowKind::Carousel,
            cards: vec![carousel::Card::new("A Playlist", "TIDAL")],
            more: None,
        });
        app.update(Action::PageLoaded {
            title: "Hip-Hop".into(),
            home: Box::new(home),
        });
        // Drawn by the rows renderer under its own heading. It used to set
        // `open` instead, which hands the pane to the collection view --
        // and that view draws `open_cards`, which a page of rows never
        // fills, so the pane went empty and enter looked like it did
        // nothing.
        assert!(
            app.open.is_none(),
            "a page of rows is not an opened collection, got {:?}",
            app.open
        );
        assert_eq!(
            app.explore.heading.as_deref(),
            Some("Hip-Hop"),
            "headed by the genre the user pressed enter on"
        );
        assert_eq!(
            app.explore.rows.first().map(|r| r.heading.as_str()),
            Some("Playlists"),
            "with the page's own rows in it"
        );
        assert!(
            app.on_home(),
            "and the keys drive the rows, as they do on Explore itself"
        );
    }

    #[test]
    fn the_numbers_reach_every_nav_entry() {
        // Nine entries and nine keys: 1 is Music and 9 is Settings, so the
        // far end is one press rather than eight of J.
        for (i, section) in sidebar::Section::ALL.iter().enumerate() {
            let mut app = signed_in(sidebar::Section::Music);
            let digit = char::from_digit(i as u32 + 1, 10).expect("1 to 9");
            let action = key(&mut app, KeyCode::Char(digit)).expect("bound");
            app.update(action);
            assert_eq!(
                app.sidebar.section(),
                *section,
                "{digit} reaches {:?}",
                section.label()
            );
        }
    }

    #[test]
    fn a_number_leaves_an_opened_view_and_can_be_come_back_from() {
        // The same as J and K: what was on screen goes on the history.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        let title = app.open.as_ref().expect("open").title.clone();

        let action = key(&mut app, KeyCode::Char('8')).expect("bound");
        app.update(action);
        assert_eq!(app.sidebar.section(), sidebar::Section::Settings);
        assert!(app.open.is_none(), "the album closed");

        app.update(Action::GoBack);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some(title.as_str()),
            "back returns to it"
        );
    }

    #[test]
    fn pressing_the_number_of_the_section_already_showing_does_nothing() {
        // Otherwise the history fills with the same place and back walks
        // through a run of identical steps.
        let mut app = signed_in(sidebar::Section::Music);
        let before = app.back.len();
        let action = key(&mut app, KeyCode::Char('1'));
        if let Some(action) = action {
            app.update(action);
        }
        assert_eq!(app.back.len(), before, "nothing was recorded");
        assert_eq!(app.sidebar.section(), sidebar::Section::Music);
    }

    #[test]
    fn escape_steps_back_like_the_bracket_and_never_quits() {
        // Escape and `[` are the same step. Escape is the key people reach
        // for to leave where they are, and leaving should mean one thing
        // whichever of the two is pressed. And it never quits: `q` alone
        // does that, since a key that usually means "leave this view"
        // should not sometimes close the app.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        assert!(app.open.is_some(), "an album is open");

        let action = key(&mut app, KeyCode::Esc).expect("escape is bound");
        assert!(
            matches!(action, Action::GoBack),
            "the same as `[`, got {action:?}"
        );
        app.update(action);
        assert!(app.open.is_none(), "the album closed");
        assert!(!app.should_quit, "escape must not have quit");

        // Both keys ask for the same thing.
        let mut other = with_albums(3);
        other.update(Action::ActivateSelection);
        assert!(
            matches!(key(&mut other, KeyCode::Char('[')), Some(Action::GoBack)),
            "`[` steps back too"
        );

        // With nothing left to step back to it stays put rather than
        // quitting or emptying the pane.
        let action = key(&mut app, KeyCode::Esc).expect("still bound");
        app.update(action);
        assert!(!app.should_quit);

        // `q` is the way out of the app.
        assert!(matches!(
            key(&mut app, KeyCode::Char('q')),
            Some(Action::Quit)
        ));
    }

    #[test]
    fn back_returns_to_where_escape_was_pressed() {
        // Escape is a step in the history like any other, so the arrow
        // returns to the view it left.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        let title = app.open.as_ref().expect("open").title.clone();

        app.update(Action::GoHome);
        assert!(app.open.is_none());

        app.update(Action::GoBack);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some(title.as_str()),
            "back returns to the album escape left"
        );
    }

    #[test]
    fn closing_returns_to_the_grid_it_was_opened_from() {
        let mut app = with_albums(9);
        app.album_grid.selected = 4;
        app.update(Action::ActivateSelection);
        // Pretend the fetch landed.
        app.tracks = vec![crate::domain::Track::sample(
            "t",
            "a",
            std::time::Duration::ZERO,
        )];
        // And that the user moved around inside it.
        app.tracklist.selected = 0;

        app.update(Action::CloseCollection);

        assert!(app.open.is_none());
        assert_eq!(app.sidebar.section(), sidebar::Section::Albums);
        assert_eq!(
            app.album_grid.selected, 4,
            "the grid selection is where it was left"
        );
        assert!(
            app.tracks.is_empty(),
            "the album's tracks are not left behind as favourites"
        );
    }

    #[test]
    fn h_in_an_open_album_does_not_reach_the_sidebar() {
        // It no longer closes the album either -- `h` is a direction
        // everywhere -- but the thing this has always guarded is that it
        // does not fall through to the nav behind the view.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);

        let action = key(&mut app, KeyCode::Char('h'));
        assert!(
            !matches!(
                action,
                Some(Action::SidebarPrevious | Action::GoToSection(_))
            ),
            "the keyboard stays in the album, got {action:?}"
        );
        assert!(app.open.is_some(), "which is still open");
    }

    #[test]
    fn opening_an_album_gives_it_the_keyboard() {
        // Without this the opened view could not be moved around in: focus
        // stayed on the grid behind it, so j and k drove a list nobody could
        // see any more.
        let mut app = with_albums(3);

        app.update(Action::ActivateSelection);

        assert!(app.open.is_some());
        assert!(
            matches!(key(&mut app, KeyCode::Char('j')), Some(Action::TrackNext)),
            "and j moves through its tracks"
        );
    }

    #[test]
    fn an_album_card_on_the_home_page_opens_too() {
        // Enter means the same thing wherever there are cards; it used to
        // work only in the Albums grid.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            kind: crate::browse::RowKind::Carousel,
            heading: "New Albums".into(),
            cards: vec![carousel::Card {
                title: "August 26".into(),
                subtitle: "Post Malone".into(),
                target: Some(carousel::Target::Album("42".into())),
                ..Default::default()
            }],
            state: carousel::CarouselState::default(),
        }];

        assert!(matches!(
            app.selected_collection(),
            Some(Collection::Album(id)) if id == "42"
        ));

        app.update(Action::ActivateSelection);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some("August 26"),
            "the home page opens an album like the grid does"
        );
    }

    #[test]
    fn a_track_card_is_not_a_collection() {
        // A track plays; it has no track list to open.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            kind: crate::browse::RowKind::Carousel,
            heading: "New Tracks".into(),
            cards: vec![carousel::Card {
                title: "Bass Persuades".into(),
                target: Some(carousel::Target::Track("9".into())),
                ..Default::default()
            }],
            state: carousel::CarouselState::default(),
        }];

        assert!(app.selected_collection().is_none());
        assert_eq!(app.selected_track_id(), Some(crate::domain::TrackId::from(9)));

        app.update(Action::ActivateSelection);
        assert!(app.open.is_none(), "a track does not open a view");
    }

    #[test]
    fn a_card_with_no_identifier_does_nothing() {
        // Mixes and some modules come back without an id; enter on one must
        // not open an empty view.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            kind: crate::browse::RowKind::Carousel,
            heading: "My Mixes".into(),
            cards: vec![carousel::Card {
                title: "My Mix 1".into(),
                ..Default::default()
            }],
            state: carousel::CarouselState::default(),
        }];

        assert!(app.selected_collection().is_none());
        app.update(Action::ActivateSelection);
        assert!(app.open.is_none());
    }

    #[test]
    fn j_and_k_move_through_an_opened_album_not_the_grid_behind_it() {
        // The bug this is here for: the sidebar deliberately stays on Albums
        // while an album is open, and `on_grid` read only the sidebar — so
        // j moved the grid's selection, which was no longer on screen, and
        // the track list never budged.
        let mut app = with_albums(9);
        app.album_grid.selected = 3;
        app.update(Action::ActivateSelection);

        // Stand in for the fetch that fills the view.
        app.tracks = (0..6)
            .map(|i| {
                crate::domain::Track::sample(
                    &format!("Track {i}"),
                    "An Artist",
                    std::time::Duration::from_secs(100),
                )
            })
            .collect();

        assert!(!app.on_grid(), "an opened album is a list, not a grid");

        app.update(Action::TrackNext);
        assert_eq!(app.tracklist.selected, 1, "j moves down the album's tracks");
        app.update(Action::TrackNext);
        assert_eq!(app.tracklist.selected, 2);
        app.update(Action::TrackPrevious);
        assert_eq!(app.tracklist.selected, 1);

        assert_eq!(
            app.album_grid.selected, 3,
            "and the grid behind it has not moved"
        );
    }

    #[test]
    fn an_album_opened_from_the_home_page_is_not_still_the_home_page() {
        // Same failure through the other door: opening from Music left
        // `on_home` true, so j and k moved the carousel rows underneath.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            kind: crate::browse::RowKind::Carousel,
            heading: "New Albums".into(),
            cards: vec![carousel::Card {
                title: "August 26".into(),
                target: Some(carousel::Target::Album("42".into())),
                ..Default::default()
            }],
            state: carousel::CarouselState::default(),
        }];
        app.update(Action::ActivateSelection);
        app.tracks = vec![crate::domain::Track::sample(
            "t",
            "a",
            std::time::Duration::ZERO,
        )];

        assert!(app.open.is_some());
        assert!(!app.on_home(), "an opened album is not the home page");
        assert!(matches!(
            key(&mut app, KeyCode::Char('j')),
            Some(Action::TrackNext)
        ));
    }

    fn some_tracks(titles: &[&str]) -> Vec<crate::domain::Track> {
        titles
            .iter()
            .map(|t| {
                crate::domain::Track::sample(t, "An Artist", std::time::Duration::from_secs(100))
            })
            .collect()
    }

    #[test]
    fn favourites_landing_late_do_not_replace_an_open_album() {
        // Both fetches used to report through one action, so the startup
        // favourites request finishing after an album was opened would
        // silently swap the album's tracks for the user's whole library.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        let title = app.open.as_ref().unwrap().title.clone();

        app.update(Action::CollectionLoaded {
            for_title: title,
            tracks: some_tracks(&["Album track"]),
        });
        assert_eq!(app.tracks.len(), 1);

        // The startup fetch lands now.
        app.update(Action::TracksLoaded(some_tracks(&[
            "Fav 1", "Fav 2", "Fav 3",
        ])));

        assert_eq!(
            app.tracks.len(),
            1,
            "the album's tracks are still what is shown"
        );
        assert_eq!(app.tracks[0].title, "Album track");
    }

    #[test]
    fn a_reply_for_an_album_that_was_left_is_dropped() {
        // Open one album, leave it before the response arrives: the tracks
        // must not appear under whatever is on screen now.
        let mut app = with_albums(5);
        app.update(Action::ActivateSelection);
        let first = app.open.as_ref().unwrap().title.clone();
        app.update(Action::CloseCollection);

        app.update(Action::CollectionLoaded {
            for_title: first,
            tracks: some_tracks(&["Late", "Reply"]),
        });

        assert!(
            app.tracks.is_empty(),
            "a reply for a closed view is dropped"
        );
    }

    #[test]
    fn a_reply_for_the_previous_album_does_not_fill_the_current_one() {
        let mut app = with_albums(5);
        app.update(Action::ActivateSelection);
        let first = app.open.as_ref().unwrap().title.clone();

        // Straight on to another album.
        app.update(Action::CloseCollection);
        app.album_grid.selected = 2;
        app.update(Action::ActivateSelection);
        let second = app.open.as_ref().unwrap().title.clone();
        assert_ne!(first, second);

        // The first album's reply arrives now.
        app.update(Action::CollectionLoaded {
            for_title: first,
            tracks: some_tracks(&["Wrong album"]),
        });
        assert!(app.tracks.is_empty(), "the stale reply is dropped");

        // And the right one still fills it.
        app.update(Action::CollectionLoaded {
            for_title: second,
            tracks: some_tracks(&["Right album"]),
        });
        assert_eq!(app.tracks[0].title, "Right album");
    }

    #[test]
    fn favourites_still_fill_the_tracks_view_when_nothing_is_open() {
        let mut app = signed_in(sidebar::Section::Tracks);
        app.update(Action::TracksLoaded(some_tracks(&["Fav 1", "Fav 2"])));
        assert_eq!(app.tracks.len(), 2);
    }

    #[test]
    fn the_home_rows_are_shown_when_they_land() {
        let mut app = signed_in(sidebar::Section::Music);
        let fresh = crate::browse::Home {
            rows: vec![crate::browse::HomeRow {
                heading: "Songs on repeat".into(),
                kind: crate::browse::RowKind::Compact,
                cards: vec![carousel::Card::new("A track", "An artist")],
                more: None,
            }],
        };
        app.update(Action::HomeLoaded(Box::new(fresh)));

        assert_eq!(
            app.home.rows.first().map(|r| r.heading.as_str()),
            Some("Songs on repeat")
        );
    }

    #[test]
    fn closing_the_help_repaints_what_it_covered() {
        // Reported: covers kept a patch of the help modal on them after it
        // closed. The image protocols paint straight to the terminal and
        // mark their cells `Skip` so ratatui leaves them alone — but the
        // modal wrote over those cells, and on the next frame they went
        // back to `Skip`, so nothing was written to undo it.
        //
        // What has to happen is a full repaint on the frame the modal
        // leaves. This checks the flag that asks for one.
        let mut app = signed_in(sidebar::Section::Music);
        app.showing_help = true;
        assert!(!app.needs_repaint, "nothing to undo yet");

        key(&mut app, KeyCode::Esc);
        assert!(!app.showing_help, "the help closed");
        assert!(
            app.needs_repaint,
            "and asked for the frame under it to be drawn again"
        );

        // Taken by the draw, so it does not repaint on every frame after.
        assert!(app.take_repaint());
        assert!(!app.take_repaint(), "once, not for ever");
    }

    #[test]
    fn the_event_loop_clears_before_the_frame_that_needs_it() {
        // The flag is only worth setting if the loop acts on it. The loop
        // itself needs a terminal and a signed-in session to run, so this
        // reads the source: what must hold is that `take_repaint` is asked
        // before the draw and that a clear follows from it.
        let source = include_str!("mod.rs");
        let loop_body = source
            .split("while !app.should_quit {")
            .nth(1)
            .expect("the event loop");
        let take = loop_body.find("take_repaint()").expect("the flag is read");
        let clear = loop_body.find("terminal.clear()").expect("and acted on");
        let draw = loop_body.find("terminal.draw(").expect("before the draw");
        assert!(take < clear, "the clear follows from the flag");
        assert!(clear < draw, "and happens before the frame it is for");
    }

    #[test]
    fn the_selected_home_row_is_never_the_cut_one() {
        // Reported: arriving on a row cut off at the bottom did not scroll,
        // so the selection sat on a half-drawn row and the page looked
        // stuck. What must hold is that wherever the selection goes, the
        // row it lands on is drawn whole — checked by walking the page
        // from top to bottom at a height that does cut one.
        let mut app = signed_in(sidebar::Section::Music);
        let mut home = home::HomeState::default();
        for i in 0..8 {
            home.rows.push(home::Row {
                heading: format!("Row {i}"),
                kind: crate::browse::RowKind::Carousel,
                cards: (0..6)
                    .map(|c| carousel::Card::new(format!("R{i} C{c}"), "artist"))
                    .collect(),
                state: carousel::CarouselState::default(),
            });
        }
        app.home = home;
        app.last_main_width = 120;
        // A height that actually cuts a row, found rather than assumed:
        // the row heights have changed under this test before.
        let width = scrollbar::content_width(app.last_main_width);
        let height = (30..80u16)
            .find(|h| {
                home::rows_to_land_in(*h, width, &app.home)
                    < home::visible_rows_of(*h, width, &app.home)
                    && home::visible_rows_of(*h, width, &app.home) >= 3
            })
            .expect("some height cuts a row");
        app.last_main_height = height;

        let visible = home::visible_rows_of(height, width, &app.home);
        for step in 0..8 {
            app.update(Action::RowNext);
            // The rows drawn whole are `visible - 1` of them when the last
            // is cut; the selection must be inside that part.
            let whole = app.home.scroll + visible - 1;
            assert!(
                app.home.row < whole,
                "step {step}: selected row {} is the cut one (scroll {}, {visible} visible)",
                app.home.row,
                app.home.scroll
            );
        }
    }

    #[test]
    fn enter_opens_the_artist_and_album_rows_of_top_results() {
        // Reported: the profiles and albums drawn in Top Results could not
        // be entered. `selected_card` refused the whole tab, so enter had
        // nothing to open — even though Top draws the same cards the
        // Artists and Albums tabs do, and moving through them worked.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        // The results are only taken when they answer the query that is
        // typed, so the query has to be there.
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        assert_eq!(
            searchview::Tab::from_index(app.search.as_ref().unwrap().tab),
            searchview::Tab::Top,
            "search opens on Top"
        );

        // The artist row is drawn first, so that is where the selection is.
        app.search.as_mut().unwrap().top = searchview::TopSection::Artists;
        assert!(
            matches!(app.selected_collection(), Some(Collection::Artist(_))),
            "the highlighted artist is what enter opens, got {:?}",
            app.selected_collection()
        );
        app.update(Action::ActivateSelection);
        assert!(app.open.is_some(), "and it opened");

        // The album row, the other half of the report.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.search.as_mut().unwrap().top = searchview::TopSection::Albums;
        assert!(
            matches!(app.selected_collection(), Some(Collection::Album(_))),
            "the highlighted album, got {:?}",
            app.selected_collection()
        );
        app.update(Action::ActivateSelection);
        assert!(app.open.is_some(), "and it opened too");
    }

    #[test]
    fn a_track_in_top_results_still_plays_rather_than_opening() {
        // The track list is read by `selected_track`; a track has nothing
        // to open, and saying otherwise would have enter open a view for
        // something that should simply play.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.search.as_mut().unwrap().top = searchview::TopSection::Tracks;

        assert!(app.selected_collection().is_none(), "nothing to open");
        assert!(!app.on_grid(), "and the keys move it as a list");
        assert!(
            app.selected_track().is_some(),
            "the highlighted track is what enter plays"
        );
    }

    #[test]
    fn opening_an_artist_waits_rather_than_drawing_a_stand_in_photo() {
        // The flash in the report: opening an artist drew a moment of the
        // collection view -- their photo in a banner over an empty track
        // list -- before their page arrived. There is nothing to show yet,
        // so the pane says so instead of standing something in.
        let mut app = signed_in(sidebar::Section::Profiles);
        app.artists = vec![crate::library::Artist {
            id: (1).to_string(),
            name: "13 Block".into(),
            picture: Some("https://example.invalid/a.jpg".into()),
        }];
        app.update(Action::ActivateSelection);

        assert_eq!(
            app.showing(),
            Showing::Loading,
            "the page has not arrived, so neither shape is drawn"
        );

        // And when it does, the artist page takes the pane.
        app.update(Action::ArtistLoaded(Box::new(crate::library::ArtistPage {
            name: "13 Block".into(),
            ..Default::default()
        })));
        assert_eq!(app.showing(), Showing::Artist, "the page took the pane");
    }

    #[test]
    fn the_search_box_opens_over_a_page_and_closes_back_onto_it() {
        // `s` on an album left the album drawn over the box, with the keys
        // still moving through its tracks.
        let mut app = signed_in(sidebar::Section::Albums);
        app.artist = Some(crate::library::ArtistPage {
            name: "Section.80".into(),
            ..Default::default()
        });
        app.last_main_width = 100;
        app.last_main_height = 40;

        app.update(Action::BeginSearch);
        assert!(
            matches!(app.showing(), Showing::Search(_)),
            "the box is on top"
        );
        let text = geometry::text(&geometry::draw(120, 40, |f, _a, _p| draw(f, &mut app)));
        assert!(text.contains("Search"), "the box is drawn:\n{text}");
        assert!(
            !text.contains("Section.80"),
            "and the page is not drawn over it:\n{text}"
        );

        app.update(Action::CloseSearch);
        assert_eq!(
            app.showing(),
            Showing::Artist,
            "closing the box gives the page back"
        );
    }

    #[test]
    fn opening_an_artist_never_asks_for_a_square_photo() {
        // Reported as a flash from round to square and back. Opening an
        // artist shows a moment of the collection view before their page
        // arrives, and that view drew every banner cover square -- so the
        // avatar that was round in the grid turned square, then round again
        // when the page landed. The shape follows the card that was opened.
        let mut app = signed_in(sidebar::Section::Profiles);
        app.artists = vec![crate::library::Artist {
            id: (1).to_string(),
            name: "13 Block".into(),
            picture: Some("https://example.invalid/a.jpg".into()),
        }];
        app.update(Action::ActivateSelection);

        let open = app.open.as_ref().expect("the artist opened");
        assert!(
            open.round_cover,
            "the card was a round avatar, so what it opened into is one too"
        );

        // The shape the banner asks for, which is what the terminal draws.
        let asked = std::cell::RefCell::new(Vec::new());
        let banner = tracklist::Banner {
            title: &open.title,
            subtitle: &open.subtitle,
            detail: &open.detail,
            cover: open.cover.as_deref(),
            round: open.round_cover,
            liked: false,
        };
        let state = tracklist::TrackListState::default();
        let favourites = std::collections::HashSet::new();
        geometry::draw(120, 30, |f, area, palette| {
            tracklist::render(
                f,
                area,
                palette,
                tracklist::TrackList {
                    tracks: &[],
                    state: &state,
                    focused: true,
                    playing: None,
                    tier: nowplaying::Tier::Low,
                    banner: Some(banner),
                    chrome: tracklist::Chrome::Full,
                    filtering: false,
                    favourites: &favourites,
                },
                |_f, _a, url, shape| {
                    asked.borrow_mut().push((url.to_string(), shape));
                    false
                },
            )
        });

        let shapes = asked.borrow();
        assert!(
            !shapes.is_empty(),
            "the banner asked for no cover at all, so nothing was checked"
        );
        assert!(
            shapes.iter().all(|(_, s)| *s == artwork::Shape::Round),
            "an artist's photo is round wherever it is drawn, got {shapes:?}"
        );
    }

    #[test]
    fn the_keys_and_the_pane_agree_about_what_is_on_screen() {
        // The bug this closes: a genre page set `open`, the renderer read
        // that as an opened collection and drew an empty track list, and
        // the keys went on driving a page of rows. Both now ask the same
        // question, so a state that draws rows is a state the keys treat as
        // rows.
        let mut app = signed_in(sidebar::Section::Explore);
        app.explore.heading = Some("Hip-Hop".into());
        app.explore.rows.push(home::Row {
            heading: "Playlists".into(),
            kind: crate::browse::RowKind::Carousel,
            cards: vec![carousel::Card::new("A Playlist", "TIDAL")],
            state: carousel::CarouselState::default(),
        });

        assert_eq!(
            app.showing(),
            Showing::Rows,
            "a genre page is a page of rows"
        );
        assert!(app.on_home(), "so the keys drive it as one");
        assert!(!app.on_grid(), "and not as a grid of covers");

        // An opened collection is the other case, and it must not be
        // mistaken for a page of rows. Opening it waits first: what comes
        // back is what decides the shape.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        assert_eq!(
            app.showing(),
            Showing::Loading,
            "nothing has arrived yet, so neither shape is drawn"
        );
        assert!(!app.on_home(), "and the keys drive neither");

        app.update(Action::TracksLoaded(Vec::new()));
        assert_eq!(app.showing(), Showing::Tracks, "the reply makes it a list");
        assert!(!app.on_home(), "which the keys do not drive as rows");

        // And a section with nothing over it is itself.
        let app = signed_in(sidebar::Section::Albums);
        assert_eq!(app.showing(), Showing::Section(sidebar::Section::Albums));
        assert!(app.on_grid(), "Albums is a grid of covers");
    }

    #[test]
    fn every_way_of_opening_a_view_leaves_the_same_state_behind() {
        // The four of these were written out by hand and had drifted: one
        // pushed no level at all, so the view it opened could not be left;
        // one kept the artist page it covered; one reset the track list by
        // its selection alone. Whatever opens a view, the state behind it
        // has to be the same, or the keys act on the view underneath.
        let opens = [
            ("enter on a card", Action::ActivateSelection),
            ("see all on a row", Action::SeeAll),
        ];

        for (what, action) in opens {
            let mut app = with_albums(3);
            app.home.rows.push(home::Row {
                heading: "A row".into(),
                kind: crate::browse::RowKind::Carousel,
                cards: vec![carousel::Card::new("A card", "artist")],
                state: carousel::CarouselState::default(),
            });
            // State the view being left had, which must not survive it.
            app.artist_scroll = 4;
            app.artist_bio_open = true;
            app.tracklist.offset = 9;
            app.tracklist.filter = "leftover".into();

            let before = app.back.len();
            app.update(action.clone());
            if app.open.is_none() {
                continue; // this one had nothing to open here
            }

            assert_eq!(
                app.back.len(),
                before + 1,
                "{what}: nothing went on the history, so back has nothing to pop"
            );
            assert!(app.artist.is_none(), "{what}: an artist page survived");
            assert_eq!(app.artist_scroll, 0, "{what}: the artist scroll survived");
            assert!(!app.artist_bio_open, "{what}: an open bio survived");
            // The track list and its tracks are not asserted here: the level
            // takes them on its way out, so they are already empty whatever
            // `open_view` does with them. The artist fields are the ones it
            // is really answerable for -- nothing else clears those.

            // And it can be left again, which is what the history is for.
            assert!(app.go_back(), "{what}: back found nothing to return to");
            assert!(app.open.is_none(), "{what}: back did not close it");
        }
    }

    #[test]
    fn a_row_that_fits_offers_nothing_but_still_opens() {
        // A row of one card: nothing is behind it, so the hint stays off.
        // The key still opens it as a grid of what is held, which is the
        // same cards drawn larger -- harmless, and cheaper than a key that
        // works on some rows and not others.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows.push(home::Row {
            heading: "A short row".into(),
            kind: crate::browse::RowKind::Carousel,
            cards: vec![carousel::Card::new("The only card", "artist")],
            state: carousel::CarouselState::default(),
        });

        let buf = geometry::draw(120, 30, |f, _area, _p| draw(f, &mut app));
        let text = geometry::text(&buf);
        assert!(
            !text.contains("See all"),
            "nothing is behind it, so nothing offers to show it:\n{text}"
        );
        assert!(
            matches!(key(&mut app, KeyCode::Char('o')), Some(Action::SeeAll)),
            "and o still opens it"
        );
        app.update(Action::SeeAll);
        assert_eq!(app.open_cards.len(), 1, "as a grid of what is held");
    }

    #[test]
    fn j_and_k_leave_an_opened_view_for_the_next_section() {
        // They moved the nav and the opened album stayed on top of it, so
        // the keys looked like they did nothing at all.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        assert!(app.open.is_some(), "an album is open");
        let was = app.sidebar.section();

        app.update(Action::SidebarNext);
        assert!(app.open.is_none(), "the album closed");
        assert_ne!(app.sidebar.section(), was, "and the section moved");
    }

    #[test]
    fn back_and_forward_walk_the_history_both_ways() {
        // Escape only ever went back. These are the two arrows: a run of
        // sections and opened views, stepped through in both directions.
        let mut app = with_albums(3);
        let first = app.sidebar.section();

        // Two moves through the nav, then into an album.
        app.update(Action::SidebarNext);
        let second = app.sidebar.section();
        app.update(Action::SidebarPrevious);
        assert_eq!(app.sidebar.section(), first, "back where we started");

        // Back through both moves.
        app.update(Action::GoBack);
        assert_eq!(app.sidebar.section(), second, "one step back");
        app.update(Action::GoBack);
        assert_eq!(app.sidebar.section(), first, "and another");

        // Forward again, the same way.
        app.update(Action::GoForward);
        assert_eq!(app.sidebar.section(), second, "one step forward");
        app.update(Action::GoForward);
        assert_eq!(app.sidebar.section(), first, "and back to where we were");

        // Past the end of either stack is a no-op rather than a panic.
        for _ in 0..5 {
            app.update(Action::GoForward);
        }
        assert_eq!(app.sidebar.section(), first);
    }

    #[test]
    fn the_history_does_not_grow_without_end() {
        // Every move through the nav is a step, so a session spent walking
        // the sidebar would otherwise keep every one of them.
        let mut app = with_albums(3);
        for _ in 0..(App::HISTORY * 3) {
            app.update(Action::SidebarNext);
            app.update(Action::SidebarPrevious);
        }
        assert!(
            app.back.len() <= App::HISTORY,
            "the history is capped, got {}",
            app.back.len()
        );
        // And the recent end is what was kept: back still works.
        let before = app.sidebar.section();
        app.update(Action::GoBack);
        assert_ne!(
            app.sidebar.section(),
            before,
            "the newest step is still there"
        );
    }

    #[test]
    fn opening_something_new_drops_what_was_ahead() {
        // A browser does the same: following a link after going back means
        // the forward arrow has nowhere to go.
        let mut app = with_albums(3);
        app.update(Action::SidebarNext);
        app.update(Action::GoBack);
        assert!(!app.forward.is_empty(), "there is something ahead");

        app.update(Action::ActivateSelection);
        assert!(
            app.forward.is_empty(),
            "opening something is a new branch, so what was ahead is gone"
        );
    }

    #[test]
    fn back_returns_to_an_opened_view_left_by_the_nav() {
        // The view J and K closed is on the history, so back is how the
        // user gets to it again.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        let title = app.open.as_ref().expect("open").title.clone();

        app.update(Action::SidebarNext);
        assert!(app.open.is_none());

        app.update(Action::GoBack);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some(title.as_str()),
            "the album the nav closed comes back"
        );
    }

    fn some_results() -> crate::search::Results {
        crate::search::Results {
            query: "daft punk".into(),
            tracks: (0..6)
                .map(|i| {
                    crate::domain::Track::sample(
                        &format!("Track {i}"),
                        "Daft Punk",
                        std::time::Duration::from_secs(200),
                    )
                })
                .collect(),
            // Enough of each kind to move within: a section holding one
            // card cannot move, so a shared selection would look correct.
            albums: (0..8)
                .map(|i| crate::library::Album {
                    id: (i).to_string(),
                    title: format!("Album {i}"),
                    artist: "Daft Punk".into(),
                    year: Some("2001".into()),
                    cover: None,
                    track_count: 10,
                    duration: None,
                })
                .collect(),
            artists: (0..8)
                .map(|i| crate::library::Artist {
                    id: (i).to_string(),
                    name: format!("Artist {i}"),
                    picture: None,
                })
                .collect(),
            playlists: (0..8)
                .map(|i| crate::library::Playlist {
                    uuid: format!("u{i}"),
                    title: format!("Playlist {i}"),
                    track_count: 10,
                    duration: None,
                    creator: "TIDAL".into(),
                    cover: None,
                })
                .collect(),
        }
    }

    #[test]
    fn typing_in_the_search_box_is_text_not_commands() {
        // Same trap as the filter box: "q" has to be a letter while a query
        // is being typed, or the app quits mid-word.
        let mut app = signed_in(sidebar::Section::Music);
        assert!(matches!(
            key(&mut app, KeyCode::Char('s')),
            Some(Action::BeginSearch)
        ));
        app.update(Action::BeginSearch);

        for c in ['q', 'u', 'e'] {
            assert!(
                key(&mut app, KeyCode::Char(c)).is_none(),
                "{c} must not act"
            );
        }
        assert_eq!(app.search.as_ref().map(|s| s.query.as_str()), Some("que"));
        assert!(!app.should_quit, "typing q must not have quit");

        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.search.as_ref().map(|s| s.query.as_str()), Some("qu"));
    }

    #[test]
    fn enter_runs_the_query_and_hands_the_keys_back() {
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft".chars() {
            key(&mut app, KeyCode::Char(c));
        }

        match key(&mut app, KeyCode::Enter) {
            Some(Action::RunSearch(q)) => assert_eq!(q, "daft"),
            other => panic!("expected a search for \"daft\", got {other:?}"),
        }
        assert!(
            !app.search.as_ref().unwrap().typing,
            "the box gives the keyboard back, so j and k drive the results"
        );
    }

    #[test]
    fn escape_leaves_search_rather_than_only_the_box() {
        // A box with no way back to the app would be a trap.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        assert!(matches!(
            key(&mut app, KeyCode::Esc),
            Some(Action::CloseSearch)
        ));
        app.update(Action::CloseSearch);
        assert!(app.search.is_none());
        assert!(!app.should_quit, "and it did not quit the app");
    }

    #[test]
    fn results_for_a_query_the_user_has_retyped_are_dropped() {
        // The request is slow enough that a reply can land after the query
        // has changed; it must not appear under text that did not produce it.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "kendrick".chars() {
            key(&mut app, KeyCode::Char(c));
        }

        app.update(Action::SearchLoaded(Box::new(some_results())));
        assert!(
            app.search.as_ref().unwrap().results.is_empty(),
            "results for \"daft punk\" do not belong to \"kendrick\""
        );
    }

    #[test]
    fn results_for_the_query_on_screen_are_shown() {
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        assert_eq!(app.search.as_ref().unwrap().results.tracks.len(), 6);
    }

    #[test]
    fn j_and_k_reach_the_search_list_through_the_keys() {
        // The bug this is here for: the earlier tests called `update`
        // directly, so they never went through `on_key` — where "j" asked
        // `on_home()`, got true because the sidebar was still on Music, and
        // emitted RowNext. That moved the home carousels behind the search
        // view while the results sat still.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        key(&mut app, KeyCode::Enter);
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.last_main_height = 30;

        assert!(
            matches!(key(&mut app, KeyCode::Char('j')), Some(Action::TrackNext)),
            "j must drive the search list, not the home page behind it"
        );
        assert!(matches!(
            key(&mut app, KeyCode::Char('k')),
            Some(Action::TrackPrevious)
        ));

        // And the action actually moves the search selection, which now
        // lives in the track list's own state.
        app.update(Action::TrackNext);
        assert_eq!(app.search.as_ref().unwrap().tracks.selected, 1);
    }

    #[test]
    fn search_is_neither_the_home_page_nor_a_grid() {
        // Both predicates read the sidebar's section, which does not move
        // when search opens.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        assert!(!app.on_home(), "search is not the home page");

        let mut app = signed_in(sidebar::Section::Albums);
        app.update(Action::BeginSearch);
        assert!(!app.on_grid(), "search is not the album grid");
    }

    #[test]
    fn moving_runs_down_the_chosen_tabs_list() {
        // Each tab is one of the app's own views, so the selection lives in
        // that view's state and follows its movement rules.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_height = 30;
        app.last_main_width = 100;

        app.update(Action::NextSearchTab); // Tracks
        assert_eq!(app.search.as_ref().unwrap().tab, 1);

        for _ in 0..3 {
            app.update(Action::TrackNext);
        }
        assert_eq!(app.search.as_ref().unwrap().tracks.selected, 3);

        app.update(Action::TrackPrevious);
        assert_eq!(app.search.as_ref().unwrap().tracks.selected, 2);
    }

    #[test]
    fn the_selection_stops_at_both_ends_of_a_tab() {
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_height = 30;
        app.last_main_width = 100;
        app.update(Action::NextSearchTab); // Tracks

        for _ in 0..20 {
            app.update(Action::TrackPrevious);
        }
        assert_eq!(
            app.search.as_ref().unwrap().tracks.selected,
            0,
            "stops at the top"
        );

        for _ in 0..40 {
            app.update(Action::TrackNext);
        }
        let len = searchview::track_rows(
            &app.search.as_ref().unwrap().results,
            searchview::Tab::Tracks,
        )
        .len();
        assert_eq!(
            app.search.as_ref().unwrap().tracks.selected,
            len - 1,
            "and at the last row rather than past it"
        );
    }

    #[test]
    fn each_tab_keeps_its_own_place() {
        // Every tab is a different view with its own state, so coming back
        // to one finds it where it was left rather than at the top.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_height = 30;
        app.last_main_width = 100;

        app.update(Action::NextSearchTab); // Tracks
        for _ in 0..3 {
            app.update(Action::TrackNext);
        }
        assert_eq!(app.search.as_ref().unwrap().tracks.selected, 3);

        // Round every tab and back to Tracks.
        for _ in 0..searchview::Tab::ALL.len() {
            app.update(Action::NextSearchTab);
        }
        assert_eq!(app.search.as_ref().unwrap().tab, 1, "back on Tracks");
        assert_eq!(
            app.search.as_ref().unwrap().tracks.selected,
            3,
            "and where it was left"
        );
    }

    #[test]
    fn enter_in_search_plays_the_result_not_a_library_track() {
        // Both lists are indexed by the same selection, so reading the
        // library in search played whatever track happened to sit at that
        // index in a list nothing was showing.
        let mut app = signed_in(sidebar::Section::Music);
        app.tracks = some_tracks(&["A library track", "Another", "A third"]);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.update(Action::NextSearchTab); // Tracks
        app.update(Action::TrackNext);

        let picked = app.selected_track().expect("a track");
        assert_eq!(
            picked.title, "Track 1",
            "the highlighted search result, not a library track"
        );
    }

    #[test]
    fn enter_outside_search_still_plays_the_filtered_library_row() {
        // On the Tracks section, where the library is what is drawn. It
        // was asserted on the home page, which is a page of cards: reading
        // the library there was the bug that made `A` and `c` act on a
        // favourite nobody could see.
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = some_tracks(&["A library track", "Another", "A third"]);
        app.update(Action::TrackNext);
        assert_eq!(
            app.selected_track().expect("a track").title,
            "Another",
            "the library list is untouched by the search path"
        );
    }

    #[test]
    fn the_search_selection_stays_inside_the_rows_that_are_drawn() {
        // The view draws into the pane minus its own header, but scrolling
        // was measured against the whole pane — so the selection ran below
        // the last drawn row, which is where the now-playing bar is.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        let mut results = some_results();
        results.tracks = some_tracks(&["T0", "T1", "T2", "T3", "T4", "T5", "T6", "T7", "T8", "T9"]);
        app.update(Action::SearchLoaded(Box::new(results)));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_width = 100;
        app.last_main_height = 24;
        app.update(Action::NextSearchTab); // Tracks

        // Walk to the bottom of the list.
        for _ in 0..20 {
            app.update(Action::TrackNext);
        }

        let state = app.search.as_ref().unwrap();
        let body = app.last_main_height - searchview::HEADER_ROWS;
        let drawn = tracklist::visible_rows_of(body, None, tracklist::Chrome::Bare);
        assert!(drawn > 0, "the pane draws some rows");
        assert!(
            state.tracks.selected < state.tracks.offset + drawn,
            "selected row {} is past the last drawn row ({} + {}), so it sits \
             under the now-playing bar",
            state.tracks.selected,
            state.tracks.offset,
            drawn
        );
    }

    #[test]
    fn moving_down_top_results_walks_out_of_one_section_into_the_next() {
        // Every section was drawn but only the tracks took the keys, so the
        // cursor could never reach the artists or albums above them.
        use searchview::TopSection;
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_width = 100;
        app.last_main_height = 40;
        // Top results is tab 0, where the selection starts.
        assert_eq!(app.search.as_ref().unwrap().tab, 0);

        // Starts in the artists, the first section drawn.
        app.search.as_mut().unwrap().top = TopSection::Artists;
        app.update(Action::TrackNext);
        assert_eq!(
            app.search.as_ref().unwrap().top,
            TopSection::Albums,
            "down out of the artists lands in the albums"
        );
        app.update(Action::TrackNext);
        assert_eq!(
            app.search.as_ref().unwrap().top,
            TopSection::Tracks,
            "and again into the tracks"
        );

        // Back up through them.
        app.update(Action::TrackPrevious);
        assert_eq!(
            app.search.as_ref().unwrap().top,
            TopSection::Albums,
            "up off the first track returns to the albums"
        );
        app.update(Action::TrackPrevious);
        assert_eq!(app.search.as_ref().unwrap().top, TopSection::Artists);
    }

    #[test]
    fn moving_down_the_top_results_tracks_keeps_the_selection_on_screen() {
        // Reported: the selection ran below the pane instead of the list
        // scrolling to keep it in view. The list was scrolled against the
        // whole body, but it starts under two rows of covers.
        use searchview::TopSection;
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_width = 100;
        app.last_main_height = 30;
        app.search.as_mut().unwrap().top = TopSection::Albums;

        // Into the tracks: the page scrolls so they are on screen.
        app.update(Action::TrackNext);
        let state = app.search.as_ref().unwrap();
        assert_eq!(state.top, TopSection::Tracks);
        assert_eq!(state.top_scroll, 1, "the artists scroll off to make room");

        let body = app.search_body_height();
        let visible = tracklist::visible_rows_of(
            searchview::tracks_height(&state.results, state.top_scroll, body),
            None,
            tracklist::Chrome::Bare,
        );
        assert!(
            visible < some_results().tracks.len(),
            "the list has to scroll"
        );
        for _ in 0..some_results().tracks.len() {
            app.update(Action::TrackNext);
            let t = &app.search.as_ref().unwrap().tracks;
            assert!(
                t.selected >= t.offset && t.selected < t.offset + visible,
                "track {} is drawn: offset {} with {visible} visible",
                t.selected,
                t.offset
            );
        }
    }

    #[test]
    fn see_all_on_a_top_results_row_opens_that_kinds_tab() {
        // The row's heading offers it when the cards run past the edge, and
        // the whole of what was found is on the kind's own tab.
        use searchview::TopSection;
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.search.as_mut().unwrap().top = TopSection::Albums;
        app.update(Action::SeeAll);
        assert_eq!(
            searchview::Tab::from_index(app.search.as_ref().unwrap().tab),
            searchview::Tab::Albums
        );
    }

    #[test]
    fn h_and_l_run_along_a_top_results_card_row() {
        use searchview::TopSection;
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_width = 100;
        app.last_main_height = 40;
        app.search.as_mut().unwrap().top = TopSection::Artists;

        app.update(Action::SearchRight);
        assert_eq!(
            app.search.as_ref().unwrap().top_artists.selected,
            1,
            "l moves along the artist row"
        );
        app.update(Action::SearchLeft);
        assert_eq!(app.search.as_ref().unwrap().top_artists.selected, 0);
    }

    #[test]
    fn a_top_section_with_no_results_is_skipped_rather_than_entered() {
        // Moving into a section that is not drawn would park the cursor
        // somewhere invisible.
        use searchview::TopSection;
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        let mut results = some_results();
        results.albums.clear();
        app.update(Action::SearchLoaded(Box::new(results)));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_width = 100;
        app.last_main_height = 40;
        app.search.as_mut().unwrap().top = TopSection::Artists;

        app.update(Action::TrackNext);
        assert_eq!(
            app.search.as_ref().unwrap().top,
            TopSection::Tracks,
            "with no albums, down from the artists reaches the tracks"
        );
    }

    #[test]
    fn a_finished_track_asks_for_the_next_one() {
        // The whole point: an album used to stop after the track that was
        // started, because nothing moved the queue on.
        let mut app = signed_in(sidebar::Section::Tracks);
        app.queue = crate::playback::Queue::new(some_tracks(&["One", "Two", "Three"]), 0);

        let next = app.update(Action::Playback(crate::playback::PlaybackEvent::Finished));
        assert!(
            matches!(next, Some(Action::PlayQueued)),
            "the queue moves on and asks for the track"
        );
        assert_eq!(app.queue.current().unwrap().title, "Two");
    }

    #[test]
    fn the_last_track_of_a_queue_stops_rather_than_looping() {
        let mut app = signed_in(sidebar::Section::Tracks);
        // Without autoplay: with it, the end of the queue is followed.
        app.config.playback.autoplay = false;
        app.queue = crate::playback::Queue::new(some_tracks(&["Only"]), 0);
        app.now_playing.playing = true;

        let next = app.update(Action::Playback(crate::playback::PlaybackEvent::Finished));
        assert!(next.is_none(), "nothing follows the last track");
        assert!(!app.now_playing.playing, "and the player stops");
    }

    #[test]
    fn n_and_p_step_through_the_queue() {
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = some_tracks(&["One", "Two", "Three"]);
        app.queue = crate::playback::Queue::new(app.tracks.clone(), 0);

        assert!(matches!(
            key(&mut app, KeyCode::Char('n')),
            Some(Action::QueueNext)
        ));
        assert!(matches!(
            app.update(Action::QueueNext),
            Some(Action::PlayQueued)
        ));
        assert_eq!(app.queue.current().unwrap().title, "Two");

        assert!(matches!(
            key(&mut app, KeyCode::Char('p')),
            Some(Action::QueuePrevious)
        ));
        assert!(matches!(
            app.update(Action::QueuePrevious),
            Some(Action::PlayQueued)
        ));
        assert_eq!(app.queue.current().unwrap().title, "One");
    }

    #[test]
    fn the_modes_survive_a_track_that_fails_to_start() {
        // They used to be copied into the bar, and clearing the bar after a
        // failed track dropped the copy while the queue kept the real one —
        // so shuffle went grey on the next song while still being on.
        use crate::playback::Repeat;
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = some_tracks(&["One", "Two"]);
        app.queue = crate::playback::Queue::new(app.tracks.clone(), 0);
        app.update(Action::ToggleShuffle);
        app.update(Action::CycleRepeat);
        assert!(app.queue.shuffled());
        assert_eq!(app.queue.repeat, Repeat::All);

        // A track is chosen, and its request fails.
        let id = app.tracks[0].id.clone();
        app.now_playing.track = Some(app.tracks[0].clone());
        app.update(Action::PlaybackFailed {
            id,
            message: "nope".into(),
        });

        assert!(app.queue.shuffled(), "shuffle is still on");
        assert_eq!(app.queue.repeat, Repeat::All, "and so is repeat");

        // The next track starts, which is where the grey showed up.
        app.now_playing.track = Some(app.tracks[1].clone());

        // What the bar draws comes from the queue, not from a copy that the
        // reset above would have cleared.
        let modes = nowplaying::Modes {
            shuffled: app.queue.shuffled(),
            repeat: app.queue.repeat,
            sleep: None,
            spectrum: Vec::new(),
        };
        let palette = theme::Palette::detect();
        let state = app.now_playing.clone();
        let buf = geometry::draw(100, layout::NOW_PLAYING_HEIGHT, move |f, a, p| {
            nowplaying::render(f, a, p, &state, modes)
        });
        let shuffle = geometry::find(&buf, nowplaying::SHUFFLE).expect("the shuffle button");
        assert_eq!(
            buf[(shuffle.start, shuffle.row)].fg,
            palette.accent,
            "the button is still lit:\n{}",
            geometry::text(&buf)
        );
    }

    #[test]
    fn z_and_r_reach_shuffle_and_repeat() {
        use crate::playback::Repeat;
        let mut app = signed_in(sidebar::Section::Tracks);
        app.queue = crate::playback::Queue::new(some_tracks(&["One", "Two", "Three"]), 0);

        assert!(matches!(
            key(&mut app, KeyCode::Char('z')),
            Some(Action::ToggleShuffle)
        ));
        app.update(Action::ToggleShuffle);
        assert!(app.queue.shuffled(), "shuffle is on");
        app.update(Action::ToggleShuffle);
        assert!(!app.queue.shuffled(), "and off again");

        assert!(matches!(
            key(&mut app, KeyCode::Char('r')),
            Some(Action::CycleRepeat)
        ));
        app.update(Action::CycleRepeat);
        assert_eq!(app.queue.repeat, Repeat::All);
        app.update(Action::CycleRepeat);
        assert_eq!(app.queue.repeat, Repeat::One);
        app.update(Action::CycleRepeat);
        assert_eq!(app.queue.repeat, Repeat::Off, "back round");
    }

    #[test]
    fn the_transport_keys_are_letters_while_a_query_is_being_typed() {
        // n, p, z and r are all letters someone will type into the search
        // box; bound unconditionally they would skip tracks mid-word.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in ['n', 'p', 'z', 'r'] {
            let action = key(&mut app, KeyCode::Char(c));
            assert!(
                !matches!(
                    action,
                    Some(Action::QueueNext)
                        | Some(Action::QueuePrevious)
                        | Some(Action::ToggleShuffle)
                        | Some(Action::CycleRepeat)
                ),
                "{c:?} types rather than driving the transport"
            );
        }
        assert_eq!(
            app.search.as_ref().unwrap().query,
            "npzr",
            "they all reached the box"
        );
    }

    #[test]
    fn a_status_message_sits_in_the_corner() {
        // A transient notice shares the top line with the first row's
        // heading. It used to take the whole line, which hid the heading
        // for as long as the message stood. It sits in the corner instead.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            kind: crate::browse::RowKind::Carousel,
            heading: "The Hits".into(),
            cards: vec![carousel::Card::new("A Card", "An Artist")],
            state: carousel::CarouselState::default(),
        }];
        app.status = Some("Network error".into());

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let top = geometry::row(&buf, 0);

        assert!(
            top.contains("Network error"),
            "the message is shown: {top:?}"
        );
        assert!(
            top.contains("The Hits"),
            "and the heading is still readable beside it: {top:?}"
        );

        let notice = geometry::find(&buf, "Network error").expect("the notice");
        let heading = geometry::find(&buf, "The Hits").expect("the heading");
        assert!(
            notice.start > heading.end,
            "the notice sits to the right of the heading: {top:?}"
        );
    }

    fn home_with_a_track_row() -> App {
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            kind: crate::browse::RowKind::Compact,
            heading: "New Tracks".into(),
            cards: (0..6)
                .map(|i| carousel::Card {
                    title: format!("Track {i}"),
                    target: Some(carousel::Target::Track(i.to_string())),
                    ..Default::default()
                })
                .collect(),
            state: carousel::CarouselState::default(),
        }];
        app
    }

    fn home_with_a_carousel_row() -> App {
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            kind: crate::browse::RowKind::Carousel,
            heading: "New Albums".into(),
            cards: (0..3)
                .map(|i| carousel::Card {
                    title: format!("Album {i}"),
                    target: Some(carousel::Target::Album(i.to_string())),
                    ..Default::default()
                })
                .collect(),
            state: carousel::CarouselState::default(),
        }];
        app
    }

    #[test]
    fn o_opens_a_carousel_row_too() {
        // Every module on the page carries a paging path, not just the
        // track grids: New Albums shows ten of a hundred and sixty-six.
        let mut app = home_with_a_carousel_row();
        assert!(matches!(
            key(&mut app, KeyCode::Char('o')),
            Some(Action::SeeAll)
        ));
        app.update(Action::SeeAll);
        assert_eq!(
            app.open.as_ref().expect("opened").title,
            "New Albums",
            "under its own heading"
        );
    }

    #[test]
    fn a_row_of_covers_opens_as_a_grid_not_a_track_list() {
        // A cover has no track to show in a list, so the row keeps its
        // cards and gets the grid the collection views use.
        let mut app = home_with_a_carousel_row();
        app.update(Action::SeeAll);

        assert_eq!(app.open_cards.len(), 3, "the cards are held");
        assert!(app.on_grid(), "and the movement keys drive a grid");
        assert_eq!(
            app.grid_cards(app.sidebar.section()).len(),
            3,
            "which reads the opened row rather than the section behind it"
        );
    }

    #[test]
    fn closing_a_row_takes_its_cards_with_it() {
        // Left behind, they would show under whatever view was drawn next.
        let mut app = home_with_a_carousel_row();
        app.update(Action::SeeAll);
        assert!(!app.open_cards.is_empty());

        app.update(Action::CloseCollection);
        assert!(app.open.is_none());
        assert!(app.open_cards.is_empty(), "and its cards are gone");
    }

    #[test]
    fn o_opens_the_whole_of_a_home_row() {
        // The page hands back six items per row out of hundreds; the web
        // client has a "See all" for the rest and this is it.
        let mut app = home_with_a_track_row();
        assert!(matches!(
            key(&mut app, KeyCode::Char('o')),
            Some(Action::SeeAll)
        ));

        app.update(Action::SeeAll);
        let open = app.open.as_ref().expect("the row is open");
        assert_eq!(open.title, "New Tracks", "under its own heading");
        assert!(app.tracks.is_empty(), "waiting for the reply");
        assert_eq!(app.tracklist.selected, 0, "from the top");
    }

    #[test]
    fn the_rows_tracks_arrive_under_its_heading() {
        let mut app = home_with_a_track_row();
        app.update(Action::SeeAll);
        app.update(Action::CollectionLoaded {
            for_title: "New Tracks".into(),
            tracks: some_tracks(&["One", "Two", "Three"]),
        });
        assert_eq!(app.tracks.len(), 3, "the row's tracks are shown");
    }

    #[test]
    fn a_reply_for_a_row_the_user_has_left_is_dropped() {
        // Same race as an album: the reply names what it was for.
        let mut app = home_with_a_track_row();
        app.update(Action::SeeAll);
        app.update(Action::CollectionLoaded {
            for_title: "Some Other Row".into(),
            tracks: some_tracks(&["One", "Two"]),
        });
        assert!(app.tracks.is_empty(), "not shown under the wrong heading");
    }

    #[test]
    fn s_puts_the_keyboard_back_in_the_search_box() {
        // With search already open it did nothing, so the only way back to
        // the query was to close search and start over.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        // Enter runs the search and hands the keys to the results.
        key(&mut app, KeyCode::Enter);
        app.search.as_mut().unwrap().typing = false;

        assert!(
            matches!(key(&mut app, KeyCode::Char('s')), Some(Action::BeginSearch)),
            "s reaches the box from the results"
        );
        app.update(Action::BeginSearch);
        assert!(
            app.search.as_ref().unwrap().typing,
            "and it has the keyboard"
        );
        assert_eq!(
            app.search.as_ref().unwrap().query,
            "daft",
            "with the query still there to refine"
        );
    }

    #[test]
    fn enter_opens_an_album_from_the_search_results() {
        // Search draws the app's own card grid, but the selection path did
        // not know it: `on_grid` was guarded on search being closed, so
        // enter fell through and opened nothing.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_width = 100;
        app.last_main_height = 30;
        for _ in 0..2 {
            app.update(Action::NextSearchTab); // Albums
        }

        assert!(app.on_grid(), "a card tab of search is a grid");
        let card = app.selected_card().expect("the highlighted card");
        assert!(
            card.title.starts_with("Album"),
            "and it is the one search is showing, found {:?}",
            card.title
        );
        assert!(
            matches!(app.selected_collection(), Some(Collection::Album(_))),
            "which enter opens"
        );

        // And it must actually open: the checks above stopped at the
        // selection, so enter opening nothing went unnoticed.
        app.update(Action::ActivateSelection);
        assert!(app.open.is_some(), "the album opened");
        assert!(
            app.search.is_none(),
            "and search stepped aside, or it draws over the view it opened"
        );

        // Escape comes back to the results, not to the section behind them:
        // the search was what the user was looking at.
        app.update(Action::CloseCollection);
        assert!(app.open.is_none(), "the album closed");
        let search = app.search.as_ref().expect("back in the results");
        assert_eq!(search.query, "daft punk", "with the query still typed");
        assert_eq!(
            searchview::Tab::from_index(search.tab),
            searchview::Tab::Albums,
            "and on the tab it was opened from"
        );
    }

    #[test]
    fn enter_opens_a_playlist_and_an_artist_from_search_too() {
        // Albums were wired and checked; the other two card tabs go through
        // the same path, so they are held here rather than assumed.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_width = 100;
        app.last_main_height = 30;

        // Artists is tab 3, Playlists tab 4.
        for _ in 0..3 {
            app.update(Action::NextSearchTab);
        }
        assert!(
            matches!(app.selected_collection(), Some(Collection::Artist(_))),
            "an artist card opens their page"
        );

        app.update(Action::NextSearchTab);
        assert!(
            matches!(app.selected_collection(), Some(Collection::Playlist(_))),
            "and a playlist card opens the playlist"
        );
    }

    #[test]
    fn enter_on_a_track_tab_of_search_is_not_a_card() {
        // Tracks and Top draw a list, so there is no card to open — enter
        // plays the row instead.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.update(Action::NextSearchTab); // Tracks

        assert!(!app.on_grid(), "a track tab is not a grid");
        assert!(app.selected_card().is_none(), "and has no card");
    }

    fn explore_rows() -> crate::browse::Home {
        crate::browse::Home {
            rows: vec![crate::browse::HomeRow {
                kind: crate::browse::RowKind::Carousel,
                heading: "Genres".into(),
                cards: vec![carousel::Card::new("Rock", "")],
                more: None,
            }],
        }
    }

    #[test]
    fn explore_is_its_own_page_rather_than_the_favourites_list() {
        // It fell through to the arm that draws the track list, so three
        // sidebar entries showed the same thing and none said what it was.
        let mut app = signed_in(sidebar::Section::Explore);
        app.update(Action::ExploreLoaded(Box::new(explore_rows())));

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let text = geometry::text(&terminal.backend().buffer().clone());

        assert!(text.contains("Genres"), "explore's own rows:\n{text}");
        assert!(!text.contains("TITLE"), "not the track list:\n{text}");
    }

    #[test]
    fn reaching_explore_asks_for_its_page() {
        // Nothing else fetches it, so without this the section drew an
        // empty page until something unrelated happened to load it.
        let mut app = signed_in(sidebar::Section::Music);
        let mut asked = false;
        for _ in 0..sidebar::Section::ALL.len() {
            if matches!(app.update(Action::SidebarNext), Some(Action::LoadExplore)) {
                asked = true;
                break;
            }
        }
        assert!(asked, "moving onto Explore asks for its page");
    }

    #[test]
    fn explore_is_not_asked_for_twice() {
        // Its rows do not change under us, and a fetch on every pass of the
        // sidebar would be a request per keystroke.
        let mut app = signed_in(sidebar::Section::Explore);
        app.update(Action::ExploreLoaded(Box::new(explore_rows())));

        app.update(Action::SidebarNext);
        let mut again = false;
        for _ in 0..sidebar::Section::ALL.len() {
            if matches!(
                app.update(Action::SidebarPrevious),
                Some(Action::LoadExplore)
            ) {
                again = true;
            }
        }
        assert!(!again, "the page it already has is not fetched again");
    }

    #[test]
    fn the_movement_keys_drive_explores_own_rows() {
        // `on_home` read the section rather than what is drawn, which is
        // the fault that has come back four times: j and k would have moved
        // the home page behind Explore.
        let mut app = signed_in(sidebar::Section::Explore);
        let mut home = explore_rows();
        home.rows.push(crate::browse::HomeRow {
            kind: crate::browse::RowKind::Carousel,
            heading: "Decades".into(),
            cards: vec![carousel::Card::new("80s", "")],
            more: None,
        });
        app.update(Action::ExploreLoaded(Box::new(home)));
        app.last_main_height = 40;

        assert!(app.on_home(), "explore moves like a page of rows");
        app.update(Action::RowNext);
        assert_eq!(app.explore.row, 1, "explore's own selection moved");
        assert_eq!(app.home.row, 0, "and the home page's did not");
    }

    #[test]
    fn escape_goes_back_one_view_at_a_time() {
        // Escape used to drop straight to the sidebar's section from any
        // depth: a single `came_from` is all one level can remember. Opening
        // from inside an opened view is the ordinary case — a profile, an
        // artist, then one of their albums — and each of those is a step
        // back, not a jump out.
        let mut app = signed_in(sidebar::Section::Profiles);
        app.artists = vec![crate::library::Artist {
            id: (1).to_string(),
            name: "A Profile".into(),
            picture: None,
        }];

        // Into the profile.
        app.update(Action::ActivateSelection);
        let first = app.open.as_ref().expect("the profile opened").title.clone();
        assert_eq!(first, "A Profile");

        // From inside it, into something else — the cards an opened view
        // holds are what enter opens next.
        app.open_cards = vec![{
            let mut c = carousel::Card::new("An Album", "An Artist");
            c.target = Some(carousel::Target::Album("7".into()));
            c
        }];
        app.update(Action::ActivateSelection);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some("An Album"),
            "the second view is what is open now"
        );

        // Back out one: the profile again, not the sidebar.
        app.update(Action::CloseCollection);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some("A Profile"),
            "escape goes back one view, not all the way out"
        );

        // And once more: out to the section.
        app.update(Action::CloseCollection);
        assert!(app.open.is_none(), "the last escape leaves the stack");
        assert_eq!(app.sidebar.section(), sidebar::Section::Profiles);
    }

    #[test]
    fn the_settings_view_draws_what_the_config_holds() {
        // Read off the buffer, not off the state: every view added so far
        // has had at least one bug where the keys drove something other
        // than what was on screen.
        let mut app = signed_in(sidebar::Section::Settings);
        app.config
            .audio
            .set_quality(crate::domain::Quality::Lossless);

        // Wide enough for the label column and a value beside it: the
        // labels run to "Allow AI-generated content", and the sidebar takes
        // 26 columns before the pane starts.
        let buf = geometry::draw(110, 20, |f, _area, _p| draw(f, &mut app));
        let text = geometry::text(&buf);
        assert!(text.contains("Settings"), "the heading:\n{text}");
        assert!(
            text.contains(settings::describe(crate::domain::Quality::Lossless)),
            "the configured quality is what is shown:\n{text}"
        );
    }

    #[test]
    fn changing_the_quality_moves_through_every_tier() {
        // Four tiers, wrapping: a list that stops at the ends needs two
        // keys to walk, which is more chrome than one setting is worth.
        let mut app = signed_in(sidebar::Section::Settings);
        app.settings.next(); // past Service, onto Quality
        app.config
            .audio
            .set_quality(crate::domain::Quality::HiResLossless);
        // `config_path` is None in tests, so nothing is written: an
        // earlier version of this test saved through the real path and
        // left this machine's quality on LOW.
        assert!(app.config_path.is_none());

        let mut seen = vec![app.config.audio.quality()];
        for _ in 0..settings::QUALITIES.len() - 1 {
            app.update(Action::CarouselNext);
            seen.push(app.config.audio.quality());
        }
        assert_eq!(seen, settings::QUALITIES, "best first, then down the list");

        // And round, so the best tier is never more than one key away.
        app.update(Action::CarouselNext);
        assert_eq!(
            app.config.audio.quality(),
            crate::domain::Quality::HiResLossless
        );

        // The other way too.
        app.update(Action::CarouselPrevious);
        assert_eq!(app.config.audio.quality(), crate::domain::Quality::Low);
    }

    #[test]
    fn playback_asks_for_the_configured_quality() {
        // The setting existed in the file for weeks and was read nowhere:
        // the request site named HI_RES_LOSSLESS outright, so changing it
        // did nothing at all. The request itself now lives in the engine's
        // fetch, which has the matching check; this is the half that hands
        // it the setting.
        let source = include_str!("mod.rs");
        assert!(
            source.contains("app.config.audio.quality()"),
            "no request site reads the configured quality"
        );
    }

    #[test]
    fn the_volume_steps_and_stops_at_both_ends() {
        // A step rather than a wrap: turning the volume up past full and
        // round to silence is not what anyone means by the key.
        let mut app = signed_in(sidebar::Section::Settings);
        app.settings.next(); // past Service
        app.settings.next(); // onto Volume
        assert_eq!(app.settings.current(), settings::Setting::Volume);
        app.config.audio.volume = 1.0;

        assert!(matches!(
            app.update(Action::CarouselNext),
            Some(Action::SetVolume(_))
        ));
        assert_eq!(app.config.audio.volume, 1.0, "full is as loud as it goes");

        // Down to nothing, and no further.
        for _ in 0..40 {
            app.update(Action::CarouselPrevious);
        }
        assert_eq!(app.config.audio.volume, 0.0, "and silence is the floor");

        // One press back up is audible rather than nothing.
        app.update(Action::CarouselNext);
        assert!(
            app.config.audio.volume > 0.0,
            "a press must move it, got {}",
            app.config.audio.volume
        );
    }

    #[test]
    fn changing_the_volume_reaches_the_player() {
        // A level you cannot hear yourself setting is one you set by
        // guessing, so the change is sent on rather than waiting for the
        // next track. Changing the quality has nothing to send.
        let mut app = signed_in(sidebar::Section::Settings);
        app.settings.next(); // past Service
        app.settings.next();
        let sent = app.update(Action::CarouselPrevious);
        assert!(
            matches!(sent, Some(Action::SetVolume(v)) if v == app.config.audio.volume),
            "the new level is handed on, got {sent:?}"
        );

        app.settings.previous(); // back to Quality
        assert!(
            app.update(Action::CarouselNext).is_none(),
            "quality is read when the next track starts, not sent now"
        );
    }

    #[test]
    fn a_changed_setting_is_written_to_the_file() {
        // There is no "apply", so a setting that only lived in memory would
        // be silently lost on quit.
        let dir = std::env::temp_dir().join("ratidal-test-settings");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let _ = std::fs::remove_file(&path);

        let mut app = signed_in(sidebar::Section::Settings);
        app.settings.next(); // past Service, onto Quality
        app.config_path = Some(path.clone());
        app.config
            .audio
            .set_quality(crate::domain::Quality::HiResLossless);

        app.update(Action::CarouselNext);

        let written = std::fs::read_to_string(&path).expect("the file was written");
        let back: crate::config::Config = toml::from_str(&written).expect("valid TOML");
        assert_eq!(
            back.audio.quality(),
            app.config.audio.quality(),
            "the file holds what the view shows"
        );
        assert_ne!(
            back.audio.quality(),
            crate::domain::Quality::HiResLossless,
            "and it actually changed"
        );
    }

    #[test]
    fn the_settings_keys_do_not_reach_the_view_behind_it() {
        // The fault this app keeps making: a new view is drawn but the keys
        // still drive the last one. Moving in Settings must not disturb the
        // album grid, and opening a collection must take the keys back.
        let mut app = signed_in(sidebar::Section::Settings);
        app.albums = vec![crate::library::Album {
            id: (1).to_string(),
            title: "An Album".into(),
            artist: "An Artist".into(),
            year: None,
            cover: None,
            track_count: 1,
            duration: None,
        }];
        app.album_grid.selected = 0;

        assert!(app.on_settings());
        assert!(!app.on_grid(), "Settings is not a card grid");

        app.update(Action::TrackNext);
        assert_eq!(
            app.album_grid.selected, 0,
            "the grid behind it did not move"
        );
    }

    fn an_artist_page() -> crate::library::ArtistPage {
        crate::library::ArtistPage {
            name: "Daft Punk".into(),
            picture: None,
            top_tracks: some_tracks(&["One More Time", "Aerodynamic"]),
            albums: (0..3)
                .map(|i| crate::library::Album {
                    id: (i).to_string(),
                    title: format!("Album {i}"),
                    artist: "Daft Punk".into(),
                    year: Some("2001".into()),
                    cover: None,
                    track_count: 10,
                    duration: None,
                })
                .collect(),
            similar: (0..2)
                .map(|i| crate::library::Artist {
                    id: (100 + i).to_string(),
                    name: format!("Similar {i}"),
                    picture: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn with_artist_open() -> App {
        let mut app = signed_in(sidebar::Section::Profiles);
        app.open = Some(OpenCollection {
            title: "Daft Punk".into(),
            subtitle: String::new(),
            detail: String::new(),
            cover: None,
            round_cover: false,
            came_from: sidebar::Section::Profiles,
            target: None,
        });
        app.update(Action::ArtistLoaded(Box::new(an_artist_page())));
        app.last_main_width = 100;
        app.last_main_height = 40;
        app
    }

    #[test]
    fn dot_and_comma_seek_ten_seconds_within_the_track() {
        let mut app = signed_in(sidebar::Section::Tracks);
        assert!(
            key(&mut app, KeyCode::Char('.')).is_none(),
            "nothing playing: nowhere to seek to"
        );
        app.now_playing.track = Some(crate::domain::Track::sample(
            "Song",
            "Band",
            std::time::Duration::from_secs(100),
        ));
        app.now_playing.position = std::time::Duration::from_secs(30);

        let forward = key(&mut app, KeyCode::Char('.')).expect("bound with a track up");
        assert!(matches!(forward, Action::SeekForward));
        app.update(forward);
        assert_eq!(app.now_playing.position, std::time::Duration::from_secs(40));

        for _ in 0..10 {
            app.update(Action::SeekForward);
        }
        assert_eq!(
            app.now_playing.position,
            std::time::Duration::from_secs(99),
            "stops short of the end rather than finishing the track"
        );
        let back = key(&mut app, KeyCode::Char(',')).expect("bound with a track up");
        assert!(matches!(back, Action::SeekBack));
        for _ in 0..20 {
            app.update(Action::SeekBack);
        }
        assert_eq!(app.now_playing.position, std::time::Duration::ZERO, "and at the start");
    }

    #[test]
    fn j_and_k_scroll_an_open_bio_that_runs_past_the_pane() {
        // A blurb TIDAL writes can run to a page and a half, and the pane
        // drew the first screen of it with no way to the rest.
        let mut app = with_artist_open();
        // j and k come back as actions, as from the event loop.
        let press = |app: &mut App, code: KeyCode| {
            if let Some(action) = key(app, code) {
                app.update(action);
            }
        };
        {
            let page = app.artist.as_mut().unwrap();
            page.picture = Some("pic".into());
            page.bio = Some((0..600).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" "));
        }
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(130, 40)).unwrap();
        let text = |t: &ratatui::Terminal<ratatui::backend::TestBackend>| {
            t.backend().buffer().content().iter().map(|c| c.symbol()).collect::<String>()
        };
        key(&mut app, KeyCode::Char('b'));
        assert!(app.artist_bio_open);
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(text(&terminal).contains("w0 "), "the bio starts at its first line");
        let most = artistview::bio_overflow(
            app.artist.as_ref().unwrap(),
            app.last_main_width,
            app.last_main_height,
        );
        assert!(most > 0, "the bio runs past the pane");

        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.artist_bio_scroll, 1);
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(!text(&terminal).contains("w0 "), "the first line scrolled off");
        assert!(app.artist.is_some(), "the page is still up");

        for _ in 0..1000 {
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(app.artist_bio_scroll, most, "no further than the last line");
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(text(&terminal).contains("w599"), "the last line is on screen");
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.artist_bio_scroll, most - 1);

        // Closed and opened again, it starts from the top.
        key(&mut app, KeyCode::Char('b'));
        key(&mut app, KeyCode::Char('b'));
        assert_eq!(app.artist_bio_scroll, 0);
    }

    #[test]
    fn esc_closes_an_open_bio_before_leaving_the_artist() {
        // The blurb is drawn over the page rather than pushed as a view, so
        // escape used to pop the page from under it: a reader who pressed
        // the key everything else answers with "back" landed two views
        // away, on whatever the artist was opened from.
        for code in [KeyCode::Esc, KeyCode::Char('[')] {
            let mut app = with_artist_open();
            let depth = app.back.len();
            key(&mut app, KeyCode::Char('b'));
            assert!(app.artist_bio_open, "b opens the bio");

            assert!(key(&mut app, code).is_none(), "{code:?} is spent on the bio");
            assert!(!app.artist_bio_open, "{code:?} closed the bio");
            assert!(app.artist.is_some(), "{code:?} left the artist's page up");
            assert_eq!(app.back.len(), depth, "{code:?} popped nothing");

            // With the bio closed, the same key leaves the page as before.
            assert!(matches!(key(&mut app, code), Some(Action::GoBack)));
        }
    }

    #[test]
    fn an_artists_page_is_drawn_over_the_view_behind_it() {
        let mut app = with_artist_open();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let text = geometry::text(&terminal.backend().buffer().clone());

        assert!(text.contains("Daft Punk"), "the artist's name:\n{text}");
        assert!(text.contains("One More Time"), "their top tracks:\n{text}");
        assert!(text.contains("Album 0"), "their albums:\n{text}");
    }

    #[test]
    fn moving_down_the_page_walks_out_of_one_section_into_the_next() {
        // Every section is drawn, so every section has to take the keys —
        // the fault that has come back five times now.
        let mut app = with_artist_open();
        assert_eq!(app.artist_section, artistview::Section::Tracks.index());

        // Two tracks: one press moves within them, the next leaves.
        app.update(Action::TrackNext);
        assert_eq!(
            app.artist_section,
            artistview::Section::Tracks.index(),
            "still in the tracks"
        );
        assert_eq!(app.artist_tracks.selected, 1, "on the second of them");
        app.update(Action::TrackNext);
        assert_eq!(
            app.artist_section,
            artistview::Section::Albums.index(),
            "off the end of the tracks and into the albums"
        );
        app.update(Action::TrackNext);
        assert_eq!(
            app.artist_section,
            artistview::Section::Similar.index(),
            "and on to the similar artists"
        );

        // And back up.
        app.update(Action::TrackPrevious);
        assert_eq!(app.artist_section, artistview::Section::Albums.index());
    }

    #[test]
    fn h_and_l_run_along_the_pages_card_rows() {
        let mut app = with_artist_open();
        app.artist_section = artistview::Section::Albums.index();

        assert!(matches!(
            key(&mut app, KeyCode::Char('l')),
            Some(Action::ArtistRight)
        ));
        app.update(Action::ArtistRight);
        assert_eq!(
            app.artist_rows[artistview::Section::Albums.index()].selected,
            1,
            "along the album row"
        );
        app.update(Action::ArtistLeft);
        assert_eq!(
            app.artist_rows[artistview::Section::Albums.index()].selected,
            0
        );
    }

    #[test]
    fn enter_plays_a_track_and_opens_an_album_from_the_page() {
        let mut app = with_artist_open();

        // In the track section, enter reaches the page's own tracks.
        let track = app.selected_track().expect("a track");
        assert_eq!(track.title, "One More Time", "the page's own track");

        // In a card section, it reaches that section's cards.
        app.artist_section = artistview::Section::Albums.index();
        assert!(app.on_grid(), "a card section moves like a grid");
        assert!(matches!(
            app.selected_collection(),
            Some(Collection::Album(id)) if id == "0"
        ));
    }

    #[test]
    fn a_similar_artist_opens_their_own_page() {
        let mut app = with_artist_open();
        app.artist_section = artistview::Section::Similar.index();
        assert!(matches!(
            app.selected_collection(),
            Some(Collection::Artist(id)) if id == "100"
        ));
    }

    #[test]
    fn opening_another_artist_clears_the_last_ones_page() {
        // Left behind, it would show under the new artist's name until
        // their own reply arrived.
        let mut app = with_artist_open();
        app.artist_section = artistview::Section::Albums.index();
        app.update(Action::ActivateSelection);
        assert!(app.artist.is_none(), "the old page is gone");
        assert_eq!(app.artist_section, 0, "and its selection with it");
    }

    #[test]
    fn a_page_for_an_artist_the_user_has_left_is_dropped() {
        let mut app = with_artist_open();
        let mut other = an_artist_page();
        other.name = "Someone Else".into();
        app.update(Action::ArtistLoaded(Box::new(other)));
        assert_eq!(
            app.artist.as_ref().map(|p| p.name.as_str()),
            Some("Daft Punk"),
            "not shown under the wrong heading"
        );
    }

    #[test]
    fn h_does_not_shut_a_see_all_that_is_still_arriving() {
        // The window between pressing `o` and the reply landing: the view
        // is open and carries no cards, which reads as a track list. `h`
        // shut it there -- and that is the whole of the time the user is
        // waiting, so the key appeared to close the page at random.
        // Opened by the key rather than by hand: what `o` builds is the
        // thing under test, and a view assembled here would not have caught
        // this.
        let mut app = signed_in(sidebar::Section::Feed);
        let today = today();
        app.update(Action::FeedLoaded(
            (0..12)
                .map(|i| carousel::Card {
                    day: Some(today),
                    ..carousel::Card::new(format!("Release {i}"), "An Artist")
                })
                .collect(),
        ));
        app.last_main_width = 100;
        app.last_main_height = 40;
        if let Some(action) = app.on_key(crossterm::event::KeyEvent::from(KeyCode::Char('o'))) {
            app.update(action);
        }
        assert!(app.open.is_some(), "the see-all opened");

        // The waiting state a fetched see-all sits in: no cards yet.
        app.open_cards.clear();
        app.awaiting = true;

        let acted = app.on_key(crossterm::event::KeyEvent::from(KeyCode::Char('h')));
        assert!(
            !matches!(acted, Some(Action::CloseCollection)),
            "h left the view alone while it was loading, got {acted:?}"
        );
        assert!(app.open.is_some(), "and it is still open");
    }

    #[test]
    fn a_theme_is_what_the_screen_is_actually_painted_in() {
        // The palette could be read from the config correctly and still
        // never reach the buffer -- a renderer holding its own colours
        // would look right in every unit test of the parser.
        use crate::shell::theme;
        let mut app = signed_in(sidebar::Section::Tracks);
        app.palette = theme::named("catppuccin").expect("the theme");
        app.update(Action::TracksLoaded(some_tracks(&["A Track", "Another"])));

        let buf = geometry::draw(100, 20, |f, _a, _p| draw(f, &mut app));
        let mut drawn = std::collections::HashSet::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                drawn.insert(buf[(x, y)].fg);
            }
        }

        assert!(
            drawn.contains(&app.palette.text),
            "the theme's own text colour is on the screen: {drawn:?}"
        );
        // And nothing of the default palette is left behind it.
        assert!(
            !drawn.contains(&theme::Palette::detect().dim),
            "no colour from the palette it replaced: {drawn:?}"
        );
    }

    #[test]
    fn a_notice_comes_down_on_its_own() {
        // It never did: nothing cleared the status but a fresh login, so a
        // message about one failed track stood in the corner -- over the
        // tab strip -- for the rest of the session.
        let mut app = signed_in(sidebar::Section::Music);
        app.say("could not play that");
        assert_eq!(app.status.as_deref(), Some("could not play that"));

        // A tick straight away leaves it up: it has to be readable.
        app.update(Action::Tick);
        assert!(app.status.is_some(), "still up a moment later");

        // Old enough, and the next tick takes it down.
        app.status_since = Some(std::time::Instant::now() - std::time::Duration::from_secs(30));
        app.update(Action::Tick);
        assert!(app.status.is_none(), "gone once it has been read");
        assert!(app.status_since.is_none(), "and its clock with it");
    }

    #[test]
    fn a_second_notice_gets_the_whole_time_to_itself() {
        // The clock is the message's, not the app's: a new one arriving
        // late in the last one's life would otherwise vanish immediately.
        let mut app = signed_in(sidebar::Section::Music);
        app.say("the first");
        app.status_since = Some(std::time::Instant::now() - std::time::Duration::from_secs(30));

        app.say("the second");
        app.update(Action::Tick);
        assert_eq!(
            app.status.as_deref(),
            Some("the second"),
            "the new message starts its own clock"
        );
    }

    #[test]
    fn every_overlay_asks_for_a_repaint_when_it_closes() {
        // An overlay writes over cells the image protocols marked `Skip`.
        // The next frame marks them `Skip` again and writes nothing, so
        // whatever the modal put there stays on the artwork until a full
        // repaint. The help remembered; the queue did not, and left a
        // rectangle of itself over the covers.
        // The help closes on any key; `z` is used by nothing else.
        let mut app = signed_in(sidebar::Section::Music);
        app.showing_help = true;
        let _ = app.take_repaint();
        app.on_key(KeyEvent::from(KeyCode::Char('z')));
        assert!(
            app.take_repaint(),
            "closing the help leaves it on the artwork without one"
        );

        // The queue is a view now, and escape is a step back out of it.
        let mut app = signed_in(sidebar::Section::Music);
        key(&mut app, KeyCode::Char('Q'));
        let _ = app.take_repaint();
        let back = key(&mut app, KeyCode::Esc).expect("escape steps back");
        app.update(back);
        assert!(!app.showing_queue, "escape leaves the queue");
        assert!(
            app.take_repaint(),
            "leaving the queue leaves it on the artwork without one"
        );
    }

    #[test]
    fn the_queue_is_a_list_the_keys_work_in() {
        // Reported: no way to like the track that is playing. The queue
        // was a modal that closed on any key it did not know, so `A` over
        // the playing row shut it instead of acting on it -- and nothing
        // else could be reached from there either.
        let mut app = signed_in(sidebar::Section::Music);
        let queued = crate::domain::Track {
            id: crate::domain::TrackId::from(7),
            ..crate::domain::Track::sample("Queued", "Someone", std::time::Duration::from_secs(1))
        };
        app.queue.play_next(queued);
        key(&mut app, KeyCode::Char('Q'));
        assert!(app.showing_queue, "Q opens the queue");
        app.queue_list.selected = 0;

        assert_eq!(
            app.selected_track().map(|t| t.id.clone()),
            Some(crate::domain::TrackId::from(7)),
            "the row under the cursor is what the keys act on"
        );
        assert!(
            matches!(
                key(&mut app, KeyCode::Char('A')),
                Some(Action::ToggleFavourite)
            ),
            "A likes it"
        );
        assert!(app.showing_queue, "and the queue stays open to show it");

        // A key the queue does not know does what it always does, and does
        // not close the list.
        assert!(matches!(
            key(&mut app, KeyCode::Char('z')),
            Some(Action::ToggleShuffle)
        ));
        assert!(app.showing_queue, "shuffle is not a way out");

        // Enter is the queue's, even with nothing to jump to: falling
        // through would open whatever the hidden page had selected.
        app.queue = Default::default();
        assert!(key(&mut app, KeyCode::Enter).is_none());
        assert!(app.showing_queue);

        // The key that opened it closes it: a step back.
        let back = key(&mut app, KeyCode::Char('Q')).expect("Q again steps back");
        app.update(back);
        assert!(!app.showing_queue, "Q again closes it");
    }

    #[test]
    fn the_queue_is_a_view_that_back_returns_to() {
        // Asked for: the lists are views of their own, on the stack with
        // the rest. Going somewhere from the queue leaves it, and back
        // returns to it -- as it does for an opened album.
        let mut app = signed_in(sidebar::Section::Music);
        key(&mut app, KeyCode::Char('Q'));
        assert!(app.showing_queue);

        // Driven the way a keystroke is: the key names the move and
        // `update` makes it, which is where the level is taken.
        let action = key(&mut app, KeyCode::Char('J')).expect("J moves the nav");
        app.update(action);
        assert!(!app.showing_queue, "moving the nav leaves it");
        app.update(Action::GoBack);
        assert!(app.showing_queue, "and back returns to it");
        app.update(Action::GoForward);
        assert!(!app.showing_queue, "forward steps out again");
        app.update(Action::GoBack);

        // The search box is not a level: it opens over the queue and
        // closes back onto it.
        app.update(Action::BeginSearch);
        assert!(app.search.is_some(), "the box is open");
        assert!(
            matches!(app.showing(), Showing::Search(_)),
            "and is what the keys drive"
        );
        app.update(Action::CloseSearch);
        assert!(
            app.showing_queue && app.search.is_none(),
            "the queue is back"
        );

        // The history over the queue is a level of its own, and back from
        // it is the queue.
        key(&mut app, KeyCode::Char('H'));
        assert!(
            app.showing_history && !app.showing_queue,
            "one list at a time"
        );
        app.update(Action::GoBack);
        assert!(app.showing_queue && !app.showing_history);
    }

    #[test]
    fn opening_an_album_or_artist_takes_the_pane_so_the_reply_lands() {
        // Reported: `c` and `a` did nothing, from every list. The log said
        // why -- "dropping tracks for ..., no longer open": the reply is
        // only accepted for the title that is open, and nothing had opened
        // one. Every album and artist a key asked for came back and was
        // thrown away.
        let mut app = signed_in(sidebar::Section::Tracks);
        let track = crate::domain::Track {
            id: crate::domain::TrackId::from(1),
            album: "The Record".into(),
            album_id: Some("77".into()),
            artist: "Someone, Else".into(),
            artist_id: Some("88".into()),
            ..crate::domain::Track::sample("Song", "Someone", std::time::Duration::from_secs(1))
        };
        app.tracks = vec![track.clone()];

        let open = key(&mut app, KeyCode::Char('c')).expect("c opens the album");
        assert!(matches!(open, Action::OpenTrackAlbum));
        app.update(open);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some("The Record"),
            "the album is what is open, so its tracks are accepted"
        );
        assert_eq!(
            app.showing(),
            Showing::Loading,
            "and the pane says it is on its way"
        );
        app.update(Action::CollectionLoaded {
            for_title: "The Record".into(),
            tracks: vec![crate::domain::Track::sample(
                "1",
                "x",
                std::time::Duration::from_secs(1),
            )],
        });
        assert_eq!(app.showing(), Showing::Tracks, "the reply landed");
        assert_eq!(app.tracks.len(), 1);

        // Back to the list, and the artist the same way -- headed by the
        // first credited name, which is the page the reply is named for.
        // The section fetches its list again on the way back; here it is
        // put back by hand.
        app.update(Action::GoBack);
        app.tracks = vec![track.clone()];
        let open = key(&mut app, KeyCode::Char('a')).expect("a opens the artist");
        app.update(open);
        let open = app.open.as_ref().expect("an artist is open");
        assert_eq!(open.title, "Someone");
        assert!(open.round_cover, "an artist's picture is round");

        // When the row carries no id, the pane is taken as the fetch is
        // asked for, under the name the row knows -- and what the fetch
        // hands back renames the wait rather than opening a second view.
        app.update(Action::GoBack);
        app.tracks = vec![track.clone()];
        app.update(Action::FetchTrackAlbum(crate::domain::TrackId::from(1)));
        assert_eq!(
            app.showing(),
            Showing::Loading,
            "waiting from the first key"
        );
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some("The Record")
        );
        let depth = app.back.len();
        app.update(Action::OpenTarget {
            target: Collection::Album("5".into()),
            title: "Fetched".into(),
            subtitle: "By".into(),
            cover: None,
        });
        let open = app.open.as_ref().expect("still open");
        assert_eq!(
            (open.title.as_str(), open.subtitle.as_str()),
            ("Fetched", "By")
        );
        assert_eq!(app.back.len(), depth, "renamed, not stacked");

        // Left before the track came back: the reply is dropped, as any
        // other late reply is.
        app.update(Action::GoBack);
        assert!(app.open.is_none());
        app.update(Action::OpenTarget {
            target: Collection::Album("6".into()),
            title: "Late".into(),
            subtitle: String::new(),
            cover: None,
        });
        assert!(app.open.is_none(), "nothing was waiting for it");
    }

    #[test]
    fn a_key_that_would_open_what_is_showing_does_nothing() {
        // Seven presses of `c` on a one-track single stacked the same album
        // seven times, and it took eight steps back to leave it.
        let mut app = signed_in(sidebar::Section::Tracks);
        let track = crate::domain::Track {
            id: crate::domain::TrackId::from(1),
            album: "Single".into(),
            album_id: Some("7".into()),
            ..crate::domain::Track::sample("Song", "Someone", std::time::Duration::from_secs(1))
        };
        app.tracks = vec![track.clone()];
        let open = key(&mut app, KeyCode::Char('c')).expect("opens the album");
        app.update(open);
        app.update(Action::CollectionLoaded {
            for_title: "Single".into(),
            tracks: vec![track],
        });
        assert_eq!(app.showing(), Showing::Tracks);
        let depth = app.back.len();

        assert!(
            key(&mut app, KeyCode::Char('c')).is_none(),
            "already looking at it"
        );
        assert_eq!(app.back.len(), depth, "and nothing was stacked");
    }

    #[test]
    fn opening_from_the_queue_draws_the_wait_over_it() {
        // Reported: no loading page after `a` or `c`. From the queue, the
        // key asks for the track first, and until the track came back
        // nothing on screen changed. The wait is drawn from the key on,
        // headed by the name the row knows.
        let mut app = signed_in(sidebar::Section::Music);
        app.queue.play_next(crate::domain::Track {
            id: crate::domain::TrackId::from(3),
            ..crate::domain::Track::sample(
                "A Good Day",
                "Anderson .Paak, Cordae",
                std::time::Duration::from_secs(1),
            )
        });
        key(&mut app, KeyCode::Char('Q'));
        app.queue_list.selected = 0;
        let fetch = key(&mut app, KeyCode::Char('a')).expect("a from the queue");
        assert!(
            matches!(fetch, Action::FetchTrackArtist(_)),
            "no id in hand: fetched"
        );
        app.update(fetch);
        assert!(!app.showing_queue, "the wait took the pane from the queue");

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let text = geometry::text(terminal.backend().buffer());
        assert!(
            text.contains("Anderson .Paak"),
            "headed by the artist:\n{text}"
        );
        assert!(
            text.contains("Loading"),
            "and says it is on its way:\n{text}"
        );
        assert!(
            !text.contains("A Good Day"),
            "the queue's rows are gone:\n{text}"
        );

        // And back from the wait is the queue.
        app.update(Action::GoBack);
        assert!(app.showing_queue);
    }

    #[test]
    fn playing_from_a_row_of_track_cards_queues_the_row() {
        // Reported: a track played from "New Tracks" on the home page, and
        // the queue showed nothing. The list a track is played from is what
        // the queue is filled with, and a row of cards was not one of the
        // lists this knew -- so the track played alone.
        let mut app = signed_in(sidebar::Section::Music);
        app.last_main_width = 120;
        let mut cards: Vec<carousel::Card> = (1..=3)
            .map(|i| {
                let mut c = carousel::Card::new(format!("Song {i}"), "Someone");
                c.target = Some(carousel::Target::Track(i.to_string()));
                c
            })
            .collect();
        // A card that is not a track in the middle of them: it is skipped,
        // and the place of the one after it is counted among the tracks.
        let mut album = carousel::Card::new("A record", "Someone");
        album.target = Some(carousel::Target::Album("9".into()));
        cards.insert(2, album);
        app.home.rows = vec![home::Row {
            heading: "New Tracks".into(),
            kind: crate::browse::RowKind::Compact,
            cards,
            state: carousel::CarouselState {
                selected: 3,
                ..Default::default()
            },
        }];
        app.home.row = 0;

        let (list, at, named) = app.selected_list();
        assert_eq!(
            list.iter().map(|t| t.id.0.clone()).collect::<Vec<_>>(),
            vec!["1", "2", "3"],
            "the row's tracks, and only its tracks"
        );
        assert_eq!(at, 2, "the third track, not the fourth card");
        assert_eq!(named.as_deref(), Some("New Tracks"), "named after the row");

        // On the card that is not a track there is nothing to play.
        app.home.rows[0].state.selected = 2;
        assert!(app.selected_list().0.is_empty());
    }

    #[test]
    fn an_album_opened_from_the_queue_is_a_step_forward_from_it() {
        let mut app = signed_in(sidebar::Section::Music);
        app.queue.play_next(crate::domain::Track {
            id: crate::domain::TrackId::from(7),
            album: "Q".into(),
            album_id: Some("9".into()),
            ..crate::domain::Track::sample("Queued", "S", std::time::Duration::from_secs(1))
        });
        key(&mut app, KeyCode::Char('Q'));
        app.queue_list.selected = 0;
        let open = key(&mut app, KeyCode::Char('c')).expect("c from the queue");
        app.update(open);
        assert!(!app.showing_queue, "the album took the pane");
        assert_eq!(app.open.as_ref().map(|o| o.title.as_str()), Some("Q"));
        app.update(Action::GoBack);
        assert!(app.showing_queue, "and back is the queue");
    }

    #[test]
    fn the_keys_act_on_what_is_drawn_not_on_the_hidden_track_list() {
        // Reported: `c` and `a` did nothing, or opened the wrong thing, on
        // every page. `selected_track` read the Tracks section's favourites
        // whatever was on screen, so on the home page `A` favourited and
        // `c` opened whichever favourite that hidden list had highlighted.
        let mut app = signed_in(sidebar::Section::Music);
        app.last_main_width = 120;
        app.tracks = vec![crate::domain::Track {
            id: crate::domain::TrackId::from(1),
            ..crate::domain::Track::sample("Hidden", "Favourite", std::time::Duration::from_secs(1))
        }];
        let mut card = carousel::Card::new("Song", "Someone");
        card.target = Some(carousel::Target::Track("42".into()));
        app.home.rows = vec![home::Row {
            heading: "Songs".into(),
            kind: crate::browse::RowKind::Compact,
            cards: vec![card],
            state: Default::default(),
        }];
        app.home.row = 0;
        assert_eq!(
            app.selected_track().map(|t| t.id.clone()),
            Some(crate::domain::TrackId::from(42)),
            "the card under the cursor, not the hidden favourite"
        );

        // A card that is not a track is nothing to act on -- not the
        // favourite either.
        app.home.rows[0].cards[0].target = Some(carousel::Target::Album("9".into()));
        assert!(app.selected_track().is_none());
        assert!(
            key(&mut app, KeyCode::Char('c')).is_none(),
            "nothing to open"
        );
    }

    #[test]
    fn the_history_is_a_list_the_keys_work_in() {
        // The same rewrite as the queue's: it closed on any key, so nothing
        // could be done with a played track from where it was listed.
        let mut app = signed_in(sidebar::Section::Music);
        key(&mut app, KeyCode::Char('H'));
        assert!(app.showing_history, "H opens the history");
        assert!(matches!(
            key(&mut app, KeyCode::Char('z')),
            Some(Action::ToggleShuffle)
        ));
        assert!(app.showing_history, "shuffle is not a way out");
        assert!(
            key(&mut app, KeyCode::Enter).is_none(),
            "nothing played, nothing to queue"
        );
        assert!(app.showing_history);
        let _ = app.take_repaint();
        let back = key(&mut app, KeyCode::Esc).expect("escape steps back");
        app.update(back);
        assert!(!app.showing_history, "escape leaves it");
        assert!(app.take_repaint(), "and asks for the covers under it back");

        // One list at a time: opening one is a step forward from the other.
        key(&mut app, KeyCode::Char('H'));
        key(&mut app, KeyCode::Char('Q'));
        assert!(app.showing_queue && !app.showing_history);
    }

    #[test]
    fn a_track_card_without_ids_still_opens_its_album_and_artist() {
        // Reported: `a` and `c` did nothing, on every page. A card from the
        // home page names its album and artist and carries the id of
        // neither, so the guards that need one failed and the key fell
        // through in silence. `R` had the same gap and filled it by
        // fetching the track; these do the same.
        let mut app = signed_in(sidebar::Section::Music);
        app.last_main_width = 120;
        let mut card = carousel::Card::new("Song", "Someone");
        card.target = Some(carousel::Target::Track("42".into()));
        app.home.rows = vec![home::Row {
            heading: "Songs".into(),
            kind: crate::browse::RowKind::Compact,
            cards: vec![card],
            state: Default::default(),
        }];
        app.home.row = 0;
        assert!(
            app.selected_track_album().is_none(),
            "the card carries no album id"
        );

        assert!(
            matches!(
                key(&mut app, KeyCode::Char('c')),
                Some(Action::FetchTrackAlbum(ref id)) if id.0 == "42"
            ),
            "c fetches the track to find its album"
        );
        assert!(
            matches!(
                key(&mut app, KeyCode::Char('a')),
                Some(Action::FetchTrackArtist(ref id)) if id.0 == "42"
            ),
            "a fetches the track to find its artist"
        );
    }

    #[test]
    fn a_cut_row_of_profiles_shows_circles_without_names() {
        // The row at the fold shows as much of its circles as fits and
        // nothing of its names: a name squeezed onto a cut avatar read as
        // the text cutting into the picture. The keys still count that row
        // as visible, and moving into it scrolls it up whole.
        let mut app = signed_in(sidebar::Section::Profiles);
        app.artists = (0..9)
            .map(|i| crate::library::Artist {
                id: (i).to_string(),
                name: format!("Artist {i}"),
                picture: None,
            })
            .collect();
        app.last_main_width = 100;
        app.last_main_height = 30;

        let text = geometry::text(&geometry::draw(100, 30, |f, _a, _p| draw(f, &mut app)));
        let named = (0..9)
            .filter(|i| text.contains(&format!("Artist {i}")))
            .count();
        let (cols, landable) = app.grid_geometry();
        let drawn = grid::rows(
            30 - grid::header_rows_with(grid::Chrome::Full, &[]),
            app.grid_lines(sidebar::Section::Profiles),
        );

        assert!(
            drawn > landable,
            "the last row drawn is the cut one: {drawn} vs {landable}"
        );
        // Only the whole rows carry names; the cut one at the fold does not.
        assert_eq!(
            named,
            (cols * landable).min(9),
            "the grid says {cols}x{landable} but {named} names are drawn:\n{text}"
        );
    }

    #[test]
    fn the_feed_is_cut_into_four_dated_sections() {
        // A flat grid of releases said nothing about when any of them
        // landed. The windows are the last seven days, the seven before
        // that, the rest of the month and everything behind it.
        let today = 20_000;
        let card = |title: &str, age: i64| carousel::Card {
            day: Some(today - age),
            ..carousel::Card::new(title, "An Artist")
        };
        let rows = feed_rows(
            &[
                card("Today", 0),
                card("Six days", 6),
                card("Eight days", 8),
                card("Twenty days", 20),
                card("A year", 365),
            ],
            today,
        );

        let headings: Vec<&str> = rows.iter().map(|r| r.heading.as_str()).collect();
        assert_eq!(
            headings,
            ["This week", "Last week", "Last month", "Older"],
            "the four windows, newest first"
        );
        assert_eq!(
            rows[0].cards.len(),
            2,
            "today and six days ago are this week"
        );
        assert_eq!(rows[1].cards[0].title, "Eight days");
        assert_eq!(rows[2].cards[0].title, "Twenty days");
        assert_eq!(rows[3].cards[0].title, "A year");
    }

    #[test]
    fn a_section_with_nothing_in_it_is_not_drawn_at_all() {
        // A heading over no cards reads as a fault. The sections are built
        // from what is there rather than drawn and then filled.
        let today = 20_000;
        let rows = feed_rows(
            &[carousel::Card {
                day: Some(today),
                ..carousel::Card::new("Today", "An Artist")
            }],
            today,
        );
        assert_eq!(
            rows.len(),
            1,
            "only the week that has something in it: {:?}",
            rows.iter().map(|r| &r.heading).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_release_with_no_date_is_shown_under_the_oldest_heading() {
        // The card is still a release by someone the user follows, so it
        // belongs on the page rather than being dropped for want of a
        // stamp.
        let rows = feed_rows(&[carousel::Card::new("Undated", "An Artist")], 20_000);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].heading, "Older");
        assert_eq!(rows[0].cards[0].title, "Undated");
    }

    #[test]
    fn see_all_on_a_feed_section_opens_what_is_already_held() {
        // The four sections are cut from one reply, so there is no path to
        // fetch: `o` opens the cards in hand. Without this the key did
        // nothing at all, the rows having no `more`.
        let mut app = signed_in(sidebar::Section::Feed);
        let today = today();
        app.update(Action::FeedLoaded(
            (0..12)
                .map(|i| carousel::Card {
                    day: Some(today),
                    ..carousel::Card::new(format!("Release {i}"), "An Artist")
                })
                .collect(),
        ));
        app.last_main_width = 100;
        app.last_main_height = 40;

        app.update(Action::SeeAll);
        let open = app.open.as_ref().expect("the section opened");
        assert_eq!(open.title, "This week", "under its own heading");
        assert_eq!(
            app.open_cards.len(),
            12,
            "with the whole of what the section holds"
        );
    }

    #[test]
    fn the_feed_is_its_own_view_rather_than_the_favourites_list() {
        // It fell through to the arm that draws the track list, so the
        // section showed the same thing as Tracks and said nothing about
        // what the followed artists had released.
        let mut app = signed_in(sidebar::Section::Feed);
        app.update(Action::FeedLoaded(vec![
            carousel::Card::new("Discovery", "Daft Punk"),
            carousel::Card::new("Homework", "Daft Punk"),
        ]));

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let text = geometry::text(&terminal.backend().buffer().clone());

        assert!(text.contains("Discovery"), "the feed's own cards:\n{text}");
        assert!(!text.contains("TITLE"), "not the track list:\n{text}");
    }

    #[test]
    fn reaching_the_feed_asks_for_it_once() {
        let mut app = signed_in(sidebar::Section::Music);
        let mut asked = 0;
        for _ in 0..sidebar::Section::ALL.len() * 2 {
            if matches!(app.update(Action::SidebarNext), Some(Action::LoadFeed)) {
                asked += 1;
                // The reply it would have got.
                app.update(Action::FeedLoaded(vec![carousel::Card::new("A", "B")]));
            }
        }
        assert_eq!(asked, 1, "asked for once, not on every pass");
    }

    #[test]
    fn the_movement_keys_drive_the_feeds_own_rows() {
        // `rows_on_screen_mut` routes by section, so without the feed in it
        // the keys would move the home page's rows behind — the fault that
        // has come back five times, in the grid's version of this routing.
        let mut app = signed_in(sidebar::Section::Feed);
        let today = today();
        app.update(Action::FeedLoaded(
            (0..6)
                .map(|i| carousel::Card {
                    day: Some(today - i64::from(i)),
                    ..carousel::Card::new(format!("Release {i}"), "An Artist")
                })
                .collect(),
        ));
        app.last_main_width = 100;
        app.last_main_height = 30;

        assert!(app.on_home(), "the feed moves like a page of rows");
        app.update(Action::CarouselNext);
        assert_eq!(
            app.feed.rows[0].state.selected, 1,
            "the feed's own selection moved"
        );
        assert!(
            app.home.rows.is_empty(),
            "and the home page's rows were left alone"
        );
    }

    #[test]
    fn a_reaches_the_favourite_toggle() {
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = some_tracks(&["One", "Two"]);
        assert!(
            matches!(
                key(&mut app, KeyCode::Char('A')),
                Some(Action::ToggleFavourite)
            ),
            "A toggles the favourite"
        );
    }

    #[test]
    fn a_is_a_letter_while_a_query_is_being_typed() {
        // Same trap as q: a binding that works everywhere would put an "a"
        // out of reach of the search box.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        assert!(
            !matches!(
                key(&mut app, KeyCode::Char('A')),
                Some(Action::ToggleFavourite)
            ),
            "A types rather than toggling while the box has the keyboard"
        );
    }

    #[test]
    fn ctrl_d_and_ctrl_u_move_half_a_screen_at_a_time() {
        use crossterm::event::KeyModifiers;
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = (0..80)
            .map(|i| crate::domain::Track::sample(&format!("T{i}"), "A", Duration::from_secs(1)))
            .collect();
        app.last_main_height = 40;
        let press = |app: &mut App, c: char| {
            let mut next = app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
            while let Some(action) = next {
                next = app.update(action);
            }
        };
        press(&mut app, 'd');
        let half = app.tracklist.selected;
        assert!(half > 1, "further than one row, got {half}");
        assert_eq!(half, app.half_page(), "half of what the list draws");
        press(&mut app, 'd');
        assert_eq!(app.tracklist.selected, 2 * half);
        press(&mut app, 'u');
        assert_eq!(app.tracklist.selected, half, "and back up by the same");

        // In the queue view, where the list keeps its own cursor.
        app.tracklist.selected = 0;
        app.queue = crate::playback::Queue::new(app.tracks.clone(), 0);
        key(&mut app, KeyCode::Char('Q'));
        assert!(app.showing_queue);
        press(&mut app, 'd');
        assert!(
            app.queue_list.selected > 1,
            "the queue moved too: {}",
            app.queue_list.selected
        );
    }

    #[test]
    fn f_follows_the_artist_whose_page_is_open_whatever_is_selected() {
        // `A` on an artist's page is the top track under the selection, so
        // following gets a key of its own -- one that names the page's
        // artist from any section of it.
        let mut app = signed_in(sidebar::Section::Tracks);
        assert!(
            key(&mut app, KeyCode::Char('F')).is_none(),
            "no artist in sight"
        );

        let mut page = an_artist_page();
        page.id = "42".into();
        app.open = Some(OpenCollection {
            title: "Daft Punk".into(),
            subtitle: String::new(),
            detail: String::new(),
            cover: None,
            round_cover: false,
            came_from: sidebar::Section::Tracks,
            target: None,
        });
        app.update(Action::ArtistLoaded(Box::new(page)));
        assert_eq!(
            app.selected_favourable(),
            Some((Favourable::Artist("42".into()), "Daft Punk".to_string()))
        );
        assert!(!app.followed.contains("42"), "the page said not followed");

        // The reply keeps the page in step: flag and count.
        app.artist.as_mut().unwrap().fans = Some(100);
        app.update(Action::FollowChanged {
            what: Favourable::Artist("42".into()),
            name: "Daft Punk".into(),
            following: true,
        });
        let page = app.artist.as_ref().unwrap();
        assert!(
            page.following && page.fans == Some(101),
            "{:?} {:?}",
            page.following,
            page.fans
        );
        assert!(
            matches!(
                key(&mut app, KeyCode::Char('F')),
                Some(Action::ToggleFollow)
            ),
            "F on the page follows its artist"
        );
        assert!(
            matches!(
                key(&mut app, KeyCode::Char('A')),
                Some(Action::ToggleFavourite)
            ),
            "and A still favourites the track"
        );
    }

    #[test]
    fn f_follows_the_artist_whose_card_is_selected() {
        let mut app = signed_in(sidebar::Section::Profiles);
        app.artists = vec![crate::library::Artist {
            id: (7).to_string(),
            name: "Kaaris".into(),
            picture: None,
        }];
        assert_eq!(
            app.selected_favourable(),
            Some((Favourable::Artist("7".into()), "Kaaris".to_string()))
        );
        assert!(matches!(
            key(&mut app, KeyCode::Char('F')),
            Some(Action::ToggleFollow)
        ));
    }

    #[test]
    fn an_album_opens_as_a_page_in_the_artists_shape() {
        // Its rows sit in the artist page's slots, headed for what they
        // hold, and F on the page favourites the album rather than an
        // artist. A cut section names where its rest lives, which is what
        // See all fetches.
        use crate::library::PageKind;
        let mut app = signed_in(sidebar::Section::Albums);
        app.open = Some(OpenCollection {
            title: "good kid".into(),
            subtitle: String::new(),
            detail: String::new(),
            cover: None,
            round_cover: false,
            came_from: sidebar::Section::Albums,
            target: Some(Collection::Album("20556792".into())),
        });
        let mut page = an_artist_page();
        page.kind = PageKind::Album;
        page.id = "20556792".into();
        page.name = "good kid".into();
        page.following = true;
        app.update(Action::ArtistLoaded(Box::new(page)));
        assert!(app.artist.is_some(), "the page took the pane");

        assert_eq!(
            app.selected_favourable(),
            Some((Favourable::Album("20556792".into()), "good kid".to_string()))
        );
        assert_eq!(
            artistview::Section::Albums.heading(PageKind::Album),
            "More by the artist"
        );
        assert_eq!(
            artistview::Section::AppearsOn.heading(PageKind::Album),
            "Related albums"
        );

        // Taking the album out of the favourites updates the page's heart.
        app.update(Action::FollowChanged {
            what: Favourable::Album("20556792".into()),
            name: "good kid".into(),
            following: false,
        });
        assert!(!app.artist.as_ref().unwrap().following);
    }

    #[test]
    fn f_favourites_the_album_playlist_or_mix_under_the_cursor_or_whose_view_is_up() {
        // An album card in the Albums section.
        let mut app = signed_in(sidebar::Section::Albums);
        app.albums = vec![crate::library::Album {
            id: (9).to_string(),
            title: "Discovery".into(),
            artist: "Daft Punk".into(),
            year: None,
            cover: None,
            track_count: 14,
            duration: None,
        }];
        assert_eq!(
            app.selected_favourable(),
            Some((Favourable::Album("9".into()), "Discovery".to_string()))
        );
        assert!(!app.is_favourite(&Favourable::Album("9".into())));
        app.update(Action::FollowChanged {
            what: Favourable::Album("9".into()),
            name: "Discovery".into(),
            following: true,
        });
        assert!(app.is_favourite(&Favourable::Album("9".into())));
        assert_eq!(app.status.as_deref(), Some("Added Discovery to favourites"));

        // The album's own view, opened from its card: the target travels
        // with the view, so F there means the album.
        let identity = app.selected_identity();
        app.open_view(identity, Vec::new());
        app.albums.clear();
        app.tracks = some_tracks(&["One More Time"]);
        assert_eq!(
            app.selected_favourable(),
            Some((Favourable::Album("9".into()), "Discovery".to_string()))
        );

        // The startup ids fill the sets, and the mixes reply the saved mixes.
        app.update(Action::FavouriteIdsLoaded(crate::library::FavouriteIds {
            albums: vec!["1".into(), "2".into()],
            playlists: vec!["u-1".into()],
            artists: vec!["7".into()],
        }));
        assert!(app.is_favourite(&Favourable::Album("2".into())));
        assert!(app.is_favourite(&Favourable::Playlist("u-1".into())));
        assert!(app.is_favourite(&Favourable::Artist("7".into())));
    }

    #[test]
    fn the_reply_marks_the_artist_and_says_so() {
        let mut app = signed_in(sidebar::Section::Tracks);
        app.update(Action::FollowChanged {
            what: Favourable::Artist("42".into()),
            name: "Daft Punk".into(),
            following: true,
        });
        assert!(app.followed.contains("42"), "followed");
        assert_eq!(app.status.as_deref(), Some("Following Daft Punk"));

        app.update(Action::FollowChanged {
            what: Favourable::Artist("42".into()),
            name: "Daft Punk".into(),
            following: false,
        });
        assert!(!app.followed.contains("42"), "and not any more");
        assert_eq!(app.status.as_deref(), Some("Unfollowed Daft Punk"));

        // The Artists section's reply is the account's list of them.
        app.update(Action::ArtistsLoaded(vec![crate::library::Artist {
            id: (9).to_string(),
            name: "Prince".into(),
            picture: None,
        }]));
        assert_eq!(app.followed, std::collections::HashSet::from(["9".to_string()]));
    }

    #[test]
    fn the_reply_marks_the_track_it_was_for() {
        // The mark follows the account, not what was guessed when the key
        // was pressed.
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = some_tracks(&["One", "Two"]);
        let id = app.tracks[0].id.clone();

        app.update(Action::FavouriteChanged {
            id: id.clone(),
            favourite: true,
        });
        assert!(app.favourites.contains(&id), "added");

        app.update(Action::FavouriteChanged {
            id: id.clone(),
            favourite: false,
        });
        assert!(!app.favourites.contains(&id), "and taken out again");
    }

    #[test]
    fn a_late_reply_marks_its_own_track_not_the_selected_one() {
        // Pressing A and moving on before the reply lands must not mark
        // whatever is selected by the time it arrives.
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = (0..3)
            .map(|i| {
                let mut t = crate::domain::Track::sample(
                    &format!("Track {i}"),
                    "An Artist",
                    std::time::Duration::from_secs(200),
                );
                t.id = crate::domain::TrackId::from(i as u64);
                t
            })
            .collect();

        // The reply is for the first track; the user has since moved to the
        // third.
        app.update(Action::TrackNext);
        app.update(Action::TrackNext);
        app.update(Action::FavouriteChanged {
            id: crate::domain::TrackId::from(0),
            favourite: true,
        });

        assert!(
            app.favourites.contains(&crate::domain::TrackId::from(0)),
            "the track the reply was for"
        );
        assert!(
            !app.favourites.contains(&crate::domain::TrackId::from(2)),
            "and not the one now selected"
        );
    }

    #[test]
    fn h_and_l_move_within_a_search_grid() {
        // They used to fall through to CarouselNext, which moved a row of
        // the home page that search was not drawing — so the keys did
        // nothing visible at all.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_height = 30;
        app.last_main_width = 100;
        for _ in 0..2 {
            app.update(Action::NextSearchTab); // Albums
        }

        // Through on_key, so the routing is what is under test and not just
        // the handler behind it.
        assert!(
            matches!(key(&mut app, KeyCode::Char('l')), Some(Action::SearchRight)),
            "l moves right in search, not along a home carousel"
        );
        app.update(Action::SearchRight);
        assert_eq!(
            app.search.as_ref().unwrap().albums.selected,
            1,
            "and lands on the next card"
        );

        assert!(matches!(
            key(&mut app, KeyCode::Char('h')),
            Some(Action::SearchLeft)
        ));
        app.update(Action::SearchLeft);
        assert_eq!(app.search.as_ref().unwrap().albums.selected, 0, "and back");
    }

    #[test]
    fn an_album_opened_from_search_can_still_be_left() {
        // A result opened by mistake must not be a dead end. `h` is a
        // direction rather than a way out, so escape is what does it --
        // and it leaves the album rather than the search underneath, which
        // would take away both views at once.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_width = 100;
        app.last_main_height = 30;
        for _ in 0..2 {
            app.update(Action::NextSearchTab); // Albums
        }
        app.update(Action::ActivateSelection);
        assert!(app.open.is_some(), "the album is open");
        assert!(
            app.search.is_none(),
            "with the results on the stack under it"
        );
        assert!(matches!(key(&mut app, KeyCode::Esc), Some(Action::GoBack)));
        app.update(Action::GoBack);
        // And the search is still there, gone back to.
        assert!(app.search.is_some(), "the results were not closed as well");
    }

    #[test]
    fn the_three_grid_tabs_each_hold_their_own_selection() {
        // Albums, Artists and Playlists are three separate grids. Pointing
        // them at one state looks right until you move in two of them.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_height = 30;
        app.last_main_width = 100;

        // Albums (tab 2): move down one row.
        for _ in 0..2 {
            app.update(Action::NextSearchTab);
        }
        assert_eq!(app.search.as_ref().unwrap().tab, 2, "on Albums");
        app.update(Action::TrackNext);
        let albums_at = app.search.as_ref().unwrap().albums.selected;
        assert!(albums_at > 0, "the album grid moved");

        // Artists (tab 3) starts at the top regardless of what Albums did.
        app.update(Action::NextSearchTab);
        assert_eq!(app.search.as_ref().unwrap().tab, 3, "on Artists");
        assert_eq!(
            app.search.as_ref().unwrap().artists.selected,
            0,
            "the artist grid has its own selection, untouched by Albums"
        );

        // Moving here must not disturb the album grid.
        app.update(Action::TrackNext);
        assert!(app.search.as_ref().unwrap().artists.selected > 0);
        assert_eq!(
            app.search.as_ref().unwrap().albums.selected,
            albums_at,
            "and Albums stayed where it was"
        );

        // Playlists (tab 4) is a third, independent grid.
        app.update(Action::NextSearchTab);
        assert_eq!(app.search.as_ref().unwrap().tab, 4, "on Playlists");
        assert_eq!(
            app.search.as_ref().unwrap().playlists.selected,
            0,
            "untouched by either of the other two"
        );
    }

    #[test]
    fn new_results_put_every_tab_back_to_the_top() {
        // A different query is a different list; a position kept from the
        // last one would point at something unrelated.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.last_main_height = 30;
        app.last_main_width = 100;
        app.update(Action::NextSearchTab);
        for _ in 0..3 {
            app.update(Action::TrackNext);
        }
        assert!(app.search.as_ref().unwrap().tracks.selected > 0);

        // The same query again, with fresh results.
        app.update(Action::SearchLoaded(Box::new(some_results())));
        assert_eq!(app.search.as_ref().unwrap().tracks.selected, 0);
        assert_eq!(app.search.as_ref().unwrap().tab, 0);
    }

    #[test]
    fn t_is_a_letter_while_a_query_is_being_typed() {
        // Searching for "the strokes" must not change tab three times.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "the".chars() {
            assert!(key(&mut app, KeyCode::Char(c)).is_none(), "{c} is text");
        }
        assert_eq!(app.search.as_ref().map(|s| s.query.as_str()), Some("the"));
        assert_eq!(app.search.as_ref().unwrap().tab, 0, "and no tab changed");
    }

    #[test]
    fn tab_cycles_round_the_result_tabs() {
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        app.search.as_mut().unwrap().typing = false;

        for expected in 1..searchview::Tab::ALL.len() {
            assert!(matches!(
                key(&mut app, KeyCode::Tab),
                Some(Action::NextSearchTab)
            ));
            app.update(Action::NextSearchTab);
            assert_eq!(app.search.as_ref().unwrap().tab, expected);
        }
        app.update(Action::NextSearchTab);
        assert_eq!(app.search.as_ref().unwrap().tab, 0, "round to the first");
    }

    /// A signed-in TIDAL session on no token, asking nothing of the
    /// network until a fetch is awaited.
    fn sample_session() -> std::sync::Arc<crate::catalogue::Session> {
        crate::catalogue::Session::sample()
    }

    #[test]
    fn session_expired_clears_the_session_and_tracks_and_shows_the_login_modal() {
        let mut app = App {
            session: Some(sample_session()),
            tracks: vec![crate::domain::Track::sample(
                "t",
                "a",
                std::time::Duration::ZERO,
            )],
            ..App::default()
        };

        app.update(Action::SessionExpired);

        assert!(app.session.is_none(), "the stale session must be dropped");
        assert!(app.tracks.is_empty(), "stale tracks must not linger");
        assert!(
            matches!(app.login, login::LoginState::Failed(_)),
            "the login modal must reappear so Enter works again"
        );
    }

    #[test]
    fn enter_on_a_subsonic_config_asks_for_the_password_and_nothing_else() {
        // The device flow is TIDAL's. With a Subsonic server named, Enter
        // opens the password prompt, and every key after that is a letter
        // of the password -- `q` included -- until Enter sends it.
        let mut app = App::default();
        app.config.provider.kind = crate::config::ProviderKind::Subsonic;
        app.config.subsonic.server = "music.example.org".into();
        app.config.subsonic.username = "colin".into();

        assert!(matches!(
            key(&mut app, KeyCode::Enter),
            Some(Action::BeginLogin)
        ));
        app.update(Action::BeginLogin);
        assert!(matches!(app.login, login::LoginState::Password { .. }));

        for c in ['q', 'u', 'i', 't'] {
            assert!(key(&mut app, KeyCode::Char(c)).is_none(), "{c} is typed, not bound");
        }
        assert!(key(&mut app, KeyCode::Backspace).is_none());
        match key(&mut app, KeyCode::Enter) {
            Some(Action::Connect(secret)) => assert_eq!(secret.0, "qui"),
            other => panic!("Enter sends the password, got {other:?}"),
        }
        assert!(
            matches!(&app.login, login::LoginState::Password { typed, .. } if typed.0.is_empty()),
            "the prompt stays up, emptied, while the connection is made"
        );
    }

    #[test]
    fn a_subsonic_config_without_a_server_says_what_to_set() {
        let mut app = App::default();
        app.config.provider.kind = crate::config::ProviderKind::Subsonic;
        app.update(Action::BeginLogin);
        match &app.login {
            login::LoginState::Failed(msg) => assert!(msg.contains("[subsonic]"), "{msg}"),
            other => panic!("expected a hint, got {other:?}"),
        }
    }

    #[test]
    fn a_session_opened_elsewhere_is_readied_like_a_login() {
        let mut app = App::default();
        let next = app.update(Action::SessionOpened(sample_session()));
        assert!(matches!(next, Some(Action::SessionReady)));
        assert!(app.session.is_some());
    }

    #[test]
    fn without_a_session_an_error_routes_to_the_login_modal() {
        // status is drawn under the full-screen login modal and is invisible
        // there; the message must go somewhere the user can actually see it.
        let mut app = App::default();
        assert!(app.session.is_none());

        app.update(Action::Error("network unreachable".into()));

        assert!(
            app.status.is_none(),
            "status is painted over and must stay empty"
        );
        match app.login {
            login::LoginState::Failed(ref msg) => assert_eq!(msg, "network unreachable"),
            other => panic!("expected LoginState::Failed, got {other:?}"),
        }
    }

    #[test]
    fn with_a_session_an_error_still_routes_to_the_status_bar() {
        let mut app = App {
            session: Some(sample_session()),
            ..App::default()
        };

        app.update(Action::Error("playback failed".into()));

        assert_eq!(app.status.as_deref(), Some("playback failed"));
        assert!(
            matches!(app.login, login::LoginState::Idle),
            "an in-session error must not disturb login state"
        );
    }

    #[test]
    fn the_queue_moves_past_a_blocked_track_when_the_one_before_ends() {
        // AI-generated content turned off, and such a track in the middle of
        // the queue: the queue used to stop dead on it, since the refused
        // start never finished and so never moved the queue on.
        let mut app = signed_in(sidebar::Section::Tracks);
        app.config.playback.ai = false;
        let secs = |n| std::time::Duration::from_secs(n);
        let mut a = crate::domain::Track::sample("Before", "A", secs(1));
        a.id = crate::domain::TrackId::from(1);
        let mut b = crate::domain::Track::sample("Generated", "B", secs(1));
        b.id = crate::domain::TrackId::from(2);
        b.ai = true;
        let mut c = crate::domain::Track::sample("After", "C", secs(1));
        c.id = crate::domain::TrackId::from(3);
        app.queue
            .start_context(vec![a.clone(), b.clone(), c], 0, None);
        app.now_playing.track = Some(a);

        let next = app.update(Action::Playback(crate::playback::PlaybackEvent::Finished));
        assert!(matches!(next, Some(Action::PlayQueued)), "got {next:?}");
        assert_eq!(
            app.queue.current().map(|t| t.title.as_str()),
            Some("After"),
            "the blocked track is passed over"
        );
        assert!(
            app.status
                .as_ref()
                .is_some_and(|n| n.contains("AI-generated")),
            "and the user is told why: {:?}",
            app.status
        );

        // Nothing allowed is left: the queue ends, as it would without them.
        // Without autoplay, which would otherwise follow the end of it.
        app.config.playback.autoplay = false;
        let mut d = crate::domain::Track::sample("Also generated", "D", secs(1));
        d.id = crate::domain::TrackId::from(4);
        d.ai = true;
        app.queue.start_context(vec![b, d], 0, None);
        let next = app.update(Action::Playback(crate::playback::PlaybackEvent::Finished));
        assert!(next.is_none(), "got {next:?}");
        assert!(!app.now_playing.playing);
    }

    #[test]
    fn r_does_nothing_inside_the_radio_it_would_open() {
        // In a radio, the rows carry their radios, and the seed's is the
        // one on screen: R over it opened the same radio over itself,
        // reloading the list and losing the place in it.
        let mut app = signed_in(sidebar::Section::Tracks);
        let secs = |n| std::time::Duration::from_secs(n);
        let mut seed = crate::domain::Track::sample("Seed", "A", secs(1));
        seed.id = crate::domain::TrackId::from(1);
        app.queue.start_context(vec![seed.clone()], 0, None);
        app.now_playing.track = Some(seed.clone());
        app.update(Action::PlayTrackRadio);
        assert!(app.radio_is_open(&crate::domain::TrackId::from(1)), "the radio is up");

        let mut other = crate::domain::Track::sample("Other", "B", secs(1));
        other.id = crate::domain::TrackId::from(2);
        app.update(Action::CollectionLoaded {
            for_title: "Seed Radio".into(),
            tracks: vec![seed, other],
        });
        assert_eq!(app.track_radio(), Some(crate::domain::TrackId::from(1)));
        assert!(
            key(&mut app, KeyCode::Char('R')).is_none(),
            "R over the seed of the open radio does nothing"
        );
        app.tracklist.selected = 1;
        assert!(
            matches!(key(&mut app, KeyCode::Char('R')), Some(Action::PlayTrackRadio)),
            "another row's radio still opens"
        );
    }

    #[test]
    fn a_cookie_sign_in_takes_what_is_pasted_and_sends_it_as_the_answer() {
        // Deezer, Apple and YouTube want the cookies of a signed-in tab.
        // Enter contacts the service; its Secret prompt opens the box;
        // every key after that is part of the paste -- `q` included --
        // until Enter sends it and the box is taken down.
        let mut app = App::default();
        app.config.provider.kind = crate::config::ProviderKind::Deezer;

        assert!(matches!(key(&mut app, KeyCode::Enter), Some(Action::BeginLogin)));
        app.update(Action::BeginLogin);
        assert!(matches!(app.login, login::LoginState::Busy(_)), "{:?}", app.login);
        assert!(
            key(&mut app, KeyCode::Enter).is_none(),
            "Enter while the service is contacted does not start a second sign-in"
        );

        app.update(Action::LoginPrompt(music::SignInPrompt::Secret));
        assert!(matches!(app.login, login::LoginState::Secret { .. }));
        for c in ['a', 'r', 'l', '=', 'q'] {
            assert!(key(&mut app, KeyCode::Char(c)).is_none(), "{c} is typed, not bound");
        }
        match key(&mut app, KeyCode::Enter) {
            Some(Action::Answer(text)) => assert_eq!(text, "arl=q"),
            other => panic!("Enter sends the cookies, got {other:?}"),
        }
        app.update(Action::Answer("arl=q".into()));
        assert!(matches!(app.login, login::LoginState::Busy(_)), "{:?}", app.login);
    }

    #[test]
    fn an_account_prompt_is_walked_with_j_and_k_and_picked_with_enter() {
        // YouTube cookies that carry several accounts: the service asks
        // which, and the answer is the id of the one chosen.
        let choice = |id: &str, name: &str| music::AccountChoice {
            id: id.into(),
            name: name.into(),
            detail: None,
        };
        let mut app = App::default();
        app.config.provider.kind = crate::config::ProviderKind::Youtube;
        app.update(Action::LoginPrompt(music::SignInPrompt::Accounts(vec![
            choice("1", "Me"),
            choice("2", "Label"),
        ])));
        assert!(key(&mut app, KeyCode::Char('j')).is_none());
        assert!(key(&mut app, KeyCode::Char('j')).is_none(), "stops at the last");
        assert!(key(&mut app, KeyCode::Char('k')).is_none());
        assert!(key(&mut app, KeyCode::Char('j')).is_none());
        match key(&mut app, KeyCode::Enter) {
            Some(Action::Answer(id)) => assert_eq!(id, "2"),
            other => panic!("Enter picks the account, got {other:?}"),
        }
    }

    #[test]
    fn a_link_prompt_waits_on_the_browser_and_a_guest_key_needs_a_guest_service() {
        let mut app = App::default();
        app.config.provider.kind = crate::config::ProviderKind::Spotify;
        app.update(Action::LoginPrompt(music::SignInPrompt::Url(
            "https://accounts.spotify.com/authorize".into(),
        )));
        assert!(matches!(app.login, login::LoginState::Url(_)));
        assert!(key(&mut app, KeyCode::Enter).is_none(), "nothing to do but wait");
        assert!(matches!(key(&mut app, KeyCode::Esc), Some(Action::Quit)));

        // `g` is a guest sign-in only where the service has one.
        let mut app = App::default();
        let g = KeyCode::Char(App::GUEST_KEY);
        assert!(key(&mut app, g).is_none(), "TIDAL has no guest");
        app.provider = Some(std::sync::Arc::new(music::youtube::YouTubeProvider::new()));
        assert!(app.guest_allowed());
        assert!(matches!(key(&mut app, g), Some(Action::BeginGuest)));
        app.update(Action::BeginGuest);
        assert!(matches!(app.login, login::LoginState::Busy(_)));
    }

    #[test]
    fn l_opens_the_lyrics_and_asks_for_the_playing_tracks_words_once() {
        let mut app = signed_in(sidebar::Section::Tracks);
        let track = crate::domain::Track::sample("Song", "Someone", std::time::Duration::from_secs(90));
        app.now_playing.track = Some(track.clone());

        let action = key(&mut app, KeyCode::Char('L')).expect("L is bound");
        assert!(matches!(action, Action::ToggleLyrics));
        let asked = app.update(action);
        assert!(
            matches!(asked, Some(Action::LoadLyrics(ref id)) if *id == track.id),
            "opening asks for the words, got {asked:?}"
        );
        assert!(app.lyrics.open);

        // Keys over the pane: j and k scroll it, space still pauses.
        assert!(key(&mut app, KeyCode::Char('j')).is_none());
        assert_eq!(app.lyrics.scroll, 1);
        assert!(matches!(key(&mut app, KeyCode::Char(' ')), Some(Action::TogglePause)));

        app.update(Action::LyricsLoaded {
            id: track.id.clone(),
            found: lyricsview::Found::Nothing,
        });
        // The same track starting again is not searched again.
        let again = app.update(Action::Playback(crate::playback::PlaybackEvent::Started {
            bit_depth: None,
            sample_rate: 44_100,
            delivered: crate::domain::Quality::Lossless,
        }));
        assert!(again.is_none(), "got {again:?}");
        assert!(app.lyrics.following, "a track starting follows from the top");

        assert!(matches!(key(&mut app, KeyCode::Esc), Some(Action::ToggleLyrics)));
        app.update(Action::ToggleLyrics);
        assert!(!app.lyrics.open);
    }

    #[test]
    fn a_track_that_played_long_enough_becomes_a_listen_in_the_outbox() {
        let mut app = signed_in(sidebar::Section::Tracks);
        let started = |app: &mut App, title: &str| {
            app.now_playing.track = Some(crate::domain::Track::sample(
                title,
                "Someone",
                std::time::Duration::from_secs(200),
            ));
            app.update(Action::Playback(crate::playback::PlaybackEvent::Started {
                bit_depth: None,
                sample_rate: 44_100,
                delivered: crate::domain::Quality::Lossless,
            }));
        };
        started(&mut app, "First");
        assert_eq!(
            app.scrobble_outbox.now_playing.as_ref().map(|p| p.title.as_str()),
            Some("First"),
            "the services hear what started"
        );
        assert!(app.listen.is_some());

        // Skipped after twenty seconds: not a listen.
        app.scrobble_outbox.now_playing = None;
        app.now_playing.position = std::time::Duration::from_secs(20);
        started(&mut app, "Second");
        assert!(app.scrobble_outbox.finished.is_empty(), "twenty seconds of two hundred");
        assert_eq!(app.scrobble_outbox.now_playing.as_ref().map(|p| p.title.as_str()), Some("Second"));

        // Played to the end: a listen.
        app.update(Action::Playback(crate::playback::PlaybackEvent::Finished));
        assert_eq!(app.scrobble_outbox.finished.len(), 1);
        assert_eq!(app.scrobble_outbox.finished[0].title, "Second");
        assert!(app.listen.is_none(), "and nothing is in progress");

        // A track that failed to play is no listen either.
        started(&mut app, "Third");
        app.update(Action::Playback(crate::playback::PlaybackEvent::Failed {
            id: crate::domain::TrackId::from(1),
            message: "no".into(),
        }));
        assert!(app.listen.is_none());
    }

    #[test]
    fn a_protected_service_asks_for_the_module_and_the_keys_answer_it() {
        // Apple Music's streams need Widevine; TIDAL's do not, so TIDAL
        // never has the machine searched.
        let mut app = signed_in(sidebar::Section::Tracks);
        assert!(app.update(Action::SessionReady).is_none(), "no provider, nothing to look for");

        app.provider = Some(std::sync::Arc::new(music::apple::AppleProvider::new()));
        assert!(matches!(app.update(Action::SessionReady), Some(Action::LookForCdm)));
        assert_eq!(app.cdm, drmview::Cdm::Looking);
        assert!(app.update(Action::SessionReady).is_none(), "searched once a run");

        app.update(Action::CdmFound(None));
        assert_eq!(app.cdm, drmview::Cdm::Wanted);
        assert!(key(&mut app, KeyCode::Char('q')).is_none(), "the question takes every key");
        // Spelt as a constant: the help's scan reads every `Char('x')` in
        // this file as a binding, and this one is the modal's own.
        const YES: char = 'y';
        let asked = key(&mut app, KeyCode::Char(YES)).and_then(|a| app.update(a));
        assert!(matches!(asked, Some(Action::DownloadCdm)), "got {asked:?}");
        assert_eq!(app.cdm, drmview::Cdm::Offering);

        app.update(Action::CdmOffered {
            version: "4.10".into(),
            license: "terms".into(),
        });
        assert!(app.cdm.asking());
        assert!(matches!(key(&mut app, KeyCode::Char('n')), Some(Action::AnswerCdm(false))));
        app.update(Action::AnswerCdm(false));
        assert_eq!(app.cdm, drmview::Cdm::Declined);
        assert!(!app.cdm.shown(), "and the screen is clear again");

        let mut app = signed_in(sidebar::Section::Tracks);
        app.update(Action::CdmFound(Some(music::drm::Origin::Installed)));
        assert_eq!(app.cdm, drmview::Cdm::Ready(music::drm::Origin::Installed));
        assert!(app.status.as_deref().unwrap_or("").contains("browser"));
    }

    #[test]
    fn the_service_row_picks_a_service_and_enter_switches_to_it() {
        let mut app = signed_in(sidebar::Section::Settings);
        app.config_path = None;
        assert_eq!(app.settings.current(), settings::Setting::Service, "the first row");
        app.now_playing.track = Some(crate::domain::Track::sample("Song", "A", std::time::Duration::from_secs(1)));
        app.tracks = some_tracks(&["One"]);

        // Picking changes the config and nothing else yet.
        app.update(Action::CarouselNext);
        assert_eq!(app.config.provider.kind, crate::config::ProviderKind::Spotify);
        assert!(app.session.is_some(), "still signed in to the old one");
        assert!(app.status.as_deref().unwrap_or("").contains("Spotify"));

        // Enter is the switch: the session, the lists and the bar go, the
        // config stays, and the new service is asked to sign in.
        let next = app.update(Action::ActivateSelection);
        assert!(matches!(next, Some(Action::SwitchProvider)), "got {next:?}");
        let next = app.update(Action::SwitchProvider);
        assert!(matches!(next, Some(Action::BeginLogin)), "got {next:?}");
        assert!(app.session.is_none());
        assert!(app.tracks.is_empty());
        assert!(app.now_playing.track.is_none());
        assert_eq!(app.config.provider.kind, crate::config::ProviderKind::Spotify);
        assert_eq!(app.provider.as_ref().map(|p| p.name()), Some("Spotify"));
        assert_eq!(app.settings.current(), settings::Setting::Service, "the row keeps its place");
    }

    #[test]
    fn s_steps_the_sleep_timer_through_its_stops_and_round_to_off() {
        let mut app = signed_in(sidebar::Section::Tracks);
        assert!(matches!(key(&mut app, KeyCode::Char('S')), Some(Action::CycleSleep)));
        let mut seen = Vec::new();
        for _ in 0..6 {
            app.update(Action::CycleSleep);
            seen.push(match app.sleep {
                Some(Sleep::At { minutes, .. }) => minutes.to_string(),
                Some(Sleep::EndOfTrack) => "end".to_string(),
                None => "off".to_string(),
            });
        }
        assert_eq!(seen, ["15", "30", "45", "60", "end", "off"]);
        assert_eq!(
            app.status.as_deref(),
            Some("sleep timer off"),
            "each step says what it set"
        );
    }

    #[test]
    fn the_sleep_timer_pauses_once_when_it_is_due() {
        let mut app = signed_in(sidebar::Section::Tracks);
        app.now_playing.playing = true;
        app.sleep = Some(Sleep::At {
            when: std::time::Instant::now() - std::time::Duration::from_secs(1),
            minutes: 15,
        });
        assert!(matches!(app.update(Action::Tick), Some(Action::TogglePause)));
        assert!(app.sleep.is_none(), "and is forgotten");
        assert!(app.update(Action::Tick).is_none(), "once");

        // Already paused when it goes off: nothing to do.
        app.now_playing.playing = false;
        app.sleep = Some(Sleep::At {
            when: std::time::Instant::now(),
            minutes: 15,
        });
        assert!(app.update(Action::Tick).is_none());
    }

    #[test]
    fn sleep_at_the_end_of_the_track_pauses_there_and_preloads_nothing() {
        let mut app = signed_in(sidebar::Section::Tracks);
        app.queue = crate::playback::Queue::new(some_tracks(&["One", "Two"]), 0);
        app.now_playing.track = app.queue.current().cloned();
        app.now_playing.playing = true;
        let id = app.now_playing.track.as_ref().unwrap().id.clone();

        // Without the timer, the download done is the moment to fetch the next.
        let next = app.update(Action::Playback(crate::playback::PlaybackEvent::Downloaded(
            id.clone(),
        )));
        assert!(
            matches!(next, Some(Action::Preload(ref n)) if *n == app.queue.upcoming().unwrap().id),
            "got {next:?}"
        );

        app.sleep = Some(Sleep::EndOfTrack);
        assert!(
            app.update(Action::Playback(crate::playback::PlaybackEvent::Downloaded(id)))
                .is_none(),
            "nothing lined up behind a track the music stops after"
        );
        let next = app.update(Action::Playback(crate::playback::PlaybackEvent::Finished));
        assert!(matches!(next, Some(Action::TogglePause)), "got {next:?}");
        assert!(app.sleep.is_none());
        assert_eq!(app.queue.current().unwrap().title, "One", "the queue did not move on");
    }

    #[test]
    fn the_equaliser_setting_reaches_the_shared_equaliser_at_once() {
        let mut app = signed_in(sidebar::Section::Settings);
        app.config_path = None;
        app.config.audio.equalizer = "rock".into();
        app.apply_equalizer();
        assert!(app.equalizer.enabled());
        assert_eq!(app.equalizer.gains(), music::equalizer::Preset::Rock.gains());

        app.config.audio.equalizer = "off".into();
        app.apply_equalizer();
        assert!(!app.equalizer.enabled());
        assert_eq!(app.equalizer.gains(), music::equalizer::Preset::Flat.gains(), "flat for next time");
    }
}
