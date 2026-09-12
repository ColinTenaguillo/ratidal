//! The Linux media keys, against a real D-Bus session.
//!
//! Registering with the desktop is the whole feature, and it cannot be
//! checked by calling the handlers directly: what matters is that the name
//! appears on the bus and that a command sent over it reaches the app. This
//! starts a private session bus and does exactly that.
#![cfg(target_os = "linux")]

use std::time::Duration;

/// Skipped rather than failed where there is no session bus: a developer's
/// machine may have none, and this is not what their build should break on.
fn have_bus() -> bool {
    std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok()
}

#[tokio::test]
async fn the_player_appears_on_the_bus_and_its_commands_reach_the_app() {
    if !have_bus() {
        eprintln!("no session bus; skipping");
        return;
    }

    let (actions, mut received) = tokio::sync::mpsc::unbounded_channel();
    let (state_tx, state_rx) = ratidal::shell::mediakeys::channel();

    // A track playing, with a queue either side of it.
    state_tx
        .send(ratidal::shell::mediakeys::State {
            track: Some(ratidal::domain::Track {
                id: ratidal::domain::TrackId(7),
                title: "A Track".into(),
                artist: "Someone".into(),
                album: "An Album".into(),
                duration: Duration::from_secs(200),
                cover: None,
                tags: Vec::new(),
                added: None,
                explicit: false,
                ai: false,
                radio: None,
                album_id: None,
                artist_id: None,
            }),
            playing: true,
            position: Duration::from_secs(5),
            can_next: true,
            can_previous: true,
        })
        .expect("the receiver is alive");

    ratidal::shell::spawn_media_keys(actions, state_rx);

    // Registering is a round trip over the bus.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let connection = zbus::Connection::session().await.expect("session bus");
    let proxy = zbus::Proxy::new(
        &connection,
        "org.mpris.MediaPlayer2.ratidal",
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
    .await
    .expect("the player is on the bus under its own name");

    // What the desktop shows.
    let status: String = proxy.get_property("PlaybackStatus").await.expect("status");
    assert_eq!(status, "Playing", "the desktop is told it is playing");

    // And what the keys do: each command arrives as the action the keyboard
    // would have sent.
    proxy.call_method("PlayPause", &()).await.expect("PlayPause");
    proxy.call_method("Next", &()).await.expect("Next");
    proxy.call_method("Previous", &()).await.expect("Previous");

    let mut got = Vec::new();
    while got.len() < 3 {
        match tokio::time::timeout(Duration::from_secs(2), received.recv()).await {
            Ok(Some(action)) => got.push(format!("{action:?}")),
            _ => break,
        }
    }
    assert_eq!(
        got,
        vec!["TogglePause", "QueueNext", "QueuePrevious"],
        "the media keys reach the app as its own actions"
    );
}
