//! `inAppPurchaseAppStoreReviewScreenshots`: the one screenshot App Review
//! wants of the purchase in the app.
//!
//! Same three steps as any App Store Connect asset: reserve (`POST`, which
//! returns pre-signed upload operations), PUT the bytes to each operation,
//! then commit (`PATCH` with `uploaded: true` and the file's MD5).

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Deserializer, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, OptionalDocument, Resource, ResourceId, ToOne, UpdateBody,
    UpdateData,
};
use smbcloud_ascapi_core::{Client, Error, Result};

pub const RESOURCE_TYPE: &str = "inAppPurchaseAppStoreReviewScreenshots";

/// Apple sends `null` for these lists once an asset is committed.
fn null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadOperation {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub length: u64,
    #[serde(default)]
    pub offset: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    pub request_headers: Vec<RequestHeader>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewScreenshotAttributes {
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
    #[serde(default)]
    pub source_file_checksum: Option<String>,
    #[serde(default)]
    pub asset_delivery_state: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub upload_operations: Vec<UploadOperation>,
}

pub type ReviewScreenshot = Resource<ReviewScreenshotAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateAttributes {
    file_name: String,
    file_size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateRelationships {
    in_app_purchase_v2: ToOne,
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
                in_app_purchase_v2: ToOne {
                    data: ResourceId {
                        resource_type: crate::in_app_purchase::RESOURCE_TYPE,
                        id: in_app_purchase_id.to_string(),
                    },
                },
            }),
        },
    }
}

/// The slice of `bytes` an upload operation asks for. One that reaches past
/// the end of the file is an error: sending a short or empty chunk would
/// commit a corrupt asset under the full-file checksum.
fn chunk_for(op: &UploadOperation, bytes: &[u8]) -> Result<Vec<u8>> {
    let start = op.offset as usize;
    let end = start.saturating_add(op.length as usize);
    bytes.get(start..end).map(<[u8]>::to_vec).ok_or_else(|| {
        Error::UploadPlan(format!(
            "an upload operation covers bytes {start}..{end}, but the file has {}",
            bytes.len()
        ))
    })
}

/// PUT each slice of `bytes` that Apple's upload operations ask for.
pub(crate) async fn upload_operations(
    client: &Client,
    operations: &[UploadOperation],
    bytes: &[u8],
) -> Result<()> {
    for op in operations {
        let chunk = chunk_for(op, bytes)?;
        let headers: Vec<(String, String)> = op
            .request_headers
            .iter()
            .map(|h| (h.name.clone(), h.value.clone()))
            .collect();
        let method: Method = op.method.parse().unwrap_or(Method::PUT);
        client
            .upload_bytes(method, &op.url, &headers, chunk)
            .await?;
    }
    Ok(())
}

#[async_trait]
pub trait InAppPurchaseReviewScreenshotApi {
    /// `GET /v2/inAppPurchases/{id}/appStoreReviewScreenshot`.
    async fn get_review_screenshot(
        &self,
        in_app_purchase_id: &str,
    ) -> Result<Option<ReviewScreenshot>>;

    /// `DELETE /v1/inAppPurchaseAppStoreReviewScreenshots/{id}`.
    async fn delete_review_screenshot(&self, id: &str) -> Result<()>;

    /// Reserve, upload and commit `bytes` as the product's review
    /// screenshot. A product holds one; delete the old one first.
    async fn upload_review_screenshot(
        &self,
        in_app_purchase_id: &str,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<ReviewScreenshot>;
}

#[async_trait]
impl InAppPurchaseReviewScreenshotApi for Client {
    async fn get_review_screenshot(
        &self,
        in_app_purchase_id: &str,
    ) -> Result<Option<ReviewScreenshot>> {
        let path = format!("/v2/inAppPurchases/{in_app_purchase_id}/appStoreReviewScreenshot");
        let doc: OptionalDocument<ReviewScreenshotAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn delete_review_screenshot(&self, id: &str) -> Result<()> {
        let path = format!("/v1/inAppPurchaseAppStoreReviewScreenshots/{id}");
        self.request_no_content::<()>(Method::DELETE, &path, &[], None)
            .await
    }

    async fn upload_review_screenshot(
        &self,
        in_app_purchase_id: &str,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<ReviewScreenshot> {
        let checksum = format!("{:x}", md5::compute(&bytes));
        let body = create_body(in_app_purchase_id, &file_name, bytes.len() as u64);
        let reserved: Document<ReviewScreenshotAttributes> = self
            .request(
                Method::POST,
                "/v1/inAppPurchaseAppStoreReviewScreenshots",
                &[],
                Some(&body),
            )
            .await?;
        let reserved = reserved.data;

        upload_operations(self, &reserved.attributes.upload_operations, &bytes).await?;

        let path = format!("/v1/inAppPurchaseAppStoreReviewScreenshots/{}", reserved.id);
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
        let doc: Document<ReviewScreenshotAttributes> = self
            .request(Method::PATCH, &path, &[], Some(&commit))
            .await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reserve_body_names_the_v2_relationship() {
        let v = serde_json::to_value(create_body("IAP1", "paywall.png", 1234)).unwrap();
        assert_eq!(v["data"]["type"], "inAppPurchaseAppStoreReviewScreenshots");
        assert_eq!(v["data"]["attributes"]["fileName"], "paywall.png");
        assert_eq!(v["data"]["attributes"]["fileSize"], 1234);
        assert_eq!(
            v["data"]["relationships"]["inAppPurchaseV2"]["data"]["id"],
            "IAP1"
        );
    }

    #[test]
    fn a_committed_screenshot_with_null_operations_deserializes() {
        let doc: Document<ReviewScreenshotAttributes> = serde_json::from_value(serde_json::json!({
            "data": { "id": "S1", "type": RESOURCE_TYPE,
              "attributes": { "fileName": "paywall.png", "fileSize": 10,
                "uploadOperations": null, "assetDeliveryState": { "state": "COMPLETE" } } }
        }))
        .unwrap();
        assert!(doc.data.attributes.upload_operations.is_empty());
    }

    fn op(offset: u64, length: u64) -> UploadOperation {
        UploadOperation {
            method: "PUT".into(),
            url: "https://example.invalid".into(),
            length,
            offset,
            request_headers: vec![],
        }
    }

    #[test]
    fn a_chunk_inside_the_file_is_cut_out() {
        assert_eq!(chunk_for(&op(2, 3), b"abcdefg").unwrap(), b"cde");
    }

    #[test]
    fn a_chunk_past_the_end_of_the_file_is_an_error() {
        assert!(matches!(
            chunk_for(&op(5, 10), b"abcdefg"),
            Err(Error::UploadPlan(_))
        ));
        assert!(chunk_for(&op(u64::MAX, 10), b"abc").is_err());
    }
}
