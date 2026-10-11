//! Read-only lookup for `Build` — the processed artifact behind an uploaded
//! `.ipa`/`.pkg` — and `PreReleaseVersion`, the version string App Store
//! Connect groups builds under for TestFlight.
//!
//! Uploading the binary itself is out of this crate's scope (done via
//! `xcrun altool`/Xcode); this module exists so a caller can check whether App
//! Store Connect finished processing a build as `VALID` or `INVALID` after
//! upload — the signal that actually confirms a binary-rejection fix worked,
//! independent of an `AppStoreVersion`'s `appVersionState` (which only
//! changes once a build is attached to the version and resubmitted — outside
//! this crate's scope).

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{Document, Resource};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

use crate::app_store_version::Platform;

pub const RESOURCE_TYPE: &str = "builds";
pub const PRE_RELEASE_VERSION_RESOURCE_TYPE: &str = "preReleaseVersions";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreReleaseVersionAttributes {
    pub version: Option<String>,
    pub platform: Option<Platform>,
}

pub type PreReleaseVersion = Resource<PreReleaseVersionAttributes>;

/// How to narrow a `list_builds` query. `None` fields are left off the
/// request, so the default — all fields `None` — lists every build for the
/// app, matching the original parameter-free behavior.
#[derive(Debug, Clone, Default)]
pub struct BuildFilter {
    /// `filter[version]` — the build (upload) number, e.g. `"42"`. Note this
    /// is the build's own `version`, *not* the marketing `preReleaseVersion`
    /// string; use [`BuildFilter::pre_release_version`] for the latter.
    pub version: Option<String>,
    /// `filter[preReleaseVersion.version]` — the marketing version the build
    /// sits under, e.g. `"1.1.0"`.
    pub pre_release_version: Option<String>,
    /// `filter[processingState]` — one of `PROCESSING`, `FAILED`, `INVALID`,
    /// `VALID`.
    pub processing_state: Option<String>,
}

impl BuildFilter {
    /// Collect the set fields into `filter[...]` query pairs. Returns owned
    /// strings because the values come from the caller at runtime.
    fn query_pairs(&self) -> Vec<(&'static str, String)> {
        let mut pairs = Vec::new();
        if let Some(v) = &self.version {
            pairs.push(("filter[version]", v.clone()));
        }
        if let Some(v) = &self.pre_release_version {
            pairs.push(("filter[preReleaseVersion.version]", v.clone()));
        }
        if let Some(v) = &self.processing_state {
            pairs.push(("filter[processingState]", v.clone()));
        }
        pairs
    }
}

/// Uploaded builds, which a version must have attached.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait BuildsApi {
    /// `GET /v1/builds?filter[app]={app_id}`, sorted newest-first by
    /// `uploadedDate` client-side, so the build most recently uploaded (e.g.
    /// by a fixplist re-upload) is `list_builds(app_id, &filter).await?.first()`.
    ///
    /// The top-level collection rather than `/v1/apps/{app_id}/builds`:
    /// that related-resource endpoint only accepts `fields`/`limit` and
    /// rejects `filter[...]` and `sort`.
    ///
    /// Pass `&BuildFilter::default()` for every build, or set fields to
    /// narrow by build number, pre-release (marketing) version, or
    /// processing state.
    async fn list_builds(&self, app_id: &str, filter: &BuildFilter) -> Result<Vec<Build>>;

    /// `GET /v1/builds/{id}` — a single build by its App Store Connect id.
    async fn get_build(&self, id: &str) -> Result<Build>;

    /// `GET /v1/preReleaseVersions?filter[app]={app_id}`, optionally narrowed
    /// to one platform (the related `/v1/apps/{app_id}/preReleaseVersions`
    /// endpoint does not accept `filter[platform]`). These are the marketing version strings (e.g. `1.1.0`)
    /// TestFlight groups builds under; a build's
    /// `filter[preReleaseVersion.version]` matches one of these.
    async fn list_pre_release_versions(
        &self,
        app_id: &str,
        filter_platform: Option<Platform>,
    ) -> Result<Vec<PreReleaseVersion>>;
}

