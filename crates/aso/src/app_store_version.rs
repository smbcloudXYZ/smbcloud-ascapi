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
    CreateBody, CreateData, Document, OptionalDocument, Resource, ResourceId, ToOne, UpdateBody,
    UpdateData, UpdateRelationshipsBody, UpdateRelationshipsData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

use crate::build::{Build, BuildAttributes};

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
    pub(crate) fn as_query_value(self) -> &'static str {
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
    /// `MANUAL`, `AFTER_APPROVAL` or `SCHEDULED`. Kept as the string Apple
    /// sent, so a value added later still parses.
    pub release_type: Option<String>,
    pub earliest_release_date: Option<String>,
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

/// Editable attributes for a `PATCH /v1/appStoreVersions/{id}`. App Store
/// Connect has no "rename a version" action: changing `version_string` on
/// the platform's editable version (e.g. a staged `1.0.9` to `1.1.0`) is a
/// plain attribute update. Only the fields set here are sent, so passing
/// just `version_string` leaves `copyright` untouched. The version must be
/// in an editable state (`PREPARE_FOR_SUBMISSION`, `DEVELOPER_REJECTED`).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreVersionUpdateFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    /// How the approved version goes live.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_type: Option<ReleaseType>,
    /// When a `SCHEDULED` version may go live, ISO 8601 with an offset,
    /// e.g. `2026-11-10T09:00:00+01:00`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earliest_release_date: Option<String>,
}

/// How an approved version is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReleaseType {
    /// Wait for the developer: the version sits in `PENDING_DEVELOPER_RELEASE`
    /// until a release request is made.
    #[serde(rename = "MANUAL")]
    Manual,
    /// Go live as soon as App Review approves.
    #[serde(rename = "AFTER_APPROVAL")]
    AfterApproval,
    /// Go live on or after `earliestReleaseDate`.
    #[serde(rename = "SCHEDULED")]
    Scheduled,
}

impl ReleaseType {
    pub fn as_str(self) -> &'static str {
        match self {
            ReleaseType::Manual => "MANUAL",
            ReleaseType::AfterApproval => "AFTER_APPROVAL",
            ReleaseType::Scheduled => "SCHEDULED",
        }
    }
}

pub const RELEASE_REQUEST_RESOURCE_TYPE: &str = "appStoreVersionReleaseRequests";
pub const PHASED_RELEASE_RESOURCE_TYPE: &str = "appStoreVersionPhasedReleases";

/// The state `app_version_state` reports for an approved version that waits
/// for a release request.
pub const PENDING_DEVELOPER_RELEASE: &str = "PENDING_DEVELOPER_RELEASE";

/// `POST /v1/appStoreVersionReleaseRequests`. Apple wants the version as a
/// relationship and no attributes.
#[derive(Debug, Clone, Serialize)]
pub struct ReleaseRequestBody {
    pub data: ReleaseRequestData,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseRequestData {
    #[serde(rename = "type")]
    pub resource_type: &'static str,
    pub relationships: ReleaseRequestRelationships,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRequestRelationships {
    pub app_store_version: ToOne,
}

impl ReleaseRequestBody {
    pub fn for_version(version_id: &str) -> Self {
        Self {
            data: ReleaseRequestData {
                resource_type: RELEASE_REQUEST_RESOURCE_TYPE,
                relationships: ReleaseRequestRelationships {
                    app_store_version: ToOne {
                        data: ResourceId {
                            resource_type: RESOURCE_TYPE,
                            id: version_id.to_string(),
                        },
                    },
                },
            },
        }
    }
}

/// A phased release of a version: Apple rolls the update out to a growing
/// share of automatic-update users over seven days.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhasedReleaseAttributes {
    /// `INACTIVE`, `ACTIVE`, `PAUSED` or `COMPLETE`.
    pub phased_release_state: Option<String>,
    pub start_date: Option<String>,
    pub current_day_number: Option<i64>,
    pub total_pause_duration: Option<i64>,
}

pub type PhasedRelease = Resource<PhasedReleaseAttributes>;

/// What `phased-release` does to a version's phased release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhasedReleaseAction {
    Create,
    Pause,
    Resume,
    Complete,
}

