use crate::auth::ApiKey;
use crate::error::{Error, Result};
use crate::jsonapi::ErrorDocument;
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const BASE_URL: &str = "https://api.appstoreconnect.apple.com";
// Refresh a bit before the token's real expiry so an in-flight request never
// races a signature Apple has already started rejecting.
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(60);

struct CachedToken {
    value: String,
    minted_at: Instant,
    lifetime: Duration,
}

/// Thin async HTTP client for the App Store Connect API. Handles JWT minting
/// (cached and refreshed automatically) and JSON:API request/response
/// plumbing. Resource-specific calls (apps, app store versions, bundle IDs,
/// ...) are implemented as additional `impl Client` blocks alongside each
/// resource's types, in their own modules.
pub struct Client {
    http: reqwest::Client,
    api_key: ApiKey,
    base_url: String,
    token: Mutex<Option<CachedToken>>,
}

impl Client {
    pub fn new(api_key: ApiKey) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key,
            base_url: BASE_URL.to_string(),
            token: Mutex::new(None),
        }
    }

    /// Override the API host — only meaningful for pointing the client at a
    /// mock server in tests.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    fn bearer_token(&self) -> Result<String> {
        let mut guard = self
            .token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(cached) = guard.as_ref() {
            if cached.minted_at.elapsed() + TOKEN_REFRESH_MARGIN < cached.lifetime {
                return Ok(cached.value.clone());
            }
        }

        let value = self.api_key.token()?;
        *guard = Some(CachedToken {
            value: value.clone(),
            minted_at: Instant::now(),
            lifetime: Duration::from_secs(self.api_key.lifetime_secs()),
        });
        Ok(value)
    }

    /// Send a request and decode a JSON:API response body into `T`. Use
    /// `request_no_content` instead for calls (typically `DELETE`) that
    /// return an empty `204` body.
    pub(crate) async fn request<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&B>,
    ) -> Result<T> {
        let (status, bytes) = self.send(method, path, query, body).await?;
        if !status.is_success() {
            return Err(api_error(status, &bytes));
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Send a request that returns no body on success (typically `DELETE`).
    pub(crate) async fn request_no_content<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&B>,
    ) -> Result<()> {
        let (status, bytes) = self.send(method, path, query, body).await?;
        if !status.is_success() {
            return Err(api_error(status, &bytes));
        }
        Ok(())
    }

    async fn send<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&B>,
    ) -> Result<(StatusCode, Vec<u8>)> {
        let url = format!("{}{}", self.base_url, path);
        let token = self.bearer_token()?;
        let mut req = self.http.request(method, &url).bearer_auth(token);
        if !query.is_empty() {
            req = req.query(query);
        }
        if let Some(b) = body {
            req = req.json(b);
        }

        let response = req.send().await?;
        let status = response.status();
        let bytes = response.bytes().await?.to_vec();
        Ok((status, bytes))
    }

    /// PUTs a raw byte body to a pre-signed asset-upload URL (as returned by
    /// e.g. `appScreenshots.uploadOperations`). Unlike [`Client::request`],
    /// this does **not** prefix `base_url` or attach the ASC bearer token —
    /// upload URLs are pre-signed and carry their own auth in
    /// `request_headers`; sending our JWT alongside would be wrong for a
    /// host that isn't `api.appstoreconnect.apple.com`.
    pub(crate) async fn upload_bytes(
        &self,
        method: Method,
        url: &str,
        headers: &[(String, String)],
        body: Vec<u8>,
    ) -> Result<()> {
        let mut req = self.http.request(method, url);
        for (name, value) in headers {
            req = req.header(name.as_str(), value.as_str());
        }
        let response = req.body(body).send().await?;
        let status = response.status();
        if !status.is_success() {
            let bytes = response.bytes().await?.to_vec();
            return Err(api_error(status, &bytes));
        }
        Ok(())
    }
}

fn api_error(status: StatusCode, bytes: &[u8]) -> Error {
    let detail = match serde_json::from_slice::<ErrorDocument>(bytes) {
        Ok(doc) if !doc.errors.is_empty() => doc
            .errors
            .iter()
            .map(|e| format!("{} ({}): {}", e.title, e.code, e.detail))
            .collect::<Vec<_>>()
            .join("; "),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    };
    Error::Api {
        status: status.as_u16(),
        detail,
    }
}
