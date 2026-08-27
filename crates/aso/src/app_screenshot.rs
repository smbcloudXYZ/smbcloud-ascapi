//! An `AppScreenshot` is one image binary within an `AppScreenshotSet`.
//! Uploading one is a three-step JSON:API + raw-HTTP dance (see
//! <https://developer.apple.com/documentation/appstoreconnectapi/uploading-assets-to-app-store-connect>):
//!
//! 1. `POST /v1/appScreenshots` reserves the asset and returns pre-signed
//!    `uploadOperations` (one or more chunked PUT requests).
//! 2. The image bytes are PUT to each operation's URL with its exact
//!    `requestHeaders` — these URLs are *not* App Store Connect API
//!    endpoints, so no bearer token is sent.
//! 3. `PATCH /v1/appScreenshots/{id}` with `uploaded: true` and an MD5
//!    checksum of the file commits the upload.
//!
//! [`AppScreenshotsApi::upload_app_screenshot`] does all three steps in one call.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Deserializer, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, ListDocument, Resource, ResourceId, ToOne, UpdateBody,
    UpdateData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "appScreenshots";

/// App Store Connect sends `uploadOperations: null` (not an omitted field)
/// once a screenshot has been committed — plain `#[serde(default)]` only
/// covers a *missing* field, not an explicit `null`, so `Vec<T>` fails to
/// deserialize without this.
fn null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadOperation {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub length: u64,
    #[serde(default)]
    pub offset: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub request_headers: Vec<RequestHeader>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDeliveryState {
    pub state: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub errors: Vec<serde_json::Value>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub warnings: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScreenshotAttributes {
    pub file_name: String,
    pub file_size: u64,
    #[serde(default)]
    pub source_file_checksum: Option<String>,
    #[serde(default)]
    pub image_asset: Option<serde_json::Value>,
    #[serde(default)]
    pub asset_delivery_state: Option<AssetDeliveryState>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub upload_operations: Vec<UploadOperation>,
    #[serde(default)]
    pub uploaded: Option<bool>,
}

pub type AppScreenshot = Resource<AppScreenshotAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScreenshotCreateAttributes {
    pub file_name: String,
    pub file_size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScreenshotSetRelationship {
    pub app_screenshot_set: ToOne,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScreenshotCommitAttributes {
    pub uploaded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file_checksum: Option<String>,
}

/// Screenshot image binaries within a set.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait AppScreenshotsApi {
    /// `GET /v1/appScreenshotSets/{id}/appScreenshots`.
    async fn list_app_screenshots(&self, app_screenshot_set_id: &str)
        -> Result<Vec<AppScreenshot>>;

    /// `POST /v1/appScreenshots` — reserves the asset and returns
    /// pre-signed `uploadOperations`. Prefer [`AppScreenshotsApi::upload_app_screenshot`]
    /// unless you need to drive the upload/commit steps yourself.
    async fn create_app_screenshot(
        &self,
        app_screenshot_set_id: &str,
        attributes: AppScreenshotCreateAttributes,
    ) -> Result<AppScreenshot>;

    /// PUTs `bytes` to every one of `screenshot`'s reserved
    /// `uploadOperations`, slicing the buffer per operation's `offset` /
    /// `length`. Does not commit the upload — call
    /// [`AppScreenshotsApi::commit_app_screenshot`] (or use
    /// [`AppScreenshotsApi::upload_app_screenshot`]) after this succeeds.
    async fn upload_app_screenshot_bytes(
        &self,
        screenshot: &AppScreenshot,
        bytes: &[u8],
    ) -> Result<()>;

    /// `PATCH /v1/appScreenshots/{id}` with `uploaded: true` — tells App
    /// Store Connect the binary is fully transferred so it can start
    /// processing/validating the asset.
    async fn commit_app_screenshot(
        &self,
        id: &str,
        source_file_checksum: String,
    ) -> Result<AppScreenshot>;

    /// `DELETE /v1/appScreenshots/{id}`.
    async fn delete_app_screenshot(&self, id: &str) -> Result<()>;

    /// Reserve, upload, and commit an image in one call: `bytes` becomes an
    /// `AppScreenshot` under `app_screenshot_set_id`, named `file_name`.
    /// This is the entry point most callers want; the lower-level
    /// `create_app_screenshot` / `upload_app_screenshot_bytes` /
    /// `commit_app_screenshot` are exposed for callers that need to
    /// checkpoint between steps (e.g. a CLI resuming a failed upload).
    async fn upload_app_screenshot(
        &self,
        app_screenshot_set_id: &str,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<AppScreenshot>;
}

#[async_trait]
impl AppScreenshotsApi for Client {
    async fn list_app_screenshots(
        &self,
        app_screenshot_set_id: &str,
    ) -> Result<Vec<AppScreenshot>> {
        let path = format!("/v1/appScreenshotSets/{app_screenshot_set_id}/appScreenshots");
        let doc: ListDocument<AppScreenshotAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn create_app_screenshot(
        &self,
        app_screenshot_set_id: &str,
        attributes: AppScreenshotCreateAttributes,
    ) -> Result<AppScreenshot> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes,
                relationships: Some(AppScreenshotSetRelationship {
                    app_screenshot_set: ToOne {
                        data: ResourceId {
                            resource_type: crate::app_screenshot_set::RESOURCE_TYPE,
                            id: app_screenshot_set_id.to_string(),
                        },
                    },
                }),
            },
        };
        let doc: Document<AppScreenshotAttributes> = self
            .request(Method::POST, "/v1/appScreenshots", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    async fn upload_app_screenshot_bytes(
        &self,
        screenshot: &AppScreenshot,
        bytes: &[u8],
    ) -> Result<()> {
        for op in &screenshot.attributes.upload_operations {
            let start = op.offset as usize;
            let end = start + op.length as usize;
            let chunk = bytes
                .get(start..end)
                .map(|slice| slice.to_vec())
                .unwrap_or_default();
            let headers: Vec<(String, String)> = op
                .request_headers
                .iter()
                .map(|h| (h.name.clone(), h.value.clone()))
                .collect();
            let method: Method = op.method.parse().unwrap_or(Method::PUT);
            self.upload_bytes(method, &op.url, &headers, chunk).await?;
        }
        Ok(())
    }

    async fn commit_app_screenshot(
        &self,
        id: &str,
        source_file_checksum: String,
    ) -> Result<AppScreenshot> {
        let path = format!("/v1/appScreenshots/{id}");
        let body = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: id.to_string(),
                attributes: AppScreenshotCommitAttributes {
                    uploaded: true,
                    source_file_checksum: Some(source_file_checksum),
                },
            },
        };
        let doc: Document<AppScreenshotAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }

    async fn delete_app_screenshot(&self, id: &str) -> Result<()> {
        let path = format!("/v1/appScreenshots/{id}");
        self.request_no_content::<()>(Method::DELETE, &path, &[], None)
            .await
    }

    async fn upload_app_screenshot(
        &self,
        app_screenshot_set_id: &str,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<AppScreenshot> {
        let checksum = format!("{:x}", md5::compute(&bytes));
        let screenshot = self
            .create_app_screenshot(
                app_screenshot_set_id,
                AppScreenshotCreateAttributes {
                    file_name,
                    file_size: bytes.len() as u64,
                },
            )
            .await?;
        self.upload_app_screenshot_bytes(&screenshot, &bytes)
            .await?;
        self.commit_app_screenshot(&screenshot.id, checksum).await
    }
}
