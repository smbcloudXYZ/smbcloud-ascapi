use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::auth::{ServiceAccount, TokenCache};
use crate::error::{Error, Result};
use crate::image::{Image, ImageType};
use crate::listing::{Listing, ListingFields};
use crate::track::Track;

const API_ROOT: &str = "https://androidpublisher.googleapis.com";

/// An open edit. Nothing in it is visible to users until it is committed.
#[derive(Debug, Clone, Deserialize)]
pub struct Edit {
    pub id: String,
    #[serde(default, rename = "expiryTimeSeconds")]
    pub expiry_time_seconds: Option<String>,
}

/// Build `/androidpublisher/v3/applications/{package}/edits…`. `suffix` is
/// appended verbatim, so it carries its own leading `/` or `:`.
pub(crate) fn edits_path(package: &str, suffix: &str) -> String {
    format!("/androidpublisher/v3/applications/{package}/edits{suffix}")
}

pub(crate) fn upload_path(package: &str, edit_id: &str, locale: &str, image: ImageType) -> String {
    format!(
        "/upload/androidpublisher/v3/applications/{package}/edits/{edit_id}/listings/{locale}/{}?uploadType=media",
        image.api_name()
    )
}

pub struct PlayClient {
    http: reqwest::Client,
    account: ServiceAccount,
    tokens: TokenCache,
    root: String,
}

impl PlayClient {
    pub fn new(account: ServiceAccount) -> Self {
        Self {
            http: reqwest::Client::new(),
            account,
            tokens: TokenCache::new(),
            root: API_ROOT.to_string(),
        }
    }

    /// The service account's email, for diagnostics.
    pub fn account_email(&self) -> &str {
        &self.account.client_email
    }

    async fn send(
        &self,
        method: Method,
        path_and_query: &str,
        body: Option<(&str, Vec<u8>)>,
    ) -> Result<reqwest::Response> {
        let token = self.tokens.get(&self.http, &self.account).await?;
        let mut request = self
            .http
            .request(method, format!("{}{path_and_query}", self.root))
            .bearer_auth(token);
        if let Some((content_type, bytes)) = body {
            request = request.header("Content-Type", content_type).body(bytes);
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

    async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T> {
        let body = match body {
            Some(value) => Some(("application/json", serde_json::to_vec(value)?)),
            None => (method == Method::POST).then_some(("application/json", b"{}".to_vec())),
        };
        let response = self.send(method, path, body).await?;
        Ok(response.json().await?)
    }

    async fn no_body(&self, method: Method, path: &str) -> Result<()> {
        self.send(method, path, None).await?;
        Ok(())
    }

    pub async fn insert_edit(&self, package: &str) -> Result<Edit> {
        self.json(Method::POST, &edits_path(package, ""), None::<&()>)
            .await
    }

    pub async fn get_edit(&self, package: &str, edit_id: &str) -> Result<Edit> {
        self.json(
            Method::GET,
            &edits_path(package, &format!("/{edit_id}")),
            None::<&()>,
        )
        .await
    }

    pub async fn validate_edit(&self, package: &str, edit_id: &str) -> Result<()> {
        let path = edits_path(package, &format!("/{edit_id}:validate"));
        self.send(
            Method::POST,
            &path,
            Some(("application/json", b"{}".to_vec())),
        )
        .await?;
        Ok(())
    }

    /// Commit the edit. `changes_not_sent_for_review` is for apps with
    /// rejected changes pending: Play then publishes without sending them
    /// for review again.
    pub async fn commit_edit(
        &self,
        package: &str,
        edit_id: &str,
        changes_not_sent_for_review: bool,
    ) -> Result<()> {
        let query = if changes_not_sent_for_review {
            "?changesNotSentForReview=true"
        } else {
            ""
        };
        let path = edits_path(package, &format!("/{edit_id}:commit{query}"));
        self.send(
            Method::POST,
            &path,
            Some(("application/json", b"{}".to_vec())),
        )
        .await?;
        Ok(())
    }

    pub async fn delete_edit(&self, package: &str, edit_id: &str) -> Result<()> {
        self.no_body(Method::DELETE, &edits_path(package, &format!("/{edit_id}")))
            .await
    }

    pub async fn list_listings(&self, package: &str, edit_id: &str) -> Result<Vec<Listing>> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            listings: Vec<Listing>,
        }
        let page: Page = self
            .json(
                Method::GET,
                &edits_path(package, &format!("/{edit_id}/listings")),
                None::<&()>,
            )
            .await?;
        Ok(page.listings)
    }

