use std::path::Path;

use reqwest::header::{CONTENT_TYPE, IF_MATCH};
use reqwest::Method;
use serde_json::Value;

use crate::client::AmazonClient;
use crate::error::{Error, Result};

/// The body type Amazon requires for APK uploads.
pub const APK_CONTENT_TYPE: &str = "application/vnd.android.package-archive";

async fn read_apk(path: &Path) -> Result<Vec<u8>> {
    tokio::fs::read(path)
        .await
        .map_err(|source| Error::FileRead {
            path: path.display().to_string(),
            source,
        })
}

impl AmazonClient {
    /// APKs on the edit, as Amazon returns them (id, versionCode, name, …).
    pub async fn list_apks(&self, app_id: &str, edit_id: &str) -> Result<Value> {
        let url = self.url(app_id, &format!("/edits/{edit_id}/apks"));
        let request = self.request(Method::GET, &url).await?;
        Ok(self.send(request).await?.json().await?)
    }

    /// Upload the first APK of an edit.
    pub async fn upload_apk(&self, app_id: &str, edit_id: &str, apk: &Path) -> Result<Value> {
        let body = read_apk(apk).await?;
        let url = self.url(app_id, &format!("/edits/{edit_id}/apks/upload"));
        let request = self
            .request(Method::POST, &url)
            .await?
            .header(CONTENT_TYPE, APK_CONTENT_TYPE)
            .body(body);
        Ok(self.send(request).await?.json().await?)
    }

    /// The APK's `ETag`, which `replace` must echo in `If-Match`.
    pub async fn apk_etag(&self, app_id: &str, edit_id: &str, apk_id: &str) -> Result<String> {
        let url = self.url(app_id, &format!("/edits/{edit_id}/apks/{apk_id}"));
        let request = self.request(Method::GET, &url).await?;
        let response = self.send(request).await?;
        response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or(Error::NoEtag("apk"))
    }

    /// Replace an APK in place. `etag` comes from [`Self::apk_etag`].
    pub async fn replace_apk(
        &self,
        app_id: &str,
        edit_id: &str,
        apk_id: &str,
        etag: &str,
        apk: &Path,
    ) -> Result<Value> {
        let body = read_apk(apk).await?;
        let url = self.url(app_id, &format!("/edits/{edit_id}/apks/{apk_id}/replace"));
        let request = self
            .request(Method::PUT, &url)
            .await?
            .header(CONTENT_TYPE, APK_CONTENT_TYPE)
            .header(IF_MATCH, etag)
            .body(body);
        Ok(self.send(request).await?.json().await?)
    }

    pub async fn delete_apk(&self, app_id: &str, edit_id: &str, apk_id: &str) -> Result<()> {
        let url = self.url(app_id, &format!("/edits/{edit_id}/apks/{apk_id}"));
        let request = self.request(Method::DELETE, &url).await?;
        self.send(request).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Credentials;
    use crate::client::testserver::serve;

    const TOKEN_OK: &str =
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 26\r\nConnection: close\r\n\r\n{\"access_token\":\"tok-123\"}";
    const JSON_OK: &str =
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\nConnection: close\r\n\r\n{\"id\":1}";
    const ETAG_OK: &str =
        "HTTP/1.1 200 OK\r\nETag: \"v7\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    fn creds() -> Credentials {
        Credentials::from_json(r#"{"client_id":"cid","client_secret":"csecret"}"#).unwrap()
    }

    fn temp_apk() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("amazon-test-{}.apk", std::process::id()));
        std::fs::write(&path, b"PK-apk-bytes").unwrap();
        path
    }

    #[tokio::test]
    async fn the_token_request_posts_the_client_credentials_form() {
        let (base, seen) = serve(vec![TOKEN_OK, JSON_OK]).await;
        let client = AmazonClient::new(creds())
            .with_endpoints(&format!("{base}/auth/o2/token"), &format!("{base}/apps"));
        client.list_apks("APP", "EDIT").await.unwrap();
        let requests = seen.await.unwrap();
        assert!(requests[0].starts_with("POST /auth/o2/token"));
        assert!(requests[0].contains("grant_type=client_credentials"));
        assert!(requests[0].contains("scope=appstore%3A%3Aapps%3Areadwrite"));
        assert!(requests[1].starts_with("GET /apps/APP/edits/EDIT/apks "));
        assert!(requests[1]
            .to_lowercase()
            .contains("authorization: bearer tok-123"));
    }

    #[tokio::test]
    async fn upload_sends_the_apk_content_type_and_bytes() {
        let (base, seen) = serve(vec![TOKEN_OK, JSON_OK]).await;
        let client = AmazonClient::new(creds())
            .with_endpoints(&format!("{base}/auth/o2/token"), &format!("{base}/apps"));
        let apk = temp_apk();
        client.upload_apk("APP", "EDIT", &apk).await.unwrap();
        let requests = seen.await.unwrap();
        assert!(requests[1].starts_with("POST /apps/APP/edits/EDIT/apks/upload "));
        assert!(requests[1]
            .to_lowercase()
            .contains("content-type: application/vnd.android.package-archive"));
        assert!(requests[1].ends_with("PK-apk-bytes"));
    }

    #[tokio::test]
    async fn replace_carries_the_etag_in_if_match() {
        let (base, seen) = serve(vec![TOKEN_OK, ETAG_OK, JSON_OK]).await;
        let client = AmazonClient::new(creds())
            .with_endpoints(&format!("{base}/auth/o2/token"), &format!("{base}/apps"));
        let etag = client.apk_etag("APP", "EDIT", "APK").await.unwrap();
        assert_eq!(etag, "\"v7\"");
        client
            .replace_apk("APP", "EDIT", "APK", &etag, &temp_apk())
            .await
            .unwrap();
        let requests = seen.await.unwrap();
        assert!(requests[2].starts_with("PUT /apps/APP/edits/EDIT/apks/APK/replace "));
        assert!(requests[2].to_lowercase().contains("if-match: \"v7\""));
    }
}
