use reqwest::Method;
use serde_json::Value;

use crate::auth::{Credentials, TokenCache};
use crate::error::{Error, Result};

const API_ROOT: &str = "https://manage.devcenter.microsoft.com";

/// Build `/v1.0/my/applications/{id}…`. `suffix` carries its own leading `/`.
pub(crate) fn app_path(product_id: &str, suffix: &str) -> String {
    format!("/v1.0/my/applications/{product_id}{suffix}")
}

/// Build `/v1.0/my/inappproducts/{id}…` for an add-on (in-app product).
pub(crate) fn addon_path(addon_id: &str, suffix: &str) -> String {
    format!("/v1.0/my/inappproducts/{addon_id}{suffix}")
}

/// Turn an `@nextLink` (relative to `/v1.0/my/`) into a path for `send`.
pub(crate) fn next_link_path(link: &str) -> String {
    if link.starts_with('/') {
        link.to_string()
    } else {
        format!("/v1.0/my/{link}")
    }
}

pub struct MsStoreClient {
    http: reqwest::Client,
    credentials: Credentials,
    tokens: TokenCache,
    root: String,
}

impl MsStoreClient {
    pub fn new(credentials: Credentials) -> Self {
        Self {
            http: reqwest::Client::new(),
            credentials,
            tokens: TokenCache::new(),
            root: API_ROOT.to_string(),
        }
    }

    /// The product id from the credentials, used when none is passed.
    pub fn default_product_id(&self) -> &str {
        &self.credentials.product_id
    }

    async fn send(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let token = self.tokens.get(&self.http, &self.credentials).await?;
        let mut request = self
            .http
            .request(method.clone(), format!("{}{path}", self.root))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request.json(body);
        } else if method == Method::POST {
            request = request.header("Content-Length", "0");
        }
        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            });
        }
        let text = response.text().await?;
        if text.trim().is_empty() {
            Ok(Value::Null)
        } else {
            Ok(serde_json::from_str(&text)?)
        }
    }

    /// The app, including `pendingApplicationSubmission` when a draft exists.
    pub async fn get_app(&self, product_id: &str) -> Result<Value> {
        self.send(Method::GET, &app_path(product_id, ""), None)
            .await
    }

    /// Create a submission cloned from the last published one.
    pub async fn create_submission(&self, product_id: &str) -> Result<Value> {
        // No body: the API clones the last published submission, and an
        // empty JSON object is read as a submission with no listings.
        self.send(Method::POST, &app_path(product_id, "/submissions"), None)
            .await
    }

    pub async fn get_submission(&self, product_id: &str, submission_id: &str) -> Result<Value> {
        self.send(
            Method::GET,
            &app_path(product_id, &format!("/submissions/{submission_id}")),
            None,
        )
        .await
    }

    pub async fn update_submission(
        &self,
        product_id: &str,
        submission_id: &str,
        submission: &Value,
    ) -> Result<Value> {
        self.send(
            Method::PUT,
            &app_path(product_id, &format!("/submissions/{submission_id}")),
            Some(submission),
        )
        .await
    }

    pub async fn submission_status(&self, product_id: &str, submission_id: &str) -> Result<Value> {
        self.send(
            Method::GET,
            &app_path(product_id, &format!("/submissions/{submission_id}/status")),
            None,
        )
        .await
    }

    /// Commit the submission to certification:
    /// `POST /v1.0/my/applications/{id}/submissions/{sid}/commit`.
    pub async fn commit_submission(&self, product_id: &str, submission_id: &str) -> Result<Value> {
        self.send(
            Method::POST,
            &app_path(product_id, &format!("/submissions/{submission_id}/commit")),
            None,
        )
        .await
    }

    pub async fn delete_submission(&self, product_id: &str, submission_id: &str) -> Result<()> {
        self.send(
            Method::DELETE,
            &app_path(product_id, &format!("/submissions/{submission_id}")),
            None,
        )
        .await?;
        Ok(())
    }

    /// The Store ids of every add-on of an app
    /// (`GET /v1.0/my/applications/{id}/listinappproducts`), following
    /// `@nextLink`.
    pub async fn list_addon_ids(&self, product_id: &str) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        let mut path = app_path(product_id, "/listinappproducts?top=100");
        loop {
            let page = self.send(Method::GET, &path, None).await?;
            for row in page
                .get("value")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(id) = row.get("inAppProductId").and_then(Value::as_str) {
                    ids.push(id.to_string());
                }
            }
            match page.get("@nextLink").and_then(Value::as_str) {
                Some(link) if !link.is_empty() => path = next_link_path(link),
                _ => return Ok(ids),
            }
        }
    }

    /// An add-on, including `productId` (the id the app buys with) and
    /// `pendingInAppProductSubmission` when a draft exists.
    pub async fn get_addon(&self, addon_id: &str) -> Result<Value> {
        self.send(Method::GET, &addon_path(addon_id, ""), None)
            .await
    }

    /// Create an add-on submission cloned from the last published one.
    pub async fn create_addon_submission(&self, addon_id: &str) -> Result<Value> {
        self.send(Method::POST, &addon_path(addon_id, "/submissions"), None)
            .await
    }

    pub async fn get_addon_submission(&self, addon_id: &str, submission_id: &str) -> Result<Value> {
        self.send(
            Method::GET,
            &addon_path(addon_id, &format!("/submissions/{submission_id}")),
            None,
        )
        .await
    }

    pub async fn update_addon_submission(
        &self,
        addon_id: &str,
        submission_id: &str,
        submission: &Value,
    ) -> Result<Value> {
        self.send(
            Method::PUT,
            &addon_path(addon_id, &format!("/submissions/{submission_id}")),
            Some(submission),
        )
        .await
    }

    /// PUT a zip to a submission's `fileUploadUrl`, an Azure Blob SAS URL.
    /// It carries its own authorization, so no bearer token is sent.
    pub async fn upload_submission_files(&self, upload_url: &str, zip: Vec<u8>) -> Result<()> {
        let response = self
            .http
            .put(upload_url)
            .header("x-ms-blob-type", "BlockBlob")
            .body(zip)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_follow_the_v1_layout() {
        assert_eq!(app_path("9P69", ""), "/v1.0/my/applications/9P69");
        assert_eq!(
            app_path("9P69", "/submissions"),
            "/v1.0/my/applications/9P69/submissions"
        );
        assert_eq!(
            app_path("9P69", "/submissions/123/status"),
            "/v1.0/my/applications/9P69/submissions/123/status"
        );
        assert_eq!(
            app_path("9P69", "/submissions/123/commit"),
            "/v1.0/my/applications/9P69/submissions/123/commit"
        );
    }

    #[test]
    fn addon_urls_and_next_links() {
        assert_eq!(
            addon_path("9NBL", "/submissions/1"),
            "/v1.0/my/inappproducts/9NBL/submissions/1"
        );
        assert_eq!(
            next_link_path("applications/9P69/listinappproducts?skip=100&top=100"),
            "/v1.0/my/applications/9P69/listinappproducts?skip=100&top=100"
        );
        assert_eq!(next_link_path("/v1.0/my/x"), "/v1.0/my/x");
    }
}