    pub async fn get_listing(&self, package: &str, edit_id: &str, locale: &str) -> Result<Listing> {
        self.json(
            Method::GET,
            &edits_path(package, &format!("/{edit_id}/listings/{locale}")),
            None::<&()>,
        )
        .await
    }

    /// `listings.patch`; a locale the app does not have yet is created with
    /// `listings.update` (PUT), which Play treats as an upsert.
    pub async fn patch_listing(
        &self,
        package: &str,
        edit_id: &str,
        locale: &str,
        fields: &ListingFields,
    ) -> Result<Listing> {
        let path = edits_path(package, &format!("/{edit_id}/listings/{locale}"));
        match self.json(Method::PATCH, &path, Some(fields)).await {
            Err(Error::Api { status: 404, .. }) => {
                self.json(Method::PUT, &path, Some(fields)).await
            }
            other => other,
        }
    }

    pub async fn list_images(
        &self,
        package: &str,
        edit_id: &str,
        locale: &str,
        image_type: ImageType,
    ) -> Result<Vec<Image>> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            images: Vec<Image>,
        }
        let path = edits_path(
            package,
            &format!("/{edit_id}/listings/{locale}/{}", image_type.api_name()),
        );
        let page: Page = self.json(Method::GET, &path, None::<&()>).await?;
        Ok(page.images)
    }

    pub async fn upload_image(
        &self,
        package: &str,
        edit_id: &str,
        locale: &str,
        image_type: ImageType,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<Image> {
        #[derive(Deserialize)]
        struct Uploaded {
            image: Image,
        }
        let path = upload_path(package, edit_id, locale, image_type);
        let response = self
            .send(Method::POST, &path, Some((content_type, bytes)))
            .await?;
        let uploaded: Uploaded = response.json().await?;
        Ok(uploaded.image)
    }

    /// Remove every image in a bucket; returns how many Play reports deleted.
    pub async fn delete_all_images(
        &self,
        package: &str,
        edit_id: &str,
        locale: &str,
        image_type: ImageType,
    ) -> Result<usize> {
        #[derive(Deserialize)]
        struct Deleted {
            #[serde(default)]
            deleted: Vec<Image>,
        }
        let path = edits_path(
            package,
            &format!("/{edit_id}/listings/{locale}/{}", image_type.api_name()),
        );
        let deleted: Deleted = self.json(Method::DELETE, &path, None::<&()>).await?;
        Ok(deleted.deleted.len())
    }

    pub async fn get_track(&self, package: &str, edit_id: &str, track: &str) -> Result<Track> {
        self.json(
            Method::GET,
            &edits_path(package, &format!("/{edit_id}/tracks/{track}")),
            None::<&()>,
        )
        .await
    }

    pub async fn update_track(&self, package: &str, edit_id: &str, track: &Track) -> Result<Track> {
        self.json(
            Method::PUT,
            &edits_path(package, &format!("/{edit_id}/tracks/{}", track.track)),
            Some(track),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_urls_follow_the_v3_layout() {
        assert_eq!(
            edits_path("xyz.smbcloud.SiGit", ""),
            "/androidpublisher/v3/applications/xyz.smbcloud.SiGit/edits"
        );
        assert_eq!(
            edits_path("p", "/E1:commit"),
            "/androidpublisher/v3/applications/p/edits/E1:commit"
        );
        assert_eq!(
            edits_path("p", "/E1/listings/en-US"),
            "/androidpublisher/v3/applications/p/edits/E1/listings/en-US"
        );
    }

    #[test]
    fn uploads_use_the_upload_host_path_with_media_type() {
        assert_eq!(
            upload_path("p", "E1", "en-US", ImageType::PhoneScreenshots),
            "/upload/androidpublisher/v3/applications/p/edits/E1/listings/en-US/phoneScreenshots?uploadType=media"
        );
    }
}
