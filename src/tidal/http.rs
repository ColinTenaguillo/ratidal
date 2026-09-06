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

/// Whether a 401 body means the SESSION is finished, as opposed to this one
/// request being refused.
///
/// The distinction decides whether the stored token gets deleted, so it has to
/// be narrow: treating every 401 as an expiry signed the user out whenever a
/// single endpoint refused a request for its own reasons.
fn is_auth_failure(body: &str) -> bool {
    body.contains("\"subStatus\":11002")        // token expired
        || body.contains("\"subStatus\":11003")  // token invalid
        || body.contains("\"subStatus\":6001") // session no longer exists
}

impl Client {
    pub fn new(token: StoredToken) -> Self {
        // Without a timeout a stalled response leaves the request pending
        // forever: the spawned task never reports back and the UI shows a
        // spinner with no way to know it will never resolve.
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self { http, token }
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
            // A 401 does NOT always mean the session is finished. TIDAL uses
            // it for a capped client_id (4005) and for content the account
            // cannot reach, and `Unauthorized` makes the shell delete the
            // stored token — so only a genuine authentication failure may
            // return it. Anything else is passed on as a normal response and
            // fails where it is used, costing one request rather than the
            // whole session.
            let body = resp.text().await?;
            if is_auth_failure(&body) {
                return Err(TidalError::Unauthorized);
            }
            tracing::warn!(
                "401 that is not an auth failure, keeping the session: {}",
                body.chars().take(200).collect::<String>()
            );
            return Ok(body);
        }

        Ok(resp.text().await?)
    }

    /// The raw body of any v1 GET, for probing the API and capturing
    /// fixtures. The parsers are written against what this returns rather
    /// than against what the shape is assumed to be — guessing a field name
    /// has cost several rounds already, and `#[serde(default)]` makes a wrong
    /// guess silent.
    pub async fn get_raw(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<String, TidalError> {
        self.get(path, query).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_expiry_ends_the_session() {
        assert!(is_auth_failure(
            r#"{"status":401,"subStatus":11002,"userMessage":"The token has expired."}"#
        ));
        assert!(is_auth_failure(
            r#"{"status":401,"subStatus":11003,"userMessage":"Token is invalid"}"#
        ));
        assert!(is_auth_failure(
            r#"{"status":401,"subStatus":6001,"userMessage":"Session does not exist"}"#
        ));
    }

    #[test]
    fn a_capped_client_id_does_not_end_the_session() {
        // 4005 means this client_id cannot stream. The fix is a config edit;
        // signing the user out helps nobody.
        assert!(!is_auth_failure(
            r#"{"status":401,"subStatus":4005,"userMessage":"Asset is not ready for playback"}"#
        ));
    }

    #[test]
    fn an_unfamiliar_401_keeps_the_session() {
        // The failure this came from: every 401 counted as an expiry, so one
        // refused request deleted a token still valid for hours. Keeping the
        // session costs a failed request; the other default costs the session.
        assert!(!is_auth_failure(
            r#"{"status":401,"subStatus":4006,"userMessage":"Something else"}"#
        ));
        assert!(!is_auth_failure(r#"{"status":401}"#));
        assert!(!is_auth_failure("<html>an error page</html>"));
        assert!(!is_auth_failure(""));
    }
}
