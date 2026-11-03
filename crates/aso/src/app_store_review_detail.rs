//! `AppStoreReviewDetail` — the "App Review Information" of an App Store
//! Version: contact details, a demo account and notes for the reviewer.
//!
//! Unlike the Beta App Review detail, this one belongs to a version and does
//! not exist until it is created. Reading a version without one returns
//! `None`; use [`AppStoreReviewDetailApi::create_app_store_review_detail`]
//! then.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, OptionalDocument, Resource, ResourceId, ToOne, UpdateBody,
    UpdateData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "appStoreReviewDetails";

/// Maximum `notes` length App Store Connect accepts, in characters.
pub const NOTES_MAX_LEN: usize = 4000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreReviewDetailAttributes {
    pub contact_first_name: Option<String>,
    pub contact_last_name: Option<String>,
    pub contact_phone: Option<String>,
    pub contact_email: Option<String>,
    pub demo_account_name: Option<String>,
    pub demo_account_password: Option<String>,
    pub demo_account_required: Option<bool>,
    pub notes: Option<String>,
}

pub type AppStoreReviewDetail = Resource<AppStoreReviewDetailAttributes>;

/// Fields to send. `None` fields are left out, so an update is partial.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreReviewDetailWrite {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub demo_account_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub demo_account_password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub demo_account_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Relationships {
    app_store_version: ToOne,
}

#[async_trait]
pub trait AppStoreReviewDetailApi {
    /// `GET /v1/appStoreVersions/{id}/appStoreReviewDetail`, `None` when the
    /// version has no review information yet.
    async fn get_app_store_review_detail(
        &self,
        app_store_version_id: &str,
    ) -> Result<Option<AppStoreReviewDetail>>;

    /// `POST /v1/appStoreReviewDetails`
    async fn create_app_store_review_detail(
        &self,
        app_store_version_id: &str,
        write: AppStoreReviewDetailWrite,
    ) -> Result<AppStoreReviewDetail>;

    /// `PATCH /v1/appStoreReviewDetails/{id}`
    async fn update_app_store_review_detail(
        &self,
        id: &str,
        write: AppStoreReviewDetailWrite,
    ) -> Result<AppStoreReviewDetail>;
}

#[async_trait]
impl AppStoreReviewDetailApi for Client {
    async fn get_app_store_review_detail(
        &self,
        app_store_version_id: &str,
    ) -> Result<Option<AppStoreReviewDetail>> {
        let path = format!("/v1/appStoreVersions/{app_store_version_id}/appStoreReviewDetail");
        let doc: OptionalDocument<AppStoreReviewDetailAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn create_app_store_review_detail(
        &self,
        app_store_version_id: &str,
        write: AppStoreReviewDetailWrite,
    ) -> Result<AppStoreReviewDetail> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes: write,
                relationships: Some(Relationships {
                    app_store_version: ToOne {
                        data: ResourceId {
                            resource_type: crate::app_store_version::RESOURCE_TYPE,
                            id: app_store_version_id.to_string(),
                        },
                    },
                }),
            },
        };
        let doc: Document<AppStoreReviewDetailAttributes> = self
            .request(Method::POST, "/v1/appStoreReviewDetails", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    async fn update_app_store_review_detail(
        &self,
        id: &str,
        write: AppStoreReviewDetailWrite,
    ) -> Result<AppStoreReviewDetail> {
        let path = format!("/v1/appStoreReviewDetails/{id}");
        let body = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: id.to_string(),
                attributes: write,
            },
        };
        let doc: Document<AppStoreReviewDetailAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_leaves_out_fields_that_are_not_set() {
        let write = AppStoreReviewDetailWrite {
            demo_account_name: Some("demo@example.com".into()),
            demo_account_required: Some(true),
            ..Default::default()
        };
        let v = serde_json::to_value(&write).unwrap();
        assert_eq!(v["demoAccountName"], "demo@example.com");
        assert_eq!(v["demoAccountRequired"], true);
        assert!(v.get("notes").is_none());
        assert!(v.get("demoAccountPassword").is_none());
    }

    #[test]
    fn a_version_without_review_information_reads_as_none() {
        let doc: OptionalDocument<AppStoreReviewDetailAttributes> =
            serde_json::from_value(serde_json::json!({ "data": null })).unwrap();
        assert!(doc.data.is_none());
    }
}
