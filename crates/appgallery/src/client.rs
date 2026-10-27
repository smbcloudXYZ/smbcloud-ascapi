use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Method;

use crate::auth::ServiceAccount;
use crate::error::{Error, Result};

const API_ROOT: &str = "https://connect-api.cloud.huawei.com";

/// A signed-in AppGallery Connect client. Every request carries a freshly
/// signed JWT as its bearer token; signing is local, so there is no token
/// exchange to cache.
pub struct AppGalleryClient {
    http: reqwest::Client,
    account: ServiceAccount,
    root: String,
}

impl AppGalleryClient {
    pub fn new(account: ServiceAccount) -> Self {
        Self {
            http: reqwest::Client::new(),
            account,
            root: API_ROOT.to_string(),
        }
    }

    /// The Service Account's key id, for diagnostics.
    pub fn key_id(&self) -> &str {
        &self.account.key_id
    }

    /// Send an authenticated request and return the response when it is a
    /// 2xx, otherwise [`Error::Api`]. `path_and_query` starts with `/`. The
    /// typed Publishing API calls build on this.
    pub async fn send(
        &self,
        method: Method,
        path_and_query: &str,
        json_body: Option<&serde_json::Value>,
    ) -> Result<reqwest::Response> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let token = self.account.assertion(now)?;
        let mut request = self
            .http
            .request(method, format!("{}{path_and_query}", self.root))
            .bearer_auth(token);
        if let Some(body) = json_body {
            request = request.json(body);
        }
        let response = request.send().await?;
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            Err(Error::Api {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            })
        }
    }
}
