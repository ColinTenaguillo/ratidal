pub mod artwork;
pub mod carousel;
pub mod grid;
pub mod help;
pub mod home;
pub mod layout;
pub mod login;
pub mod nowplaying;
pub mod sidebar;
pub mod theme;
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
    TracksLoaded(Vec<crate::domain::Track>),
    PlaylistsLoaded(Vec<crate::library::Playlist>),
    AlbumsLoaded(Vec<crate::library::Album>),
    ArtistsLoaded(Vec<crate::library::Artist>),
    /// Boxed: the home page is by far the largest payload an Action carries,
    /// and every other variant would otherwise grow to match it.
    HomeLoaded(Box<crate::browse::Home>),
    CarouselNext,
    CarouselPrevious,
    RowNext,
    RowPrevious,
    NextTab,
    ToggleFocus,
    Playback(crate::playback::PlaybackEvent),
    TogglePause,
}

/// What opening a card in a grid loads.
#[derive(Debug, Clone)]
pub enum Collection {
    Playlist(String),
    Album(u64),
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
    pub now_playing: nowplaying::NowPlaying,
    pub home: home::HomeState,
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
        match section {
            sidebar::Section::Playlists => self
                .playlists
                .iter()
                .map(|p| carousel::Card {
                    title: p.title.clone(),
                    subtitle: p.creator.clone(),
                    detail: format!("{} tracks", p.track_count),
                    cover_url: p.cover.clone(),
                    round: false,
                })
                .collect(),
            sidebar::Section::Albums => self
                .albums
                .iter()
                .map(|a| carousel::Card {
                    title: a.title.clone(),
                    subtitle: a.artist.clone(),
                    detail: a.year.clone().unwrap_or_default(),
                    cover_url: a.cover.clone(),
                    round: false,
                })
                .collect(),
            sidebar::Section::Profiles => self
                .artists
                .iter()
                .map(|a| carousel::Card {
                    title: a.name.clone(),
                    cover_url: a.picture.clone(),
                    round: true,
                    ..Default::default()
                })
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
        match section {
            sidebar::Section::Albums => &self.album_grid,
            sidebar::Section::Profiles => &self.artist_grid,
            _ => &self.playlist_grid,
        }
    }

    pub fn grid_state_mut(&mut self, section: sidebar::Section) -> &mut grid::GridState {
        match section {
            sidebar::Section::Albums => &mut self.album_grid,
            sidebar::Section::Profiles => &mut self.artist_grid,
            _ => &mut self.playlist_grid,
        }
    }

    /// What the selected card in a grid opens, if it opens anything. The
    /// index is into the filtered list, which is why the filter is applied
    /// here rather than indexing the source directly.
    pub fn selected_collection(&self) -> Option<Collection> {
        let section = self.sidebar.section();
        let state = self.grid_state(section);
        let cards = self.grid_cards(section);
        // The index into the unfiltered list, which is what indexes
        // `playlists` and `albums`. Matching by position in the filtered
        // list would point at the wrong item whenever a filter is set.
        let index = grid::filter_indices(&cards, &state.filter)
            .get(state.selected)
            .copied()?;

        match section {
            sidebar::Section::Playlists => {
                self.playlists.get(index).map(|p| Collection::Playlist(p.uuid.clone()))
            }
            sidebar::Section::Albums => self.albums.get(index).map(|a| Collection::Album(a.id)),
            // An artist has no track list of its own to open.
            _ => None,
        }
    }

    /// Whether the selection in the main pane is already at its left edge,
    /// which is when `h` should leave for the sidebar instead of moving.
    fn at_left_edge(&self) -> bool {
        if self.on_grid() {
            let (cols, _) = self.grid_geometry();
            return self.grid_state(self.sidebar.section()).selected.is_multiple_of(cols.max(1));
        }
        if self.on_home() {
            return self
                .home
                .current_row()
                .is_none_or(|row| row.state.selected == 0);
        }
        // The track table has no horizontal movement, so h always leaves.
        true
    }

