use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Deserializer};
use tokio::sync::Mutex;

use crate::error::{Error, Result};

/// The audience the submission API accepts tokens for.
pub const RESOURCE: &str = "https://manage.devcenter.microsoft.com";
/// Refresh this long before the token expires.
const REFRESH_MARGIN: Duration = Duration::from_secs(60);

pub const KEY_TENANT_ID: &str = "MSSTORE_TENANT_ID";
pub const KEY_CLIENT_ID: &str = "MSSTORE_CLIENT_ID";
pub const KEY_CLIENT_SECRET: &str = "MSSTORE_CLIENT_SECRET";
pub const KEY_SELLER_ID: &str = "MSSTORE_SELLER_ID";
pub const KEY_PRODUCT_ID: &str = "MSSTORE_PRODUCT_ID";

/// Partner Center API credentials plus the app they act on.
#[derive(Clone)]
pub struct Credentials {
    pub tenant_id: String,
    pub client_id: String,
    client_secret: String,
    pub seller_id: String,
    pub product_id: String,
}

// Hand-written so `{:?}` can never print the client secret.
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("tenant_id", &self.tenant_id)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("seller_id", &self.seller_id)
            .field("product_id", &self.product_id)
            .finish()
    }
}

/// Parse a dotenv file: `KEY=VALUE` lines, `#` comments, an optional
/// `export ` prefix, and one pair of matching quotes around a value.
pub fn parse_dotenv(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let value = ['"', '\'']
            .iter()
            .find_map(|q| value.strip_prefix(*q).and_then(|v| v.strip_suffix(*q)))
            .unwrap_or(value);
        map.insert(key.trim().to_string(), value.to_string());
    }
    map
}

impl Credentials {
    /// Build from a key lookup. A key that is absent or empty is an error
    /// naming the key.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let need = |key: &'static str| {
            get(key)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .ok_or(Error::MissingCredential(key))
        };
        Ok(Self {
            tenant_id: need(KEY_TENANT_ID)?,
            client_id: need(KEY_CLIENT_ID)?,
            client_secret: need(KEY_CLIENT_SECRET)?,
            seller_id: need(KEY_SELLER_ID)?,
            product_id: need(KEY_PRODUCT_ID)?,
        })
    }

    pub fn from_env_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| Error::EnvRead {
            path: path.display().to_string(),
            source,
        })?;
        let map = parse_dotenv(&text);
        Self::from_lookup(|key| map.get(key).cloned())
    }

    pub fn from_process_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn token_url(&self) -> String {
        token_url(&self.tenant_id)
    }

    /// The client-credentials form body.
    pub fn token_form(&self) -> Vec<(&'static str, String)> {
        vec![
            ("grant_type", "client_credentials".to_string()),
            ("client_id", self.client_id.clone()),
            ("client_secret", self.client_secret.clone()),
            ("resource", RESOURCE.to_string()),
        ]
    }
}

pub(crate) fn token_url(tenant: &str) -> String {
    format!("https://login.microsoftonline.com/{tenant}/oauth2/token")
}

/// Entra's v1 endpoint returns `expires_in` as a string; accept a number too.
fn seconds<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<u64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Secs {
        Num(u64),
        Text(String),
    }
    Ok(match Option::<Secs>::deserialize(d)? {
        Some(Secs::Num(n)) => Some(n),
        Some(Secs::Text(s)) => s.parse().ok(),
        None => None,
    })
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default, deserialize_with = "seconds")]
    expires_in: Option<u64>,
}

/// Fetches and caches the access token until shortly before it expires.
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
        credentials: &Credentials,
    ) -> Result<String> {
        let mut cached = self.cached.lock().await;
        if let Some((token, expires)) = cached.as_ref() {
            if Instant::now() < *expires {
                return Ok(token.clone());
            }
        }
        let response = http
            .post(credentials.token_url())
            .form(&credentials.token_form())
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

    fn sample() -> Credentials {
        let map = parse_dotenv(
            "MSSTORE_TENANT_ID=tenant-1\nMSSTORE_CLIENT_ID=client-1\n\
             MSSTORE_CLIENT_SECRET=\"s3cret\"\nMSSTORE_SELLER_ID=42\nMSSTORE_PRODUCT_ID=9P69\n",
        );
        Credentials::from_lookup(|k| map.get(k).cloned()).unwrap()
    }

    #[test]
    fn token_url_embeds_the_tenant() {
        assert_eq!(
            sample().token_url(),
            "https://login.microsoftonline.com/tenant-1/oauth2/token"
        );
    }

    #[test]
    fn token_form_is_client_credentials_for_the_devcenter_resource() {
        let form = sample().token_form();
        let get = |k: &str| form.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("grant_type"), Some("client_credentials"));
        assert_eq!(get("client_id"), Some("client-1"));
        assert_eq!(get("client_secret"), Some("s3cret"));
        assert_eq!(
            get("resource"),
            Some("https://manage.devcenter.microsoft.com")
        );
    }

    #[test]
    fn debug_output_never_contains_the_secret() {
        let shown = format!("{:?}", sample());
        assert!(!shown.contains("s3cret"));
        assert!(shown.contains("client-1"));
    }

    #[test]
    fn dotenv_handles_comments_export_and_quotes() {
        let map = parse_dotenv("# c\n\nexport A='x y'\nB = \"z\"\nC=\nnot a pair\n");
        assert_eq!(map["A"], "x y");
        assert_eq!(map["B"], "z");
        assert_eq!(map["C"], "");
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn an_empty_value_is_reported_by_key_without_any_value() {
        let map = parse_dotenv("MSSTORE_TENANT_ID=t\nMSSTORE_CLIENT_ID=\n");
        let err = Credentials::from_lookup(|k| map.get(k).cloned()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "missing or empty credential MSSTORE_CLIENT_ID"
        );
    }

    #[test]
    fn expires_in_parses_as_string_or_number() {
        let a: TokenResponse =
            serde_json::from_str(r#"{"access_token":"t","expires_in":"3599"}"#).unwrap();
        let b: TokenResponse =
            serde_json::from_str(r#"{"access_token":"t","expires_in":3599}"#).unwrap();
        assert_eq!((a.expires_in, b.expires_in), (Some(3599), Some(3599)));
    }
}