#[async_trait]
impl BuildsApi for Client {
    async fn list_builds(&self, app_id: &str, filter: &BuildFilter) -> Result<Vec<Build>> {
        let owned = filter.query_pairs();
        let mut query: Vec<(&str, &str)> = vec![("filter[app]", app_id), ("limit", "200")];
        query.extend(owned.iter().map(|(k, v)| (*k, v.as_str())));
        let mut builds = self
            .list_all::<BuildAttributes>("/v1/builds", &query)
            .await?;
        builds.sort_by(|a, b| b.attributes.uploaded_date.cmp(&a.attributes.uploaded_date));
        Ok(builds)
    }

    async fn get_build(&self, id: &str) -> Result<Build> {
        let path = format!("/v1/builds/{id}");
        let doc: Document<BuildAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn list_pre_release_versions(
        &self,
        app_id: &str,
        filter_platform: Option<Platform>,
    ) -> Result<Vec<PreReleaseVersion>> {
        let mut query: Vec<(&str, &str)> = vec![("filter[app]", app_id), ("limit", "200")];
        if let Some(platform) = filter_platform {
            query.push(("filter[platform]", platform.as_query_value()));
        }
        self.list_all::<PreReleaseVersionAttributes>("/v1/preReleaseVersions", &query)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smbcloud_ascapi_core::jsonapi::ListDocument;

    #[test]
    fn default_filter_sends_no_query_pairs() {
        // `BuildFilter::default()` must list every build for the app — the
        // original parameter-free behavior — so it adds no `filter[...]`.
        assert!(BuildFilter::default().query_pairs().is_empty());
    }

    #[test]
    fn each_field_maps_to_its_asc_filter_key() {
        // The ASC filter keys are load-bearing: `version` is the build
        // number, but the marketing version is `preReleaseVersion.version`,
        // not `preReleaseVersion`. Getting these wrong silently returns the
        // wrong builds (or all of them), so pin every key and value.
        assert_eq!(
            BuildFilter {
                version: Some("42".into()),
                ..Default::default()
            }
            .query_pairs(),
            vec![("filter[version]", "42".to_string())],
        );
        assert_eq!(
            BuildFilter {
                pre_release_version: Some("1.1.0".into()),
                ..Default::default()
            }
            .query_pairs(),
            vec![("filter[preReleaseVersion.version]", "1.1.0".to_string())],
        );
        assert_eq!(
            BuildFilter {
                processing_state: Some("VALID".into()),
                ..Default::default()
            }
            .query_pairs(),
            vec![("filter[processingState]", "VALID".to_string())],
        );
    }

    #[test]
    fn all_fields_set_are_all_sent() {
        let pairs = BuildFilter {
            version: Some("42".into()),
            pre_release_version: Some("1.1.0".into()),
            processing_state: Some("VALID".into()),
        }
        .query_pairs();
        assert_eq!(
            pairs,
            vec![
                ("filter[version]", "42".to_string()),
                ("filter[preReleaseVersion.version]", "1.1.0".to_string()),
                ("filter[processingState]", "VALID".to_string()),
            ],
        );
    }

    #[test]
    fn deserializes_a_builds_list_response() {
        let body = serde_json::json!({
            "data": [
                {
                    "id": "BUILD1",
                    "type": "builds",
                    "attributes": {
                        "version": "42",
                        "uploadedDate": "2024-01-02T03:04:05Z",
                        "expired": false,
                        "processingState": "VALID",
                        "minOsVersion": "17.0"
                    }
                }
            ]
        });

        let doc: ListDocument<BuildAttributes> =
            serde_json::from_value(body).expect("a builds list must deserialize");
        assert_eq!(doc.data.len(), 1);
        let build = &doc.data[0];
        assert_eq!(build.id, "BUILD1");
        assert_eq!(build.attributes.version.as_deref(), Some("42"));
        assert_eq!(build.attributes.processing_state.as_deref(), Some("VALID"));
        assert_eq!(build.attributes.expired, Some(false));
    }

    #[test]
    fn deserializes_a_pre_release_versions_response() {
        let body = serde_json::json!({
            "data": [
                {
                    "id": "PRV1",
                    "type": "preReleaseVersions",
                    "attributes": {
                        "version": "1.1.0",
                        "platform": "IOS"
                    }
                }
            ]
        });

        let doc: ListDocument<PreReleaseVersionAttributes> =
            serde_json::from_value(body).expect("a preReleaseVersions list must deserialize");
        assert_eq!(doc.data.len(), 1);
        let prv = &doc.data[0];
        assert_eq!(prv.attributes.version.as_deref(), Some("1.1.0"));
        assert_eq!(prv.attributes.platform, Some(Platform::Ios));
    }
}
