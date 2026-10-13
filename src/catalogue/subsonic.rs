//! A Subsonic server through sonora's provider: Navidrome, Gonic,
//! Airsonic and the rest of the family.
//!
//! The provider keeps the credentials itself once signed in, so a later
//! launch restores without asking; the shell only ever hands the password
//! on, and the config file names the server and the user.

use std::sync::Arc;

use anyhow::Result;
use music::{MusicProvider, SignIn};

use crate::catalogue::{Catalogue, Sonora};

const NAME: &str = "Subsonic";

/// What the provider wants for a sign-in that asks nothing: no prompt is
/// ever shown for credentials, and no line is ever typed back.
fn silent() -> (music::PromptSink, music::InputSource) {
    let (_, input) = tokio::sync::mpsc::unbounded_channel();
    (Arc::new(|_| {}), input)
}

/// The session the provider remembered, if it remembered one.
pub async fn restore() -> Result<Option<Arc<dyn Catalogue>>> {
    let provider = music::subsonic::SubsonicProvider::new();
    Ok(provider
        .restore()
        .await?
        .map(|session| Arc::new(Sonora::new(NAME, session)) as Arc<dyn Catalogue>))
}

/// Signs in, which the provider checks against the server before it keeps
/// the credentials.
pub async fn connect(server: &str, username: &str, password: String) -> Result<Arc<dyn Catalogue>> {
    let provider = music::subsonic::SubsonicProvider::new();
    let (prompt, input) = silent();
    let session = provider
        .sign_in(
            SignIn::Credentials {
                server: server.to_owned(),
                username: username.to_owned(),
                password,
            },
            prompt,
            input,
        )
        .await?;
    Ok(Arc::new(Sonora::new(NAME, session)))
}