impl PhasedReleaseAction {
    /// The state the phased release ends in.
    pub fn target_state(self) -> &'static str {
        match self {
            PhasedReleaseAction::Create => "INACTIVE",
            PhasedReleaseAction::Pause => "PAUSED",
            PhasedReleaseAction::Resume => "ACTIVE",
            PhasedReleaseAction::Complete => "COMPLETE",
        }
    }

    /// Whether `current` (`None` when the version has no phased release)
    /// lets this action run. `Err` says why not, `Ok(true)` that the release
    /// is already in the target state and nothing needs sending.
    pub fn check(self, current: Option<&str>) -> std::result::Result<bool, String> {
        match (self, current) {
            (PhasedReleaseAction::Create, None) => Ok(false),
            (PhasedReleaseAction::Create, Some(state)) => Err(format!(
                "the version already has a phased release ({state})"
            )),
            (_, None) => Err("the version has no phased release; create one first".to_string()),
            (PhasedReleaseAction::Pause, Some("ACTIVE")) => Ok(false),
            (PhasedReleaseAction::Resume, Some("PAUSED")) => Ok(false),
            (PhasedReleaseAction::Complete, Some("ACTIVE" | "PAUSED")) => Ok(false),
            (action, Some(state)) if state == action.target_state() => Ok(true),
            (action, Some(state)) => Err(format!(
                "cannot go from {state} to {}; pause needs ACTIVE, resume needs PAUSED, complete needs ACTIVE or PAUSED",
                action.target_state()
            )),
        }
    }
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

    /// `PATCH /v1/appStoreVersions/{id}` with attributes — e.g. change the
    /// version string of the platform's editable version. Apple exposes no
    /// separate rename action; this is how `1.0.9` becomes `1.1.0`.
    async fn update_app_store_version(
        &self,
        id: &str,
        fields: AppStoreVersionUpdateFields,
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

    /// `GET /v1/appStoreVersions/{id}/build` — the build currently attached
    /// to this version, or `None` when there isn't one. The version resource
    /// itself only carries a link to it, so this is how to confirm what a
    /// submission would send.
    async fn get_app_store_version_build(&self, id: &str) -> Result<Option<Build>>;

    /// `POST /v1/appStoreVersionReleaseRequests` — release an approved
    /// version that waits in `PENDING_DEVELOPER_RELEASE`. Apple answers with
    /// a resource that carries no state, so read the version back to see it.
    async fn release_app_store_version(&self, id: &str) -> Result<()>;

    /// `GET /v1/appStoreVersions/{id}/appStoreVersionPhasedRelease` — the
    /// version's phased release, or `None` when it has none.
    async fn get_app_store_version_phased_release(&self, id: &str)
        -> Result<Option<PhasedRelease>>;

    /// `POST /v1/appStoreVersionPhasedReleases` — start a phased release for
    /// a version that has none. It is created `INACTIVE` and turns `ACTIVE`
    /// when the version is released.
    async fn create_phased_release(&self, version_id: &str) -> Result<PhasedRelease>;

    /// `PATCH /v1/appStoreVersionPhasedReleases/{id}` with a
    /// `phasedReleaseState`: `ACTIVE` resumes, `PAUSED` pauses, `COMPLETE`
    /// releases to everyone now.
    async fn update_phased_release(&self, id: &str, state: &str) -> Result<PhasedRelease>;
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
        self.list_all::<AppStoreVersionAttributes>(&path, &query)
            .await
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

    async fn update_app_store_version(
        &self,
        id: &str,
        fields: AppStoreVersionUpdateFields,
    ) -> Result<AppStoreVersion> {
        let path = format!("/v1/appStoreVersions/{id}");
        let body = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: id.to_string(),
                attributes: fields,
            },
        };
        let doc: Document<AppStoreVersionAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
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

    async fn get_app_store_version_build(&self, id: &str) -> Result<Option<Build>> {
        let path = format!("/v1/appStoreVersions/{id}/build");
        let doc: OptionalDocument<BuildAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn release_app_store_version(&self, id: &str) -> Result<()> {
        self.request_no_content(
            Method::POST,
            "/v1/appStoreVersionReleaseRequests",
            &[],
            Some(&ReleaseRequestBody::for_version(id)),
        )
        .await
    }

    async fn get_app_store_version_phased_release(
        &self,
        id: &str,
    ) -> Result<Option<PhasedRelease>> {
        let path = format!("/v1/appStoreVersions/{id}/appStoreVersionPhasedRelease");
        let doc: OptionalDocument<PhasedReleaseAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn create_phased_release(&self, version_id: &str) -> Result<PhasedRelease> {
        let body = phased_release_create_body(version_id);
        let doc: Document<PhasedReleaseAttributes> = self
            .request(
                Method::POST,
                "/v1/appStoreVersionPhasedReleases",
                &[],
                Some(&body),
            )
            .await?;
        Ok(doc.data)
    }

    async fn update_phased_release(&self, id: &str, state: &str) -> Result<PhasedRelease> {
        let path = format!("/v1/appStoreVersionPhasedReleases/{id}");
        let doc: Document<PhasedReleaseAttributes> = self
            .request(
                Method::PATCH,
                &path,
                &[],
                Some(&phased_release_update_body(id, state)),
            )
            .await?;
        Ok(doc.data)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhasedReleaseStateAttributes {
    pub phased_release_state: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhasedReleaseRelationships {
    pub app_store_version: ToOne,
}

/// The body that starts a phased release for `version_id`.
pub fn phased_release_create_body(
    version_id: &str,
) -> CreateBody<PhasedReleaseStateAttributes, PhasedReleaseRelationships> {
    CreateBody {
        data: CreateData {
            resource_type: PHASED_RELEASE_RESOURCE_TYPE,
            attributes: PhasedReleaseStateAttributes {
                phased_release_state: PhasedReleaseAction::Create.target_state().to_string(),
            },
            relationships: Some(PhasedReleaseRelationships {
                app_store_version: ToOne {
                    data: ResourceId {
                        resource_type: RESOURCE_TYPE,
                        id: version_id.to_string(),
                    },
                },
            }),
        },
    }
}

/// The body that moves phased release `id` to `state`.
pub fn phased_release_update_body(
    id: &str,
    state: &str,
) -> UpdateBody<PhasedReleaseStateAttributes> {
    UpdateBody {
        data: UpdateData {
            resource_type: PHASED_RELEASE_RESOURCE_TYPE,
            id: id.to_string(),
            attributes: PhasedReleaseStateAttributes {
                phased_release_state: state.to_string(),
            },
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `PATCH` responses are deserialized as a whole
    /// [`AppStoreVersionAttributes`], so every attribute except `platform`
    /// has to stay optional — otherwise a response that carries only the
    /// platform (as a rename does) fails to parse after App Store Connect
    /// has already applied the update. Making one of these required turns
    /// this into a failure.
    #[test]
    fn a_version_response_omitting_every_optional_attribute_still_parses() {
        let doc: Document<AppStoreVersionAttributes> = serde_json::from_value(json!({
            "data": {
                "id": "b1f9f3a2-6f9a-4a4e-8a4f-3f5b0a9c0d11",
                "type": "appStoreVersions",
                "attributes": { "platform": "IOS" }
            }
        }))
        .unwrap();

        assert_eq!(doc.data.attributes.platform, Platform::Ios);
        assert!(doc.data.attributes.version_string.is_none());
        assert!(doc.data.attributes.copyright.is_none());
    }

    /// The other half of that contract: a response that does carry the
    /// attributes round-trips, so `versionString` is read back from the
    /// rename request's own reply.
    #[test]
    fn a_version_response_reads_back_the_renamed_string() {
        let doc: Document<AppStoreVersionAttributes> = serde_json::from_value(json!({
            "data": {
                "id": "b1f9f3a2-6f9a-4a4e-8a4f-3f5b0a9c0d11",
                "type": "appStoreVersions",
                "attributes": {
                    "platform": "MAC_OS",
                    "versionString": "1.1.0",
                    "appVersionState": "PREPARE_FOR_SUBMISSION",
                    "copyright": "2026 SplitFire AB"
                }
            }
        }))
        .unwrap();

        assert_eq!(doc.data.attributes.platform, Platform::MacOs);
        assert_eq!(doc.data.attributes.version_string.as_deref(), Some("1.1.0"));
        assert_eq!(
            doc.data.attributes.app_version_state.as_deref(),
            Some("PREPARE_FOR_SUBMISSION")
        );
    }

    /// Apple answers `{"data": null}` for a version with no build attached;
    /// that has to parse as "none", not as an error.
    #[test]
    fn a_null_build_body_parses_to_none() {
        let doc: OptionalDocument<BuildAttributes> =
            serde_json::from_value(json!({ "data": null })).unwrap();
        assert!(doc.data.is_none());
    }

    #[test]
    fn a_populated_build_body_parses_with_its_version() {
        let doc: OptionalDocument<BuildAttributes> = serde_json::from_value(json!({
            "data": {
                "id": "build-1",
                "type": "builds",
                "attributes": {
                    "version": "42",
                    "processingState": "VALID",
                    "usesNonExemptEncryption": false
                }
            }
        }))
        .unwrap();

        let build = doc.data.unwrap();
        assert_eq!(build.id, "build-1");
        assert_eq!(build.attributes.version.as_deref(), Some("42"));
        assert_eq!(build.attributes.uses_non_exempt_encryption, Some(false));
    }

    #[test]
    fn an_update_sends_only_the_release_fields_that_are_set() {
        let fields = AppStoreVersionUpdateFields {
            release_type: Some(ReleaseType::Scheduled),
            earliest_release_date: Some("2026-11-10T09:00:00+01:00".to_string()),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&fields).unwrap(),
            json!({
                "releaseType": "SCHEDULED",
                "earliestReleaseDate": "2026-11-10T09:00:00+01:00"
            })
        );
        let manual = AppStoreVersionUpdateFields {
            release_type: Some(ReleaseType::Manual),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&manual).unwrap(),
            json!({ "releaseType": "MANUAL" })
        );
    }

    #[test]
    fn a_release_request_names_the_version_and_sends_no_attributes() {
        assert_eq!(
            serde_json::to_value(ReleaseRequestBody::for_version("V1")).unwrap(),
            json!({
                "data": {
                    "type": "appStoreVersionReleaseRequests",
                    "relationships": {
                        "appStoreVersion": {
                            "data": { "type": "appStoreVersions", "id": "V1" }
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn phased_release_bodies_match_the_api() {
        assert_eq!(
            serde_json::to_value(phased_release_create_body("V1")).unwrap(),
            json!({
                "data": {
                    "type": "appStoreVersionPhasedReleases",
                    "attributes": { "phasedReleaseState": "INACTIVE" },
                    "relationships": {
                        "appStoreVersion": {
                            "data": { "type": "appStoreVersions", "id": "V1" }
                        }
                    }
                }
            })
        );
        assert_eq!(
            serde_json::to_value(phased_release_update_body("P1", "PAUSED")).unwrap(),
            json!({
                "data": {
                    "type": "appStoreVersionPhasedReleases",
                    "id": "P1",
                    "attributes": { "phasedReleaseState": "PAUSED" }
                }
            })
        );
    }

    #[test]
    fn phased_release_actions_only_run_from_a_state_that_allows_them() {
        use PhasedReleaseAction::*;
        assert_eq!(Create.check(None), Ok(false));
        assert!(Create.check(Some("INACTIVE")).is_err());
        assert!(Pause.check(None).is_err());
        assert_eq!(Pause.check(Some("ACTIVE")), Ok(false));
        assert_eq!(Pause.check(Some("PAUSED")), Ok(true));
        assert!(Pause.check(Some("INACTIVE")).is_err());
        assert_eq!(Resume.check(Some("PAUSED")), Ok(false));
        assert_eq!(Resume.check(Some("ACTIVE")), Ok(true));
        assert!(Resume.check(Some("COMPLETE")).is_err());
        assert_eq!(Complete.check(Some("ACTIVE")), Ok(false));
        assert_eq!(Complete.check(Some("PAUSED")), Ok(false));
        assert_eq!(Complete.check(Some("COMPLETE")), Ok(true));
        assert!(Complete.check(Some("INACTIVE")).is_err());
    }

    #[test]
    fn a_phased_release_response_parses() {
        let doc: OptionalDocument<PhasedReleaseAttributes> = serde_json::from_value(json!({
            "data": {
                "id": "P1",
                "type": "appStoreVersionPhasedReleases",
                "attributes": {
                    "phasedReleaseState": "ACTIVE",
                    "startDate": "2026-11-10T08:00:00Z",
                    "currentDayNumber": 2,
                    "totalPauseDuration": 0
                }
            }
        }))
        .unwrap();
        let release = doc.data.unwrap();
        assert_eq!(
            release.attributes.phased_release_state.as_deref(),
            Some("ACTIVE")
        );
        assert_eq!(release.attributes.current_day_number, Some(2));
        let none: OptionalDocument<PhasedReleaseAttributes> =
            serde_json::from_value(json!({ "data": null })).unwrap();
        assert!(none.data.is_none());
    }
}
