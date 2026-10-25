use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::error::{Error, Result};

pub const SCOPE: &str = "https://www.googleapis.com/auth/androidpublisher";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const JWT_LIFETIME_SECS: u64 = 3600;
/// Refresh this long before the token expires.
const REFRESH_MARGIN: Duration = Duration::from_secs(60);

/// The fields of a Google service-account key file this crate uses.
#[derive(Clone, Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    private_key: String,
    #[serde(default)]
    token_uri: Option<String>,
}

// Hand-written so `{:?}` can never print the private key.
impl std::fmt::Debug for ServiceAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceAccount")
            .field("client_email", &self.client_email)
            .field("private_key", &"<redacted>")
            .finish()
    }
}

impl ServiceAccount {
    pub fn from_json(json: &str) -> Result<Self> {
        let account: Self = serde_json::from_str(json).map_err(|_| Error::ServiceAccountInvalid)?;
        if account.client_email.is_empty() || account.private_key.is_empty() {
            return Err(Error::ServiceAccountInvalid);
        }
        Ok(account)
    }

    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path).map_err(|source| Error::ServiceAccountRead {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&json)
    }

    pub fn token_uri(&self) -> &str {
        self.token_uri.as_deref().unwrap_or(DEFAULT_TOKEN_URI)
    }

    /// Sign the RS256 assertion exchanged for an access token.
    pub fn assertion(&self, now_secs: u64) -> Result<String> {
        let claims = Claims::new(&self.client_email, self.token_uri(), now_secs);
        let mut header = Header::new(Algorithm::RS256);
        header.typ = Some("JWT".to_string());
        let key = EncodingKey::from_rsa_pem(self.private_key.as_bytes())?;
        Ok(jsonwebtoken::encode(&header, &claims, &key)?)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Claims {
    pub iss: String,
    pub scope: String,
    pub aud: String,
    pub iat: u64,
    pub exp: u64,
}

impl Claims {
    pub(crate) fn new(email: &str, audience: &str, now_secs: u64) -> Self {
        Self {
            iss: email.to_string(),
            scope: SCOPE.to_string(),
            aud: audience.to_string(),
            iat: now_secs,
            exp: now_secs + JWT_LIFETIME_SECS,
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// Mints and caches the access token until shortly before it expires.
pub(crate) struct TokenCache {
    cached: Mutex<Option<(String, Instant)>>,
}

impl TokenCache {
    pub(crate) fn new() -> Self {
        Self {
            cached: Mutex::new(None),
        }
    }

    pub(crate) async fn get(
        &self,
        http: &reqwest::Client,
        account: &ServiceAccount,
    ) -> Result<String> {
        let mut cached = self.cached.lock().await;
        if let Some((token, expires)) = cached.as_ref() {
            if Instant::now() < *expires {
                return Ok(token.clone());
            }
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let assertion = account.assertion(now)?;
        let response = http
            .post(account.token_uri())
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Token {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            });
        }
        let body: TokenResponse = response.json().await?;
        let lifetime = Duration::from_secs(body.expires_in.unwrap_or(3600));
        let expires = Instant::now() + lifetime.saturating_sub(REFRESH_MARGIN);
        *cached = Some((body.access_token.clone(), expires));
        Ok(body.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_carry_scope_audience_and_one_hour_lifetime() {
        let claims = Claims::new(
            "svc@example.iam.gserviceaccount.com",
            DEFAULT_TOKEN_URI,
            1000,
        );
        assert_eq!(claims.iss, "svc@example.iam.gserviceaccount.com");
        assert_eq!(
            claims.scope,
            "https://www.googleapis.com/auth/androidpublisher"
        );
        assert_eq!(claims.aud, "https://oauth2.googleapis.com/token");
        assert_eq!(claims.iat, 1000);
        assert_eq!(claims.exp, 4600);
    }

    #[test]
    fn token_uri_defaults_when_the_key_file_omits_it() {
        let account =
            ServiceAccount::from_json(r#"{"client_email":"a@b","private_key":"k"}"#).unwrap();
        assert_eq!(account.token_uri(), "https://oauth2.googleapis.com/token");
    }

    #[test]
    fn a_key_file_missing_the_key_is_rejected_without_echoing_it() {
        let err = ServiceAccount::from_json(r#"{"client_email":"a@b","private_key":""}"#)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("a@b"));
        assert!(ServiceAccount::from_json("not json").is_err());
    }

    #[test]
    fn debug_output_never_contains_the_private_key() {
        let account =
            ServiceAccount::from_json(r#"{"client_email":"a@b","private_key":"SECRET"}"#).unwrap();
        let shown = format!("{account:?}");
        assert!(!shown.contains("SECRET"));
        assert!(shown.contains("a@b"));
    }

    #[test]
    fn a_malformed_pem_fails_to_sign_instead_of_panicking() {
        let account =
            ServiceAccount::from_json(r#"{"client_email":"a@b","private_key":"nope"}"#).unwrap();
        assert!(account.assertion(0).is_err());
    }
}
