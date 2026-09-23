//! The macOS media keys, through `MPRemoteCommandCenter`.
//!
//! The keys are delivered to whatever has registered with the system, and
//! registering needs a Core Foundation run loop on the **main** thread. A
//! terminal application has no such loop of its own, so [`run_with_main_loop`]
//! makes one: the loop keeps the main thread and the whole app moves to a
//! second thread with the tokio runtime on it.
//!
//! That is the same shape a windowed application has, without the window.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use apple_cf::cf::CFRunLoop;
use mediaplayer::now_playing::{NowPlayingInfo, NowPlayingInfoCenter, PlaybackState};
use mediaplayer::remote_commands::{
    CommandEvent, CommandToken, HandlerStatus, RemoteCommandCenter,
};

use super::mediakeys::{State, StateReceiver};
use super::Action;

/// Run `app` with a Core Foundation run loop on the main thread.
///
/// The media keys are only delivered to a process running such a loop, and
/// it has to be the main thread -- Cocoa refuses anywhere else. So the app
/// itself goes to another thread, and this one does nothing but pump events
/// until the app is done.
pub fn run_with_main_loop<F, Fut>(app: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()>,
{
    let finished = Arc::new(AtomicBool::new(false));

    let done = finished.clone();
    std::thread::spawn(move || {
        match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime.block_on(app()),
            Err(e) => eprintln!("could not start the runtime: {e}"),
        }
        done.store(true, Ordering::Release);
        // Wake the loop below, which is otherwise asleep for up to a second.
        CFRunLoop::main().stop();
    });

    while !finished.load(Ordering::Acquire) {
        // A bounded run rather than a blocking one: the flag above is what
        // ends this, and an unbounded loop would need the stop to land
        // exactly right to be seen.
        let _ = CFRunLoop::current().run_in_default_mode(Duration::from_secs(1), false);
    }
}

/// Hand a command to the app the way a keystroke would.
fn command(
    actions: tokio::sync::mpsc::UnboundedSender<Action>,
    action: Action,
) -> impl FnMut(CommandEvent) -> HandlerStatus + Send + 'static {
    move |_| {
        // A closed channel means the app is shutting down; the system does
        // not need telling that.
        let _ = actions.send(action.clone());
        HandlerStatus::Success
    }
}

/// Register for the media keys, and keep the Control Center told what is
/// playing.
///
/// The tokens have to outlive the handlers, so they are held for the life of
/// the process rather than dropped at the end of this function -- dropping a
/// `CommandToken` unregisters its handler.
pub fn spawn(actions: tokio::sync::mpsc::UnboundedSender<Action>, state: StateReceiver) {
    let center = RemoteCommandCenter::shared();
    let tokens: Vec<CommandToken> = vec![
        center.on_toggle_play_pause(command(actions.clone(), Action::TogglePause)),
        center.on_play(command(actions.clone(), Action::TogglePause)),
        center.on_pause(command(actions.clone(), Action::TogglePause)),
        center.on_next_track(command(actions.clone(), Action::QueueNext)),
        center.on_previous_track(command(actions, Action::QueuePrevious)),
    ];
    // Never dropped: a token that goes out of scope takes its handler with
    // it, and the keys would stop working the moment this returned.
    std::mem::forget(tokens);
    tracing::info!("registered for the macOS media keys");

    tokio::spawn(async move {
        let mut watch = state;
        let mut last = watch.borrow().clone();
        announce(&last);
        while watch.changed().await.is_ok() {
            let now = watch.borrow().clone();
            if !last.differs_from(&now) {
                continue;
            }
            announce(&now);
            last = now;
        }
    });
}

/// Put the track in the Control Center, or clear it.
fn announce(state: &State) {
    let center = NowPlayingInfoCenter::default_center();
    let Some(track) = state.track.as_ref() else {
        center.set_playback_state(PlaybackState::Stopped);
        return;
    };

    let info = NowPlayingInfo {
        title: Some(track.title.clone()),
        artist: Some(track.artist.clone()),
        album_title: Some(track.album.clone()),
        playback_duration: Some(track.duration.as_secs_f64()),
        elapsed_playback_time: Some(state.position.as_secs_f64()),
        // Zero rather than one when paused: the system uses the rate to run
        // its own clock, and a paused track would otherwise keep counting.
        playback_rate: Some(if state.playing { 1.0 } else { 0.0 }),
        ..Default::default()
    };
    center.set_now_playing_info(&info);
    center.set_playback_state(if state.playing {
        PlaybackState::Playing
    } else {
        PlaybackState::Paused
    });
}
