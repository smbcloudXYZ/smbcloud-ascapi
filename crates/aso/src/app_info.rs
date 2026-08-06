//! An `AppInfo` is the platform-independent, category/age-rating side of an
//! app's metadata; its localized name/subtitle live on
//! [`crate::app_info_localization`]. App Store Connect creates and manages
//! `AppInfo` rows itself (there is no create/delete endpoint) — this module
//! only reads them and updates category/age-rating relationships.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{Document, ListDocument, Resource};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "appInfos";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoAttributes {
    pub app_store_state: Option<String>,
    pub app_store_age_rating: Option<String>,
    pub brazil_age_rating: Option<String>,
    pub kids_age_band: Option<String>,
}

pub type AppInfo = Resource<AppInfoAttributes>;

/// AppInfos: the container for localized names and subtitles.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait AppInfosApi {
    /// `GET /v1/apps/{app_id}/appInfos`. An app usually has one current
    /// `AppInfo` (plus, briefly, a second pending one while an edit awaits
    /// review) — this is the parent resource `AppInfoLocalization`s hang
    /// off of, and the id you need for `create_app_info_localization`.
    async fn list_app_infos(&self, app_id: &str) -> Result<Vec<AppInfo>>;

    async fn get_app_info(&self, id: &str) -> Result<AppInfo>;
}

#[async_trait]
impl AppInfosApi for Client {
    async fn list_app_infos(&self, app_id: &str) -> Result<Vec<AppInfo>> {
        let path = format!("/v1/apps/{app_id}/appInfos");
        let doc: ListDocument<AppInfoAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn get_app_info(&self, id: &str) -> Result<AppInfo> {
        let path = format!("/v1/appInfos/{id}");
        let doc: Document<AppInfoAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }
}
