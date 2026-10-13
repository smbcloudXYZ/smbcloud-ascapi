//! `BetaAppReviewDetail` — contact and demo info that Apple's Beta App Review
//! team uses when reviewing an app for external TestFlight distribution.
//!
//! Every app has exactly one `BetaAppReviewDetail`. It does not need to be
//! created — it exists by default. The caller `GET`s it to see the current
//! state, and `PATCH`es it to update contact information before submitting
//! a build for Beta App Review.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{Document, Resource, UpdateBody, UpdateData};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "betaAppReviewDetails";

/// Contact and demo-account information used by the Beta App Review team.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BetaAppReviewDetailAttributes {
    /// Contact email for Beta App Review questions.
    pub contact_email: Option<String>,
    /// Contact first name.
    pub contact_first_name: Option<String>,
    /// Contact last name.
    pub contact_last_name: Option<String>,
    /// Contact phone number (international format recommended).
    pub contact_phone: Option<String>,
    /// Username for a demo account, if the app requires login to review.
    pub demo_account_name: Option<String>,
    /// Password for the demo account.
    pub demo_account_password: Option<String>,
    /// Whether a demo account is required to review the app.
    pub demo_account_required: Option<bool>,
    /// Free-form notes for the reviewer (max 4000 chars).
    pub notes: Option<String>,
}

/// A beta app review detail resource.
pub type BetaAppReviewDetail = Resource<BetaAppReviewDetailAttributes>;

// ── update helper ─────────────────────────────────────────────────────────────

/// Partial update attributes — all fields are optional so a caller can
/// update only what has changed.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BetaAppReviewDetailUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub demo_account_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub demo_account_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub demo_account_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

// ── trait ─────────────────────────────────────────────────────────────────────

/// Beta app review detail operations on App Store Connect.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait BetaAppReviewDetailApi {
    /// `GET /v1/apps/{app_id}/betaAppReviewDetail` — fetch the review detail
    /// for an app via the relationship sub-resource. This is the canonical
    /// way to get the detail without knowing its opaque id up front.
    async fn get_beta_app_review_detail(&self, app_id: &str) -> Result<BetaAppReviewDetail>;

    /// `PATCH /v1/betaAppReviewDetails/{id}` — update contact / demo-account
    /// fields. The id comes from a prior `get_beta_app_review_detail` call.
    ///
    /// Only fields set (non-`None`) in `update` are sent; other fields are
    /// left unchanged on Apple's side.
    async fn update_beta_app_review_detail(
        &self,
        id: &str,
        update: BetaAppReviewDetailUpdate,
    ) -> Result<BetaAppReviewDetail>;
}

#[async_trait]
impl BetaAppReviewDetailApi for Client {
    async fn get_beta_app_review_detail(&self, app_id: &str) -> Result<BetaAppReviewDetail> {
        let path = format!("/v1/apps/{app_id}/betaAppReviewDetail");
        let doc: Document<BetaAppReviewDetailAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn update_beta_app_review_detail(
        &self,
        id: &str,
        update: BetaAppReviewDetailUpdate,
    ) -> Result<BetaAppReviewDetail> {
        let path = format!("/v1/betaAppReviewDetails/{id}");
        let body = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: id.to_string(),
                attributes: update,
            },
        };
        let doc: Document<BetaAppReviewDetailAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_a_beta_app_review_detail() {
        let body = serde_json::json!({
            "data": {
                "id": "DETAIL1",
                "type": "betaAppReviewDetails",
                "attributes": {
                    "contactEmail": "dev@example.com",
                    "contactFirstName": "Jane",
                    "contactLastName": "Developer",
                    "contactPhone": "+1-555-0100",
                    "demoAccountName": "demo@example.com",
                    "demoAccountPassword": "hunter2",
                    "demoAccountRequired": true,
                    "notes": "Tap the red button to reach the main feature."
                }
            }
        });

        let doc: Document<BetaAppReviewDetailAttributes> =
            serde_json::from_value(body).expect("review detail must deserialize");
        let attrs = &doc.data.attributes;
        assert_eq!(doc.data.id, "DETAIL1");
        assert_eq!(attrs.contact_email.as_deref(), Some("dev@example.com"));
        assert_eq!(attrs.contact_first_name.as_deref(), Some("Jane"));
        assert_eq!(attrs.demo_account_required, Some(true));
        assert_eq!(
            attrs.notes.as_deref(),
            Some("Tap the red button to reach the main feature.")
        );
    }

    #[test]
    fn update_body_skips_none_fields() {
        let update = BetaAppReviewDetailUpdate {
            contact_email: Some("new@example.com".into()),
            notes: Some("Updated notes.".into()),
            ..Default::default()
        };
        let body = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: "DETAIL1".into(),
                attributes: update,
            },
        };
        let v = serde_json::to_value(&body).unwrap();
        let attrs = &v["data"]["attributes"];
        assert_eq!(attrs["contactEmail"], "new@example.com");
        assert_eq!(attrs["notes"], "Updated notes.");
        // Fields not set must be absent (not null), so the PATCH is truly partial.
        assert!(attrs.get("contactFirstName").is_none() || attrs["contactFirstName"].is_null());
        assert!(attrs.get("demoAccountRequired").is_none());
    }
}
