use crate::auth::store::StoredToken;
use crate::config::AuthConfig;

const AUTH_BASE: &str = "https://auth.tidal.com/v1/oauth2";
const SCOPE: &str = "r_usr w_usr w_sub";

#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    /// Fully qualified, scheme included — TIDAL omits the scheme.
    pub verification_uri: String,
    pub interval_secs: u64,
    pub expires_in_secs: u64,
}

#[derive(Debug)]
pub enum PollOutcome {
    Granted(StoredToken),
    Pending,
    SlowDown,
    Expired,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("HTTP {status} returned a non-JSON body: {body}")]
    NonJsonBody { status: u16, body: String },
    #[error("{error}: {description}")]
    Oauth { error: String, description: String },
}

#[derive(serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct DeviceCodeDto {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    interval: u64,
    expires_in: u64,
}

impl Default for DeviceCodeDto {
    fn default() -> Self {
        Self {
            device_code: String::new(),
            user_code: String::new(),
            verification_uri: String::new(),
            verification_uri_complete: String::new(),
            interval: 2,
            expires_in: 300,
        }
    }
}

pub(crate) fn parse_device_code(body: &str) -> Result<DeviceCode, AuthError> {
    // TIDAL uses camelCase here, unlike the token endpoint's snake_case.
    let dto: DeviceCodeDto = serde_json::from_str(body).map_err(|_| AuthError::NonJsonBody {
        status: 200,
        body: body.chars().take(200).collect(),
    })?;

    let raw = if dto.verification_uri_complete.is_empty() {
        &dto.verification_uri
    } else {
        &dto.verification_uri_complete
    };
    let uri = if raw.starts_with("http") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };

    Ok(DeviceCode {
        device_code: dto.device_code,
        user_code: dto.user_code,
        verification_uri: uri,
        interval_secs: dto.interval.max(1),
        expires_in_secs: dto.expires_in,
    })
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct TokenDto {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    error: String,
    error_description: String,
    user: UserDto,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct UserDto {
    #[serde(rename = "userId")]
    user_id: u64,
    #[serde(rename = "countryCode")]
    country_code: String,
}

pub(crate) fn parse_poll(body: &str, status: u16) -> Result<PollOutcome, AuthError> {
    // Non-JSON happens: an HTML error page from the CDN, or an empty 401.
    // Never let it abort the caller's poll schedule.
    let dto: TokenDto = serde_json::from_str(body).map_err(|_| AuthError::NonJsonBody {
        status,
        body: body.chars().take(200).collect(),
    })?;

    match dto.error.as_str() {
        "" => Ok(PollOutcome::Granted(StoredToken {
            access_token: dto.access_token,
            refresh_token: dto.refresh_token,
            expires_at: now_unix().saturating_add(dto.expires_in),
            country_code: if dto.user.country_code.is_empty() {
                "US".into()
            } else {
                dto.user.country_code
            },
            user_id: dto.user.user_id,
        })),
        "authorization_pending" => Ok(PollOutcome::Pending),
        "slow_down" => Ok(PollOutcome::SlowDown),
        "expired_token" => Ok(PollOutcome::Expired),
        other => Err(AuthError::Oauth {
            error: other.to_string(),
            description: dto.error_description,
        }),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub async fn start_login(
    http: &reqwest::Client,
    cfg: &AuthConfig,
) -> Result<DeviceCode, AuthError> {
    let body = http
        .post(format!("{AUTH_BASE}/device_authorization"))
        .form(&[("client_id", cfg.client_id.as_str()), ("scope", SCOPE)])
        .send()
        .await?
        .text()
        .await?;
    parse_device_code(&body)
}

pub async fn poll_once(
    http: &reqwest::Client,
    cfg: &AuthConfig,
    code: &DeviceCode,
) -> Result<PollOutcome, AuthError> {
    // Credentials go in the FORM BODY. Sending them as HTTP Basic auth
    // (reqwest's .basic_auth) makes CloudFront reject the request with a 403
    // HTML page before it reaches TIDAL. Verified during the spike.
    let resp = http
        .post(format!("{AUTH_BASE}/token"))
        .form(&[
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
            ("device_code", code.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    let status = resp.status().as_u16();
    let body = resp.text().await?;
    parse_poll(&body, status)
}

pub async fn refresh(
    http: &reqwest::Client,
    cfg: &AuthConfig,
    refresh_token: &str,
) -> Result<StoredToken, AuthError> {
    let resp = http
        .post(format!("{AUTH_BASE}/token"))
        .form(&[
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    let status = resp.status().as_u16();
    let body = resp.text().await?;

    match parse_poll(&body, status)? {
        PollOutcome::Granted(mut t) => {
            // A refresh response often omits the refresh_token; keep the old one.
            if t.refresh_token.is_empty() {
                t.refresh_token = refresh_token.to_string();
            }
            Ok(t)
        }
        _ => Err(AuthError::Oauth {
            error: "refresh_failed".into(),
            description: "refresh did not return a token".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_device_code_response() {
        // The shape captured during development, with the code values replaced.
        let body = r#"{"deviceCode":"00000000-0000-0000-0000-000000000000",
                       "expiresIn":300,"interval":2,"userCode":"ABCDE",
                       "verificationUri":"link.tidal.com",
                       "verificationUriComplete":"link.tidal.com/ABCDE"}"#;
        let d = parse_device_code(body).unwrap();
        assert_eq!(d.user_code, "ABCDE");
        assert_eq!(d.interval_secs, 2);
        assert_eq!(d.expires_in_secs, 300);
        // TIDAL returns the URI without a scheme; we must add one or the
        // browser-open and the displayed link are both broken.
        assert_eq!(d.verification_uri, "https://link.tidal.com/ABCDE");
    }

    #[test]
    fn authorization_pending_is_not_an_error() {
        let body = r#"{"error":"authorization_pending",
                       "error_description":"Device Authorization code is not authorized yet",
                       "status":400,"sub_status":1002}"#;
        assert!(matches!(parse_poll(body, 400).unwrap(), PollOutcome::Pending));
    }

    #[test]
    fn expired_token_is_reported_distinctly() {
        let body = r#"{"error":"expired_token","error_description":"expired","status":400}"#;
        assert!(matches!(parse_poll(body, 400).unwrap(), PollOutcome::Expired));
    }

    #[test]
    fn slow_down_is_reported_distinctly() {
        let body = r#"{"error":"slow_down","error_description":"too fast","status":400}"#;
        assert!(matches!(parse_poll(body, 400).unwrap(), PollOutcome::SlowDown));
    }

    #[test]
    fn a_granted_token_is_parsed_with_its_country() {
        let body = r#"{"access_token":"at","refresh_token":"rt","expires_in":14400,
                       "token_type":"Bearer",
                       "user":{"userId":10000001,"countryCode":"TH"}}"#;
        match parse_poll(body, 200).unwrap() {
            PollOutcome::Granted(t) => {
                assert_eq!(t.access_token, "at");
                assert_eq!(t.country_code, "TH");
                assert_eq!(t.user_id, 10_000_001);
            }
            other => panic!("expected Granted, got {other:?}"),
        }
    }

    #[test]
    fn a_non_json_body_is_a_named_error_not_a_parse_panic() {
        // Sending HTTP Basic auth to this endpoint returns a CloudFront 403
        // HTML page. During the spike this crashed the poll loop; it must
        // surface as a diagnosable error instead.
        let html = "<!DOCTYPE HTML><HTML><H1>403 ERROR</H1>Request blocked.</HTML>";
        match parse_poll(html, 403) {
            Err(AuthError::NonJsonBody { status, .. }) => assert_eq!(status, 403),
            other => panic!("expected NonJsonBody, got {other:?}"),
        }
    }
}
