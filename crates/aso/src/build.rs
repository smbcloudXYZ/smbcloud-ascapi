//! Read-only lookup for `Build` — the processed artifact behind an uploaded
//! `.ipa`/`.pkg`. Uploading itself is out of this crate's scope (done via
//! `xcrun altool`/Xcode, see the CLI's sibling build scripts); this module
//! exists so a caller can check whether App Store Connect finished
//! processing a build as `VALID` or `INVALID` after upload — the signal that
//! actually confirms a binary-rejection fix worked, independent of an
//! `AppStoreVersion`'s `appVersionState` (which only changes once a build is
//! attached to the version and resubmitted — outside this crate's scope).

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{ListDocument, Resource};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "builds";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildAttributes {
    pub version: Option<String>,
    pub uploaded_date: Option<String>,
    pub expired: Option<bool>,
    pub processing_state: Option<String>,
    pub min_os_version: Option<String>,
}

pub type Build = Resource<BuildAttributes>;

/// Uploaded builds, which a version must have attached.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait BuildsApi {
    /// `GET /v1/apps/{app_id}/builds`, sorted newest-first by
    /// `uploadedDate` (client-side — this endpoint's `sort` query parameter
    /// is rejected by the API, unlike most other list endpoints), so the
    /// build most recently uploaded (e.g. by a fixplist re-upload) is
    /// `list_builds(app_id).await?.first()`.
    async fn list_builds(&self, app_id: &str) -> Result<Vec<Build>>;
}

#[async_trait]
impl BuildsApi for Client {
    async fn list_builds(&self, app_id: &str) -> Result<Vec<Build>> {
        let path = format!("/v1/apps/{app_id}/builds");
        let doc: ListDocument<BuildAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        let mut builds = doc.data;
        builds.sort_by(|a, b| b.attributes.uploaded_date.cmp(&a.attributes.uploaded_date));
        Ok(builds)
    }
}
