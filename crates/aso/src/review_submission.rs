//! `ReviewSubmission` — how an App Store Version reaches App Review.
//!
//! Submitting takes three requests: a submission for the app and platform,
//! an item on it pointing at the version, then a PATCH setting `submitted`.
//! App Store Connect keeps at most one unsubmitted (`READY_FOR_REVIEW`)
//! submission per app and platform, and creating a second one fails, so a
//! caller should reuse an open one rather than always creating. The
//! frontend crate's `review` module does that orchestration.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, Resource, ResourceId, ToOne, UpdateBody, UpdateData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

use crate::app_store_version::Platform;

pub const RESOURCE_TYPE: &str = "reviewSubmissions";
pub const ITEM_RESOURCE_TYPE: &str = "reviewSubmissionItems";

/// A submission's `state`: `READY_FOR_REVIEW` (open, not yet submitted),
/// `WAITING_FOR_REVIEW`, `IN_REVIEW`, `UNRESOLVED_ISSUES`, `CANCELING`,
/// `COMPLETING`, or `COMPLETE`.
pub const STATE_READY_FOR_REVIEW: &str = "READY_FOR_REVIEW";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSubmissionAttributes {
    pub platform: Option<Platform>,
    pub state: Option<String>,
    pub submitted_date: Option<String>,
}

pub type ReviewSubmission = Resource<ReviewSubmissionAttributes>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSubmissionItemAttributes {
    pub state: Option<String>,
}

pub type ReviewSubmissionItem = Resource<ReviewSubmissionItemAttributes>;

#[derive(Debug, Clone, Serialize)]
struct CreateSubmissionAttributes {
    platform: Platform,
}

#[derive(Debug, Clone, Serialize)]
struct CreateSubmissionRelationships {
    app: ToOne,
}

/// An item carries only relationships, so it gets its own body type rather
/// than [`CreateBody`] with an empty `attributes` object.
#[derive(Debug, Clone, Serialize)]
struct CreateItemBody {
    data: CreateItemData,
}

#[derive(Debug, Clone, Serialize)]
struct CreateItemData {
    #[serde(rename = "type")]
    resource_type: &'static str,
    relationships: CreateItemRelationships,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateItemRelationships {
    review_submission: ToOne,
    app_store_version: ToOne,
}

#[derive(Debug, Clone, Serialize)]
struct SubmittedAttributes {
    submitted: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CanceledAttributes {
    canceled: bool,
}

/// Review submissions for an app.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait ReviewSubmissionsApi {
    /// `GET /v1/reviewSubmissions?filter[app]={app_id}`, optionally narrowed
    /// to one platform and to submissions in one of `states`.
    async fn list_review_submissions(
        &self,
        app_id: &str,
        platform: Option<Platform>,
        states: &[&str],
    ) -> Result<Vec<ReviewSubmission>>;

    /// `POST /v1/reviewSubmissions` — an empty, unsubmitted submission.
    async fn create_review_submission(
        &self,
        app_id: &str,
        platform: Platform,
    ) -> Result<ReviewSubmission>;

    /// `GET /v1/reviewSubmissions/{id}/items`.
    async fn list_review_submission_items(
        &self,
        submission_id: &str,
    ) -> Result<Vec<ReviewSubmissionItem>>;

    /// `POST /v1/reviewSubmissionItems` — put an App Store Version on a
    /// submission.
    async fn add_app_store_version_to_review_submission(
        &self,
        submission_id: &str,
        app_store_version_id: &str,
    ) -> Result<ReviewSubmissionItem>;

    /// `PATCH /v1/reviewSubmissions/{id}` with `submitted: true` — sends it
    /// to App Review.
    async fn submit_review_submission(&self, submission_id: &str) -> Result<ReviewSubmission>;

    /// `PATCH /v1/reviewSubmissions/{id}` with `canceled: true` — withdraws a
    /// submission that hasn't been reviewed yet.
    async fn cancel_review_submission(&self, submission_id: &str) -> Result<ReviewSubmission>;
}

#[async_trait]
impl ReviewSubmissionsApi for Client {
    async fn list_review_submissions(
        &self,
        app_id: &str,
        platform: Option<Platform>,
        states: &[&str],
    ) -> Result<Vec<ReviewSubmission>> {
        let states = states.join(",");
        let query = list_query(app_id, platform, &states);
        self.list_all::<ReviewSubmissionAttributes>("/v1/reviewSubmissions", &query)
            .await
    }

