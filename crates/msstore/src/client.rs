use reqwest::Method;
use serde_json::Value;

use crate::auth::{Credentials, TokenCache};
use crate::error::{Error, Result};

const API_ROOT: &str = "https://manage.devcenter.microsoft.com";

/// Build `/v1.0/my/applications/{id}…`. `suffix` carries its own leading `/`.
pub(crate) fn app_path(product_id: &str, suffix: &str) -> String {
    format!("/v1.0/my/applications/{product_id}{suffix}")
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

    pub async fn delete_submission(&self, product_id: &str, submission_id: &str) -> Result<()> {
        self.send(
            Method::DELETE,
            &app_path(product_id, &format!("/submissions/{submission_id}")),
            None,
        )
        .await?;
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
    }
}
