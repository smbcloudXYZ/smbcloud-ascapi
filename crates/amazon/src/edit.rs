//! The edit is Amazon's draft of the next release: APKs, targeting and
//! listing changes accumulate on it until it is committed in the console.

use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::client::AmazonClient;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    pub id: String,
    #[serde(default)]
    pub status: Option<String>,
    /// Fields this crate does not model are kept so output stays complete.
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl AmazonClient {
    /// The open edit, or `None` when the app has none (HTTP 404).
    pub async fn get_edit(&self, app_id: &str) -> Result<Option<Edit>> {
        let url = self.url(app_id, "/edits");
        let response = self.request(Method::GET, &url).await?.send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            });
        }
        // Amazon answers 200 with `{}` when the app has no open edit.
        let value: serde_json::Value = response.json().await?;
        if value.get("id").is_none() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(value)?))
    }

    /// Open a new edit. Fails if one is already open.
    pub async fn create_edit(&self, app_id: &str) -> Result<Edit> {
        let url = self.url(app_id, "/edits");
        let request = self.request(Method::POST, &url).await?;
        Ok(self.send(request).await?.json().await?)
    }

    /// Ask Amazon to validate the edit; the reply lists any problems.
    pub async fn validate_edit(&self, app_id: &str, edit_id: &str) -> Result<serde_json::Value> {
        let url = self.url(app_id, &format!("/edits/{edit_id}/validate"));
        let request = self.request(Method::POST, &url).await?;
        let response = self.send(request).await?;
        let text = response.text().await?;
        if text.trim().is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn delete_edit(&self, app_id: &str, edit_id: &str) -> Result<()> {
        let url = self.url(app_id, &format!("/edits/{edit_id}"));
        let request = self.request(Method::DELETE, &url).await?;
        self.send(request).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::auth::Credentials;
    use crate::client::testserver::serve;
    use crate::AmazonClient;

    const TOKEN_OK: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 26\r\nConnection: close\r\n\r\n{\"access_token\":\"tok-123\"}";
    const EMPTY: &str =
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";

    #[tokio::test]
    async fn an_empty_object_means_no_open_edit() {
        let (base, _seen) = serve(vec![TOKEN_OK, EMPTY]).await;
        let creds = Credentials::from_json(r#"{"client_id":"c","client_secret":"s"}"#).unwrap();
        let client = AmazonClient::new(creds)
            .with_endpoints(&format!("{base}/auth/o2/token"), &format!("{base}/apps"));
        assert!(client.get_edit("APP").await.unwrap().is_none());
    }
}
