use std::path::PathBuf;

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::auth::{Authenticator, OAuthClient, StoredToken};
use crate::error::{Error, Result};
use crate::types::{Account, AdUnit, App, CreateAdUnit};

const API_ROOT: &str = "https://admob.googleapis.com";

/// `accounts/pub-123` from either `pub-123` or `accounts/pub-123`.
pub(crate) fn account_name(account: &str) -> String {
    if account.starts_with("accounts/") {
        account.to_string()
    } else {
        format!("accounts/{account}")
    }
}

/// A 403 on create means limited access has not been granted; every other
/// failure stays a plain API error.
pub(crate) fn map_create_error(error: Error) -> Error {
    match error {
        Error::Api {
            status: 403,
            detail,
        } => Error::CreateNotPermitted { detail },
        other => other,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
    #[serde(default)]
    next_page_token: Option<String>,
}

pub struct AdmobClient {
    http: reqwest::Client,
    auth: Authenticator,
    root: String,
}

impl AdmobClient {
    /// `token_path` defaults to `~/.config/asc/admob-token.json`.
    pub fn new(oauth: OAuthClient, token_path: Option<PathBuf>) -> Self {
        Self {
            http: reqwest::Client::new(),
            auth: Authenticator::new(oauth, token_path.unwrap_or_else(StoredToken::default_path)),
            root: API_ROOT.to_string(),
        }
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path_and_query: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T> {
        let token = self.auth.access_token(&self.http).await?;
        let mut request = self
            .http
            .request(method, format!("{}{path_and_query}", self.root))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await?;
        let status = response.status();
        if status.is_success() {
            Ok(response.json().await?)
        } else {
            Err(Error::Api {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            })
        }
    }

    /// Follow `nextPageToken` until the list ends. `key` is the response
    /// field holding the items.
    async fn list_all<T: DeserializeOwned>(&self, path: &str, key: &str) -> Result<Vec<T>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut url = reqwest::Url::parse("https://x.invalid").expect("static url");
            url.set_path(path);
            if let Some(t) = &token {
                url.query_pairs_mut().append_pair("pageToken", t);
            }
            let target = match url.query() {
                Some(q) => format!("{}?{q}", url.path()),
                None => url.path().to_string(),
            };
            let value: serde_json::Value = self.request(Method::GET, &target, None::<&()>).await?;
            let mut shaped = serde_json::Map::new();
            shaped.insert("items".into(), value.get(key).cloned().unwrap_or_default());
            if let Some(next) = value.get("nextPageToken") {
                shaped.insert("nextPageToken".into(), next.clone());
            }
            let page: Page<T> = serde_json::from_value(serde_json::Value::Object(shaped))?;
            out.extend(page.items);
            match page.next_page_token {
                Some(next) if !next.is_empty() => token = Some(next),
                _ => return Ok(out),
            }
        }
    }

    pub async fn list_accounts(&self) -> Result<Vec<Account>> {
        self.list_all("/v1/accounts", "account").await
    }

    pub async fn list_apps(&self, account: &str) -> Result<Vec<App>> {
        let path = format!("/v1/{}/apps", account_name(account));
        self.list_all(&path, "apps").await
    }

    pub async fn list_ad_units(&self, account: &str) -> Result<Vec<AdUnit>> {
        let path = format!("/v1/{}/adUnits", account_name(account));
        self.list_all(&path, "adUnits").await
    }

    /// `accounts.adUnits.create` (v1beta). Limited access: see
    /// [`Error::CreateNotPermitted`].
    pub async fn create_ad_unit(&self, account: &str, body: &CreateAdUnit) -> Result<AdUnit> {
        let path = format!("/v1beta/{}/adUnits", account_name(account));
        self.request(Method::POST, &path, Some(body))
            .await
            .map_err(map_create_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_403_on_create_maps_to_the_limited_access_error() {
        let mapped = map_create_error(Error::Api {
            status: 403,
            detail: "denied".to_string(),
        });
        let text = mapped.to_string();
        assert!(matches!(mapped, Error::CreateNotPermitted { .. }));
        assert!(text.contains("limited access"));
        assert!(text.contains("asc admob adunits list"));
    }

    #[test]
    fn other_create_failures_pass_through() {
        let mapped = map_create_error(Error::Api {
            status: 400,
            detail: "bad".to_string(),
        });
        assert!(matches!(mapped, Error::Api { status: 400, .. }));
    }

    #[test]
    fn account_names_accept_both_forms() {
        assert_eq!(account_name("pub-1"), "accounts/pub-1");
        assert_eq!(account_name("accounts/pub-1"), "accounts/pub-1");
    }
}
