//! `inAppPurchaseImages`: the 1024x1024 promotional image the App Store
//! shows for a product (product page, offer codes, payment sheet).
//!
//! The same reserve, PUT, commit sequence as the review screenshot, and the
//! same upload operations. Unlike the screenshot a product can hold several
//! images, so listing returns a `Vec`.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, Resource, ResourceId, ToOne, UpdateBody, UpdateData,
};
use smbcloud_ascapi_core::{Client, Result};

use crate::review_screenshot::{null_as_default, upload_operations, UploadOperation};

pub const RESOURCE_TYPE: &str = "inAppPurchaseImages";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionalImageAttributes {
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub source_file_checksum: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub upload_operations: Vec<UploadOperation>,
}

pub type PromotionalImage = Resource<PromotionalImageAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateAttributes {
    file_name: String,
    file_size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateRelationships {
    in_app_purchase: ToOne,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommitAttributes {
    uploaded: bool,
    source_file_checksum: String,
}

/// The reserve (`POST`) body.
pub fn create_body(
    in_app_purchase_id: &str,
    file_name: &str,
    file_size: u64,
) -> impl Serialize + Send + Sync {
    CreateBody {
        data: CreateData {
            resource_type: RESOURCE_TYPE,
            attributes: CreateAttributes {
                file_name: file_name.to_string(),
                file_size,
            },
            relationships: Some(CreateRelationships {
                in_app_purchase: ToOne {
                    data: ResourceId {
                        resource_type: crate::in_app_purchase::RESOURCE_TYPE,
                        id: in_app_purchase_id.to_string(),
                    },
                },
            }),
        },
    }
}

#[async_trait]
pub trait InAppPurchasePromotionalImageApi {
    /// `GET /v2/inAppPurchases/{id}/images`.
    async fn list_promotional_images(
        &self,
        in_app_purchase_id: &str,
    ) -> Result<Vec<PromotionalImage>>;

    /// `DELETE /v1/inAppPurchaseImages/{id}`.
    async fn delete_promotional_image(&self, id: &str) -> Result<()>;

    /// Reserve, upload and commit `bytes` as a promotional image.
    async fn upload_promotional_image(
        &self,
        in_app_purchase_id: &str,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<PromotionalImage>;
}

#[async_trait]
impl InAppPurchasePromotionalImageApi for Client {
    async fn list_promotional_images(
        &self,
        in_app_purchase_id: &str,
    ) -> Result<Vec<PromotionalImage>> {
        let path = format!("/v2/inAppPurchases/{in_app_purchase_id}/images");
        self.list_all::<PromotionalImageAttributes>(&path, &[("limit", "200")])
            .await
    }

    async fn delete_promotional_image(&self, id: &str) -> Result<()> {
        let path = format!("/v1/inAppPurchaseImages/{id}");
        self.request_no_content::<()>(Method::DELETE, &path, &[], None)
            .await
    }

    async fn upload_promotional_image(
        &self,
        in_app_purchase_id: &str,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<PromotionalImage> {
        let checksum = format!("{:x}", md5::compute(&bytes));
        let body = create_body(in_app_purchase_id, &file_name, bytes.len() as u64);
        let reserved: Document<PromotionalImageAttributes> = self
            .request(Method::POST, "/v1/inAppPurchaseImages", &[], Some(&body))
            .await?;
        let reserved = reserved.data;

        upload_operations(self, &reserved.attributes.upload_operations, &bytes).await?;

        let path = format!("/v1/inAppPurchaseImages/{}", reserved.id);
        let commit = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: reserved.id.clone(),
                attributes: CommitAttributes {
                    uploaded: true,
                    source_file_checksum: checksum,
                },
            },
        };
        let doc: Document<PromotionalImageAttributes> = self
            .request(Method::PATCH, &path, &[], Some(&commit))
            .await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reserve_body_names_the_product_relationship() {
        let v = serde_json::to_value(create_body("IAP1", "unlock.png", 99)).unwrap();
        assert_eq!(v["data"]["type"], "inAppPurchaseImages");
        assert_eq!(v["data"]["attributes"]["fileName"], "unlock.png");
        assert_eq!(v["data"]["attributes"]["fileSize"], 99);
        assert_eq!(
            v["data"]["relationships"]["inAppPurchase"]["data"]["id"],
            "IAP1"
        );
    }

    #[test]
    fn a_committed_image_with_null_operations_deserializes() {
        let doc: Document<PromotionalImageAttributes> = serde_json::from_value(serde_json::json!({
            "data": { "id": "I1", "type": RESOURCE_TYPE,
              "attributes": { "fileName": "unlock.png", "fileSize": 10,
                "uploadOperations": null, "state": "PREPARE_FOR_SUBMISSION" } }
        }))
        .unwrap();
        assert!(doc.data.attributes.upload_operations.is_empty());
    }
}
