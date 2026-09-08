pub mod artistview;
pub mod artwork;
pub mod carousel;
#[cfg(test)]
pub mod geometry;
pub mod grid;
pub mod help;
pub mod icons;
pub mod keymap;
pub mod home;
pub mod inputbox;
pub mod layout;
pub mod loadingview;
pub mod mediakeys;
#[cfg(target_os = "linux")]
mod mediakeys_linux;
#[cfg(target_os = "macos")]
pub mod mediakeys_macos;

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
    LoginStarted(crate::auth::DeviceCode),
    LoginPolled(login::PollResult),
    Authenticated(crate::auth::StoredToken),
    /// A token refreshed mid-session. Kept apart from `Authenticated`,
    /// which is a fresh login and fetches the whole library: doing that on
    /// a renewal asks for a second copy of everything already held.
    SessionRenewed(crate::auth::StoredToken),
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
    /// Open the artist's radio: a mix built around them.
    PlayArtistRadio,
    /// The radio built around the track that is playing.
    PlayTrackRadio,
    /// The selected card is a track whose radio the page did not send --
    /// the home page omits it. Fetch the track, then play its radio.
    FetchTrackRadio(crate::domain::TrackId),
    /// A mix to open under a name and artwork of its own.
    OpenMix {
        mix: String,
        title: String,
        cover: Option<String>,
    },
    /// The queue ran out and autoplay is on: fetch this radio and play on.
    Autoplay(String),
    /// The radio came back: queue it and start.
    QueueRadio(Vec<crate::domain::Track>),
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
    /// An opened row that holds covers rather than tracks.
    RowLoaded {
        for_title: String,
        cards: Vec<carousel::Card>,
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
    LoadMixes,
    MixesLoaded(Box<crate::browse::Mixes>),
    FeedLoaded(Vec<carousel::Card>),
    /// Boxed: the home page is by far the largest payload an Action carries,
    /// and every other variant would otherwise grow to match it.
    /// A home tab's rows. Carries which tab, so a reply for one the user
    /// has since left is dropped rather than shown under the wrong heading.
    HomeLoaded {
        tab: crate::browse::Tab,
        home: Box<crate::browse::Home>,
    },
    CarouselNext,
    CarouselPrevious,
    RowNext,
    RowPrevious,
    NextTab,
    LoadTab(crate::browse::Tab),
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
    ToggleShuffle,
    CycleRepeat,
    /// The reply: what the track's state is now, so the mark matches the
    /// account rather than what was guessed when the key was pressed.
    FavouriteChanged { id: crate::domain::TrackId, favourite: bool },
    Playback(crate::playback::PlaybackEvent),
    TogglePause,
    /// A track already showing in the bar failed to start. Distinct from
    /// `Error`: the bar has to be undone as well as the message shown, and
    /// a generic error carries no way to tell which track it was about.
    PlaybackFailed { id: crate::domain::TrackId, message: String },
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

impl Dir {
    /// `Some(down)` for a vertical move, `None` for a horizontal one.
    fn vertical(self) -> Option<bool> {
        match self {
            Dir::Down => Some(true),
            Dir::Up => Some(false),
            _ => None,
        }
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
        if let Some(page) = self.artist.as_ref() {
            if artistview::Section::from_index(self.artist_section)
                == artistview::Section::Tracks
            {
                return page.top_tracks.get(self.artist_tracks.selected).cloned();
            }
            return None;
        }
        match self.search.as_ref() {
            Some(state) => {
                let tab = searchview::Tab::from_index(state.tab);
                searchview::track_rows(&state.results, tab)
                    .get(state.tracks.selected)
                    .cloned()
            }
            None => tracklist::filter(&self.tracks, &self.tracklist.filter)
                .get(self.tracklist.selected)
                .map(|t| (*t).clone()),
        }
    }

    /// The list the selection sits in, and where in it.
    ///
    /// Playing a track queues the list it came from, which is what makes an
    /// album play on. Same source as `selected_track`, so the two cannot
    /// disagree about which list is on screen.
    fn selected_list(&self) -> (Vec<crate::domain::Track>, usize) {
        if let Some(page) = self.artist.as_ref() {
            if artistview::Section::from_index(self.artist_section)
                == artistview::Section::Tracks
            {
                return (page.top_tracks.clone(), self.artist_tracks.selected);
            }
            return (Vec::new(), 0);
        }
        match self.search.as_ref() {
            Some(state) => {
                let tab = searchview::Tab::from_index(state.tab);
                (
                    searchview::track_rows(&state.results, tab),
                    state.tracks.selected,
                )
            }
            None => (
                tracklist::filter(&self.tracks, &self.tracklist.filter)
                    .into_iter()
                    .cloned()
                    .collect(),
                self.tracklist.selected,
            ),
        }
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
        let (cols, grid_rows) = self.search_grid_geometry();
        let Some(state) = self.search.as_mut() else { return };
        let tab = searchview::Tab::from_index(state.tab);

        match tab {
            searchview::Tab::Top => {
                Self::move_in_top(state, dir, cols, height);
            }
            searchview::Tab::Tracks => {
                // A track list is one column, so left and right have nothing
                // to move along.
                let Some(down) = dir.vertical() else { return };
                let len = searchview::track_rows(&state.results, tab).len();
                if down {
                    state.tracks.next(len);
                } else {
                    state.tracks.previous();
                }
                let visible = tracklist::visible_rows_chrome(
                    height,
                    false,
                    tracklist::Chrome::Bare,
                );
                state.tracks.scroll_into_view(visible);
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
                match dir {
                    Dir::Down => grid.next_row(len, cols, grid_rows),
                    Dir::Up => grid.previous_row(cols, grid_rows),
                    Dir::Right => grid.next(len, cols, grid_rows),
                    Dir::Left => grid.previous(cols, grid_rows),
                }
            }
        }
    }

    /// Move within Top results, which stacks three sections.
    ///
    /// Each card section is a single row, so moving down off the end of one
    /// steps into the next rather than stopping — otherwise the selection
    /// would be stuck in whichever section it started in.
    fn move_in_top(
        state: &mut SearchState,
        dir: Dir,
        cols: usize,
        height: u16,
    ) {
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

        match dir {
            Dir::Down | Dir::Up => {
                let down = dir == Dir::Down;
                // Within the track list, j and k move by rows as usual, and
                // only running off the top steps back into the cards.
                if state.top == TopSection::Tracks {
                    let len = searchview::track_rows(&state.results, searchview::Tab::Top).len();
                    if down {
                        state.tracks.next(len);
                    } else if state.tracks.selected == 0 && at > 0 {
                        state.top = present[at - 1];
                        return;
                    } else {
                        state.tracks.previous();
                    }
                    let visible = tracklist::visible_rows_chrome(
                        height,
                        false,
                        tracklist::Chrome::Bare,
                    );
                    state.tracks.scroll_into_view(visible);
                    return;
                }
                // The card sections are one row tall, so a vertical move
                // always leaves them.
                let next = if down { at + 1 } else { at.saturating_sub(1) };
                if next < present.len() && (down || at > 0) {
                    state.top = present[next];
                }
            }
            Dir::Left | Dir::Right => {
                // Along a card row. The track list has no second column.
                let (len, grid) = match state.top {
                    TopSection::Artists => (state.results.artists.len(), &mut state.artists),
                    TopSection::Albums => (state.results.albums.len(), &mut state.albums),
                    TopSection::Tracks => return,
                };
                // One row, so the selection never scrolls out of view.
                if dir == Dir::Right {
                    grid.next(len, cols, 1);
                } else {
                    grid.previous(cols, 1);
                }
            }
        }
    }

    /// Columns and rows of the grid a search tab draws into.
    fn search_grid_geometry(&self) -> (usize, usize) {
        (grid::columns(self.last_main_width), grid::rows(self.search_body_height(), 2))
    }

    /// The rows the search view has left for results, under its own heading,
    /// box and tabs.
    fn search_body_height(&self) -> u16 {
        self.last_main_height.saturating_sub(searchview::HEADER_ROWS)
    }
}

/// What opening a card in a grid loads.
#[derive(Debug, Clone)]
pub enum Collection {
    Playlist(String),
    Album(u64),
    /// An artist's own page: their top tracks, albums and similar artists.
    Artist(u64),
    /// A mix, which opens as a list of its tracks.
    Mix(String),
    /// Another page of rows: a genre, a mood, a decade. What an Explore
    /// link opens.
    Page { title: String, path: String },
    /// A whole home row, from the module's own endpoint. The page returns
    /// six of these for the grid; this asks for the lot, which is what the
    /// web client's "See all" does.
    Row { heading: String, path: String },
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
    /// No view over it: whatever the sidebar points at.
    Section(sidebar::Section),
}

#[derive(Debug, Default)]
pub struct App {
    pub should_quit: bool,
    pub status: Option<String>,
    pub login: login::LoginState,
    pub session: Option<crate::auth::StoredToken>,
    pub sidebar: sidebar::SidebarState,
    pub tracklist: tracklist::TrackListState,
    pub tracks: Vec<crate::domain::Track>,
    /// What plays after this track. Playing from a list queues the whole
    /// list, so an album goes on rather than stopping after one track.
    pub queue: crate::playback::Queue,
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
    /// The user's mixes and radio stations, one tab each.
    pub mixes: crate::browse::Mixes,
    /// Which tab is showing: 0 is the user's own mixes, 1 the stations.
    pub mixes_tab: usize,
    /// One grid state per tab, so switching back finds it where it was.
    pub mixes_grid: grid::GridState,
    pub radio_grid: grid::GridState,
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
    /// When the stored token was last checked for having gone missing, so a
    /// 30fps tick does not stat the filesystem on every frame.
    pub last_token_check: Option<std::time::Instant>,
    /// When the session was last sent for renewal, so a failing refresh is
    /// retried on the minute rather than on every one of thirty ticks a
    /// second.
    pub last_renewal: Option<std::time::Instant>,
    /// Where this app instance keeps its session. `None` means the real
    /// location; a test sets it so that exercising the sign-out path cannot
    /// delete the developer's own session, which it did for a while.
    pub token_path: Option<std::path::PathBuf>,
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
            sidebar::Section::MixesAndRadio => self.mixes_cards(),
            sidebar::Section::Playlists => self
                .playlists
                .iter()
                .map(carousel::playlist_card)
                .collect(),
            sidebar::Section::Albums => self
                .albums
                .iter()
                .map(carousel::album_card)
                .collect(),
            sidebar::Section::Profiles => self
                .artists
                .iter()
                .map(carousel::artist_card)
                .collect(),
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
            sidebar::Section::MixesAndRadio if self.mixes_tab == 1 => &self.radio_grid,
            sidebar::Section::MixesAndRadio => &self.mixes_grid,
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
            sidebar::Section::MixesAndRadio if self.mixes_tab == 1 => &mut self.radio_grid,
            sidebar::Section::MixesAndRadio => &mut self.mixes_grid,
            _ => &mut self.playlist_grid,
        }
    }

    /// The whole of the home row the selection is in, when the page said
    /// there is more of it than the grid shows.
    pub fn selected_row(&self) -> Option<Collection> {
        // An artist's Top Tracks is a row like any other: the page carries
        // four of the hundred behind it, and the rest lives at a path. The
        // other sections are whole in hand, so only this one is fetched.
        if let Some(page) = self.artist.as_ref() {
            if artistview::Section::from_index(self.artist_section)
                == artistview::Section::Tracks
            {
                return Some(Collection::Row {
                    heading: format!(
                        "{} — {}",
                        page.name,
                        artistview::Section::Tracks.heading()
                    ),
                    path: page.top_tracks_path.clone()?,
                });
            }
            return None;
        }
        if !self.on_home() {
            return None;
        }
        let row = self.rows_on_screen().current_row()?;
        Some(Collection::Row {
            heading: row.heading.clone(),
            path: row.more.clone()?,
        })
    }

    /// What the selected card opens, if it opens anything: a track plays
    /// instead, and a card pointing at nothing opens nothing.
    pub fn selected_collection(&self) -> Option<Collection> {
        match self.selected_card()?.target? {
            carousel::Target::Playlist(uuid) => Some(Collection::Playlist(uuid)),
            carousel::Target::Album(id) => Some(Collection::Album(id)),
            carousel::Target::Artist(id) => Some(Collection::Artist(id)),
            carousel::Target::Mix(id) => Some(Collection::Mix(id)),
            carousel::Target::Page(path) => Some(Collection::Page {
                title: self.selected_card()?.title,
                path,
            }),
            // A track plays rather than opening.
            carousel::Target::Track(_) => None,
        }
    }

    /// The track a card plays, when it is a track rather than a collection.
    pub fn selected_track_id(&self) -> Option<crate::domain::TrackId> {
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
        // An artist's page draws card sections of its own, over everything
        // else — so it is asked first, or enter reaches the view behind it.
        if let Some(page) = self.artist.as_ref() {
            let section = artistview::Section::from_index(self.artist_section);
            if section == artistview::Section::Tracks {
                return None;
            }
            let cards = artistview::cards(page, section);
            let row = &self.artist_rows[section.index()];
            return cards.get(row.selected).cloned();
        }
        // A search tab that draws cards has one too. It reuses the app's own
        // grid to draw them, so enter has to reach the same card the grid
        // has highlighted — without this it read the collection view behind
        // search and opened nothing.
        if let Some(state) = self.search.as_ref() {
            let tab = searchview::Tab::from_index(state.tab);
            // Top stacks an artist row, an album row and a track list, and
            // `top` says which of them holds the selection. Refusing the
            // whole tab left the artists and albums drawn there impossible
            // to open — enter did nothing at all.
            if tab == searchview::Tab::Top {
                use searchview::TopSection;
                let (cards, grid) = match state.top {
                    TopSection::Artists => (
                        searchview::cards(&state.results, searchview::Tab::Artists),
                        &state.artists,
                    ),
                    TopSection::Albums => (
                        searchview::cards(&state.results, searchview::Tab::Albums),
                        &state.albums,
                    ),
                    // A track plays rather than opening, and the track list
                    // is read by `selected_track` instead.
                    TopSection::Tracks => return None,
                };
                return cards.get(grid.selected).cloned();
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
        let card = self.selected_card()?;
        Some(OpenCollection {
            title: card.title,
            subtitle: card.subtitle,
            detail: card.detail,
            cover: card.cover_url,
            round_cover: card.round,
            came_from: self.sidebar.section(),
        })
    }

    /// Whether the main pane is the Settings view.
    ///
    /// Read from what is drawn, not from the sidebar alone: an opened
    /// collection or a search covers the section, and the keys belong to
    /// whatever is actually on screen.
    pub fn on_settings(&self) -> bool {
        self.sidebar.section() == sidebar::Section::Settings
            && self.open.is_none()
            && self.search.is_none()
            && self.artist.is_none()
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
                self.status = Some(format!("could not save settings: {e}"));
            }
        }
        // The volume is heard now rather than on the next track: a level
        // you cannot hear yourself setting is one you set by guessing.
        match which {
            settings::Setting::Volume => Some(Action::SetVolume(self.config.audio.volume)),
            // The icons are already swapped -- `cycle` does that, so the row
            // shows the new set as it is chosen -- and the rest of the
            // screen picks them up on the next frame.
            settings::Setting::Quality
            | settings::Setting::Autoplay
            | settings::Setting::Explicit
            | settings::Setting::Ai
            | settings::Setting::NerdFont => None,
        }
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

    /// The radio around the track the key would act on, if the API named
    /// one.
    ///
    /// The selection first, and what is playing only when there is no track
    /// selected. Every other key here acts on the selection -- enter opens
    /// it, `A` favourites it -- and a radio key that did otherwise would
    /// start a mix from a track that is not on screen, which reads as the
    /// app doing something of its own accord.
    ///
    /// The fall-back is what keeps the obvious case working: on the home
    /// page or in Settings nothing is selected, and continuing from what is
    /// in your ears is then the only thing the key could mean.
    pub fn track_radio(&self) -> Option<String> {
        if let Some(selected) = self.selected_track() {
            return selected.radio;
        }
        self.now_playing.track.as_ref()?.radio.clone()
    }

    /// What the radio key would be named after, for the heading.
    fn track_radio_title(&self) -> String {
        self.selected_track()
            .or_else(|| self.now_playing.track.clone())
            .map(|t| t.title)
            .unwrap_or_default()
    }

    /// The artist radio of the page on screen, if there is one.
    pub fn artist_radio(&self) -> Option<String> {
        self.artist.as_ref()?.radio.clone()
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

    /// Whether the main pane is the Mixes & Radio section.
    ///
    /// Read from what is drawn: an opened mix or a search covers the
    /// section, and the keys belong to whatever is on screen.
    pub fn on_mixes(&self) -> bool {
        self.sidebar.section() == sidebar::Section::MixesAndRadio
            && self.open.is_none()
            && self.search.is_none()
            && self.artist.is_none()
    }

    /// The cards of whichever Mixes tab is showing.
    pub fn mixes_cards(&self) -> Vec<carousel::Card> {
        if self.mixes_tab == 1 {
            self.mixes.radio.clone()
        } else {
            self.mixes.mine.clone()
        }
    }

    /// How many home rows the selection may land in without scrolling.
    ///
    /// The count the renderer draws, less the last one when it is cut off
    /// by the pane's edge: landing there leaves the selection half drawn
    /// and the page looking stuck, since the row it moved to is the row
    /// already at the bottom. Scrolling it up into the whole part is what
    /// the web client does, and what the card grids already did.
    fn home_rows_to_land_in(&self) -> usize {
        let has_shortcuts = !self.rows_on_screen().shortcuts.is_empty();
        let rows = &self.rows_on_screen().rows;
        // The rows themselves, so the count matches what the renderer
        // draws: a track grid is taller than a carousel.
        let visible = home::visible_rows_of(self.last_main_height, has_shortcuts, rows);
        if home::last_row_is_cut(self.last_main_height, has_shortcuts, rows) {
            visible.saturating_sub(1).max(1)
        } else {
            visible
        }
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
        // Read before anything is taken: the fields below are moved out as
        // the struct is built, so asking afterwards always says "nothing
        // was open".
        let over_a_section = self.open.is_some() || self.artist.is_some();
        Level {
            section: self.sidebar.section(),
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
        if let Some(explore) = level.explore {
            self.explore = explore;
        }
    }

    /// Step back one view, returning whether there was one to step to.
    ///
    /// What escape does, and the back arrow: the view stepped out of goes
    /// on the forward stack so it can be stepped into again.
    fn go_back(&mut self) -> bool {
        let Some(level) = self.back.pop() else { return false };
        let leaving = self.take_level();
        self.forward.push(leaving);
        self.restore_level(level);
        true
    }

    /// Step into the view that going back stepped out of.
    fn go_forward(&mut self) -> bool {
        let Some(level) = self.forward.pop() else { return false };
        let leaving = self.take_level();
        self.remember(leaving);
        self.restore_level(level);
        true
    }

    /// Keep the selected section of an artist's page on screen.
    ///
    /// Two of them fit at a time, so the page scrolls by section: moving
    /// past the last drawn one brings it to the top rather than leaving the
    /// selection somewhere below the pane.
    fn scroll_artist_into_view(&mut self, at: usize) {
        const SHOWING: usize = 2;
        if at < self.artist_scroll {
            self.artist_scroll = at;
        } else if at >= self.artist_scroll + SHOWING {
            self.artist_scroll = at + 1 - SHOWING;
        }
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
        self.artist_tracks = tracklist::TrackListState::default();
        self.artist_rows = Default::default();

        // The opened view is what the user is now looking at, so it gets
        // the keys. Leaving focus on the view behind it made the new one
        // impossible to move around in.
        self.tracks.clear();
        self.tracklist = tracklist::TrackListState::default();
    }

    /// The heading-only identity most opens carry: a title and nothing else.
    fn heading_only(&self, title: String) -> Option<OpenCollection> {
        Some(OpenCollection {
            title,
            subtitle: String::new(),
            detail: String::new(),
            cover: None,
            round_cover: false,
            came_from: self.sidebar.section(),
        })
    }

    /// As [`heading_only`], with the artwork of whatever it was opened from.
    ///
    /// A radio is a mix, and a mix has a cover -- checked against a real
    /// response. Opened by a key rather than by enter, it had nowhere to
    /// take one from and the page came up with a blank square where every
    /// other opened view has a picture.
    fn heading_with_cover(
        &self,
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
            if self.on_mixes() { &MIXES_TABS } else { &[] },
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
            sidebar::Section::Albums
            | sidebar::Section::Feed
            | sidebar::Section::MixesAndRadio => 2,
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
        if self.artist.is_some() {
            return Showing::Artist;
        }
        if let Some(state) = self.search.as_ref() {
            return Showing::Search(searchview::Tab::from_index(state.tab));
        }
        // A page of rows opened from Explore: drawn by the rows renderer
        // under its own heading, and driven by the keys as the page of rows
        // it is. It carries no `open` -- setting one handed the pane to the
        // collection view, which drew an empty track list.
        if self.explore.heading.is_some()
            && self.sidebar.section() == sidebar::Section::Explore
        {
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
                artistview::Section::from_index(self.artist_section)
                    != artistview::Section::Tracks
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
            // Nothing to move around in until the reply lands.
            Showing::Tracks | Showing::Rows | Showing::Loading => false,
            Showing::Section(section) => matches!(
                section,
                sidebar::Section::Playlists
                    | sidebar::Section::Albums
                    | sidebar::Section::Profiles
                    | sidebar::Section::MixesAndRadio
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
                    self.status = Some(message);
                }
                None
            }
            Action::Tick => {
                // Put the session back if the file went missing underneath
                // us. It has, repeatedly, without the app deleting it — and
                // once it is gone nothing rewrites it, so a perfectly valid
                // session in memory is lost at the next launch for no reason
                // the user can see.
                self.restore_token_if_missing();
                None
            }
            Action::BeginLogin => {
                self.login = login::LoginState::Idle;
                None
            }
            Action::SessionExpired => {
                // Log the deletion, not just a failure to delete. Silence here
                // made a spurious sign-out impossible to trace: the token was
                // gone with nothing in the log to say who removed it.
                tracing::warn!("session rejected — deleting the stored token");
                self.session = None;
                if let Some(path) = self.token_path() {
                    if let Err(e) = crate::auth::store::clear_at(&path) {
                        tracing::warn!("could not clear the stale token: {e}");
                    }
                }
                self.login = login::LoginState::Failed(
                    "your session expired — press Enter to sign in again".into(),
                );
                self.tracks.clear();
                None
            }
            Action::LoginStarted(code) => {
                self.login = login::LoginState::Waiting { code };
                None
            }
            Action::LoginPolled(result) => {
                if result == login::PollResult::Expired {
                    self.login = login::LoginState::Failed("the code expired".into());
                }
                None
            }
            Action::Authenticated(token) => {
                self.session = Some(token);
                self.status = None;
                None
            }
            // The token, and nothing else: the views are already filled.
            Action::SessionRenewed(token) => {
                self.session = Some(token);
                None
            }
            Action::TrackNext => {
                if self.on_settings() {
                    self.settings.next();
                    return None;
                }
                if self.artist.is_some() {
                    self.artist_move(Dir::Down);
                    return None;
                }
                if self.search.is_some() {
                    self.search_move(Dir::Down);
                    return None;
                }
                if self.on_grid() {
                    // A grid moves by whole rows: one card at a time down a
                    // six-wide grid would be six presses per line.
                    let (cols, rows) = self.grid_geometry();
                    let len = self.grid_cards(self.sidebar.section()).len();
                    self.grid_state_mut(self.sidebar.section()).next_row(len, cols, rows);
                } else {
                    let len = tracklist::filter(&self.tracks, &self.tracklist.filter).len();
                    self.tracklist.next(len);
                    // With a banner the header is nine rows taller, so the
                    // no-banner count let the selection run three rows below
                    // the last drawn one — off the bottom, cursor gone.
                    let visible = tracklist::visible_rows_with(
                        self.last_main_height,
                        self.open.is_some(),
                    );
                    self.tracklist.scroll_into_view(visible);
                }
                None
            }
            Action::TrackPrevious => {
                if self.on_settings() {
                    self.settings.previous();
                    return None;
                }
                if self.artist.is_some() {
                    self.artist_move(Dir::Up);
                    return None;
                }
                if self.search.is_some() {
                    self.search_move(Dir::Up);
                    return None;
                }
                if self.on_grid() {
                    let (cols, rows) = self.grid_geometry();
                    self.grid_state_mut(self.sidebar.section()).previous_row(cols, rows);
                } else {
                    self.tracklist.previous();
                    // With a banner the header is nine rows taller, so the
                    // no-banner count let the selection run three rows below
                    // the last drawn one — off the bottom, cursor gone.
                    let visible = tracklist::visible_rows_with(
                        self.last_main_height,
                        self.open.is_some(),
                    );
                    self.tracklist.scroll_into_view(visible);
                }
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
                self.favourites = tracks.iter().map(|t| t.id).collect();
                if self.open.is_none() {
                    self.tracks = tracks;
                    self.tracklist.selected = 0;
                }
                None
            }
            Action::RowLoaded { for_title, cards } => {
                // The reply is here, so the pane stops waiting.
                self.awaiting = false;
                // Same rule as a track reply: a row the user has since left
                // would arrive under the wrong heading.
                match self.open.as_ref() {
                    Some(open) if open.title == for_title => {
                        self.open_cards = cards;
                        self.open_grid = grid::GridState::default();
                    }
                    _ => tracing::info!("dropping cards for {for_title:?}, no longer open"),
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
                        let total: std::time::Duration =
                            tracks.iter().map(|t| t.duration).sum();
                        let detail = carousel::collection_detail(
                            tracks.len() as u32,
                            Some(total),
                        );
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
                    Some(open) if open.title == page.name => self.artist = Some(*page),
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
                        more: row.more,
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
                        more: row.more,
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
                self.artists = artists;
                self.artist_grid.clamp(self.artists.len());
                None
            }
            Action::HomeLoaded { tab, home } => {
                // A reply for a tab the user has since left would replace
                // what they are looking at with rows from another page.
                if tab != crate::browse::Tab::from_index(self.home.tab) {
                    tracing::info!("dropping rows for {tab:?}, no longer showing");
                    return None;
                }
                let home = *home;
                self.home.shortcuts = home.shortcuts;
                self.home.rows = home
                    .rows
                    .into_iter()
                    .map(|row| home::Row {
                        heading: row.heading,
                        kind: row.kind,
                        cards: row.cards,
                        state: carousel::CarouselState::default(),
                        more: row.more,
                    })
                    .collect();
                None
            }
            Action::CarouselNext => {
                if self.on_settings() {
                    return self.change_setting(true);
                }
                if self.on_grid() {
                    let (cols, rows) = self.grid_geometry();
                    let len = self.grid_cards(self.sidebar.section()).len();
                    self.grid_state_mut(self.sidebar.section()).next(len, cols, rows);
                    return None;
                }
                // How many cards fit depends on the pane width, which only the
                // renderer knows; this is the width the layout gives it.
                let visible = carousel::visible_cards(
                    self.last_main_width.saturating_sub(1),
                );
                if let Some(row) = self.rows_on_screen_mut().current_row_mut() {
                    let len = row.cards.len();
                    row.state.next(len, visible);
                }
                None
            }
            Action::CarouselPrevious => {
                if self.on_settings() {
                    return self.change_setting(false);
                }
                if self.on_grid() {
                    let (cols, rows) = self.grid_geometry();
                    self.grid_state_mut(self.sidebar.section()).previous(cols, rows);
                    return None;
                }
                let visible = carousel::visible_cards(
                    self.last_main_width.saturating_sub(1),
                );
                if let Some(row) = self.rows_on_screen_mut().current_row_mut() {
                    row.state.previous(visible);
                }
                None
            }
            Action::RowNext => {
                let visible = self.home_rows_to_land_in();
                // A track row is a grid, so down has somewhere to go inside
                // it before leaving for the next row.
                let cols = trackgrid::columns(self.last_main_width);
                self.rows_on_screen_mut().down(visible, cols);
                None
            }
            Action::RowPrevious => {
                let visible = self.home_rows_to_land_in();
                let cols = trackgrid::columns(self.last_main_width);
                self.rows_on_screen_mut().up(visible, cols);
                None
            }
            Action::NextTab if self.on_mixes() => {
                // Both tabs come from the one fetch, so this only changes
                // which of them is drawn.
                self.mixes_tab = (self.mixes_tab + 1) % MIXES_TABS.len();
                None
            }
            Action::NextTab => {
                // The three tabs are three separate pages, not one page
                // filtered — changing tab changed a highlight and nothing
                // else, because nothing went to fetch the new one.
                self.home.next_tab();
                self.home.rows.clear();
                self.home.shortcuts.clear();
                self.home.row = 0;
                self.home.scroll = 0;
                Some(Action::LoadTab(crate::browse::Tab::from_index(self.home.tab)))
            }
            Action::LoadTab(_) => None,
            // The request is a side effect in run(), like LoadTab's.
            Action::LoadExplore => None,
            Action::LoadFeed => None,
            Action::LoadFavourites => None,
            Action::LoadMixes => None,
            Action::MixesLoaded(mixes) => {
                self.mixes = *mixes;
                None
            }
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
                if self.queue.next().is_some() {
                    return Some(Action::PlayQueued);
                }
                self.now_playing.playing = false;
                None
            }
            Action::QueuePrevious => {
                if self.queue.previous().is_some() {
                    return Some(Action::PlayQueued);
                }
                None
            }
            // Sent on to the player by the loop; nothing to change here.
            Action::SetVolume(_) => None,
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
            Action::Autoplay(_) | Action::FetchTrackRadio(_) => None,
            // The pane is taken here so it happens whether the mix was
            // opened by a key or by a reply arriving.
            Action::OpenMix { title, cover, .. } => {
                let identity = self.heading_with_cover(title, cover, false);
                self.open_view(identity, Vec::new());
                None
            }
            Action::QueueRadio(tracks) => {
                if tracks.is_empty() {
                    return None;
                }
                // A queue like any other, so the skip keys work on it and
                // the bar counts through it.
                self.queue = crate::playback::Queue::new(tracks, 0);
                Some(Action::PlayQueued)
            }
            Action::ToggleFavourite => None,
            // Applied here rather than in the loop, for the same reason
            // opening is: the loop's copy could only be exercised by running
            // the whole app.
            Action::SeeAll if self.artist.is_some() => {
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
                    match present.first() {
                        Some(first) => *first,
                        None => return None,
                    }
                };
                let heading = format!("{} — {}", page.name, section.heading());
                // Top Tracks are tracks, not covers: they open as a list, the
                // way the web does it -- and the page carries only four of
                // the hundred behind them, so this is the one section that
                // has to be fetched rather than reopened from what is in
                // hand. `cards` builds covers and has nothing for tracks,
                // which is why `o` did nothing here.
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
            Action::SeeAll => {
                if let Some(Collection::Row { heading, .. }) = self.selected_row() {
                    let identity = self.heading_only(heading);
                    self.open_view(identity, Vec::new());
                }
                None
            }
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
                self.status = Some(message);
                None
            }
            Action::TogglePause => {
                self.now_playing.playing = !self.now_playing.playing;
                None
            }
            Action::Playback(event) => {
                match event {
                    crate::playback::PlaybackEvent::Position(p) => {
                        self.now_playing.position = p;
                    }
                    crate::playback::PlaybackEvent::Started { bit_depth, sample_rate, delivered } => {
                        // Report what was DELIVERED.
                        let bits = bit_depth
                            .map(|b| format!("{b}-bit "))
                            .unwrap_or_default();
                        self.now_playing.quality = Some(format!(
                            "{bits}{:.1}kHz",
                            sample_rate as f64 / 1000.0
                        ));
                        self.now_playing.tier =
                            nowplaying::Tier::of_quality(delivered, bit_depth, sample_rate);
                        // A new stream starts at the beginning. Without this
                        // the previous track's position survived into the new
                        // one, and if that track was the longer of the two
                        // the ratio saturated and drew a full bar over a
                        // track that had barely started.
                        self.now_playing.position = std::time::Duration::ZERO;
                        self.now_playing.playing = true;
                    }
                    crate::playback::PlaybackEvent::Finished => {
                        self.now_playing.position = std::time::Duration::ZERO;
                        // A track ending on its own moves the queue on, which
                        // is what makes an album play through. `advance`
                        // rather than `next`: repeat-one applies to a track
                        // that ends, not to the skip button.
                        if self.queue.advance().is_some() {
                            return Some(Action::PlayQueued);
                        }
                        self.now_playing.playing = false;
                        // The queue is out. With autoplay on, keep going
                        // with the radio the last track named -- TIDAL's
                        // own "continue with similar content".
                        if self.config.playback.autoplay {
                            if let Some(radio) = self
                                .now_playing
                                .track
                                .as_ref()
                                .and_then(|t| t.radio.clone())
                            {
                                tracing::info!("queue empty, following the track radio");
                                return Some(Action::Autoplay(radio));
                            }
                        }
                    }
                    crate::playback::PlaybackEvent::Error(e) => {
                        self.status = Some(e);
                        self.now_playing.playing = false;
                    }
                }
                None
            }
            // Opening is applied here rather than in the loop so it can be
            // tested: the loop's copy could only be exercised by running the
            // whole app, and a test that called the helpers directly passed
            // just as well with the bug in place.
            Action::ActivateSelection if self.on_grid() || self.on_home() => {
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
            Action::PlayTrackRadio => {
                // The same step opening anything else takes: what is on
                // screen goes on the history, and the radio takes the pane.
                self.track_radio()?;
                let title = self.track_radio_title();
                // The track's own artwork: a radio built from it is about
                // that record, and the mix's own cover is not in hand until
                // its items arrive.
                let cover = self
                    .selected_track()
                    .or_else(|| self.now_playing.track.clone())
                    .and_then(|t| t.cover);
                let identity =
                    self.heading_with_cover(format!("{title} Radio"), cover, false);
                self.open_view(identity, Vec::new());
                None
            }
            Action::PlayArtistRadio => {
                // The same step opening anything else takes: what is on
                // screen goes on the history, and the radio takes the pane.
                self.artist_radio()?;
                let name = self
                    .artist
                    .as_ref()
                    .map(|a| a.name.clone())
                    .unwrap_or_default();
                // The artist's portrait, round as it is everywhere else.
                let cover = self.artist.as_ref().and_then(|a| a.picture.clone());
                let identity =
                    self.heading_with_cover(format!("{name} Radio"), cover, true);
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
                // The home page is For you: escape from the Rising tab
                // landing on Rising is not "back to the start".
                if self.home.tab != 0 {
                    self.home.tab = 0;
                    self.home.rows.clear();
                    self.home.shortcuts.clear();
                    self.home.row = 0;
                    self.home.scroll = 0;
                    return Some(Action::LoadTab(crate::browse::Tab::ForYou));
                }
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

    /// Whether the session is close enough to expiring to be renewed.
    ///
    /// The margin matters: a token checked only at the moment it expires is
    /// already too late for a request in flight, and one refreshed at
    /// startup alone leaves an app opened near the end of a session with
    /// minutes of working time and no warning when they run out.
    fn session_needs_renewing(&self) -> bool {
        /// Renew this long before the token is due to expire.
        const MARGIN: u64 = 5 * 60;

        let Some(token) = self.session.as_ref() else { return false };
        if let Some(last) = self.last_renewal {
            if std::time::Instant::now().duration_since(last) < Duration::from_secs(60) {
                return false;
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        token.expires_at.saturating_sub(now) <= MARGIN
    }

    fn mark_session_renewed(&mut self) {
        self.last_renewal = Some(std::time::Instant::now());
    }

    /// Rewrite the stored session if the file has gone missing while the
    /// app is running.
    ///
    /// Rate-limited to once a minute: a tick fires about thirty times a
    /// second, and this touches the filesystem.
    fn restore_token_if_missing(&mut self) {
        let Some(path) = self.token_path() else { return };
        self.restore_token_at(&path);
    }

    /// Where this instance's session lives.
    fn token_path(&self) -> Option<std::path::PathBuf> {
        self.token_path
            .clone()
            .or_else(crate::config::paths::token_file)
    }

    /// The part worth testing, on a path the caller chooses: writing to the
    /// real one from a test would cost the developer their session, and did.
    fn restore_token_at(&mut self, path: &std::path::Path) {
        let Some(token) = self.session.clone() else { return };

        let now = std::time::Instant::now();
        if let Some(last) = self.last_token_check {
            if now.duration_since(last) < Duration::from_secs(60) {
                return;
            }
        }
        self.last_token_check = Some(now);

        if path.exists() {
            return;
        }

        tracing::warn!("the stored session vanished while running; writing it back");
        if let Err(e) = crate::auth::store::save_to(path, &token) {
            tracing::warn!("could not write the session back: {e}");
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

    /// Move within an artist's page, which stacks three sections.
    ///
    /// The same shape as Top results: a card section is a single row, so a
    /// vertical move leaves it, and only running off the top of the tracks
    /// steps back out of them.
    fn artist_move(&mut self, dir: Dir) {
        let height = self.last_main_height.saturating_sub(artistview::HEADER_ROWS);
        let Some(page) = self.artist.as_ref() else { return };

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

        match dir {
            Dir::Down | Dir::Up => {
                let down = dir == Dir::Down;
                if current == artistview::Section::Tracks {
                    let len = page.top_tracks.len();
                    // Already on the last track: `next` would clamp and the
                    // section below would be unreachable.
                    if down && self.artist_tracks.selected + 1 >= len {
                        if at + 1 < present.len() {
                            self.artist_section = present[at + 1].index();
                        }
                        return;
                    }
                    if down {
                        self.artist_tracks.next(len);
                    } else if self.artist_tracks.selected == 0 && at > 0 {
                        self.artist_section = present[at - 1].index();
                        self.scroll_artist_into_view(at - 1);
                        return;
                    } else {
                        self.artist_tracks.previous();
                    }
                    let visible = tracklist::visible_rows_chrome(
                        height,
                        false,
                        tracklist::Chrome::Bare,
                    );
                    self.artist_tracks.scroll_into_view(visible);
                    return;
                }
                let next = if down { at + 1 } else { at.saturating_sub(1) };
                if next < present.len() && (down || at > 0) {
                    self.artist_section = present[next].index();
                    self.scroll_artist_into_view(next);
                }
            }
            Dir::Left | Dir::Right => {
                // Asked of the same function the renderer draws from, so
                // a section added to one is not missed by the other.
                let len = artistview::cards(page, current).len();
                if len == 0 {
                    return;
                }
                // How many fit across, which is what a carousel scrolls by.
                let visible = carousel::visible_cards(self.last_main_width);
                let row = &mut self.artist_rows[current.index()];
                if dir == Dir::Right {
                    row.next(len, visible);
                } else {
                    row.previous(visible);
                }
            }
        }
    }


    /// What the section just moved to still needs fetching.
    ///
    /// Explore is a page of its own, and nothing else asks for it: without
    /// this the section drew an empty page until something else happened
    /// to trigger a load.
    fn load_if_needed(&mut self) -> Option<Action> {
        match self.sidebar.section() {
            sidebar::Section::Explore if self.explore.rows.is_empty() => {
                Some(Action::LoadExplore)
            }
            sidebar::Section::Feed if self.feed_cards.is_empty() => Some(Action::LoadFeed),
            sidebar::Section::MixesAndRadio
                if self.mixes.mine.is_empty() && self.mixes.radio.is_empty() =>
            {
                Some(Action::LoadMixes)
            }
            // The three that arrive together at sign-in. Any of them can be
            // dropped on the way in -- the reply is only taken when nothing
            // is open over the pane -- and nothing asked for them a second
            // time, so the section stayed empty for the rest of the session.
            sidebar::Section::Tracks if self.tracks.is_empty() => {
                Some(Action::LoadFavourites)
            }
            sidebar::Section::Playlists if self.playlists.is_empty() => {
                Some(Action::LoadFavourites)
            }
            sidebar::Section::Albums if self.albums.is_empty() => {
                Some(Action::LoadFavourites)
            }
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
        // What the user asked this key to mean, before anything reads it.
        // Rebinding is a translation rather than a table of its own: every
        // key below is guarded by what is on screen, and a table would have
        // to carry all of that or lose it.
        //
        // Not applied to the text boxes below: while one has the keyboard
        // `q` is a letter, and a config that could change that would be a
        // config that breaks typing.
        let key = KeyEvent { code: self.keymap.resolve(key.code), ..key };

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
        if self.showing_help {
            self.showing_help = false;
            // The modal wrote over cells the image protocols had marked
            // `Skip`, and on the next frame they go back to `Skip` — so
            // nothing is written to undo it and a patch of the help stays
            // on the artwork. Only a full repaint clears that.
            self.needs_repaint = true;
            return None;
        }

        match key.code {
            // "s" puts the keyboard back in the box, whether search is open
            // or not: with the box already up it did nothing at all, so
            // there was no way back to the query except closing search and
            // starting over.
            KeyCode::Char('s') if self.session.is_some() => Some(Action::BeginSearch),
            KeyCode::Esc if self.search.is_some() => Some(Action::CloseSearch),
            KeyCode::Char('/') if self.search.is_some() => Some(Action::BeginSearch),
            // Shift-/ on most layouts, so this is the one key to remember.
            KeyCode::Char('?') if self.session.is_some() => {
                self.showing_help = true;
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
            KeyCode::Char('b') if self.artist.is_some() => {
                self.artist_bio_open = !self.artist_bio_open;
                None
            }
            // The radio around whatever is playing. `R` reaches this from
            // anywhere, which is where the user is when the thought occurs
            // -- an artist's own radio moved to `S` for that reason.
            KeyCode::Char('R') if self.track_radio().is_some() => {
                Some(Action::PlayTrackRadio)
            }
            // The home page sends its track cards with `mixes: null`, so the
            // radio is not in hand there -- but the track's own endpoint has
            // it. Fetched on demand rather than leaving the key dead on the
            // one page most people start from.
            KeyCode::Char('R') if self.selected_track_id().is_some() => self
                .selected_track_id()
                .map(Action::FetchTrackRadio),
            // The artist's radio, which the web offers in its header: an
            // endless mix built around them. It is a mix like any other, so
            // it opens as one.
            KeyCode::Char('S') if self.artist_radio().is_some() => {
                Some(Action::PlayArtistRadio)
            }
            // The nav by number, 1 through 9: there are exactly nine
            // entries, Music first and Settings last, and reaching the far
            // end with J took eight presses.
            KeyCode::Char(c @ '1'..='9') if self.session.is_some() => {
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
            KeyCode::Char('l') | KeyCode::Right if self.artist.is_some() => {
                Some(Action::ArtistRight)
            }
            KeyCode::Char('h') | KeyCode::Left if self.artist.is_some() => {
                Some(Action::ArtistLeft)
            }
            // h and l move within the search results. They used to fall
            // straight through to the carousel, which in a search grid moved
            // a row of the home page nothing was drawing.
            KeyCode::Char('l') | KeyCode::Right
                if self.search.is_some() && self.open.is_none() =>
            {
                Some(Action::SearchRight)
            }
            KeyCode::Char('h') | KeyCode::Left
                if self.search.is_some() && self.open.is_none() =>
            {
                Some(Action::SearchLeft)
            }
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
            KeyCode::Char('h') | KeyCode::Left if self.session.is_some() => {
                // Inside an opened track list, left backs out of it; in a
                // grid it moves along the row, opened or not. A see-all is
                // an opened view too, and closing it there took the user out
                // of the page they were moving around in.
                if self.open.is_some() && !self.on_grid() {
                    Some(Action::CloseCollection)
                } else {
                    Some(Action::CarouselPrevious)
                }
            }
            // Favouriting the highlighted track, both ways: pressing it on
            // one that is already a favourite takes it out again.
            KeyCode::Char('A') if self.session.is_some() => Some(Action::ToggleFavourite),
            // The web client's "See all": the page hands back six items per
            // row, and this opens the rest of them.
            KeyCode::Char('o') if self.session.is_some() => Some(Action::SeeAll),
            // The transport, which the player bar has always drawn but
            // nothing was behind.
            KeyCode::Char('n') if self.session.is_some() => Some(Action::QueueNext),
            KeyCode::Char('p') if self.session.is_some() => Some(Action::QueuePrevious),
            KeyCode::Char('z') if self.session.is_some() => Some(Action::ToggleShuffle),
            KeyCode::Char('r') if self.session.is_some() => Some(Action::CycleRepeat),
            // "t" changes tab wherever there are tabs. It was bound to the
            // home page unconditionally, so in search it moved a tab strip
            // behind the view; and search had its own key, which meant the
            // same thing under two names depending on where you were.
            KeyCode::Char('t') if self.search.is_some() => Some(Action::NextSearchTab),
            KeyCode::Char('t') if self.session.is_some() => Some(Action::NextTab),
            // Tab keeps working in search too, since it is what the web
            // client's own tab strip responds to.
            KeyCode::Tab if self.search.is_some() => Some(Action::NextSearchTab),
            // Shift-J/K reach the sidebar without moving focus first. Kept
            // alongside the focus-aware j/k rather than replaced by them:
            // it is a shortcut people learn and then rely on.
            KeyCode::Char('J') if self.session.is_some() => Some(Action::SidebarNext),
            KeyCode::Char('K') if self.session.is_some() => Some(Action::SidebarPrevious),
            KeyCode::Enter if self.session.is_some() => Some(Action::ActivateSelection),
            KeyCode::Char(' ') if self.session.is_some() => Some(Action::TogglePause),
            _ => None,
        }
    }
}

/// Fetch whatever a collection holds and send it back as tracks.
///
/// Shared by opening a card and by "See all", so an album reached either way
/// arrives through the same path — and a reply for a view the user has since
/// left is dropped by `for_title` rather than shown under the wrong heading.
fn open_collection(
    app: &App,
    target: Option<Collection>,
    for_title: String,
    action_tx: &tokio::sync::mpsc::UnboundedSender<Action>,
) {
    // Both halves used to fall out of here without a word, and a session
    // that expired while the app was running made every open do exactly
    // nothing: no request, no error, no line in the log.
    let (Some(token), Some(target)) = (&app.session, target) else {
        if app.session.is_none() {
            tracing::warn!("cannot open a collection: there is no session");
            let _ = action_tx.send(Action::Error(
                "the session has expired — restart to sign in again".into(),
            ));
        }
        return;
    };
    let (client, tx) = (crate::tidal::Client::new(token.clone()), action_tx.clone());
    tokio::spawn(async move {
        let loaded = match &target {
            Collection::Playlist(uuid) => {
                crate::library::playlist_tracks(&client, uuid).await
            }
            Collection::Album(id) => crate::library::album_tracks(&client, *id).await,
            Collection::Mix(id) => crate::library::mix_tracks(&client, id).await,
            // A genre, a mood or a decade is a page of rows, not a list of
            // tracks — the same shape Explore itself is, so it comes back
            // as its own reply and takes the pane.
            Collection::Page { title, path } => {
                match crate::browse::page_of_rows(&client, path).await {
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
            Collection::Artist(id) => {
                match crate::library::artist_page(&client, *id).await {
                    Ok(page) => {
                        let _ = tx.send(Action::ArtistLoaded(Box::new(page)));
                        return;
                    }
                    Err(e) => Err(e),
                }
            }
            // A row's items come back as cards, since that is what the
            // page's own modules are. A row of tracks becomes a track list;
            // one of covers keeps its cards and gets the grid, since a
            // cover has no track to show in a list.
            Collection::Row { path, .. } => {
                match crate::browse::module_items(&client, path, ROW_ALL).await {
                    Ok(cards) => {
                        let tracks: Vec<crate::domain::Track> =
                            cards.iter().filter_map(track_from_card).collect();
                        if tracks.len() == cards.len() && !cards.is_empty() {
                            Ok(tracks)
                        } else {
                            let _ = tx.send(Action::RowLoaded { for_title, cards });
                            return;
                        }
                    }
                    Err(e) => Err(e),
                }
            }
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

/// How many items "See all" asks a row for.
///
/// More than a page: the fetch walks the module fifty at a time, which is
/// all the API serves at once. Not the row's whole depth — New Tracks is
/// two hundred and thirty-six — since that is five requests before anything
/// is drawn, and nobody scrolls that far to find out the list was worth
/// opening.
const ROW_ALL: u32 = 150;

/// The track a card stands for, when it stands for one.
///
/// A card carries what the page said about the item — enough for a row, and
/// the same fields the player fills in when a track is started from one.
fn track_from_card(card: &carousel::Card) -> Option<crate::domain::Track> {
    let Some(carousel::Target::Track(id)) = card.target.as_ref() else {
        return None;
    };
    Some(crate::domain::Track {
        id: crate::domain::TrackId(*id),
        title: card.title.clone(),
        artist: card.subtitle.clone(),
        album: card.detail.clone(),
        cover: card.cover_url.clone(),
        ..crate::domain::Track::sample("", "", card.duration)
    })
}

/// Fetch a track's stream and hand it to the player.
///
/// Shared by starting a track by hand and by the queue moving on, so the two
/// cannot drift: the bar is filled in from what is known now, and the
/// response says what was actually delivered.
fn start_track(
    app: &mut App,
    track: Option<crate::domain::Track>,
    action_tx: &tokio::sync::mpsc::UnboundedSender<Action>,
    cmd_tx: &std::sync::mpsc::Sender<crate::playback::Cmd>,
) {
    let (Some(token), Some(track)) = (&app.session, track) else { return };
    // What the user allows. Both paths into playback come through here, so a
    // blocked track cannot be reached by starting one by hand and then
    // letting the queue run on past it.
    if let Some(why) = app.why_blocked(&track) {
        tracing::info!("not playing {:?}: {why}", track.title);
        app.status = Some(format!("{} — {why}", track.title));
        return;
    }
    let client = crate::tidal::Client::new(token.clone());
    let (id, tx, cmds) = (track.id, action_tx.clone(), cmd_tx.clone());
    // What the user asked for, not what this build prefers. The response
    // still says what was actually delivered, and the badge reads that.
    let wanted = app.config.audio.quality();
    app.now_playing.track = Some(track);
    app.now_playing.playing = true;
    tokio::spawn(async move {
        // The response says what was actually delivered, which is not
        // always what was asked for.
        match client.playback_info(id, wanted).await
        {
            Ok(info) => {
                // The response is the only source of bit depth; the decoder
                // does not expose it.
                let _ = cmds.send(crate::playback::Cmd::Play {
                    manifest: info.manifest,
                    bit_depth: info.bit_depth,
                    delivered: info.delivered,
                });
            }
            Err(crate::tidal::TidalError::Unauthorized) => {
                let _ = tx.send(Action::SessionExpired);
            }
            Err(e) => {
                let _ = tx.send(Action::PlaybackFailed { id, message: e.to_string() });
            }
        }
    });
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
    let (keys, problems) = keymap::Keymap::from_config(&config.keys);
    for problem in &problems {
        tracing::warn!("keys: {problem}");
    }
    let http = reqwest::Client::new();

    let mut app = App {
        config: config.clone(),
        config_path: crate::config::paths::config_file(),
        keymap: keys,
        status: problems
            .first()
            .map(|p| format!("config: {p}")),
        // The one page of rows the tab strip belongs to; Explore and the
        // genres it opens are the same shape without it.
        home: home::HomeState { has_tabs: true, ..Default::default() },
        ..App::default()
    };
    // Resume an existing session rather than making the user log in again.
    //
    // Every branch below logs what it decided. Without that, a spurious
    // sign-out is indistinguishable from a missing file, an unreadable one,
    // or a token judged expired — which cost several rounds of guessing.
    let stored = match crate::auth::store::load() {
        Ok(Some(token)) => Some(token),
        Ok(None) => {
            tracing::info!("no stored session; showing the login screen");
            None
        }
        Err(e) => {
            // Do NOT delete the file here. An unreadable token is usually a
            // transient read failure, and removing it turns one bad launch
            // into a permanent sign-out.
            tracing::warn!("stored session could not be read: {e}");
            None
        }
    };
    if let Some(token) = stored {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if token.is_expired_at(now) {
            // Refresh tokens outlive access tokens by days, so this recovers
            // the common overnight case without sending the user to a
            // browser. If it fails, drop through to the login screen and
            // remove the stale file so it is not retried forever.
            match crate::auth::refresh(&http, &config.auth, &token.refresh_token).await {
                Ok(fresh) => {
                    tracing::info!("stored session refreshed");
                    if let Err(e) = crate::auth::store::save(&fresh) {
                        tracing::warn!("could not persist the refreshed token: {e}");
                    }
                    app.session = Some(fresh);
                }
                // Only the server saying no means the refresh token is
                // finished. A timeout or an unreachable host says nothing
                // about it, and deleting on those turns one launch on a bad
                // network into a permanent sign-out.
                Err(e) if !e.is_refusal() => {
                    tracing::warn!(
                        "could not reach the auth server ({e}); keeping the stored session"
                    );
                }
                Err(e) => {
                    tracing::warn!("the server refused the refresh token: {e}");
                    if let Err(e) = crate::auth::store::clear() {
                        tracing::warn!("could not clear the stale token: {e}");
                    }
                }
            }
        } else {
            tracing::info!(
                "resuming the stored session, {}s of validity left",
                token.expires_at.saturating_sub(now)
            );
            app.session = Some(token);
        }
    }

    let mut events = EventStream::new();
    // ~30fps ceiling: a burst of scroll events coalesces into one frame.
    let mut ticker = tokio::time::interval(Duration::from_millis(33));
    let (action_tx, mut action_rx) = tokio::sync::mpsc::unbounded_channel::<Action>();
    let (cmd_tx, mut playback_rx) = crate::playback::spawn();

    // The OS media keys. They reach the desktop, never the terminal, so the
    // app registers itself as a media player and the commands come back as
    // the same actions the keyboard sends.
    let (media_tx, media_rx) = mediakeys::channel();
    spawn_media_keys(action_tx.clone(), media_rx);

    // A resumed session needs the same fetches a fresh login triggers.
    // Without this the app opens already signed in and shows nothing.
    if let Some(token) = app.session.clone() {
        load_collection(token, action_tx.clone());
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
            // Side effects that must not block the render loop are spawned
            // here and report back through action_tx.
            match &action {
                // Keep the session alive while the app runs. It was
                // refreshed once, at startup, so a token with minutes left
                // when the app opened expired under it -- and every fetch
                // after that returned without a word: no request, no error,
                // nothing in the log. Opening a genre from Explore simply
                // did nothing.
                //
                // Renewed a few minutes early so a request is never sent
                // with a token that expires in flight, and rate-limited
                // because a tick fires about thirty times a second.
                Action::Tick if app.session_needs_renewing() => {
                    app.mark_session_renewed();
                    let (http, cfg, tx) =
                        (http.clone(), config.auth.clone(), action_tx.clone());
                    let refresh = app
                        .session
                        .as_ref()
                        .map(|t| t.refresh_token.clone())
                        .unwrap_or_default();
                    tokio::spawn(async move {
                        match crate::auth::refresh(&http, &cfg, &refresh).await {
                            Ok(fresh) => {
                                tracing::info!("session renewed while running");
                                if let Err(e) = crate::auth::store::save(&fresh) {
                                    tracing::warn!(
                                        "could not persist the renewed token: {e}"
                                    );
                                }
                                // `SessionRenewed`, not `Authenticated`: the
                                // latter is a fresh login and fetches the
                                // whole library, which on a renewal is a
                                // second copy of everything already held --
                                // and enough requests in a burst to be
                                // rate-limited for the ones that matter.
                                let _ = tx.send(Action::SessionRenewed(fresh));
                            }
                            // Same split as at startup: only the server
                            // saying no means the refresh token is finished.
                            Err(e) if !e.is_refusal() => {
                                tracing::warn!(
                                    "could not reach the auth server to renew ({e})"
                                );
                            }
                            Err(e) => {
                                tracing::warn!("the server refused the renewal: {e}");
                                let _ = tx.send(Action::SessionExpired);
                            }
                        }
                    });
                }
                Action::BeginLogin if app.session.is_none() => {
                    let (http, cfg, tx) =
                        (http.clone(), config.auth.clone(), action_tx.clone());
                    tokio::spawn(async move {
                        match crate::auth::start_login(&http, &cfg).await {
                            Ok(code) => {
                                let _ = tx.send(Action::LoginStarted(code.clone()));
                                // Save the user copying a URL out of a TUI.
                                // Best effort: the link stays on screen, so a
                                // failure here costs nothing.
                                login::open_in_browser(&code.verification_uri);
                                poll_until_granted(http, cfg, code, tx).await;
                            }
                            Err(e) => {
                                let _ = tx.send(Action::Error(e.to_string()));
                            }
                        }
                    });
                }
                Action::Authenticated(token) => {
                    // Log the success, not only the failure. A token that is
                    // gone by the next launch with nothing in the log cannot
                    // be told apart from one that was never written.
                    match crate::auth::store::save(token) {
                        Ok(()) => tracing::info!(
                            "stored the session at {:?}",
                            crate::config::paths::token_file()
                        ),
                        Err(e) => tracing::warn!("could not persist the token: {e}"),
                    }
                    load_collection(token.clone(), action_tx.clone());
                }
                // A track card plays rather than opening, and the home page
                // is full of them. Handled before the collection case, which
                // would otherwise swallow the action and do nothing.
                Action::ActivateSelection
                    if (app.on_grid() || app.on_home())
                        && app.selected_track_id().is_some() =>
                {
                    if let (Some(token), Some(id)) = (&app.session, app.selected_track_id()) {
                        let client = crate::tidal::Client::new(token.clone());
                        let (tx, cmds) = (action_tx.clone(), cmd_tx.clone());
                        let wanted = app.config.audio.quality();
                        // The bar needs something to show now, and the card
                        // is all we know about the track until it is fetched.
                        if let Some(card) = app.selected_card() {
                            app.now_playing.track = Some(crate::domain::Track {
                                id,
                                title: card.title,
                                artist: card.subtitle,
                                // A track card carries its album in `detail`.
                                album: card.detail,
                                cover: card.cover_url,
                                ..crate::domain::Track::sample("", "", card.duration)
                            });
                        }
                        tokio::spawn(async move {
                            match client.playback_info(id, wanted).await {
                                Ok(info) => {
                                    let _ = cmds.send(crate::playback::Cmd::Play {
                                        manifest: info.manifest,
                                        bit_depth: info.bit_depth,
                                        delivered: info.delivered,
                                    });
                                }
                                Err(crate::tidal::TidalError::Unauthorized) => {
                                    let _ = tx.send(Action::SessionExpired);
                                }
                                Err(e) => {
                                    let _ = tx.send(Action::PlaybackFailed {
                                        id,
                                        message: e.to_string(),
                                    });
                                }
                            }
                        });
                    }
                }
                // Read before `update` clears the page: the same reason an
                // opened card's title is.
                Action::PlayTrackRadio => {
                    if let Some(id) = app.track_radio() {
                        let title = app.track_radio_title();
                        open_collection(
                            &app,
                            Some(Collection::Mix(id)),
                            format!("{title} Radio"),
                            &action_tx,
                        );
                    }
                }
                // The card is a track whose radio the page did not send.
                // Fetch the track for it, then open the radio the same way
                // the key would have.
                Action::FetchTrackRadio(id) => {
                    if let Some(token) = &app.session {
                        let (client, tx) = (
                            crate::tidal::Client::new(token.clone()),
                            action_tx.clone(),
                        );
                        let id = *id;
                        tokio::spawn(async move {
                            match crate::library::track(&client, id).await {
                                Ok(track) => match track.radio {
                                    Some(mix) => {
                                        let _ = tx.send(Action::OpenMix {
                                            mix,
                                            title: format!("{} Radio", track.title),
                                            // The track's own artwork: a
                                            // radio built from it is about
                                            // that record.
                                            cover: track.cover,
                                        });
                                    }
                                    None => tracing::info!(
                                        "{:?} names no radio",
                                        track.title
                                    ),
                                },
                                Err(e) => tracing::warn!("could not fetch the track: {e}"),
                            }
                        });
                    }
                }
                // A mix to open, named. Same path a radio key takes, so what
                // arrives is a view like any other.
                Action::OpenMix { mix, title, .. } => {
                    open_collection(
                        &app,
                        Some(Collection::Mix(mix.clone())),
                        title.clone(),
                        &action_tx,
                    );
                }
                Action::PlayArtistRadio => {
                    // Read before `update` clears the page: the same reason
                    // an opened card's title is.
                    if let Some(id) = app.artist_radio() {
                        let name = app
                            .artist
                            .as_ref()
                            .map(|a| a.name.clone())
                            .unwrap_or_default();
                        open_collection(
                            &app,
                            Some(Collection::Mix(id)),
                            format!("{name} Radio"),
                            &action_tx,
                        );
                    }
                }
                // A card that opens something: a playlist, an album, an
                // artist, a mix, a genre page. Anything else falls through
                // to the arm below, which plays a track.
                Action::ActivateSelection if app.what_enter_opens().is_some() => {
                    if let Some((target, for_title)) = app.what_enter_opens() {
                        open_collection(&app, Some(target), for_title, &action_tx);
                    }
                }
                // "See all": the same load, for the whole of a home row
                // rather than one card's collection.
                Action::SeeAll => {
                    let target = app.selected_row();
                    let for_title = match &target {
                        Some(Collection::Row { heading, .. }) => heading.clone(),
                        _ => String::new(),
                    };
                    open_collection(&app, target, for_title, &action_tx);
                }
                Action::ToggleFavourite => {
                    let selected = app.selected_track();
                    if let (Some(token), Some(track)) = (&app.session, selected.as_ref()) {
                        let client = crate::tidal::Client::new(token.clone());
                        let (id, tx) = (track.id, action_tx.clone());
                        // What it should become, decided here so the request
                        // matches what the user saw when they pressed it.
                        let wanted = !app.favourites.contains(&id);
                        tokio::spawn(async move {
                            let result = if wanted {
                                crate::library::add_favourite_track(&client, id).await
                            } else {
                                crate::library::remove_favourite_track(&client, id).await
                            };
                            match result {
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
                    let (list, at) = app.selected_list();
                    if !list.is_empty() {
                        let repeat = app.queue.repeat;
                        let shuffled = app.queue.shuffled();
                        app.queue = crate::playback::Queue::new(list, at);
                        app.queue.repeat = repeat;
                        if shuffled {
                            app.queue.set_shuffled(
                                true,
                                &mut crate::playback::clock_rng(),
                            );
                        }
                    }
                    let selected = app.selected_track();
                    start_track(&mut app, selected, &action_tx, &cmd_tx);
                }
                // The queue decided which track; this fetches it. Same path
                // as starting one by hand, so a queued track and a chosen
                // one cannot drift apart.
                Action::PlayQueued => {
                    let next = app.queue.current().cloned();
                    start_track(&mut app, next, &action_tx, &cmd_tx);
                }
                // Autoplay: the queue ran out and the last track named a
                // radio. Fetched the way any mix is, then played from the
                // top -- so what follows is a queue like any other and the
                // skip keys work on it.
                Action::Autoplay(mix) => {
                    if let Some(token) = &app.session {
                        let (client, tx) = (
                            crate::tidal::Client::new(token.clone()),
                            action_tx.clone(),
                        );
                        let mix = mix.clone();
                        tokio::spawn(async move {
                            match crate::library::mix_tracks(&client, &mix).await {
                                Ok(tracks) if !tracks.is_empty() => {
                                    tracing::info!("autoplay: {} tracks", tracks.len());
                                    let _ = tx.send(Action::QueueRadio(tracks));
                                }
                                Ok(_) => tracing::info!("autoplay: the radio was empty"),
                                Err(e) => tracing::warn!("autoplay failed: {e}"),
                            }
                        });
                    }
                }
                Action::RunSearch(query) => {
                    let query = query.clone();
                    if let Some(token) = &app.session {
                        let (client, t) = (
                            crate::tidal::Client::new(token.clone()),
                            action_tx.clone(),
                        );
                        tokio::spawn(async move {
                            match crate::search::search(&client, &query).await {
                                Ok(results) => {
                                    tracing::info!(
                                        "search {query:?}: {} results",
                                        results.total()
                                    );
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
                Action::LoadTab(tab) => {
                    let tab = *tab;
                    if let Some(token) = &app.session {
                        let (client, t) = (
                            crate::tidal::Client::new(token.clone()),
                            action_tx.clone(),
                        );
                        tokio::spawn(async move {
                            match crate::browse::tab_page(&client, tab).await {
                                Ok(home) => {
                                    tracing::info!(
                                        "loaded {tab:?}: {} shortcuts, {} rows",
                                        home.shortcuts.len(),
                                        home.rows.len()
                                    );
                                    let _ = t.send(Action::HomeLoaded {
                                        tab,
                                        home: Box::new(home),
                                    });
                                }
                                Err(e) => {
                                    tracing::warn!("could not load {tab:?}: {e}");
                                    let _ = t.send(Action::Error(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Action::LoadMixes => {
                    if let Some(token) = &app.session {
                        let (client, t) = (
                            crate::tidal::Client::new(token.clone()),
                            action_tx.clone(),
                        );
                        tokio::spawn(async move {
                            match crate::browse::mixes(&client).await {
                                Ok(mixes) => {
                                    tracing::info!(
                                        "loaded {} mixes and {} stations",
                                        mixes.mine.len(),
                                        mixes.radio.len()
                                    );
                                    let _ = t.send(Action::MixesLoaded(Box::new(mixes)));
                                }
                                Err(e) => {
                                    tracing::warn!("could not load mixes: {e}");
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
                    if let Some(token) = &app.session {
                        load_collection(token.clone(), action_tx.clone());
                    }
                }
                Action::LoadFeed => {
                    if let Some(token) = &app.session {
                        let (client, t) = (
                            crate::tidal::Client::new(token.clone()),
                            action_tx.clone(),
                        );
                        tokio::spawn(async move {
                            match crate::browse::feed(&client).await {
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
                    if let Some(token) = &app.session {
                        let (client, t) = (
                            crate::tidal::Client::new(token.clone()),
                            action_tx.clone(),
                        );
                        tokio::spawn(async move {
                            match crate::browse::explore(&client).await {
                                Ok(home) => {
                                    tracing::info!(
                                        "loaded explore: {} rows",
                                        home.rows.len()
                                    );
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
                _ => {}
            }
            next = app.update(action);
        }
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
                more: None,
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
    token: crate::auth::StoredToken,
    tx: tokio::sync::mpsc::UnboundedSender<Action>,
) {
    let (client, t) = (crate::tidal::Client::new(token.clone()), tx.clone());
    tokio::spawn(async move {
        match crate::library::favourite_tracks(&client).await {
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

    let (client, t) = (crate::tidal::Client::new(token.clone()), tx.clone());
    tokio::spawn(async move {
        match crate::library::playlists(&client).await {
            Ok(playlists) => {
                tracing::info!("loaded {} playlists", playlists.len());
                let _ = t.send(Action::PlaylistsLoaded(playlists));
            }
            // A failure here costs the sidebar list, not the session — log it
            // rather than throwing the user back to the login screen.
            Err(e) => tracing::warn!("could not load playlists: {e}"),
        }
    });

    let (client, t) = (crate::tidal::Client::new(token.clone()), tx.clone());
    tokio::spawn(async move {
        match crate::library::albums(&client).await {
            Ok(albums) => {
                tracing::info!("loaded {} favourite albums", albums.len());
                let _ = t.send(Action::AlbumsLoaded(albums));
            }
            Err(e) => tracing::warn!("could not load albums: {e}"),
        }
    });

    let (client, t) = (crate::tidal::Client::new(token.clone()), tx.clone());
    tokio::spawn(async move {
        match crate::library::artists(&client).await {
            Ok(artists) => {
                tracing::info!("loaded {} favourite artists", artists.len());
                let _ = t.send(Action::ArtistsLoaded(artists));
            }
            Err(e) => tracing::warn!("could not load artists: {e}"),
        }
    });

    let (client, t) = (crate::tidal::Client::new(token), tx);
    tokio::spawn(async move {
        // The same call the tab strip makes: one page loaded two ways is
        // two ways for it to differ, and it did — switching tab and back
        // dropped the rows gathered from the other pages.
        match crate::browse::tab_page(&client, crate::browse::Tab::ForYou).await {
            Ok(home) => {
                tracing::info!(
                    "loaded home: {} shortcuts, {} rows",
                    home.shortcuts.len(),
                    home.rows.len()
                );
                let _ = t.send(Action::HomeLoaded {
                    tab: crate::browse::Tab::ForYou,
                    home: Box::new(home),
                });
            }
            Err(e) => tracing::warn!("could not load the home page: {e}"),
        }
    });
}

/// Poll on the interval TIDAL asked for until the user confirms or the code
/// expires. Runs in its own task so the UI stays responsive.
async fn poll_until_granted(
    http: reqwest::Client,
    cfg: crate::config::AuthConfig,
    code: crate::auth::DeviceCode,
    tx: tokio::sync::mpsc::UnboundedSender<Action>,
) {
    let mut interval = code.interval_secs;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(code.expires_in_secs);

    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;

        match crate::auth::poll_once(&http, &cfg, &code).await {
            Ok(crate::auth::PollOutcome::Granted(token)) => {
                let _ = tx.send(Action::Authenticated(token));
                return;
            }
            Ok(crate::auth::PollOutcome::Pending) => {
                let _ = tx.send(Action::LoginPolled(login::PollResult::Pending));
            }
            Ok(crate::auth::PollOutcome::SlowDown) => {
                interval += 2;
                let _ = tx.send(Action::LoginPolled(login::PollResult::SlowDown));
            }
            Ok(crate::auth::PollOutcome::Expired) => {
                let _ = tx.send(Action::LoginPolled(login::PollResult::Expired));
                return;
            }
            Err(e) => {
                let _ = tx.send(Action::Error(e.to_string()));
                return;
            }
        }
    }
    let _ = tx.send(Action::LoginPolled(login::PollResult::Expired));
}


/// The Mixes section's two tabs: the user's own, and TIDAL's stations.
pub const MIXES_TABS: [&str; 2] = ["My mixes", "Radio"];

/// Public so `examples/screenshot.rs` renders what the app actually renders.
/// A preview that assembles the layout itself drifts from the real one, and
/// then it is checking its own copy rather than the UI.
pub fn draw(frame: &mut ratatui::Frame, app: &mut App) {
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
    if let Some(search) = app.search.as_ref() {
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
                artists: &search.artists,
                albums: &search.albums,
                top: search.top,
                favourites: &app.favourites,
                playing: app.now_playing.track.as_ref().map(|t| t.id),
                tier: app.now_playing.tier,
            },
            |frame, area, url, shape| match art.as_mut() {
                Some(a) => a.render_shaped(frame, area, url, shape),
                None => false,
            },
        );
        app.artwork = art;
    }

    // An artist's page takes the pane, as search does.
    if let Some(page) = app.artist.as_ref() {
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
                favourites: &app.favourites,
                playing: app.now_playing.track.as_ref().map(|t| t.id),
                tier: app.now_playing.tier,
            },
            |frame, area, url, shape| match art.as_mut() {
                Some(a) => a.render_shaped(frame, area, url, shape),
                None => false,
            },
        );
        app.artwork = art;
    }

    // A view opened but not yet filled: its name and a line saying it is
    // on its way. Drawn here rather than by a section's renderer because
    // what it becomes is not known until the reply lands.
    if app.showing() == Showing::Loading {
        let title = app.open.as_ref().map(|o| o.title.as_str()).unwrap_or_default();
        loadingview::render(frame, regions.main, &palette, title);
    }
    let showing = match app.showing() {
        // Already drawn above; this keeps the match from drawing over it.
        Showing::Artist | Showing::Search(_) | Showing::Loading => None,
        // An opened row of covers is a grid, not a track list — Albums is
        // the section whose renderer draws two-line cards.
        Showing::Tracks => Some(sidebar::Section::Tracks),
        Showing::Cards => Some(sidebar::Section::Albums),
        // A page of rows is drawn by the home renderer, on whichever state
        // holds it.
        Showing::Rows => Some(app.sidebar.section()),
        Showing::Section(section) => Some(section),
    };

    if let Some(showing) = showing {
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
                    playing: app.now_playing.track.as_ref().map(|t| t.id),
                    tier: app.now_playing.tier,
                },
                |frame, area, url, shape| match art.as_mut() {
                    Some(a) => a.render_shaped(frame, area, url, shape),
                    None => false,
                },
            );
            app.artwork = art;
        }
        sidebar::Section::Feed => {
            // The Feed is a page of rows like the home page, but headed and
            // filtered like a grid: its chrome is drawn here and what is
            // left of the pane goes to the row renderer. The heading rows
            // come from the grid's own count so the two cannot drift.
            let area = scrollbar::reserve(regions.main);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(ratatui::text::Line::styled(
                    "Feed",
                    palette.page_heading(),
                )),
                ratatui::layout::Rect { height: 1, ..area },
            );
            grid::render_filter(
                frame,
                ratatui::layout::Rect { y: area.y + 2, height: inputbox::HEIGHT, ..area },
                &palette,
                "Filter releases",
                &grid::GridState {
                    filter: app.feed.filter.clone(),
                    ..Default::default()
                },
                app.filtering,
            );
            let header = grid::header_rows_with(grid::Chrome::Full, &[]);
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
                    playing: app.now_playing.track.as_ref().map(|t| t.id),
                    tier: app.now_playing.tier,
                },
                |frame, area, url, shape| match art.as_mut() {
                    Some(a) => a.render_shaped(frame, area, url, shape),
                    None => false,
                },
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
        | sidebar::Section::Profiles
        | sidebar::Section::MixesAndRadio) => {
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
                    sidebar::Section::MixesAndRadio => ("Mixes & Radio", "Filter mixes"),
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
                    // The Mixes section is the one grid with tabs.
                    tabs: if section == sidebar::Section::MixesAndRadio && opened.is_none()
                    {
                        (&MIXES_TABS[..], app.mixes_tab)
                    } else {
                        (&[], 0)
                    },
                },
                |frame, area, url, shape| match art.as_mut() {
                    Some(a) => a.render_shaped(frame, area, url, shape),
                    None => false,
                },
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
                    playing: app.now_playing.track.as_ref().map(|t| t.id),
                    tier: app.now_playing.tier,
                    banner: app.open.as_ref().map(|o| tracklist::Banner {
                        title: &o.title,
                        subtitle: &o.subtitle,
                        detail: &o.detail,
                        cover: o.cover.as_deref(),
                        round: o.round_cover,
                    }),
                },
                |frame, area, url, shape| match art.as_mut() {
                    Some(a) => a.render_shaped(frame, area, url, shape),
                    None => false,
                },
            );
            app.artwork = art;
        }
    }

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
        },
        |frame, area, url, shape| match art.as_mut() {
            Some(a) => a.render_shaped(frame, area, url, shape),
            None => false,
        },
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
        login::render(frame, frame.area(), &app.login);
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
                    id: i,
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
                        t.id = crate::domain::TrackId(i);
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
                        id: i,
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
                        id: i,
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
            });
            app.update(Action::ArtistLoaded(Box::new(crate::library::ArtistPage {
                name: "Daft Punk".into(),
                picture: None,
                top_tracks_path: None,
                top_tracks: some_tracks(&["One", "Two", "Three"]),
                albums: (0..4)
                    .map(|i| crate::library::Album {
                        id: i,
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
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
                    .unwrap();
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
                t.id = crate::domain::TrackId(i);
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

        let drawn = tracklist::visible_rows_with(app.last_main_height, true);
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
        let id = track.id;
        app.now_playing.track = Some(track);
        app.now_playing.playing = true;

        app.update(Action::PlaybackFailed {
            id,
            message: "500 from /tracks/0/playbackinfopostpaywall".into(),
        });

        assert!(app.now_playing.track.is_none(), "the bar is cleared");
        assert!(!app.now_playing.playing, "and it does not claim to be playing");
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
        second.id = crate::domain::TrackId(stale_id.0 + 1);
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
    fn browse_keys_do_nothing_before_login() {
        // Every browse arm is guarded on an existing session; without the
        // guards Enter would fire ActivateSelection in every state but
        // logged-out.
        let mut app = App::default();
        assert!(app.session.is_none());

        for code in [KeyCode::Char('j'), KeyCode::Char('k'), KeyCode::Tab, KeyCode::Char(' ')] {
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
            session: Some(sample_token()),
            // The home page is the one with the tab strip, as it is in the
            // running app.
            home: home::HomeState { has_tabs: true, ..Default::default() },
            token_path: Some(std::env::temp_dir().join("ratidal-test-signedin/token.json")),
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
        assert!(app.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)).is_none());
        assert!(app.filtering);

        for c in ['q', 'u', 'e'] {
            let action = app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
            assert!(action.is_none(), "{c} must not fire a command while filtering");
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
                id: i as u64,
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
            id: 1,
            name: "2Pac".into(),
            picture: None,
        }];
        assert!(matches!(
            app.selected_collection(),
            Some(Collection::Artist(1))
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
                id: i,
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
    fn h_backs_out_of_an_opened_album() {
        // The one place left where h means something other than "left".
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);
        assert!(matches!(
            key(&mut app, KeyCode::Char('h')),
            Some(Action::CloseCollection)
        ));
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
    fn a_vanished_token_is_written_back_while_running() {
        // The token has gone missing repeatedly without the app deleting it.
        // Once it is gone nothing rewrote it, so a valid session in memory
        // was lost at the next launch for no reason the user could see.
        let path = std::env::temp_dir().join("ratidal-test-restore/token.json");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());

        let mut app = App { session: Some(sample_token()), ..App::default() };
        app.restore_token_at(&path);

        assert!(path.exists(), "a missing token must be written back");
        let back = crate::auth::store::load_from(&path).unwrap().unwrap();
        assert_eq!(back.access_token, sample_token().access_token);
    }

    #[test]
    fn an_existing_token_is_left_alone() {
        // Rewriting a file that is already there would be pointless churn,
        // and would overwrite a token another process had just refreshed.
        let path = std::env::temp_dir().join("ratidal-test-untouched/token.json");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not valid json, deliberately").unwrap();

        let mut app = App { session: Some(sample_token()), ..App::default() };
        app.restore_token_at(&path);

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "not valid json, deliberately",
            "an existing file must not be overwritten"
        );
    }

    #[test]
    fn the_write_back_does_not_stat_on_every_frame() {
        // A tick fires about thirty times a second.
        let path = std::env::temp_dir().join("ratidal-test-ratelimit/token.json");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());

        let mut app = App { session: Some(sample_token()), ..App::default() };
        app.restore_token_at(&path);
        let first = app.last_token_check;
        assert!(first.is_some(), "the first call records when it checked");

        let _ = std::fs::remove_file(&path);
        app.restore_token_at(&path);
        assert_eq!(app.last_token_check, first, "a second call within the minute is skipped");
        assert!(!path.exists(), "and it did not write again");
    }

    #[test]
    fn a_session_near_its_end_is_renewed_before_it_expires() {
        // The bug this is here for: the session was refreshed once, at
        // startup. Opened with minutes left, the app ran on until the token
        // died under it, and every fetch after that returned in silence --
        // opening a genre from Explore did nothing at all, with nothing in
        // the log to say why.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let with = |secs_left: u64| App {
            session: Some(crate::auth::StoredToken {
                expires_at: now + secs_left,
                ..sample_token()
            }),
            ..App::default()
        };

        assert!(
            !with(60 * 60).session_needs_renewing(),
            "an hour left is not worth a request"
        );
        assert!(
            with(60).session_needs_renewing(),
            "a minute left has to be renewed before the next fetch"
        );
        assert!(
            with(0).session_needs_renewing(),
            "already expired, so certainly"
        );
        assert!(
            !App::default().session_needs_renewing(),
            "signed out, so there is nothing to renew"
        );
    }

    #[test]
    fn a_renewal_is_not_asked_for_thirty_times_a_second() {
        // A tick fires about thirty times a second, and a refresh is a
        // network request. Once it has been asked for, the answer is waited
        // for rather than asked again.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut app = App {
            session: Some(crate::auth::StoredToken {
                expires_at: now + 60,
                ..sample_token()
            }),
            ..App::default()
        };

        assert!(app.session_needs_renewing(), "due for renewal");
        app.mark_session_renewed();
        assert!(
            !app.session_needs_renewing(),
            "asked once, so not again on the very next tick"
        );
    }

    #[test]
    fn nothing_is_written_back_when_signed_out() {
        let path = std::env::temp_dir().join("ratidal-test-signedout/token.json");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());

        let mut app = App::default();
        app.restore_token_at(&path);
        assert!(app.last_token_check.is_none(), "no session, nothing to restore");
        assert!(!path.exists(), "and nothing written");
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
                id: i + 1,
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
    fn s_plays_the_artists_radio() {
        // The web offers it as a button in the artist's header: an endless
        // mix built around them. On `S` rather than `R`, which now starts
        // the radio around whatever is playing -- reachable from anywhere,
        // which is where the user is when the thought occurs.
        let mut app = signed_in(sidebar::Section::Profiles);
        let mut page = an_artist_page();
        page.radio = Some("mix-1".into());
        app.artist = Some(page);

        assert_eq!(app.artist_radio().as_deref(), Some("mix-1"));
        let action = key(&mut app, KeyCode::Char('S')).expect("S is bound here");
        assert!(matches!(action, Action::PlayArtistRadio));

        app.update(action);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some("Daft Punk Radio"),
            "the radio takes the pane, named for its artist"
        );
        assert!(app.artist.is_none(), "the page it came from stepped aside");

        // And the history returns to the artist.
        app.update(Action::GoBack);
        assert!(app.artist.is_some(), "back returns to the artist's page");
    }

    #[test]
    fn r_does_nothing_where_there_is_no_radio() {
        // Not every artist has one, and the key must not claim otherwise —
        // it is repeat's own key in capitals, so a stray press elsewhere
        // should fall through rather than open an empty view.
        let mut app = signed_in(sidebar::Section::Profiles);
        let mut page = an_artist_page();
        page.radio = None;
        app.artist = Some(page);
        assert!(app.artist_radio().is_none());
        assert!(
            !matches!(key(&mut app, KeyCode::Char('R')), Some(Action::PlayArtistRadio)),
            "R does not offer a radio that is not there"
        );
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
            more: None,
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

        let action = key(&mut app, KeyCode::Char('9')).expect("bound");
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
        assert!(matches!(action, Action::GoBack), "the same as `[`, got {action:?}");
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
        assert!(matches!(key(&mut app, KeyCode::Char('q')), Some(Action::Quit)));
    }

    #[test]
    fn escape_from_another_tab_comes_back_to_for_you() {
        // The home page is For you: landing on Rising because that is where
        // the user last was is not "back to the start".
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::NextTab);
        assert_ne!(app.home.tab, 0, "on another tab");

        let asked = app.update(Action::GoHome);
        assert_eq!(app.home.tab, 0, "escape returns to For you");
        assert!(
            matches!(asked, Some(Action::LoadTab(crate::browse::Tab::ForYou))),
            "and asks for its rows, got {asked:?}"
        );
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
        assert!(app.tracks.is_empty(), "the album's tracks are not left behind as favourites");
    }

    #[test]
    fn h_backs_out_of_an_open_album_instead_of_reaching_the_sidebar() {
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);

        let action = key(&mut app, KeyCode::Char('h'));
        assert!(matches!(action, Some(Action::CloseCollection)));
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
                target: Some(carousel::Target::Album(42)),
                ..Default::default()
            }],
            state: carousel::CarouselState::default(),
                    more: None,
        }];

        assert!(matches!(
            app.selected_collection(),
            Some(Collection::Album(42))
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
                target: Some(carousel::Target::Track(9)),
                ..Default::default()
            }],
            state: carousel::CarouselState::default(),
                    more: None,
        }];

        assert!(app.selected_collection().is_none());
        assert_eq!(app.selected_track_id(), Some(crate::domain::TrackId(9)));

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
                    more: None,
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
                target: Some(carousel::Target::Album(42)),
                ..Default::default()
            }],
            state: carousel::CarouselState::default(),
                    more: None,
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
        app.update(Action::TracksLoaded(some_tracks(&["Fav 1", "Fav 2", "Fav 3"])));

        assert_eq!(app.tracks.len(), 1, "the album's tracks are still what is shown");
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

        assert!(app.tracks.is_empty(), "a reply for a closed view is dropped");
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
    fn changing_tab_asks_for_that_tabs_page() {
        // Changing tab moved a highlight and nothing else: the three tabs
        // are three separate pages, and nothing went to fetch the new one.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            heading: "From the first tab".into(),
            kind: crate::browse::RowKind::Carousel,
            cards: vec![carousel::Card::new("Card", "Artist")],
            state: carousel::CarouselState::default(),
                    more: None,
        }];

        let next = app.update(Action::NextTab);

        assert!(
            matches!(next, Some(Action::LoadTab(crate::browse::Tab::StaffPicks))),
            "a tab change asks for its page, got {next:?}"
        );
        assert!(
            app.home.rows.is_empty(),
            "and the previous tab's rows are cleared rather than left showing"
        );
    }

    #[test]
    fn the_tabs_cycle_through_every_page() {
        // However many tabs there are: each press asks for the next one,
        // and the last comes back round to the first.
        let mut app = signed_in(sidebar::Section::Music);
        let all = crate::browse::Tab::ALL;
        let wanted: Vec<crate::browse::Tab> =
            all.iter().cycle().skip(1).take(all.len()).copied().collect();
        for expected in wanted {
            match app.update(Action::NextTab) {
                Some(Action::LoadTab(got)) => assert_eq!(got, expected),
                other => panic!("expected a load for {expected:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn rows_for_a_tab_the_user_has_left_are_dropped() {
        // The fetch is slow enough that a reply can arrive after another
        // tab has been chosen; it must not land under the wrong heading.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::NextTab); // now on Staff Picks

        let stale = crate::browse::Home {
            rows: vec![crate::browse::HomeRow {
                heading: "For you row".into(),
                kind: crate::browse::RowKind::Carousel,
                cards: vec![carousel::Card::new("Card", "Artist")],
                more: None,
            }],
            ..Default::default()
        };
        app.update(Action::HomeLoaded {
            tab: crate::browse::Tab::ForYou,
            home: Box::new(stale),
        });

        assert!(app.home.rows.is_empty(), "the stale reply is dropped");
    }

    #[test]
    fn rows_for_the_tab_on_screen_are_shown() {
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::NextTab); // Staff Picks

        let fresh = crate::browse::Home {
            rows: vec![crate::browse::HomeRow {
                heading: "Songs on repeat".into(),
                kind: crate::browse::RowKind::Tracks,
                cards: vec![carousel::Card::new("A track", "An artist")],
                more: None,
            }],
            ..Default::default()
        };
        app.update(Action::HomeLoaded {
            tab: crate::browse::Tab::StaffPicks,
            home: Box::new(fresh),
        });

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
                more: None,
            });
        }
        app.home = home;
        app.last_main_width = 120;
        // A height that actually cuts a row, found rather than assumed:
        // the row heights have changed under this test before.
        let height = (30..80u16)
            .find(|h| {
                home::last_row_is_cut(*h, false, &app.home.rows)
                    && home::visible_rows_of(*h, false, &app.home.rows) >= 3
            })
            .expect("some height cuts a row");
        app.last_main_height = height;

        let visible = home::visible_rows_of(height, false, &app.home.rows);
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
            id: 1,
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
    fn opening_an_artist_never_asks_for_a_square_photo() {
        // Reported as a flash from round to square and back. Opening an
        // artist shows a moment of the collection view before their page
        // arrives, and that view drew every banner cover square -- so the
        // avatar that was round in the grid turned square, then round again
        // when the page landed. The shape follows the card that was opened.
        let mut app = signed_in(sidebar::Section::Profiles);
        app.artists = vec![crate::library::Artist {
            id: 1,
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
            more: None,
        });

        assert_eq!(app.showing(), Showing::Rows, "a genre page is a page of rows");
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
                more: Some("pages/data/whatever".into()),
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
    fn the_hint_and_the_key_agree_about_a_row_that_fits() {
        // A row of one card that still has a path behind it: the API handed
        // back more than it drew. The hint used to ask whether the cards ran
        // past the edge, which they do not, while `o` asked whether there
        // was a path, which there is -- so the row showed nothing and the
        // key fetched a whole view of more. Both ask the same question now.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows.push(home::Row {
            heading: "A short row".into(),
            kind: crate::browse::RowKind::Carousel,
            cards: vec![carousel::Card::new("The only card", "artist")],
            state: carousel::CarouselState::default(),
            more: Some("/pages/data/whatever".into()),
        });

        let buf = geometry::draw(120, 30, |f, _area, _p| draw(f, &mut app));
        let text = geometry::text(&buf);
        assert!(
            text.contains("See all"),
            "there is more behind it, so it says so:\n{text}"
        );
        assert!(
            matches!(key(&mut app, KeyCode::Char('o')), Some(Action::SeeAll)),
            "and o opens it"
        );

        // With nothing behind it, neither offers anything.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows.push(home::Row {
            heading: "All of it".into(),
            kind: crate::browse::RowKind::Carousel,
            cards: vec![carousel::Card::new("The only card", "artist")],
            state: carousel::CarouselState::default(),
            more: None,
        });
        let buf = geometry::draw(120, 30, |f, _area, _p| draw(f, &mut app));
        let text = geometry::text(&buf);
        assert!(
            !text.contains("See all"),
            "nothing is behind it, so nothing offers to show it:\n{text}"
        );
        assert!(
            app.selected_row().is_none(),
            "and the key has nothing to open"
        );
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
        assert_ne!(app.sidebar.section(), before, "the newest step is still there");
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

    fn some_mixes() -> crate::browse::Mixes {
        crate::browse::Mixes {
            mine: (0..4).map(a_mix_card).collect(),
            radio: (0..3)
                .map(|i| {
                    let mut c = carousel::Card::new(format!("Station {i}"), "An Artist");
                    c.cover_url = Some("https://x/small".into());
                    c.target = Some(carousel::Target::Mix(format!("radio-{i}")));
                    c
                })
                .collect(),
        }
    }

    #[test]
    fn the_mixes_section_has_a_tab_each_for_mine_and_radio() {
        // The user's own mixes and TIDAL's stations are different things
        // and the section is named for both, so each gets a tab.
        let mut app = signed_in(sidebar::Section::MixesAndRadio);
        app.update(Action::MixesLoaded(Box::new(some_mixes())));

        let buf = geometry::draw(120, 30, |f, _area, _p| draw(f, &mut app));
        let text = geometry::text(&buf);
        assert!(text.contains("My mixes"), "both tabs are named:\n{text}");
        assert!(text.contains("Radio"));
        assert!(text.contains("My Mix 0"), "the first tab's cards:\n{text}");
        assert!(!text.contains("Station 0"), "and not the other tab's:\n{text}");

        // `t` switches, as it does on the home page and in search.
        let action = key(&mut app, KeyCode::Char('t')).expect("t is bound here");
        app.update(action);
        let buf = geometry::draw(120, 30, |f, _area, _p| draw(f, &mut app));
        let text = geometry::text(&buf);
        assert!(text.contains("Station 0"), "the stations now:\n{text}");
        assert!(!text.contains("My Mix 0"), "and not the mixes:\n{text}");
    }

    #[test]
    fn each_mixes_tab_keeps_its_own_place() {
        // One grid state per tab, so coming back finds it where it was —
        // the same rule the collection views follow.
        let mut app = signed_in(sidebar::Section::MixesAndRadio);
        app.update(Action::MixesLoaded(Box::new(some_mixes())));

        app.update(Action::CarouselNext); // second mix
        assert_eq!(app.mixes_grid.selected, 1);

        let action = key(&mut app, KeyCode::Char('t')).expect("t is bound here");
        app.update(action);
        assert_eq!(app.radio_grid.selected, 0, "the other tab starts at its own top");
        assert!(
            matches!(app.selected_collection(), Some(Collection::Mix(ref id)) if id == "radio-0"),
            "and enter opens a station, got {:?}",
            app.selected_collection()
        );

        let action = key(&mut app, KeyCode::Char('t')).expect("t is bound here");
        app.update(action);
        assert_eq!(app.mixes_grid.selected, 1, "back where it was left");
    }

    fn a_mix_card(n: usize) -> carousel::Card {
        let mut c = carousel::Card::new(format!("My Mix {n}"), "Some artists");
        c.cover_url = Some("https://x/small".into());
        c.target = Some(carousel::Target::Mix(format!("mix-{n}")));
        c
    }

    #[test]
    fn the_mixes_section_draws_its_own_cards() {
        // It fell through to the favourites list, so the section showed
        // tracks under a heading that promised mixes.
        let mut app = signed_in(sidebar::Section::MixesAndRadio);
        app.update(Action::MixesLoaded(Box::new(crate::browse::Mixes {
            mine: (0..4).map(a_mix_card).collect(),
            radio: vec![],
        })));

        let buf = geometry::draw(120, 30, |f, _area, _p| draw(f, &mut app));
        let text = geometry::text(&buf);
        assert!(text.contains("Mixes & Radio"), "its own heading:\n{text}");
        assert!(text.contains("My Mix 0"), "and its own cards:\n{text}");
    }

    #[test]
    fn opening_a_mix_asks_for_its_tracks() {
        // A mix's id is a string, so it could not ride in the numbered
        // target every other card uses.
        let mut app = signed_in(sidebar::Section::MixesAndRadio);
        app.update(Action::MixesLoaded(Box::new(crate::browse::Mixes {
            mine: (0..4).map(a_mix_card).collect(),
            radio: vec![],
        })));

        assert!(app.on_grid(), "the section is a card grid");
        assert!(
            matches!(app.selected_collection(), Some(Collection::Mix(ref id)) if id == "mix-0"),
            "enter opens the highlighted mix, got {:?}",
            app.selected_collection()
        );

        app.update(Action::ActivateSelection);
        assert_eq!(
            app.open.as_ref().map(|o| o.title.as_str()),
            Some("My Mix 0"),
            "and the view is headed by it"
        );
    }

    #[test]
    fn entering_the_mixes_section_asks_for_them_once() {
        // The fetch is spawned by the loop; what is checked here is that
        // the section asks, and stops asking once it has them.
        // Walked from the top, bounded: the nav clamps at the last entry,
        // so a loop that waits for a section it has passed never ends.
        let mut app = signed_in(sidebar::Section::Music);
        let mut asked = false;
        for _ in 0..sidebar::Section::ALL.len() {
            if app.sidebar.section() == sidebar::Section::MixesAndRadio {
                break;
            }
            asked |= matches!(app.update(Action::SidebarNext), Some(Action::LoadMixes));
        }
        assert_eq!(app.sidebar.section(), sidebar::Section::MixesAndRadio);
        assert!(asked, "arriving at the section asks for its mixes");

        app.update(Action::MixesLoaded(Box::new(crate::browse::Mixes {
            mine: vec![a_mix_card(0)],
            radio: vec![],
        })));
        app.update(Action::SidebarPrevious);
        assert!(
            !matches!(app.update(Action::SidebarNext), Some(Action::LoadMixes)),
            "with mixes in hand it does not ask again"
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
                    id: i,
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
                    id: i,
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
        assert!(matches!(key(&mut app, KeyCode::Char('s')), Some(Action::BeginSearch)));
        app.update(Action::BeginSearch);

        for c in ['q', 'u', 'e'] {
            assert!(key(&mut app, KeyCode::Char(c)).is_none(), "{c} must not act");
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
        assert!(matches!(key(&mut app, KeyCode::Esc), Some(Action::CloseSearch)));
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
        let mut app = signed_in(sidebar::Section::Music);
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
        results.tracks = some_tracks(&[
            "T0", "T1", "T2", "T3", "T4", "T5", "T6", "T7", "T8", "T9",
        ]);
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
        let drawn = tracklist::visible_rows_chrome(body, false, tracklist::Chrome::Bare);
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
            app.search.as_ref().unwrap().artists.selected,
            1,
            "l moves along the artist row"
        );
        app.update(Action::SearchLeft);
        assert_eq!(app.search.as_ref().unwrap().artists.selected, 0);
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
        app.queue = crate::playback::Queue::new(
            some_tracks(&["One", "Two", "Three"]),
            0,
        );

        let next = app.update(Action::Playback(
            crate::playback::PlaybackEvent::Finished,
        ));
        assert!(
            matches!(next, Some(Action::PlayQueued)),
            "the queue moves on and asks for the track"
        );
        assert_eq!(app.queue.current().unwrap().title, "Two");
    }

    #[test]
    fn the_last_track_of_a_queue_stops_rather_than_looping() {
        let mut app = signed_in(sidebar::Section::Tracks);
        app.queue = crate::playback::Queue::new(some_tracks(&["Only"]), 0);
        app.now_playing.playing = true;

        let next = app.update(Action::Playback(
            crate::playback::PlaybackEvent::Finished,
        ));
        assert!(next.is_none(), "nothing follows the last track");
        assert!(!app.now_playing.playing, "and the player stops");
    }

    #[test]
    fn n_and_p_step_through_the_queue() {
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = some_tracks(&["One", "Two", "Three"]);
        app.queue = crate::playback::Queue::new(app.tracks.clone(), 0);

        assert!(matches!(key(&mut app, KeyCode::Char('n')), Some(Action::QueueNext)));
        assert!(matches!(app.update(Action::QueueNext), Some(Action::PlayQueued)));
        assert_eq!(app.queue.current().unwrap().title, "Two");

        assert!(matches!(
            key(&mut app, KeyCode::Char('p')),
            Some(Action::QueuePrevious)
        ));
        assert!(matches!(app.update(Action::QueuePrevious), Some(Action::PlayQueued)));
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
        let id = app.tracks[0].id;
        app.now_playing.track = Some(app.tracks[0].clone());
        app.update(Action::PlaybackFailed { id, message: "nope".into() });

        assert!(app.queue.shuffled(), "shuffle is still on");
        assert_eq!(app.queue.repeat, Repeat::All, "and so is repeat");

        // The next track starts, which is where the grey showed up.
        app.now_playing.track = Some(app.tracks[1].clone());

        // What the bar draws comes from the queue, not from a copy that the
        // reset above would have cleared.
        let modes = nowplaying::Modes {
            shuffled: app.queue.shuffled(),
            repeat: app.queue.repeat,
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
        app.queue = crate::playback::Queue::new(
            some_tracks(&["One", "Two", "Three"]),
            0,
        );

        assert!(matches!(key(&mut app, KeyCode::Char('z')), Some(Action::ToggleShuffle)));
        app.update(Action::ToggleShuffle);
        assert!(app.queue.shuffled(), "shuffle is on");
        app.update(Action::ToggleShuffle);
        assert!(!app.queue.shuffled(), "and off again");

        assert!(matches!(key(&mut app, KeyCode::Char('r')), Some(Action::CycleRepeat)));
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
    fn a_status_message_does_not_cover_the_tab_strip() {
        // Errors are all that reach this now — a mode change says itself in
        // the transport row. It used to take the whole top line, which is
        // where the home page draws its tabs: over them the two read as one
        // garbled line, and clearing the line first hid the tabs for as
        // long as the message stood. It sits in the corner instead.
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            kind: crate::browse::RowKind::Carousel,
            heading: "The Hits".into(),
            cards: vec![carousel::Card::new("A Card", "An Artist")],
            state: carousel::CarouselState::default(),
            more: None,
        }];
        app.status = Some("Network error".into());

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let top = geometry::row(&buf, 0);

        assert!(top.contains("Network error"), "the message is shown: {top:?}");
        assert!(
            top.contains("For you"),
            "and the tabs are still readable beside it: {top:?}"
        );

        let notice = geometry::find(&buf, "Network error").expect("the notice");
        let tabs = geometry::find(&buf, "For you").expect("the tabs");
        assert!(
            notice.start > tabs.end,
            "the notice sits to the right of the tabs: {top:?}"
        );
    }

    fn home_with_a_track_row() -> App {
        let mut app = signed_in(sidebar::Section::Music);
        app.home.rows = vec![home::Row {
            kind: crate::browse::RowKind::Tracks,
            heading: "New Tracks".into(),
            cards: (0..6)
                .map(|i| carousel::Card {
                    title: format!("Track {i}"),
                    target: Some(carousel::Target::Track(i)),
                    ..Default::default()
                })
                .collect(),
            state: carousel::CarouselState::default(),
            more: Some("pages/data/abc".into()),
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
                    target: Some(carousel::Target::Album(i)),
                    ..Default::default()
                })
                .collect(),
            state: carousel::CarouselState::default(),
            more: Some("pages/data/xyz".into()),
        }];
        app
    }

    #[test]
    fn o_opens_a_carousel_row_too() {
        // Every module on the page carries a paging path, not just the
        // track grids: New Albums shows ten of a hundred and sixty-six.
        let mut app = home_with_a_carousel_row();
        assert!(matches!(key(&mut app, KeyCode::Char('o')), Some(Action::SeeAll)));
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
        app.update(Action::RowLoaded {
            for_title: "New Albums".into(),
            cards: (0..8)
                .map(|i| carousel::Card::new(format!("Album {i}"), "An Artist"))
                .collect(),
        });

        assert_eq!(app.open_cards.len(), 8, "the cards are held");
        assert!(app.on_grid(), "and the movement keys drive a grid");
        assert_eq!(
            app.grid_cards(app.sidebar.section()).len(),
            8,
            "which reads the opened row rather than the section behind it"
        );
    }

    #[test]
    fn closing_a_row_takes_its_cards_with_it() {
        // Left behind, they would show under whatever view was drawn next.
        let mut app = home_with_a_carousel_row();
        app.update(Action::SeeAll);
        app.update(Action::RowLoaded {
            for_title: "New Albums".into(),
            cards: vec![carousel::Card::new("Album", "An Artist")],
        });
        assert!(!app.open_cards.is_empty());

        app.update(Action::CloseCollection);
        assert!(app.open.is_none());
        assert!(app.open_cards.is_empty(), "and its cards are gone");
    }

    #[test]
    fn cards_for_a_row_the_user_has_left_are_dropped() {
        let mut app = home_with_a_carousel_row();
        app.update(Action::SeeAll);
        app.update(Action::RowLoaded {
            for_title: "Some Other Row".into(),
            cards: vec![carousel::Card::new("Album", "An Artist")],
        });
        assert!(app.open_cards.is_empty(), "not shown under the wrong heading");
    }

    #[test]
    fn o_opens_the_whole_of_a_home_row() {
        // The page hands back six items per row out of hundreds; the web
        // client has a "See all" for the rest and this is it.
        let mut app = home_with_a_track_row();
        assert!(matches!(key(&mut app, KeyCode::Char('o')), Some(Action::SeeAll)));

        app.update(Action::SeeAll);
        let open = app.open.as_ref().expect("the row is open");
        assert_eq!(open.title, "New Tracks", "under its own heading");
        assert!(app.tracks.is_empty(), "waiting for the reply");
        assert_eq!(app.tracklist.selected, 0, "from the top");
    }

    #[test]
    fn see_all_does_nothing_on_a_row_that_has_no_more() {
        // A carousel scrolls, so there is nothing behind it to open.
        let mut app = home_with_a_track_row();
        app.home.rows[0].more = None;
        app.update(Action::SeeAll);
        assert!(app.open.is_none(), "nothing opened");
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
        assert!(app.search.as_ref().unwrap().typing, "and it has the keyboard");
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
            shortcuts: Vec::new(),
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
            if matches!(app.update(Action::SidebarPrevious), Some(Action::LoadExplore)) {
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
            id: 1,
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
            c.target = Some(carousel::Target::Album(7));
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
        app.config.audio.set_quality(crate::domain::Quality::Lossless);

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
        app.config.audio.set_quality(crate::domain::Quality::HiResLossless);
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
        assert_eq!(app.config.audio.quality(), crate::domain::Quality::HiResLossless);

        // The other way too.
        app.update(Action::CarouselPrevious);
        assert_eq!(app.config.audio.quality(), crate::domain::Quality::Low);
    }

    #[test]
    fn playback_asks_for_the_configured_quality() {
        // The setting existed in the file for weeks and was read nowhere:
        // both request sites named HI_RES_LOSSLESS outright, so changing it
        // did nothing at all. The request is spawned onto the runtime with
        // a live client, so what is checked is that neither site has gone
        // back to naming a tier of its own.
        let source = include_str!("mod.rs");
        let asks: Vec<&str> = source
            .lines()
            .filter(|l| l.contains(".playback_info("))
            .collect();
        assert!(!asks.is_empty(), "playback_info is called somewhere");
        for line in asks {
            assert!(
                !line.contains("Quality::"),
                "this asks for a fixed quality rather than the configured one: {line}"
            );
        }
        // And the value handed to them comes from the config.
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
        app.config_path = Some(path.clone());
        app.config.audio.set_quality(crate::domain::Quality::HiResLossless);

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
            id: 1,
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
        assert_eq!(app.album_grid.selected, 0, "the grid behind it did not move");
    }

    fn an_artist_page() -> crate::library::ArtistPage {
        crate::library::ArtistPage {
            name: "Daft Punk".into(),
            picture: None,
            top_tracks: some_tracks(&["One More Time", "Aerodynamic"]),
            albums: (0..3)
                .map(|i| crate::library::Album {
                    id: i,
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
                    id: 100 + i,
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
        });
        app.update(Action::ArtistLoaded(Box::new(an_artist_page())));
        app.last_main_width = 100;
        app.last_main_height = 40;
        app
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
        assert_eq!(app.artist_rows[artistview::Section::Albums.index()].selected, 1, "along the album row");
        app.update(Action::ArtistLeft);
        assert_eq!(app.artist_rows[artistview::Section::Albums.index()].selected, 0);
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
            Some(Collection::Album(0))
        ));
    }

    #[test]
    fn a_similar_artist_opens_their_own_page() {
        let mut app = with_artist_open();
        app.artist_section = artistview::Section::Similar.index();
        assert!(matches!(
            app.selected_collection(),
            Some(Collection::Artist(100))
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
        assert_eq!(rows[0].cards.len(), 2, "today and six days ago are this week");
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
            matches!(key(&mut app, KeyCode::Char('A')), Some(Action::ToggleFavourite)),
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
            !matches!(key(&mut app, KeyCode::Char('A')), Some(Action::ToggleFavourite)),
            "A types rather than toggling while the box has the keyboard"
        );
    }

    #[test]
    fn the_reply_marks_the_track_it_was_for() {
        // The mark follows the account, not what was guessed when the key
        // was pressed.
        let mut app = signed_in(sidebar::Section::Tracks);
        app.tracks = some_tracks(&["One", "Two"]);
        let id = app.tracks[0].id;

        app.update(Action::FavouriteChanged { id, favourite: true });
        assert!(app.favourites.contains(&id), "added");

        app.update(Action::FavouriteChanged { id, favourite: false });
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
                t.id = crate::domain::TrackId(i);
                t
            })
            .collect();

        // The reply is for the first track; the user has since moved to the
        // third.
        app.update(Action::TrackNext);
        app.update(Action::TrackNext);
        app.update(Action::FavouriteChanged {
            id: crate::domain::TrackId(0),
            favourite: true,
        });

        assert!(
            app.favourites.contains(&crate::domain::TrackId(0)),
            "the track the reply was for"
        );
        assert!(
            !app.favourites.contains(&crate::domain::TrackId(2)),
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
    fn h_still_closes_an_album_opened_from_search() {
        // Backing out of an opened album has to keep working, or a result
        // opened by mistake is a dead end.
        let mut app = signed_in(sidebar::Section::Music);
        app.update(Action::BeginSearch);
        for c in "daft punk".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        app.update(Action::SearchLoaded(Box::new(some_results())));
        app.search.as_mut().unwrap().typing = false;
        app.open = Some(OpenCollection {
            title: "Discovery".into(),
            subtitle: "Daft Punk".into(),
            detail: String::new(),
            cover: None,
            round_cover: false,
            came_from: sidebar::Section::Music,
        });
        assert!(matches!(
            key(&mut app, KeyCode::Char('h')),
            Some(Action::CloseCollection)
        ));
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
    fn t_changes_tab_in_both_views() {
        // It meant one thing on the home page and another in search, so the
        // key that changes tab depended on which view you were in — and in
        // search it moved the home page's tabs behind the results.
        let mut app = signed_in(sidebar::Section::Music);
        assert!(matches!(key(&mut app, KeyCode::Char('t')), Some(Action::NextTab)));

        app.update(Action::BeginSearch);
        app.search.as_mut().unwrap().typing = false;
        assert!(
            matches!(key(&mut app, KeyCode::Char('t')), Some(Action::NextSearchTab)),
            "in search, t changes the search tabs"
        );
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

    fn sample_token() -> crate::auth::StoredToken {
        crate::auth::StoredToken {
            access_token: "at".into(),
            refresh_token: "rt".into(),
            expires_at: 1_800_000_000,
            country_code: "US".into(),
            user_id: 1,
        }
    }

    #[test]
    fn session_expired_clears_the_session_and_tracks_and_shows_the_login_modal() {
        let mut app = App {
            session: Some(sample_token()),
            // Point the session somewhere disposable. Without this the test
            // deleted the developer's own token on every run of the suite,
            // and the sign-outs that caused were blamed on everything but
            // the tests for hours.
            token_path: Some(std::env::temp_dir().join("ratidal-test-expired/token.json")),
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
    fn without_a_session_an_error_routes_to_the_login_modal() {
        // status is drawn under the full-screen login modal and is invisible
        // there; the message must go somewhere the user can actually see it.
        let mut app = App::default();
        assert!(app.session.is_none());

        app.update(Action::Error("network unreachable".into()));

        assert!(app.status.is_none(), "status is painted over and must stay empty");
        match app.login {
            login::LoginState::Failed(ref msg) => assert_eq!(msg, "network unreachable"),
            other => panic!("expected LoginState::Failed, got {other:?}"),
        }
    }

    #[test]
    fn with_a_session_an_error_still_routes_to_the_status_bar() {
        let mut app = App { session: Some(sample_token()), ..App::default() };

        app.update(Action::Error("playback failed".into()));

        assert_eq!(app.status.as_deref(), Some("playback failed"));
        assert!(
            matches!(app.login, login::LoginState::Idle),
            "an in-session error must not disturb login state"
        );
    }
}
