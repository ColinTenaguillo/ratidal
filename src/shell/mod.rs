pub mod artistview;
pub mod artwork;
pub mod carousel;
#[cfg(test)]
pub mod geometry;
pub mod grid;
pub mod help;
pub mod home;
pub mod inputbox;
pub mod layout;
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
    BeginLogin,
    SessionExpired,
    SidebarNext,
    SidebarPrevious,
    TrackNext,
    TrackPrevious,
    ActivateSelection,
    CloseCollection,
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
    /// An artist's page. Boxed: three sections of items.
    ArtistLoaded(Box<crate::library::ArtistPage>),
    ArtistLeft,
    ArtistRight,
    /// Fetch the Explore page. Its own request, since nothing else needs it.
    LoadExplore,
    /// Fetch the activity feed.
    LoadFeed,
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
    ToggleFocus,
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
    /// The views escape backs out through, outermost first. Empty when the
    /// main pane is showing a sidebar section rather than something opened
    /// from inside one.
    pub back: Vec<Level>,
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
    /// The activity feed: releases from the artists the user follows.
    pub feed: Vec<carousel::Card>,
    pub feed_grid: grid::GridState,
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
    pub artist_tracks: tracklist::TrackListState,
    pub artist_albums: grid::GridState,
    pub artist_similar: grid::GridState,
    pub playlists: Vec<crate::library::Playlist>,
    pub albums: Vec<crate::library::Album>,
    pub artists: Vec<crate::library::Artist>,
    /// One grid state per grid view, so moving around Albums does not disturb
    /// where the user was in Playlists.
    pub playlist_grid: grid::GridState,
    pub album_grid: grid::GridState,
    pub artist_grid: grid::GridState,
    pub focus: Focus,
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
    /// When the stored token was last checked for having gone missing, so a
    /// 30fps tick does not stat the filesystem on every frame.
    pub last_token_check: Option<std::time::Instant>,
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
            // Already cards: the feed is built from a shape of its own
            // rather than from a collection the app holds.
            sidebar::Section::Feed => self.feed.clone(),
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
        if self.on_grid() {
            &self.grid_state(self.sidebar.section()).filter
        } else {
            &self.tracklist.filter
        }
    }

    /// Set it, and pull the selection back into what is left — a filter that
    /// shrinks the list under a stale index selects nothing at all.
    pub fn set_filter(&mut self, text: String) {
        if self.on_grid() {
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
            sidebar::Section::Feed => &self.feed_grid,
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
            sidebar::Section::Feed => &mut self.feed_grid,
            sidebar::Section::MixesAndRadio if self.mixes_tab == 1 => &mut self.radio_grid,
            sidebar::Section::MixesAndRadio => &mut self.mixes_grid,
            _ => &mut self.playlist_grid,
        }
    }

    /// What the selected card in a grid opens, if it opens anything. The
    /// index is into the filtered list, which is why the filter is applied
    /// here rather than indexing the source directly.
    /// The whole of the home row the selection is in, when the page said
    /// there is more of it than the grid shows.
    pub fn selected_row(&self) -> Option<Collection> {
        if !self.on_home() {
            return None;
        }
        let row = self.rows_on_screen().current_row()?;
        Some(Collection::Row {
            heading: row.heading.clone(),
            path: row.more.clone()?,
        })
    }

    pub fn selected_collection(&self) -> Option<Collection> {
        match self.selected_card()?.target? {
            carousel::Target::Playlist(uuid) => Some(Collection::Playlist(uuid)),
            carousel::Target::Album(id) => Some(Collection::Album(id)),
            carousel::Target::Artist(id) => Some(Collection::Artist(id)),
            carousel::Target::Mix(id) => Some(Collection::Mix(id)),
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
            let grid = match section {
                artistview::Section::Albums => &self.artist_albums,
                _ => &self.artist_similar,
            };
            return cards.get(grid.selected).cloned();
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
            settings::Setting::Quality => None,
        }
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

    /// Remember what the main pane holds, so escape can put it back.
    fn push_level(&mut self) {
        self.back.push(Level {
            // Taken, not cloned: a search left open would draw over the
            // view being opened, which is what made the Albums and Profils
            // tabs look as though enter did nothing at all.
            search: self.search.take(),
            open: self.open.clone(),
            artist: self.artist.take(),
            tracks: std::mem::take(&mut self.tracks),
            tracklist: std::mem::take(&mut self.tracklist),
            open_cards: std::mem::take(&mut self.open_cards),
            open_grid: std::mem::take(&mut self.open_grid),
        });
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
    fn grid_geometry(&self) -> (usize, usize) {
        // The same count the renderer draws with: keeping a second copy of
        // this arithmetic let the keys reach cards that were never drawn.
        // The same rows the renderer leaves above the cards, tabs and all:
        // when the two disagreed the keys reached cards that were never
        // drawn.
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

    /// Whether the main pane is currently a card grid, which decides what the
    /// movement keys mean: a grid moves by rows, a table by lines.
    pub fn on_grid(&self) -> bool {
        // Whatever the sidebar still points at, what is drawn wins. Reading
        // the section alone left j and k driving the view behind the one on
        // screen — a selection nobody could see. Twice: once for an opened
        // album, once for search.
        // An artist page's card sections are grids too.
        if let Some(_page) = self.artist.as_ref() {
            return artistview::Section::from_index(self.artist_section)
                != artistview::Section::Tracks;
        }
        // A search tab showing cards is a grid, and so is an opened row of
        // covers — whatever the sidebar still points at.
        if let Some(state) = self.search.as_ref() {
            let tab = searchview::Tab::from_index(state.tab);
            // Top is a grid while the selection is on one of its card
            // rows, and a list while it is in the tracks — the keys have to
            // follow what is highlighted, not what the tab is called.
            if tab == searchview::Tab::Top {
                return state.top != searchview::TopSection::Tracks;
            }
            return !tab.is_tracks();
        }
        if !self.open_cards.is_empty() && self.open.is_some() {
            return true;
        }
        self.search.is_none()
            && self.open.is_none()
            && matches!(
                self.sidebar.section(),
                sidebar::Section::Playlists
                    | sidebar::Section::Albums
                    | sidebar::Section::Profiles
                    | sidebar::Section::Feed
                    | sidebar::Section::MixesAndRadio
            )
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
                self.sidebar.next();
                self.load_if_needed()
            }
            Action::SidebarPrevious => {
                self.sidebar.previous();
                self.load_if_needed()
            }
            Action::TracksLoaded(tracks) => {
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
                // Same rule as any other reply: one for a view the user has
                // since left would arrive under the wrong heading.
                match self.open.as_ref() {
                    Some(open) if open.title == page.name => self.artist = Some(*page),
                    _ => tracing::info!("dropping the page for {:?}", page.name),
                }
                None
            }
            Action::ExploreLoaded(home) => {
                let home = *home;
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
            Action::LoadMixes => None,
            Action::MixesLoaded(mixes) => {
                self.mixes = *mixes;
                None
            }
            Action::FeedLoaded(cards) => {
                self.feed = cards;
                self.feed_grid = grid::GridState::default();
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
            // The work happens in the event loop, which has the client and
            // can spawn; this arm is here so a caller of `update` alone does
            // not silently do nothing.
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
            Action::ToggleFavourite => None,
            // Applied here rather than in the loop, for the same reason
            // opening is: the loop's copy could only be exercised by running
            // the whole app.
            Action::SeeAll => {
                if let Some(Collection::Row { heading, .. }) = self.selected_row() {
                    self.open = Some(OpenCollection {
                        title: heading,
                        subtitle: String::new(),
                        detail: String::new(),
                        cover: None,
                        came_from: self.sidebar.section(),
                    });
                    self.focus = Focus::Main;
                    self.tracks.clear();
                    self.tracklist.selected = 0;
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
            Action::ToggleFocus => {
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::Main,
                    Focus::Main => Focus::Sidebar,
                };
                None
            }
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
                    // What is on screen now, so escape can come back to it.
                    // Opening from inside an opened view is the ordinary
                    // case — a profile, an artist, then one of their albums
                    // — and each of those is a step back, not a jump to the
                    // sidebar.
                    // Read before the level is put away: `push_level` takes
                    // the cards with it, and the identity is read off the
                    // card the user pressed enter on.
                    let identity = self.selected_identity();
                    self.push_level();
                    // The sidebar stays where it is. Moving it to Tracks made
                    // an opened album look like the favourites view and
                    // highlighted the wrong nav entry.
                    self.open = identity;
                    // A page from the last artist would show under the new
                    // one's name until its own reply arrived.
                    self.artist = None;
                    self.artist_section = 0;
                    self.artist_tracks = tracklist::TrackListState::default();
                    self.artist_albums = grid::GridState::default();
                    self.artist_similar = grid::GridState::default();
                    // The opened view is what the user is now looking at, so
                    // it gets the keys. Leaving focus on the grid behind it
                    // made the new view impossible to move around in.
                    self.focus = Focus::Main;
                    self.tracks.clear();
                    self.tracklist = tracklist::TrackListState::default();
                }
                None
            }
            Action::ActivateSelection => None, // playback is a side effect in run()
            Action::CloseCollection => {
                // One step back, to whatever was on screen when this view
                // was opened — which is the view below it when they are
                // nested, and the sidebar's section at the bottom of the
                // stack. Its own selection comes back with it.
                if self.open.is_some() {
                    match self.back.pop() {
                        Some(level) => {
                            self.search = level.search;
                            self.open = level.open;
                            self.artist = level.artist;
                            self.tracks = level.tracks;
                            self.tracklist = level.tracklist;
                            self.open_cards = level.open_cards;
                            self.open_grid = level.open_grid;
                        }
                        None => self.clear_open(),
                    }
                }
                None
            }
        }
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
        // Explore is drawn by the same renderer, so it moves the same way.
        self.search.is_none()
            && self.open.is_none()
            && matches!(
                self.sidebar.section(),
                sidebar::Section::Music | sidebar::Section::Explore
            )
    }

    /// Move within an artist's page, which stacks three sections.
    ///
    /// The same shape as Top results: a card section is a single row, so a
    /// vertical move leaves it, and only running off the top of the tracks
    /// steps back out of them.
    fn artist_move(&mut self, dir: Dir) {
        let (cols, rows) = self.artist_grid_geometry();
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
                }
            }
            Dir::Left | Dir::Right => {
                let len = match current {
                    artistview::Section::Albums => page.albums.len(),
                    artistview::Section::Similar => page.similar.len(),
                    artistview::Section::Tracks => return,
                };
                let grid = match current {
                    artistview::Section::Albums => &mut self.artist_albums,
                    _ => &mut self.artist_similar,
                };
                if dir == Dir::Right {
                    grid.next(len, cols, rows);
                } else {
                    grid.previous(cols, rows);
                }
            }
        }
    }

    /// Columns and rows of the grid an artist page's card sections draw into.
    fn artist_grid_geometry(&self) -> (usize, usize) {
        let body = self
            .last_main_height
            .saturating_sub(artistview::HEADER_ROWS);
        (grid::columns(self.last_main_width), grid::rows(body, 2))
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
            sidebar::Section::Feed if self.feed.is_empty() => Some(Action::LoadFeed),
            sidebar::Section::MixesAndRadio
                if self.mixes.mine.is_empty() && self.mixes.radio.is_empty() =>
            {
                Some(Action::LoadMixes)
            }
            _ => None,
        }
    }

    /// The page of rows on screen, whichever it is.
    fn rows_on_screen(&self) -> &home::HomeState {
        if self.sidebar.section() == sidebar::Section::Explore {
            &self.explore
        } else {
            &self.home
        }
    }

    fn rows_on_screen_mut(&mut self) -> &mut home::HomeState {
        if self.sidebar.section() == sidebar::Section::Explore {
            &mut self.explore
        } else {
            &mut self.home
        }
    }

    fn on_key(&mut self, key: KeyEvent) -> Option<Action> {
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
            // Shift-/ on most layouts, so this is the one key to remember.
            // "s" opens the search box; escape inside it closes search.
            // "s" puts the keyboard back in the box, whether search is open
            // or not: with the box already up it did nothing at all, so
            // there was no way back to the query except closing search and
            // starting over.
            KeyCode::Char('s') if self.session.is_some() => Some(Action::BeginSearch),
            KeyCode::Esc if self.search.is_some() => Some(Action::CloseSearch),
            KeyCode::Char('/') if self.search.is_some() => Some(Action::BeginSearch),
            KeyCode::Char('?') if self.session.is_some() => {
                self.showing_help = true;
                None
            }
            // Escape backs out of an opened album, and does nothing at the
            // outermost view. Quitting is `q` alone: escape means "leave
            // this view" everywhere else, and a key that usually steps back
            // one level should not sometimes close the app instead.
            KeyCode::Esc if self.open.is_some() => Some(Action::CloseCollection),
            KeyCode::Esc => None,
            KeyCode::Char('q') => Some(Action::Quit),
            // Every view but the home page has a filter box.
            KeyCode::Char('/') if self.session.is_some() && !self.on_home() => {
                self.filtering = true;
                None
            }
            KeyCode::Enter if self.session.is_none() => Some(Action::BeginLogin),
            // The movement keys act on whichever pane has focus. They used
            // to always drive the main pane, which left Tab doing nothing
            // visible and the sidebar reachable only through J/K — a binding
            // nothing else in the app uses and nobody would guess.
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
                // Inside an opened album, left backs out of it; elsewhere it
                // moves along the row.
                if self.open.is_some() {
                    Some(Action::CloseCollection)
                } else {
                    Some(Action::CarouselPrevious)
                }
            }
            // "t" changes tab wherever there are tabs. It was bound to the
            // home page unconditionally, so in search it moved a tab strip
            // behind the view; and search had its own key, which meant the
            // same thing under two names depending on where you were.
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
            KeyCode::Char('t') if self.search.is_some() => Some(Action::NextSearchTab),
            KeyCode::Char('t') if self.session.is_some() => Some(Action::NextTab),
            // Tab keeps working in search too, since it is what the web
            // client's own tab strip responds to.
            KeyCode::Tab if self.search.is_some() => Some(Action::NextSearchTab),
            KeyCode::Tab if self.session.is_some() => Some(Action::ToggleFocus),
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
    let (Some(token), Some(target)) = (&app.session, target) else { return };
    let (client, tx) = (crate::tidal::Client::new(token.clone()), action_tx.clone());
    tokio::spawn(async move {
        let loaded = match &target {
            Collection::Playlist(uuid) => {
                crate::library::playlist_tracks(&client, uuid).await
            }
            Collection::Album(id) => crate::library::album_tracks(&client, *id).await,
            Collection::Mix(id) => crate::library::mix_tracks(&client, id).await,
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
    let http = reqwest::Client::new();

    let mut app = App {
        config: config.clone(),
        config_path: crate::config::paths::config_file(),
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
                Action::ActivateSelection if app.on_grid() || app.on_home() => {
                    // Opening a playlist or album loads its tracks into the
                    // Tracks view and goes there, which is what the web
                    // client does when a card is clicked.
                    //
                    // Which view the reply belongs to is taken from the
                    // selected card, not from `app.open`: this runs before
                    // `update` applies the action, so `open` is still
                    // whatever was open before — the previous album, or
                    // nothing.
                    let for_title = app.selected_card().map(|c| c.title).unwrap_or_default();
                    open_collection(&app, app.selected_collection(), for_title, &action_tx);
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

/// Which pane the keyboard is driving.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    #[default]
    Main,
}

/// Public so `examples/screenshot.rs` renders what the app actually renders.
/// A preview that assembles the layout itself drifts from the real one, and
/// then it is checking its own copy rather than the UI.
/// The Mixes section's two tabs: the user's own, and TIDAL's stations.
pub const MIXES_TABS: [&str; 2] = ["My mixes", "Radio"];

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
        app.focus == Focus::Sidebar,
    );

    // The main pane shows the home page for Music, and a track table for the
    // sections that are a flat list. An opened playlist or album overrides
    // all of that: it is a place of its own, not a section.
    let main_focused = app.focus == Focus::Main;
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
                albums: &app.artist_albums,
                similar: &app.artist_similar,
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

    let showing = if app.search.is_some() || app.artist.is_some() {
        // Already drawn above; this keeps the match from drawing over it.
        None
    } else if app.open.is_some() {
        // An opened row of covers is a grid, not a track list — Albums is
        // the section whose renderer draws two-line cards.
        if app.open_cards.is_empty() {
            Some(sidebar::Section::Tracks)
        } else {
            Some(sidebar::Section::Albums)
        }
    } else {
        Some(app.sidebar.section())
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
        | sidebar::Section::Feed
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
                    sidebar::Section::Feed => ("Feed", "Filter releases"),
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
        // keys and invisible on screen.
        let mut app = signed_in(sidebar::Section::Feed);
        app.update(Action::FeedLoaded(
            (0..14)
                .map(|i| carousel::Card::new(format!("Release {i}"), "An Artist"))
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
                came_from: sidebar::Section::Profiles,
            });
            app.update(Action::ArtistLoaded(Box::new(crate::library::ArtistPage {
                name: "Daft Punk".into(),
                picture: None,
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
        app.focus = Focus::Main;

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

    fn signed_in(section: sidebar::Section) -> App {
        let mut app = App {
            session: Some(sample_token()),
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
    fn j_and_k_always_drive_the_content() {
        // Whichever pane was last focused, j and k move through the list in
        // front of you. Making them depend on focus meant remembering an
        // invisible mode before you could move at all.
        let mut app = signed_in(sidebar::Section::Tracks);

        for focus in [Focus::Main, Focus::Sidebar] {
            app.focus = focus;
            assert!(
                matches!(key(&mut app, KeyCode::Char('j')), Some(Action::TrackNext)),
                "j moves the list with focus on {focus:?}"
            );
            assert!(matches!(
                key(&mut app, KeyCode::Char('k')),
                Some(Action::TrackPrevious)
            ));
        }
    }

    #[test]
    fn h_and_l_stay_inside_the_content() {
        // They move along a carousel row; they no longer cross into the
        // sidebar, which has keys of its own.
        let mut app = signed_in(sidebar::Section::Playlists);
        app.playlists = (0..30)
            .map(|i| crate::library::Playlist::sample(&format!("P{i}"), i))
            .collect();
        app.focus = Focus::Sidebar;

        assert!(matches!(
            key(&mut app, KeyCode::Char('l')),
            Some(Action::CarouselNext)
        ));
        assert!(matches!(
            key(&mut app, KeyCode::Char('h')),
            Some(Action::CarouselPrevious)
        ));
        assert_eq!(app.focus, Focus::Sidebar, "movement keys do not change focus");
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
        app.focus = Focus::Main;
        assert!(matches!(
            key(&mut app, KeyCode::Char('J')),
            Some(Action::SidebarNext)
        ));
        assert!(matches!(
            key(&mut app, KeyCode::Char('K')),
            Some(Action::SidebarPrevious)
        ));
        assert_eq!(app.focus, Focus::Main, "the shortcut must not steal focus");
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
    fn escape_closes_an_open_album_and_never_quits() {
        // Leaving the app is a bigger step than leaving a view, and escape
        // is the key that means "leave this view" everywhere else — so it
        // steps back where there is somewhere to go and does nothing where
        // there is not. Quitting is `q` alone.
        let mut app = with_albums(3);
        app.update(Action::ActivateSelection);

        let action = key(&mut app, KeyCode::Esc);
        assert!(matches!(action, Some(Action::CloseCollection)));
        assert!(!app.should_quit, "escape must not have quit");

        app.update(Action::CloseCollection);
        assert!(app.open.is_none(), "the album is closed");

        // With nothing open it does nothing at all.
        assert!(
            key(&mut app, KeyCode::Esc).is_none(),
            "escape at the outermost view is not a way out of the app"
        );
        assert!(!app.should_quit);

        // `q` is.
        assert!(matches!(key(&mut app, KeyCode::Char('q')), Some(Action::Quit)));
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
        app.focus = Focus::Main;

        let action = key(&mut app, KeyCode::Char('h'));
        assert!(matches!(action, Some(Action::CloseCollection)));
        assert_eq!(app.focus, Focus::Main, "focus is not what changed");
    }

    #[test]
    fn opening_an_album_gives_it_the_keyboard() {
        // Without this the opened view could not be moved around in: focus
        // stayed on the grid behind it, so j and k drove a list nobody could
        // see any more.
        let mut app = with_albums(3);
        app.focus = Focus::Sidebar;

        app.update(Action::ActivateSelection);

        assert!(app.open.is_some());
        assert_eq!(app.focus, Focus::Main, "the opened view has the keys");
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
    fn o_still_opens_a_row_that_shows_no_see_all() {
        // The hint is hidden when a row's cards all fit, but the key stays
        // bound: a row can grow between one draw and the next, and a key
        // that works only when a hint is drawn is a key nobody trusts.
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
            !text.contains("See all"),
            "one card fits, so nothing offers to show the rest:\n{text}"
        );
        assert!(
            matches!(key(&mut app, KeyCode::Char('o')), Some(Action::SeeAll)),
            "and o opens it anyway"
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
        let shuffle = geometry::find(&buf, "⤨").expect("the shuffle button");
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

        let buf = geometry::draw(80, 20, |f, _area, _p| draw(f, &mut app));
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
        }
    }

    fn with_artist_open() -> App {
        let mut app = signed_in(sidebar::Section::Profiles);
        app.open = Some(OpenCollection {
            title: "Daft Punk".into(),
            subtitle: String::new(),
            detail: String::new(),
            cover: None,
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
        assert_eq!(app.artist_albums.selected, 1, "along the album row");
        app.update(Action::ArtistLeft);
        assert_eq!(app.artist_albums.selected, 0);
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
    fn the_movement_keys_drive_the_feeds_own_grid() {
        // `grid_state_mut` routes by section, so without the feed in it the
        // keys would move the playlist grid behind — the fault that has
        // come back five times.
        let mut app = signed_in(sidebar::Section::Feed);
        app.update(Action::FeedLoaded(
            (0..6)
                .map(|i| carousel::Card::new(format!("Release {i}"), "An Artist"))
                .collect(),
        ));
        app.last_main_width = 100;
        app.last_main_height = 30;

        assert!(app.on_grid(), "the feed moves like a grid");
        app.update(Action::CarouselNext);
        assert_eq!(app.feed_grid.selected, 1, "the feed's own selection moved");
        assert_eq!(app.playlist_grid.selected, 0, "and no other grid's did");
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
