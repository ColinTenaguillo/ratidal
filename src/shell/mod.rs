pub mod layout;
pub mod login;
pub mod nowplaying;
pub mod sidebar;
pub mod tracklist;

use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::StreamExt as _;
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
    Playback(crate::playback::PlaybackEvent),
    TogglePause,
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
}

impl App {
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
            Action::Tick => None,
            Action::BeginLogin => {
                self.login = login::LoginState::Idle;
                None
            }
            Action::SessionExpired => {
                self.session = None;
                if let Err(e) = crate::auth::store::clear() {
                    tracing::warn!("could not clear the stale token: {e}");
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
                self.tracklist.next(self.tracks.len());
                None
            }
            Action::TrackPrevious => {
                self.tracklist.previous();
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

    fn on_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
            KeyCode::Enter if self.session.is_none() => Some(Action::BeginLogin),
            KeyCode::Char('j') | KeyCode::Down if self.session.is_some() => {
                Some(Action::TrackNext)
            }
            KeyCode::Char('k') | KeyCode::Up if self.session.is_some() => {
                Some(Action::TrackPrevious)
            }
            KeyCode::Tab if self.session.is_some() => Some(Action::SidebarNext),
            KeyCode::BackTab if self.session.is_some() => Some(Action::SidebarPrevious),
            KeyCode::Enter if self.session.is_some() => Some(Action::ActivateSelection),
            KeyCode::Char(' ') if self.session.is_some() => Some(Action::TogglePause),
            _ => None,
        }
    }
}

pub async fn run(terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
    let config = crate::config::Config::load()?;
    let http = reqwest::Client::new();

    let mut app = App::default();
    // Resume an existing session rather than making the user log in again.
    if let Ok(Some(token)) = crate::auth::store::load() {
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
                    if let Err(e) = crate::auth::store::save(&fresh) {
                        tracing::warn!("could not persist the refreshed token: {e}");
                    }
                    app.session = Some(fresh);
                }
                Err(e) => {
                    tracing::warn!("stored session could not be refreshed: {e}");
                    if let Err(e) = crate::auth::store::clear() {
                        tracing::warn!("could not clear the stale token: {e}");
                    }
                }
            }
        } else {
            app.session = Some(token);
        }
    }

    let mut events = EventStream::new();
    // ~30fps ceiling: a burst of scroll events coalesces into one frame.
    let mut ticker = tokio::time::interval(Duration::from_millis(33));
    let (action_tx, mut action_rx) = tokio::sync::mpsc::unbounded_channel::<Action>();
    let (cmd_tx, mut playback_rx) = crate::playback::spawn();

    while !app.should_quit {
        terminal.draw(|frame| draw(frame, &app))?;

        let action = tokio::select! {
            _ = ticker.tick() => Some(Action::Tick),
            Some(action) = action_rx.recv() => Some(action),
            Some(event) = playback_rx.recv() => Some(Action::Playback(event)),
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
                                poll_until_granted(http, cfg, code, tx).await;
                            }
                            Err(e) => {
                                let _ = tx.send(Action::Error(e.to_string()));
                            }
                        }
                    });
                }
                Action::Authenticated(token) => {
                    if let Err(e) = crate::auth::store::save(token) {
                        tracing::warn!("could not persist the token: {e}");
                    }
                    // Load the collection as soon as there is a session.
                    let (client, tx) = (
                        crate::tidal::Client::new(token.clone()),
                        action_tx.clone(),
                    );
                    tokio::spawn(async move {
                        match crate::library::favourite_tracks(&client).await {
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
                Action::ActivateSelection => {
                    if let (Some(token), Some(track)) =
                        (&app.session, app.tracks.get(app.tracklist.selected))
                    {
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

fn draw(frame: &mut ratatui::Frame, app: &App) {
    let regions = layout::split(frame.area());

    sidebar::render(frame, regions.sidebar, &app.sidebar, false);
    tracklist::render(frame, regions.main, &app.tracks, &app.tracklist, true);
    nowplaying::render(frame, regions.now_playing, &app.now_playing);

    if let Some(status) = &app.status {
        // Errors matter more than the bar; overlay one line at the top.
        let line = ratatui::layout::Rect { height: 1, ..regions.main };
        frame.render_widget(Paragraph::new(status.clone()), line);
    }

    if app.session.is_none() {
        login::render(frame, frame.area(), &app.login);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
            tracks: vec![crate::domain::Track {
                id: crate::domain::TrackId(1),
                title: "t".into(),
                artist: "a".into(),
                duration: std::time::Duration::ZERO,
                cover: None,
                tags: Vec::new(),
            }],
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