    /// Columns and visible rows of the current grid, measured from the pane
    /// the renderer last drew into.
    fn grid_geometry(&self) -> (usize, usize) {
        let lines = match self.sidebar.section() {
            sidebar::Section::Playlists => 3,
            sidebar::Section::Albums => 2,
            _ => 1,
        };
        // The heading, filter box and their blank lines sit above the cards.
        let body = self.last_main_height.saturating_sub(4);
        (grid::columns(self.last_main_width), grid::rows(body, lines))
    }

    /// Whether the main pane is currently a card grid, which decides what the
    /// movement keys mean: a grid moves by rows, a table by lines.
    pub fn on_grid(&self) -> bool {
        matches!(
            self.sidebar.section(),
            sidebar::Section::Playlists | sidebar::Section::Albums | sidebar::Section::Profiles
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
                if self.on_grid() {
                    // A grid moves by whole rows: one card at a time down a
                    // six-wide grid would be six presses per line.
                    let (cols, rows) = self.grid_geometry();
                    let len = self.grid_cards(self.sidebar.section()).len();
                    self.grid_state_mut(self.sidebar.section()).next_row(len, cols, rows);
                } else {
                    let len = tracklist::filter(&self.tracks, &self.tracklist.filter).len();
                    self.tracklist.next(len);
                    let visible = tracklist::visible_rows(self.last_main_height);
                    self.tracklist.scroll_into_view(visible);
                }
                None
            }
            Action::TrackPrevious => {
                if self.on_grid() {
                    let (cols, rows) = self.grid_geometry();
                    self.grid_state_mut(self.sidebar.section()).previous_row(cols, rows);
                } else {
                    self.tracklist.previous();
                    let visible = tracklist::visible_rows(self.last_main_height);
                    self.tracklist.scroll_into_view(visible);
                }
                None
            }
            Action::SidebarNext => {
                self.sidebar.next();
                None
            }
            Action::SidebarPrevious => {
                self.sidebar.previous();
                None
            }
            Action::TracksLoaded(tracks) => {
                self.tracks = tracks;
                self.tracklist.selected = 0;
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
            Action::HomeLoaded(home) => {
                let home = *home;
                self.home.shortcuts = home.shortcuts;
                self.home.rows = home
                    .rows
                    .into_iter()
                    .map(|(heading, cards)| home::Row {
                        heading,
                        cards,
                        state: carousel::CarouselState::default(),
                    })
                    .collect();
                None
            }
            Action::CarouselNext => {
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
                if let Some(row) = self.home.current_row_mut() {
                    let len = row.cards.len();
                    row.state.next(len, visible);
                }
                None
            }
            Action::CarouselPrevious => {
                if self.on_grid() {
                    let (cols, rows) = self.grid_geometry();
                    self.grid_state_mut(self.sidebar.section()).previous(cols, rows);
                    return None;
                }
                let visible = carousel::visible_cards(
                    self.last_main_width.saturating_sub(1),
                );
                if let Some(row) = self.home.current_row_mut() {
                    row.state.previous(visible);
                }
                None
            }
            Action::RowNext => {
                self.home.row_down();
                None
            }
            Action::RowPrevious => {
                self.home.row_up();
                None
            }
            Action::NextTab => {
                self.home.next_tab();
                None
            }
            Action::ToggleFocus => {
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::Main,
                    Focus::Main => Focus::Sidebar,
                };
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
                    crate::playback::PlaybackEvent::Started { bit_depth, sample_rate } => {
                        // Report what was DELIVERED.
                        let bits = bit_depth
                            .map(|b| format!("{b}-bit "))
                            .unwrap_or_default();
                        self.now_playing.quality = Some(format!(
                            "{bits}{:.1}kHz",
                            sample_rate as f64 / 1000.0
                        ));
                        self.now_playing.playing = true;
                    }
                    crate::playback::PlaybackEvent::Finished => {
                        self.now_playing.playing = false;
                        self.now_playing.position = std::time::Duration::ZERO;
                    }
                    crate::playback::PlaybackEvent::Error(e) => {
                        self.status = Some(e);
                        self.now_playing.playing = false;
                    }
                }
                None
            }
            Action::ActivateSelection => None, // handled as a side effect in run()
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
        self.sidebar.section() == sidebar::Section::Music
    }

    fn on_key(&mut self, key: KeyEvent) -> Option<Action> {
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
            return None;
        }

        match key.code {
            // Shift-/ on most layouts, so this is the one key to remember.
            KeyCode::Char('?') if self.session.is_some() => {
                self.showing_help = true;
                None
            }
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
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
            KeyCode::Char('j') | KeyCode::Down if self.session.is_some() => {
                match self.focus {
                    Focus::Sidebar => Some(Action::SidebarNext),
                    Focus::Main if self.on_home() => Some(Action::RowNext),
                    Focus::Main => Some(Action::TrackNext),
                }
            }
            KeyCode::Char('k') | KeyCode::Up if self.session.is_some() => {
                match self.focus {
                    Focus::Sidebar => Some(Action::SidebarPrevious),
                    Focus::Main if self.on_home() => Some(Action::RowPrevious),
                    Focus::Main => Some(Action::TrackPrevious),
                }
            }
            // h and l cross between the panes as well as moving within one,
            // so the sidebar can be left without reaching for Tab.
            KeyCode::Char('l') | KeyCode::Right if self.session.is_some() => {
                if self.focus == Focus::Sidebar {
                    self.focus = Focus::Main;
                    None
                } else {
                    Some(Action::CarouselNext)
                }
            }
            KeyCode::Char('h') | KeyCode::Left if self.session.is_some() => {
                match self.focus {
                    Focus::Sidebar => None,
                    // At the leftmost card, h leaves for the sidebar rather
                    // than doing nothing.
                    Focus::Main if self.at_left_edge() => {
                        self.focus = Focus::Sidebar;
                        None
                    }
                    Focus::Main => Some(Action::CarouselPrevious),
                }
            }
            KeyCode::Char('t') if self.session.is_some() => Some(Action::NextTab),
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

pub async fn run(
    terminal: &mut DefaultTerminal,
    picker: ratatui_image::picker::Picker,
) -> anyhow::Result<()> {
    let config = crate::config::Config::load()?;
    let http = reqwest::Client::new();

    let mut app = App::default();
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
    app.artwork = Some(artwork::Artwork::with_picker(picker, art_tx));

    while !app.should_quit {
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
                Action::ActivateSelection if app.on_grid() => {
                    // Opening a playlist or album loads its tracks into the
                    // Tracks view and goes there, which is what the web
                    // client does when a card is clicked.
                    if let (Some(token), Some(target)) =
                        (&app.session, app.selected_collection())
                    {
                        let (client, tx) = (
                            crate::tidal::Client::new(token.clone()),
                            action_tx.clone(),
                        );
                        app.sidebar.select(sidebar::Section::Tracks);
                        app.tracklist = tracklist::TrackListState::default();
                        tokio::spawn(async move {
                            let loaded = match &target {
                                Collection::Playlist(uuid) => {
                                    crate::library::playlist_tracks(&client, uuid).await
                                }
                                Collection::Album(id) => {
                                    crate::library::album_tracks(&client, *id).await
                                }
                            };
                            match loaded {
                                Ok(tracks) => {
                                    let _ = tx.send(Action::TracksLoaded(tracks));
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
                }
                Action::ActivateSelection => {
                    // The selection indexes the filtered list, not the whole
                    // one: taking it from `app.tracks` directly would play a
                    // different track than the highlighted row whenever a
                    // filter was in effect.
                    let selected = tracklist::filter(&app.tracks, &app.tracklist.filter)
                        .get(app.tracklist.selected)
                        .map(|t| (*t).clone());
                    if let (Some(token), Some(track)) = (&app.session, selected.as_ref()) {
                        let client = crate::tidal::Client::new(token.clone());
                        let (id, tx, cmds) =
                            (track.id, action_tx.clone(), cmd_tx.clone());
                        app.now_playing.track = Some(track.clone());
                        tokio::spawn(async move {
                            // Always request hi-res; the response says what
                            // was actually delivered.
                            match client
                                .playback_info(id, crate::domain::Quality::HiResLossless)
                                .await
                            {
                                Ok(info) => {
                                    // The response is the only source of bit
                                    // depth; the decoder does not expose it.
                                    let _ = cmds.send(crate::playback::Cmd::Play {
                                        manifest: info.manifest,
                                        bit_depth: info.bit_depth,
                                    });
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
                }
                Action::TogglePause => {
                    let cmd = if app.now_playing.playing {
                        crate::playback::Cmd::Pause
                    } else {
                        crate::playback::Cmd::Resume
                    };
                    let _ = cmd_tx.send(cmd);
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
        match crate::browse::home(&client).await {
            Ok(home) => {
                tracing::info!(
                    "loaded home: {} shortcuts, {} rows",
                    home.shortcuts.len(),
                    home.rows.len()
                );
                let _ = t.send(Action::HomeLoaded(Box::new(home)));
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
pub fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    let regions = layout::split(frame.area());
    let palette = app.palette;
    app.last_main_width = regions.main.width;
    app.last_main_height = regions.main.height;

    sidebar::render(
        frame,
        regions.sidebar,
        &palette,
        &app.sidebar,
        &app.playlists,
        app.focus == Focus::Sidebar,
    );

    // The main pane shows the home page for Music, and a track table for the
    // sections that are a flat list.
    let main_focused = app.focus == Focus::Main;
    match app.sidebar.section() {
        sidebar::Section::Music => {
            // `artwork` needs &mut to cache what it decodes, and the closure
            // is handed to a renderer that also borrows `app` — so take it out
            // for the duration and put it back.
            let mut art = app.artwork.take();
            home::render(
                frame,
                regions.main,
                &palette,
                &app.home,
                main_focused,
                |frame, area, url, shape| match art.as_mut() {
                    Some(a) => a.render_shaped(frame, area, url, shape),
                    None => false,
                },
            );
            app.artwork = art;
        }
        section @ (sidebar::Section::Playlists
        | sidebar::Section::Albums
        | sidebar::Section::Profiles) => {
            let cards = app.grid_cards(section);
            let visible = grid::filter(&cards, &app.grid_state(section).filter);
            let (heading, hint, lines) = match section {
                sidebar::Section::Playlists => ("Playlists", "Filter playlists", 3),
                sidebar::Section::Albums => ("Albums", "Filter albums", 2),
                _ => ("Profiles", "Filter profiles", 1),
            };
            let mut art = app.artwork.take();
            grid::render(
                frame,
                regions.main,
                &palette,
                grid::Grid {
                    heading,
                    filter_hint: hint,
                    cards: &visible,
                    state: app.grid_state(section),
                    focused: main_focused,
                    lines,
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
                    tracks: &visible,
                    state: &app.tracklist,
                    focused: main_focused,
                    playing: app.now_playing.track.as_ref().map(|t| t.id),
                },
                |frame, area, url, shape| match art.as_mut() {
                    Some(a) => a.render_shaped(frame, area, url, shape),
                    None => false,
                },
            );
            app.artwork = art;
        }
    }

    let mut art = app.artwork.take();
    nowplaying::render_with_cover(
        frame,
        regions.now_playing,
        &palette,
        &app.now_playing,
        |frame, area, url, shape| match art.as_mut() {
            Some(a) => a.render_shaped(frame, area, url, shape),
            None => false,
        },
    );
    app.artwork = art;

    // Key hints on the bar's last line. A TUI whose bindings are invisible is
    // one nobody can drive: the keys existed here before this did, and could
    // not be discovered.
    let hints = if app.session.is_none() {
        "enter sign in   q quit"
    } else if app.showing_help {
        "any key to close"
    } else if app.filtering {
        "type to filter   enter accept   esc clear"
    } else if app.on_home() {
        "hjkl move   tab focus   t tabs   space pause   ? keys   q quit"
    } else if app.on_grid() {
        "hjkl move   / filter   enter open   ? keys   q quit"
    } else {
        "jk move   / filter   enter play   space pause   ? keys   q quit"
    };
    let hint_row = ratatui::layout::Rect {
        x: regions.now_playing.x + 1,
        y: regions.now_playing.y + regions.now_playing.height.saturating_sub(1),
        width: regions.now_playing.width.saturating_sub(2),
        height: 1,
    };
    if hint_row.y < regions.now_playing.y + regions.now_playing.height {
        frame.render_widget(
            Paragraph::new(Line::styled(hints, palette.section_heading()))
                .alignment(ratatui::layout::Alignment::Right),
            hint_row,
        );
    }

    if let Some(status) = &app.status {
        // Errors matter more than the bar; overlay one line at the top.
        let line = ratatui::layout::Rect { height: 1, ..regions.main };
        frame.render_widget(
            Paragraph::new(Line::styled(status.clone(), palette.accent_text())),
            line,
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
    fn the_quality_badge_shows_the_delivered_bit_depth() {
        // The decoder cannot report bit depth, so it is threaded from the
        // playback-info response through Cmd::Play. If that ever breaks, the
        // badge silently drops to "44.1kHz" and stops confirming hi-res.
        let mut app = App::default();
        app.update(Action::Playback(crate::playback::PlaybackEvent::Started {
            bit_depth: Some(24),
            sample_rate: 44_100,
        }));
        assert_eq!(app.now_playing.quality.as_deref(), Some("24-bit 44.1kHz"));
    }

    #[test]
    fn the_quality_badge_omits_bit_depth_when_unknown() {
        let mut app = App::default();
        app.update(Action::Playback(crate::playback::PlaybackEvent::Started {
            bit_depth: None,
            sample_rate: 44_100,
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
    fn an_artist_card_has_nothing_to_open() {
        let mut app = signed_in(sidebar::Section::Profiles);
        app.artists = vec![crate::library::Artist {
            id: 1,
            name: "2Pac".into(),
            picture: None,
        }];
        assert!(app.selected_collection().is_none());
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
    fn the_movement_keys_follow_the_focus() {
        // They used to always drive the main pane, which left Tab changing a
        // highlight and nothing else, and the sidebar reachable only through
        // a binding nobody would guess.
        let mut app = signed_in(sidebar::Section::Tracks);
        assert_eq!(app.focus, Focus::Main, "focus starts on the content");
        app.update(Action::ToggleFocus);
        assert_eq!(app.focus, Focus::Sidebar);

        assert!(
            matches!(key(&mut app, KeyCode::Char('j')), Some(Action::SidebarNext)),
            "j must move the sidebar while the sidebar has focus"
        );
        assert!(matches!(
            key(&mut app, KeyCode::Char('k')),
            Some(Action::SidebarPrevious)
        ));

        app.update(Action::ToggleFocus);
        assert_eq!(app.focus, Focus::Main);
        assert!(
            matches!(key(&mut app, KeyCode::Char('j')), Some(Action::TrackNext)),
            "j must move the list once the main pane has focus"
        );
    }

    #[test]
    fn l_and_h_cross_between_the_panes() {
        let mut app = signed_in(sidebar::Section::Tracks);
        app.focus = Focus::Sidebar;

        // l leaves the sidebar without needing Tab.
        assert!(key(&mut app, KeyCode::Char('l')).is_none());
        assert_eq!(app.focus, Focus::Main);

        // The track table has no sideways movement, so h goes back.
        assert!(key(&mut app, KeyCode::Char('h')).is_none());
        assert_eq!(app.focus, Focus::Sidebar);

        // h in the sidebar has nowhere further left to go.
        assert!(key(&mut app, KeyCode::Char('h')).is_none());
        assert_eq!(app.focus, Focus::Sidebar);
    }

    #[test]
    fn h_leaves_a_grid_only_from_its_left_edge() {
        let mut app = signed_in(sidebar::Section::Playlists);
        app.playlists = (0..30)
            .map(|i| crate::library::Playlist::sample(&format!("P{i}"), i))
            .collect();
        app.focus = Focus::Main;

        // One card in: h moves within the row.
        app.update(Action::CarouselNext);
        assert_eq!(app.playlist_grid.selected, 1);
        assert!(matches!(
            key(&mut app, KeyCode::Char('h')),
            Some(Action::CarouselPrevious)
        ));
        app.update(Action::CarouselPrevious);

        // At the left edge it leaves for the sidebar instead of doing nothing.
        assert_eq!(app.playlist_grid.selected, 0);
        assert!(key(&mut app, KeyCode::Char('h')).is_none());
        assert_eq!(app.focus, Focus::Sidebar);
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
