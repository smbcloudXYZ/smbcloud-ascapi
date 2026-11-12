//! Per-APK device targeting. The body is passed through untouched: Amazon
//! owns its shape (supported and excluded devices), so the safe edit is
//! read, change the fields you mean to, and put the whole document back.

use reqwest::Method;
use serde_json::Value;

use crate::client::AmazonClient;
use crate::error::Result;

impl AmazonClient {
    pub async fn get_targeting(&self, app_id: &str, edit_id: &str, apk_id: &str) -> Result<Value> {
        let url = self.url(app_id, &format!("/edits/{edit_id}/apks/{apk_id}/targeting"));
        let request = self.request(Method::GET, &url).await?;
        Ok(self.send(request).await?.json().await?)
    }

    pub async fn put_targeting(
        &self,
        app_id: &str,
        edit_id: &str,
        apk_id: &str,
        targeting: &Value,
    ) -> Result<Value> {
        let url = self.url(app_id, &format!("/edits/{edit_id}/apks/{apk_id}/targeting"));
        let request = self.request(Method::PUT, &url).await?.json(targeting);
        let response = self.send(request).await?;
        let text = response.text().await?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&text)?)
    }
}
