use crate::auth::StoredToken;
use crate::domain::{Quality, TrackId};
use crate::playback::{manifest::parse_playback_info, PlaybackInfo};

const API_BASE: &str = "https://api.tidal.com/v1";

pub struct Client {
    http: reqwest::Client,
    token: StoredToken,
}

#[derive(Debug, thiserror::Error)]
pub enum TidalError {
    #[error("not authorized — the session may have expired")]
    Unauthorized,
    #[error(
        "this client_id cannot stream (subStatus {sub_status}): {message}. \
         Set a different client_id in config.toml."
    )]
    NotAvailable { sub_status: u32, message: String },
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unexpected response: {0}")]
    Parse(String),
}

impl Client {
    pub fn new(token: StoredToken) -> Self {
        Self { http: reqwest::Client::new(), token }
    }

    pub fn country(&self) -> &str {
        &self.token.country_code
    }

    pub fn user_id(&self) -> u64 {
        self.token.user_id
    }

    /// GET against the v1 API with the bearer token and countryCode always
    /// applied. Every caller goes through here so countryCode cannot be
    /// forgotten — omitting it produces opaque failures.
    pub(crate) async fn get(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<String, TidalError> {
        let mut params: Vec<(&str, String)> =
            vec![("countryCode", self.token.country_code.clone())];
        params.extend(query.iter().cloned());

        let resp = self
            .http
            .get(format!("{API_BASE}{path}"))
            .bearer_auth(&self.token.access_token)
            .query(&params)
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            // May be an expired token, or a capped client_id. The body's
            // subStatus distinguishes them, so read it before deciding.
            let body = resp.text().await?;
            if body.contains("4005") {
                return Ok(body);
            }
            return Err(TidalError::Unauthorized);
        }

        Ok(resp.text().await?)
    }

    /// Fetch the stream manifest.
    ///
    /// Note the lowercase parameter names — this API is not camelCase like the
    /// official one, and a typo yields a silent 400.
    pub async fn playback_info(
        &self,
        id: TrackId,
        quality: Quality,
    ) -> Result<PlaybackInfo, TidalError> {
        let body = self
            .get(
                &format!("/tracks/{id}/playbackinfopostpaywall"),
                &[
                    ("audioquality", quality.as_param().to_string()),
                    ("playbackmode", "STREAM".to_string()),
                    ("assetpresentation", "FULL".to_string()),
                ],
            )
            .await?;

        parse_playback_info(&body).map_err(|e| match e {
            crate::playback::ManifestError::NotAvailable { sub_status, message } => {
                TidalError::NotAvailable { sub_status, message }
            }
            other => TidalError::Parse(other.to_string()),
        })
    }
}
