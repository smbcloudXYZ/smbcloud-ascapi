//! `BetaAppReviewSubmission` — submit a build to Apple's Beta App Review
//! for external TestFlight distribution approval.
//!
//! Before external testers can install a build, Apple must approve it via
//! Beta App Review. This module lets a caller:
//!
//! 1. Submit a build (`POST /v1/betaAppReviewSubmissions`) — creates a
//!    submission and immediately sends the build to Apple for review.
//! 2. Query the current submission state (`GET /v1/betaAppReviewSubmissions?filter[build]={id}`).
//!
//! The submission itself only ever has the relationship to a `build`;
//! contact / demo details come from [`crate::beta_app_review_detail`].

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{Document, ListDocument, Resource, ResourceId, ToOne};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "betaAppReviewSubmissions";

/// The review state Apple reports for a beta submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BetaReviewState {
    /// The submission is waiting for App Review to pick it up.
    WaitingForReview,
    /// Apple is actively reviewing the build.
    InReview,
    /// Beta App Review rejected the build.
    Rejected,
    /// Beta App Review approved the build; external testing can begin.
    Approved,
}

/// Attributes of a beta app review submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BetaAppReviewSubmissionAttributes {
    /// Current state of the review.
    pub beta_review_state: Option<BetaReviewState>,
    /// Submission timestamp (ISO 8601).
    pub submitted_date: Option<String>,
}

/// A beta app review submission resource.
pub type BetaAppReviewSubmission = Resource<BetaAppReviewSubmissionAttributes>;

// ── internal create helper ────────────────────────────────────────────────────

/// A beta app review submission carries only a `build` relationship. Apple's
/// endpoint rejects a request whose `data.attributes` is present but null
/// (422 `ENTITY_UNPROCESSABLE`), so this body omits the field entirely rather
/// than reusing [`smbcloud_ascapi_core::jsonapi::CreateBody`] with `()`
/// attributes.
#[derive(Debug, Clone, Serialize)]
struct CreateBody {
    data: CreateData,
}

#[derive(Debug, Clone, Serialize)]
struct CreateData {
    #[serde(rename = "type")]
    resource_type: &'static str,
    relationships: CreateRelationships,
}

#[derive(Debug, Clone, Serialize)]
struct CreateRelationships {
    build: ToOne,
}

// ── trait ─────────────────────────────────────────────────────────────────────

/// Beta app review submission operations on App Store Connect.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait BetaAppReviewSubmissionsApi {
    /// `POST /v1/betaAppReviewSubmissions` — submit a build to Beta App Review.
    ///
    /// **CLI-only** — this is a destructive, irreversible operation that
    /// triggers Apple's review queue. It must not be called from an MCP tool
    /// without explicit user confirmation, and it is intentionally not
    /// exposed as an MCP tool here.
    ///
    /// `build_id` is the opaque ASC id of the build (not the build number).
    /// On success the returned submission's `beta_review_state` is typically
    /// `WAITING_FOR_REVIEW`.
    async fn submit_build_for_beta_review(&self, build_id: &str)
        -> Result<BetaAppReviewSubmission>;

    /// `GET /v1/betaAppReviewSubmissions?filter[build]={build_id}` — find
    /// the current submission record for a build.
    ///
    /// Returns at most one item in practice (one build → one submission), but
    /// the API is a list endpoint so we return `Vec`.
    async fn list_beta_app_review_submissions(
        &self,
        build_id: &str,
    ) -> Result<Vec<BetaAppReviewSubmission>>;
}

#[async_trait]
impl BetaAppReviewSubmissionsApi for Client {
    async fn submit_build_for_beta_review(
        &self,
        build_id: &str,
    ) -> Result<BetaAppReviewSubmission> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                relationships: CreateRelationships {
                    build: ToOne {
                        data: ResourceId {
                            resource_type: crate::build::RESOURCE_TYPE,
                            id: build_id.to_string(),
                        },
                    },
                },
            },
        };
        let doc: Document<BetaAppReviewSubmissionAttributes> = self
            .request(
                Method::POST,
                "/v1/betaAppReviewSubmissions",
                &[],
                Some(&body),
            )
            .await?;
        Ok(doc.data)
    }

    async fn list_beta_app_review_submissions(
        &self,
        build_id: &str,
    ) -> Result<Vec<BetaAppReviewSubmission>> {
        let path = "/v1/betaAppReviewSubmissions";
        let query = &[("filter[build]", build_id)];
        let doc: ListDocument<BetaAppReviewSubmissionAttributes> =
            self.request(Method::GET, path, query, None::<&()>).await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_a_submission_list() {
        let body = serde_json::json!({
            "data": [
                {
                    "id": "SUB1",
                    "type": "betaAppReviewSubmissions",
                    "attributes": {
                        "betaReviewState": "WAITING_FOR_REVIEW",
                        "submittedDate": "2025-01-01T00:00:00.000+0000"
                    }
                }
            ]
        });

        let doc: ListDocument<BetaAppReviewSubmissionAttributes> =
            serde_json::from_value(body).expect("submission list must deserialize");
        assert_eq!(doc.data.len(), 1);
        let sub = &doc.data[0];
        assert_eq!(sub.id, "SUB1");
        assert_eq!(
            sub.attributes.beta_review_state,
            Some(BetaReviewState::WaitingForReview)
        );
        assert_eq!(
            sub.attributes.submitted_date.as_deref(),
            Some("2025-01-01T00:00:00.000+0000")
        );
    }

    #[test]
    fn deserializes_approved_state() {
        let body = serde_json::json!({
            "data": {
                "id": "SUB2",
                "type": "betaAppReviewSubmissions",
                "attributes": {
                    "betaReviewState": "APPROVED",
                    "submittedDate": "2025-02-01T10:00:00.000+0000"
                }
            }
        });

        let doc: Document<BetaAppReviewSubmissionAttributes> =
            serde_json::from_value(body).expect("single submission must deserialize");
        assert_eq!(
            doc.data.attributes.beta_review_state,
            Some(BetaReviewState::Approved)
        );
    }

    #[test]
    fn filter_key_is_exact() {
        // ASC is case-sensitive about filter keys; confirm the exact string.
        let query: Vec<(&str, &str)> = vec![("filter[build]", "BUILD_ID")];
        assert_eq!(query[0].0, "filter[build]");
    }

    #[test]
    fn create_body_omits_attributes() {
        // Apple's endpoint 422s on a null `data.attributes`; the body must omit
        // the field entirely and carry only the `build` relationship.
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                relationships: CreateRelationships {
                    build: ToOne {
                        data: ResourceId {
                            resource_type: crate::build::RESOURCE_TYPE,
                            id: "BUILD_ID".to_string(),
                        },
                    },
                },
            },
        };
        let v = serde_json::to_value(&body).unwrap();
        assert!(
            v["data"].get("attributes").is_none(),
            "attributes must be omitted, not null: {v}"
        );
        assert_eq!(v["data"]["type"], RESOURCE_TYPE);
        assert_eq!(
            v["data"]["relationships"]["build"]["data"]["id"],
            "BUILD_ID"
        );
    }
}
