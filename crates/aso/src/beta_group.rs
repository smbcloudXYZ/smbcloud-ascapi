//! `BetaGroup` — a TestFlight distribution group that can receive builds.
//!
//! A beta group is either an *internal* tester group (only team members with
//! an Apple Developer account role) or an *external* group (anyone with an
//! email invite). Adding a build to an external group triggers the optional
//! "Beta App Review" gate; internal groups skip it.
//!
//! The writable operation here — adding a build to a group — is deliberate
//! and potentially public-facing, so it stays CLI-only in the front end.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{Resource, ResourceId};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "betaGroups";

/// Attributes of a TestFlight beta group.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BetaGroupAttributes {
    /// Display name of the group, e.g. `"Internal Testers"`.
    pub name: Option<String>,
    /// Whether this is an internal-only group (team members only).
    pub is_internal_group: Option<bool>,
    /// Whether new builds are automatically distributed to this group.
    pub has_access_to_all_builds: Option<bool>,
    /// ISO 8601 timestamp of when the group was created.
    pub created_date: Option<String>,
    /// Whether a public invite link is enabled for this group.
    pub public_link_enabled: Option<bool>,
    /// The public invite link URL, if enabled.
    pub public_link: Option<String>,
    /// Current number of testers who have accepted through the public link.
    pub public_link_limit: Option<i64>,
}

/// A TestFlight beta group resource.
pub type BetaGroup = Resource<BetaGroupAttributes>;

/// Body used to add (or replace) the builds relationship on a beta group.
///
/// `POST /v1/betaGroups/{id}/relationships/builds` appends builds;
/// `PATCH /v1/betaGroups/{id}/relationships/builds` replaces the list.
/// We only need the append form here.
#[derive(Debug, Clone, Serialize)]
struct AddBuildsBody {
    data: Vec<ResourceId>,
}

/// Beta group operations on App Store Connect.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait BetaGroupsApi {
    /// `GET /v1/betaGroups?filter[app]={app_id}` — all beta groups for
    /// an app, sorted alphabetically by `name` client-side so internal
    /// and external groups are interleaved predictably.
    async fn list_beta_groups(&self, app_id: &str) -> Result<Vec<BetaGroup>>;

    /// `GET /v1/betaGroups/{id}` — a single group by its ASC id.
    async fn get_beta_group(&self, id: &str) -> Result<BetaGroup>;

    /// `POST /v1/betaGroups/{group_id}/relationships/builds` — add one or
    /// more builds to a beta group by their ASC build ids.
    ///
    /// This is an *append* operation: existing builds in the group are
    /// unaffected. Adding a build to an external group may trigger Beta
    /// App Review if the app has not received approval for that group yet.
    async fn add_builds_to_beta_group(&self, group_id: &str, build_ids: &[&str]) -> Result<()>;
}

#[async_trait]
impl BetaGroupsApi for Client {
    async fn list_beta_groups(&self, app_id: &str) -> Result<Vec<BetaGroup>> {
        let mut groups = self
            .list_all::<BetaGroupAttributes>(
                "/v1/betaGroups",
                &[("filter[app]", app_id), ("limit", "200")],
            )
            .await?;
        groups.sort_by(|a, b| a.attributes.name.cmp(&b.attributes.name));
        Ok(groups)
    }

    async fn get_beta_group(&self, id: &str) -> Result<BetaGroup> {
        use smbcloud_ascapi_core::jsonapi::Document;
        let path = format!("/v1/betaGroups/{id}");
        let doc: Document<BetaGroupAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn add_builds_to_beta_group(&self, group_id: &str, build_ids: &[&str]) -> Result<()> {
        let path = format!("/v1/betaGroups/{group_id}/relationships/builds");
        let body = AddBuildsBody {
            data: build_ids
                .iter()
                .map(|id| ResourceId {
                    resource_type: crate::build::RESOURCE_TYPE,
                    id: id.to_string(),
                })
                .collect(),
        };
        // A successful POST to a relationships endpoint returns 204 No Content.
        // `request_no_content` (if available) or discard the empty body.
        self.request_no_content(Method::POST, &path, &[], Some(&body))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smbcloud_ascapi_core::jsonapi::ListDocument;

    #[test]
    fn deserializes_a_beta_groups_list() {
        let body = serde_json::json!({
            "data": [
                {
                    "id": "BG1",
                    "type": "betaGroups",
                    "attributes": {
                        "name": "Internal Testers",
                        "isInternalGroup": true,
                        "hasAccessToAllBuilds": true,
                        "createdDate": "2024-01-01T00:00:00.000+0000",
                        "publicLinkEnabled": false,
                        "publicLink": null,
                        "publicLinkLimit": null
                    }
                },
                {
                    "id": "BG2",
                    "type": "betaGroups",
                    "attributes": {
                        "name": "External Beta",
                        "isInternalGroup": false,
                        "hasAccessToAllBuilds": false,
                        "createdDate": "2024-02-01T00:00:00.000+0000",
                        "publicLinkEnabled": true,
                        "publicLink": "https://testflight.apple.com/join/abc123",
                        "publicLinkLimit": 1000
                    }
                }
            ]
        });

        let doc: ListDocument<BetaGroupAttributes> =
            serde_json::from_value(body).expect("beta groups list must deserialize");
        assert_eq!(doc.data.len(), 2);

        let internal = &doc.data[0];
        assert_eq!(internal.id, "BG1");
        assert_eq!(
            internal.attributes.name.as_deref(),
            Some("Internal Testers")
        );
        assert_eq!(internal.attributes.is_internal_group, Some(true));
        assert_eq!(internal.attributes.has_access_to_all_builds, Some(true));
        assert_eq!(internal.attributes.public_link_enabled, Some(false));

        let external = &doc.data[1];
        assert_eq!(external.id, "BG2");
        assert_eq!(external.attributes.name.as_deref(), Some("External Beta"));
        assert_eq!(external.attributes.is_internal_group, Some(false));
        assert_eq!(
            external.attributes.public_link.as_deref(),
            Some("https://testflight.apple.com/join/abc123")
        );
        assert_eq!(external.attributes.public_link_limit, Some(1000));
    }

    #[test]
    fn add_builds_body_serializes_correctly() {
        let body = AddBuildsBody {
            data: vec![
                ResourceId {
                    resource_type: "builds",
                    id: "BUILD1".into(),
                },
                ResourceId {
                    resource_type: "builds",
                    id: "BUILD2".into(),
                },
            ],
        };
        let v = serde_json::to_value(&body).unwrap();
        let data = v["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0]["type"], "builds");
        assert_eq!(data[0]["id"], "BUILD1");
        assert_eq!(data[1]["id"], "BUILD2");
    }

    #[test]
    fn empty_build_ids_produces_empty_data_array() {
        let body = AddBuildsBody { data: vec![] };
        let v = serde_json::to_value(&body).unwrap();
        assert!(v["data"].as_array().unwrap().is_empty());
    }
}
