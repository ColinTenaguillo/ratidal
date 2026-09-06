use crate::auth::StoredToken;
use crate::domain::{Quality, TrackId};
use crate::playback::{manifest::parse_playback_info, PlaybackInfo};

const API_HOST: &str = "https://api.tidal.com";

/// Which of TIDAL's API versions a request goes to.
///
/// Not a migration in progress: v2 is a handful of services added beside v1
/// on the same host, not a replacement for it. Measured against this
/// account, v2 answers for playlists, albums and artists — and 404s for
/// favourite tracks, the home page, an album's contents, and playbackinfo.
/// So v1 stays the base and v2 is used only where it offers something v1
/// has no endpoint for: search, the activity feed, mixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    V1,
    V2,
}

impl Api {
    fn prefix(&self) -> &'static str {
        match self {
            Api::V1 => "/v1",
            Api::V2 => "/v2",
        }
    }
}

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
    /// The server answered, but not with success. Reported separately from a
    /// parse failure because it is a different fault with a different fix:
    /// every non-401 status used to be handed back as if it were data, so a
    /// 404 or a 502 surfaced as "response was not valid JSON: EOF" — which
    /// says nothing about what went wrong.
    #[error("{status} from {path}{}", detail(.body))]
    Status { status: u16, path: String, body: String },
    #[error("unexpected response: {0}")]
    Parse(String),
}

/// A short quotation of an error body, when there is one worth showing.
fn detail(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return " (empty response)".into();
    }
    format!(": {}", body.chars().take(200).collect::<String>())
}

/// What a response means, decided from its status and body alone.
///
/// Separated from the request so it can be tested: the version of this that
/// lived inline handed every non-401 status back as if it were data, and the
/// tests that were supposed to cover it built the error by hand and passed
/// with the check deleted.
pub(crate) fn outcome(status: u16, path: &str, body: String) -> Result<String, TidalError> {
    if status == 401 {
        // A 401 does NOT always mean the session is finished. TIDAL uses it
        // for a capped client_id (4005) and for content the account cannot
        // reach, and `Unauthorized` makes the shell delete the stored token —
        // so only a genuine authentication failure may return it. Anything
        // else is passed on as a normal response and fails where it is used,
        // costing one request rather than the whole session.
        if is_auth_failure(&body) {
            return Err(TidalError::Unauthorized);
        }
        tracing::warn!(
            "401 that is not an auth failure, keeping the session: {}",
            body.chars().take(200).collect::<String>()
        );
        return Ok(body);
    }

    // Anything else that is not a success is an error, not a body. Returned
    // as data, a 404 or a 502 became a JSON parse failure several layers
    // away, with nothing to say which request had failed.
    if !(200..300).contains(&status) {
        return Err(TidalError::Status { status, path: path.to_string(), body });
    }

    Ok(body)
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
        self.get_on(Api::V1, path, query).await
    }

    /// GET against a chosen API version.
    pub(crate) async fn get_on(
        &self,
        api: Api,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<String, TidalError> {
        let mut params: Vec<(&str, String)> =
            vec![("countryCode", self.token.country_code.clone())];
        params.extend(query.iter().cloned());

        let resp = self
            .http
            .get(format!("{API_HOST}{}{path}", api.prefix()))
            .bearer_auth(&self.token.access_token)
            .query(&params)
            .send()
            .await?;

        let status = resp.status().as_u16();
        outcome(status, path, resp.text().await?)
    }

    /// POST a form to the v1 API.
    ///
    /// The favourites endpoints take form-encoded bodies, not JSON — a JSON
    /// body is accepted with a 400 that says nothing useful.
    pub(crate) async fn post_form(
        &self,
        path: &str,
        form: &[(&str, String)],
    ) -> Result<String, TidalError> {
        let resp = self
            .http
            .post(format!("{API_HOST}{}{path}", Api::V1.prefix()))
            .bearer_auth(&self.token.access_token)
            .query(&[("countryCode", self.token.country_code.clone())])
            .form(form)
            .send()
            .await?;

        let status = resp.status().as_u16();
        outcome(status, path, resp.text().await?)
    }

    /// DELETE against the v1 API.
    pub(crate) async fn delete(&self, path: &str) -> Result<String, TidalError> {
        let resp = self
            .http
            .delete(format!("{API_HOST}{}{path}", Api::V1.prefix()))
            .bearer_auth(&self.token.access_token)
            .query(&[("countryCode", self.token.country_code.clone())])
            .send()
            .await?;

        let status = resp.status().as_u16();
        outcome(status, path, resp.text().await?)
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
    fn a_failing_status_is_an_error_and_not_a_body() {
        // The failure this came from: every non-401 status was handed back
        // as data, so a 404 or a 502 surfaced several layers away as
        // "response was not valid JSON: EOF while parsing a value at line 1
        // column 0" — naming neither the request nor the status.
        for status in [400u16, 403, 404, 429, 500, 502, 503] {
            let e = outcome(status, "/users/1/favorites/tracks", String::new())
                .expect_err(&format!("{status} must not be treated as a body"));
            let text = e.to_string();
            assert!(text.contains(&status.to_string()), "{status}: {text}");
            assert!(text.contains("/users/1/favorites/tracks"), "{status}: {text}");
        }
    }

    #[test]
    fn a_success_is_passed_through_untouched() {
        let body = r#"{"items":[]}"#;
        for status in [200u16, 201, 204] {
            assert_eq!(
                outcome(status, "/whatever", body.to_string()).unwrap(),
                body,
                "{status} is a body, not an error"
            );
        }
    }

    #[test]
    fn an_empty_error_body_still_says_something() {
        let e = outcome(502, "/pages/home", String::new()).unwrap_err().to_string();
        assert!(e.contains("502"), "{e}");
        assert!(e.contains("empty"), "and that nothing came back: {e}");
    }

    #[test]
    fn an_error_body_is_quoted_but_not_dumped() {
        // Enough to recognise the fault, not so much that it buries the
        // rest of the message.
        let e = outcome(502, "/pages/home", "x".repeat(4000))
            .unwrap_err()
            .to_string();
        assert!(e.contains("502"));
        assert!(e.len() < 400, "the body is quoted, not dumped: {} chars", e.len());
    }

    #[test]
    fn a_401_that_is_not_an_auth_failure_is_still_a_body() {
        // Deleting the session over a capped client_id would sign the user
        // out for something a config edit fixes.
        let body = r#"{"status":401,"subStatus":4005,"userMessage":"nope"}"#;
        assert_eq!(outcome(401, "/tracks/1", body.to_string()).unwrap(), body);
    }

    #[test]
    fn a_401_that_is_an_auth_failure_ends_the_session() {
        let body = r#"{"status":401,"subStatus":11002,"userMessage":"expired"}"#;
        assert!(matches!(
            outcome(401, "/tracks/1", body.to_string()),
            Err(TidalError::Unauthorized)
        ));
    }

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
