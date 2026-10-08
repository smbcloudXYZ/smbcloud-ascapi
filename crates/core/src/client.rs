use crate::error::{Error, Result};
use crate::jsonapi::{path_and_query, ErrorDocument, ListDocument, Resource};
use crate::token::{IntoTokenSource, TokenSource};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Which request to send next while walking a paginated collection: the
/// caller's original `path`/`query`, or the page Apple's `links.next` named.
#[derive(Debug, PartialEq, Eq)]
enum Page<'a> {
    /// The first page, as the caller asked for it.
    First {
        path: &'a str,
        query: &'a [(&'a str, &'a str)],
    },
    /// A following page. Carries only a path because Apple's `next` URL
    /// already embeds the original `include`/`filter`/`limit` plus its
    /// `cursor`; re-appending `query` would duplicate every parameter.
    Next(String),
}

impl<'a> Page<'a> {
    /// The `path`/`query` pair to request this page with.
    fn request(&self) -> (&str, &[(&str, &str)]) {
        match self {
            Page::First { path, query } => (*path, *query),
            Page::Next(path) => (path.as_str(), &[]),
        }
    }
}

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
/// plumbing.
///
/// Resource-specific calls live in the domain crates as extension traits on
/// this type: `smbcloud-ascapi-aso` for App Metadata, `smbcloud-ascapi-signing`
/// for certificates. [`Client::request`], [`Client::request_no_content`], and
/// [`Client::upload_bytes`] are the low-level seam those traits build on, and
/// are public for that reason rather than because callers should reach for
/// them directly.
pub struct Client {
    http: reqwest::Client,
    token_source: Box<dyn TokenSource>,
    base_url: String,
    token: Mutex<Option<CachedToken>>,
}

impl Client {
    /// Build a client over anything that can mint a bearer token. An
    /// [`ApiKey`](crate::ApiKey) is the usual argument and still works
    /// unchanged, since it converts into a [`TokenSource`]; a
    /// [`StaticToken`](crate::StaticToken) or any custom source works too.
    pub fn new(token_source: impl IntoTokenSource) -> Self {
        Self {
            http: reqwest::Client::new(),
            token_source: token_source.into_token_source(),
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

        let minted = self.token_source.mint()?;
        *guard = Some(CachedToken {
            value: minted.value.clone(),
            minted_at: Instant::now(),
            lifetime: minted.lifetime,
        });
        Ok(minted.value)
    }

    /// Send a request and decode a JSON:API response body into `T`. Use
    /// `request_no_content` instead for calls (typically `DELETE`) that
    /// return an empty `204` body.
    pub async fn request<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&B>,
    ) -> Result<T> {
        let (status, bytes) = self.send(method, path, query, body, &[]).await?;
        if !status.is_success() {
            return Err(api_error(status, &bytes));
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Read an entire collection by following `links.next`, so a caller sees
    /// every row rather than Apple's first page.
    ///
    /// `path`/`query` describe the first page only. Each later page is
    /// requested from the absolute `links.next` Apple returns, which already
    /// carries the original `include`/`filter`/`limit` alongside its cursor —
    /// re-appending `query` there would duplicate every parameter.
    ///
    /// No page size is chosen here. Apple's per-collection maximums differ and
    /// only the caller knows which collection it is reading; picking a size
    /// shapes the request, where following the cursor does not. Callers that
    /// want 200-row pages pass `("limit", "200")` in `query`, as they already
    /// do.
    pub async fn list_all<A>(&self, path: &str, query: &[(&str, &str)]) -> Result<Vec<Resource<A>>>
    where
        A: DeserializeOwned,
    {
        let mut rows = Vec::new();
        let mut page = Page::First { path, query };

        loop {
            let (page_path, page_query) = page.request();
            let doc: ListDocument<A> = self
                .request(Method::GET, page_path, page_query, None::<&()>)
                .await?;
            rows.extend(doc.data);

            let Some(next) = next_page(doc.links.and_then(|links| links.next)) else {
                return Ok(rows);
            };
            page = next;
        }
    }

    /// Send a request that returns no body on success (typically `DELETE`).
    pub async fn request_no_content<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&B>,
    ) -> Result<()> {
        let (status, bytes) = self.send(method, path, query, body, &[]).await?;
        if !status.is_success() {
            return Err(api_error(status, &bytes));
        }
        Ok(())
    }

    /// Send an authenticated request whose successful response is raw bytes.
    /// Domain APIs use this for endpoints such as sales reports that return a
    /// compressed file instead of a JSON:API document.
    pub async fn request_bytes<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&B>,
        headers: &[(&str, &str)],
    ) -> Result<Vec<u8>> {
        let (status, bytes) = self.send(method, path, query, body, headers).await?;
        if !status.is_success() {
            return Err(api_error(status, &bytes));
        }
        Ok(bytes)
    }

    async fn send<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&B>,
        headers: &[(&str, &str)],
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
        for (name, value) in headers {
            req = req.header(*name, *value);
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
    pub async fn upload_bytes(
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

/// Decide the page that follows one carrying this `links.next`.
///
/// Returns `None` when the collection is done. Two ways that happens:
/// Apple offered no `next` link, or it offered one we cannot turn into a
/// path. The second case ends the walk instead of failing it — stopping
/// early shows up as a short list, where a hard failure would take down a
/// command that was otherwise working. See [`path_and_query`].
fn next_page(next_link: Option<String>) -> Option<Page<'static>> {
    Some(Page::Next(path_and_query(&next_link?)?))
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

#[cfg(test)]
mod tests {
    use super::*;

    // `links.next` is the whole paging decision, so it is pinned against the
    // URL shape App Store Connect actually sends rather than a hand-made one.
    #[test]
    fn follows_a_next_link_as_a_path() {
        let link =
            "https://api.appstoreconnect.apple.com/v1/apps?limit%5Bapps%5D=200&cursor=AQ%3D%3D";
        assert_eq!(
            next_page(Some(link.into())),
            Some(Page::Next(
                "/v1/apps?limit%5Bapps%5D=200&cursor=AQ%3D%3D".into()
            ))
        );
    }

    /// A collection is done when Apple stops offering a `next` link — which
    /// is also how it signals a single-page collection.
    #[test]
    fn ends_the_walk_on_a_last_page() {
        assert_eq!(next_page(None), None);
    }

    /// Stopping early shows up as a short list; failing here would take down
    /// a command that was otherwise working.
    #[test]
    fn ends_the_walk_on_a_link_it_cannot_parse() {
        assert_eq!(next_page(Some("/v1/apps?cursor=AQ".into())), None);
        assert_eq!(
            next_page(Some("https://api.appstoreconnect.apple.com".into())),
            None
        );
    }

    /// The first page is the caller's request, unchanged.
    #[test]
    fn first_page_keeps_the_callers_query() {
        let query = [("limit", "200"), ("filter[bundleId]", "com.x.y")];
        let page = Page::First {
            path: "/v1/apps",
            query: &query,
        };
        assert_eq!(page.request(), ("/v1/apps", &query[..]));
    }

    /// Apple's `next` URL already carries `include`/`filter`/`limit`, so
    /// re-appending the original query would duplicate every parameter.
    #[test]
    fn later_pages_send_only_the_path_apple_named() {
        let page = Page::Next("/v1/apps?limit=200&cursor=AQ".into());
        assert_eq!(page.request(), ("/v1/apps?limit=200&cursor=AQ", &[][..]));
    }
}