    async fn create_review_submission(
        &self,
        app_id: &str,
        platform: Platform,
    ) -> Result<ReviewSubmission> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes: CreateSubmissionAttributes { platform },
                relationships: Some(CreateSubmissionRelationships {
                    app: to_one(crate::app::RESOURCE_TYPE, app_id),
                }),
            },
        };
        let doc: Document<ReviewSubmissionAttributes> = self
            .request(Method::POST, "/v1/reviewSubmissions", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    async fn list_review_submission_items(
        &self,
        submission_id: &str,
    ) -> Result<Vec<ReviewSubmissionItem>> {
        let path = format!("/v1/reviewSubmissions/{submission_id}/items");
        self.list_all::<ReviewSubmissionItemAttributes>(&path, &[])
            .await
    }

    async fn add_app_store_version_to_review_submission(
        &self,
        submission_id: &str,
        app_store_version_id: &str,
    ) -> Result<ReviewSubmissionItem> {
        let body = item_body(submission_id, app_store_version_id);
        let doc: Document<ReviewSubmissionItemAttributes> = self
            .request(Method::POST, "/v1/reviewSubmissionItems", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    async fn submit_review_submission(&self, submission_id: &str) -> Result<ReviewSubmission> {
        patch_submission(self, submission_id, SubmittedAttributes { submitted: true }).await
    }

    async fn cancel_review_submission(&self, submission_id: &str) -> Result<ReviewSubmission> {
        patch_submission(self, submission_id, CanceledAttributes { canceled: true }).await
    }
}

async fn patch_submission<A: Serialize + Send + Sync>(
    client: &Client,
    id: &str,
    attributes: A,
) -> Result<ReviewSubmission> {
    let path = format!("/v1/reviewSubmissions/{id}");
    let body = UpdateBody {
        data: UpdateData {
            resource_type: RESOURCE_TYPE,
            id: id.to_string(),
            attributes,
        },
    };
    let doc: Document<ReviewSubmissionAttributes> = client
        .request(Method::PATCH, &path, &[], Some(&body))
        .await?;
    Ok(doc.data)
}

fn to_one(resource_type: &'static str, id: &str) -> ToOne {
    ToOne {
        data: ResourceId {
            resource_type,
            id: id.to_string(),
        },
    }
}

fn list_query<'a>(
    app_id: &'a str,
    platform: Option<Platform>,
    states: &'a str,
) -> Vec<(&'static str, &'a str)> {
    let mut query = vec![("filter[app]", app_id), ("limit", "200")];
    if let Some(platform) = platform {
        query.push(("filter[platform]", platform.as_query_value()));
    }
    if !states.is_empty() {
        query.push(("filter[state]", states));
    }
    query
}

fn item_body(submission_id: &str, app_store_version_id: &str) -> CreateItemBody {
    CreateItemBody {
        data: CreateItemData {
            resource_type: ITEM_RESOURCE_TYPE,
            relationships: CreateItemRelationships {
                review_submission: to_one(RESOURCE_TYPE, submission_id),
                app_store_version: to_one(
                    crate::app_store_version::RESOURCE_TYPE,
                    app_store_version_id,
                ),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_query_pins_the_filter_keys() {
        assert_eq!(
            list_query("6766108771", Some(Platform::VisionOs), "READY_FOR_REVIEW"),
            vec![
                ("filter[app]", "6766108771"),
                ("limit", "200"),
                ("filter[platform]", "VISION_OS"),
                ("filter[state]", "READY_FOR_REVIEW"),
            ]
        );
        assert_eq!(
            list_query("6766108771", None, ""),
            vec![("filter[app]", "6766108771"), ("limit", "200")]
        );
    }

    #[test]
    fn create_body_names_the_platform_and_app() {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes: CreateSubmissionAttributes {
                    platform: Platform::VisionOs,
                },
                relationships: Some(CreateSubmissionRelationships {
                    app: to_one(crate::app::RESOURCE_TYPE, "6766108771"),
                }),
            },
        };
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            json!({ "data": {
                "type": "reviewSubmissions",
                "attributes": { "platform": "VISION_OS" },
                "relationships": { "app": { "data": { "type": "apps", "id": "6766108771" } } }
            }})
        );
    }

    /// App Store Connect wants camelCase relationship names and no
    /// `attributes` key on an item.
    #[test]
    fn item_body_has_relationships_only() {
        assert_eq!(
            serde_json::to_value(item_body("SUB1", "VER1")).unwrap(),
            json!({ "data": {
                "type": "reviewSubmissionItems",
                "relationships": {
                    "reviewSubmission": { "data": { "type": "reviewSubmissions", "id": "SUB1" } },
                    "appStoreVersion": { "data": { "type": "appStoreVersions", "id": "VER1" } }
                }
            }})
        );
    }

    #[test]
    fn submit_and_cancel_patch_one_flag() {
        let submit = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: "SUB1".to_string(),
                attributes: SubmittedAttributes { submitted: true },
            },
        };
        assert_eq!(
            serde_json::to_value(submit).unwrap(),
            json!({ "data": { "type": "reviewSubmissions", "id": "SUB1",
                              "attributes": { "submitted": true } } })
        );
        assert_eq!(
            serde_json::to_value(CanceledAttributes { canceled: true }).unwrap(),
            json!({ "canceled": true })
        );
    }

    #[test]
    fn deserializes_a_submission() {
        let doc: Document<ReviewSubmissionAttributes> = serde_json::from_value(json!({
            "data": { "id": "SUB1", "type": "reviewSubmissions",
                      "attributes": { "platform": "VISION_OS", "state": "READY_FOR_REVIEW",
                                      "submittedDate": null } }
        }))
        .unwrap();
        assert_eq!(doc.data.attributes.platform, Some(Platform::VisionOs));
        assert_eq!(
            doc.data.attributes.state.as_deref(),
            Some(STATE_READY_FOR_REVIEW)
        );
    }
}
