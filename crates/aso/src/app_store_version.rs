//! An `AppStoreVersion` is App Store Connect's per-platform record for an
//! app: `platform` is one of `IOS` / `MAC_OS` / `TV_OS` / `VISION_OS`.
//! Since there is no direct "create app for platform X" endpoint (see
//! [`crate::app`]), creating an `AppStoreVersion` for a platform an app
//! doesn't have one for yet — e.g. adding visionOS to an app that so far
//! only ships on iOS/macOS — *is* how you add that platform.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, ListDocument, Resource, ResourceId, ToOne,
    UpdateRelationshipsBody, UpdateRelationshipsData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "appStoreVersions";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "IOS")]
    Ios,
    #[serde(rename = "MAC_OS")]
    MacOs,
    #[serde(rename = "TV_OS")]
    TvOs,
    #[serde(rename = "VISION_OS")]
    VisionOs,
}

impl Platform {
    fn as_query_value(self) -> &'static str {
        match self {
            Platform::Ios => "IOS",
            Platform::MacOs => "MAC_OS",
            Platform::TvOs => "TV_OS",
            Platform::VisionOs => "VISION_OS",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreVersionAttributes {
    pub platform: Platform,
    pub version_string: Option<String>,
    pub app_version_state: Option<String>,
    pub copyright: Option<String>,
    pub created_date: Option<String>,
    pub downloadable: Option<bool>,
}

pub type AppStoreVersion = Resource<AppStoreVersionAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreVersionCreateAttributes {
    pub platform: Platform,
    pub version_string: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreVersionRelationships {
    pub app: ToOne,
}

/// Per-platform version records, the unit the store reviews.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait AppStoreVersionsApi {
    /// `GET /v1/apps/{app_id}/appStoreVersions`, optionally narrowed to one
    /// platform.
    async fn list_app_store_versions(
        &self,
        app_id: &str,
        filter_platform: Option<Platform>,
    ) -> Result<Vec<AppStoreVersion>>;

    async fn get_app_store_version(&self, id: &str) -> Result<AppStoreVersion>;

    /// `POST /v1/appStoreVersions`.
    async fn create_app_store_version(
        &self,
        app_id: &str,
        attributes: AppStoreVersionCreateAttributes,
    ) -> Result<AppStoreVersion>;

    /// `DELETE /v1/appStoreVersions/{id}` — only allowed while the version
    /// hasn't been submitted for review.
    async fn delete_app_store_version(&self, id: &str) -> Result<()>;

    /// `PATCH /v1/appStoreVersions/{id}` with a `build` relationship —
    /// attaches (or replaces) the `Build` this version will submit. Needed
    /// after a re-upload fixes an `INVALID_BINARY` version: the fixed
    /// build must be attached here before the version can be resubmitted
    /// for review (resubmission itself is outside this crate's scope — see
    /// [`crate::build`]'s module doc).
    async fn set_app_store_version_build(
        &self,
        id: &str,
        build_id: &str,
    ) -> Result<AppStoreVersion>;
}

#[async_trait]
impl AppStoreVersionsApi for Client {
    async fn list_app_store_versions(
        &self,
        app_id: &str,
        filter_platform: Option<Platform>,
    ) -> Result<Vec<AppStoreVersion>> {
        let path = format!("/v1/apps/{app_id}/appStoreVersions");
        let mut query = Vec::new();
        if let Some(platform) = filter_platform {
            query.push(("filter[platform]", platform.as_query_value()));
        }
        let doc: ListDocument<AppStoreVersionAttributes> = self
            .request(Method::GET, &path, &query, None::<&()>)
            .await?;
        Ok(doc.data)
    }

    async fn get_app_store_version(&self, id: &str) -> Result<AppStoreVersion> {
        let path = format!("/v1/appStoreVersions/{id}");
        let doc: Document<AppStoreVersionAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn create_app_store_version(
        &self,
        app_id: &str,
        attributes: AppStoreVersionCreateAttributes,
    ) -> Result<AppStoreVersion> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes,
                relationships: Some(AppStoreVersionRelationships {
                    app: ToOne {
                        data: ResourceId {
                            resource_type: crate::app::RESOURCE_TYPE,
                            id: app_id.to_string(),
                        },
                    },
                }),
            },
        };
        let doc: Document<AppStoreVersionAttributes> = self
            .request(Method::POST, "/v1/appStoreVersions", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    async fn delete_app_store_version(&self, id: &str) -> Result<()> {
        let path = format!("/v1/appStoreVersions/{id}");
        self.request_no_content::<()>(Method::DELETE, &path, &[], None)
            .await
    }

    async fn set_app_store_version_build(
        &self,
        id: &str,
        build_id: &str,
    ) -> Result<AppStoreVersion> {
        let path = format!("/v1/appStoreVersions/{id}");
        let body = UpdateRelationshipsBody {
            data: UpdateRelationshipsData {
                resource_type: RESOURCE_TYPE,
                id: id.to_string(),
                relationships: AppStoreVersionBuildRelationship {
                    build: ToOne {
                        data: ResourceId {
                            resource_type: crate::build::RESOURCE_TYPE,
                            id: build_id.to_string(),
                        },
                    },
                },
            },
        };
        let doc: Document<AppStoreVersionAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }
}

/// The `relationships` payload for pointing an App Store Version at a
/// build. Its own type because App Store Connect wants a PATCH body that
/// carries relationships and no attributes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreVersionBuildRelationship {
    pub build: ToOne,
}
