//! Auto-renewable subscriptions: the `subscriptionGroups` an app holds, the
//! `subscriptions` in each group, and each subscription's 1024x1024
//! promotional image (`subscriptionImages`).
//!
//! Subscriptions are a different resource from the one-time products in
//! [`crate::in_app_purchase`], and are listed through their group rather
//! than the app. The image upload is the same reserve, PUT, commit sequence
//! as [`crate::promotional_image`], with a `subscription` relationship.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, Resource, ResourceId, ToOne, UpdateBody, UpdateData,
};
use smbcloud_ascapi_core::{Client, Result};

use crate::review_screenshot::{null_as_default, upload_operations, UploadOperation};

pub const GROUP_RESOURCE_TYPE: &str = "subscriptionGroups";
pub const RESOURCE_TYPE: &str = "subscriptions";
pub const IMAGE_RESOURCE_TYPE: &str = "subscriptionImages";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionGroupAttributes {
    #[serde(default)]
    pub reference_name: Option<String>,
}

pub type SubscriptionGroup = Resource<SubscriptionGroupAttributes>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionAttributes {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub product_id: Option<String>,
    /// e.g. `MISSING_METADATA`, `READY_TO_SUBMIT`, `WAITING_FOR_REVIEW`,
    /// `APPROVED`, `REJECTED`, `DEVELOPER_ACTION_NEEDED`.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub subscription_period: Option<String>,
    /// 1 is the highest level of service in the group.
    #[serde(default)]
    pub group_level: Option<u32>,
    #[serde(default)]
    pub family_sharable: Option<bool>,
    #[serde(default)]
    pub review_note: Option<String>,
}

pub type Subscription = Resource<SubscriptionAttributes>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionImageAttributes {
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

pub type SubscriptionImage = Resource<SubscriptionImageAttributes>;

/// One subscription together with the group it belongs to, which is what
/// `subscriptions list` prints.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupedSubscription {
    pub group_id: String,
    pub group_name: Option<String>,
    #[serde(flatten)]
    pub subscription: Subscription,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateImageAttributes {
    file_name: String,
    file_size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateImageRelationships {
    subscription: ToOne,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommitAttributes {
    uploaded: bool,
    source_file_checksum: String,
}

/// The image reserve (`POST /v1/subscriptionImages`) body.
pub fn create_image_body(
    subscription_id: &str,
    file_name: &str,
    file_size: u64,
) -> impl Serialize + Send + Sync {
    CreateBody {
        data: CreateData {
            resource_type: IMAGE_RESOURCE_TYPE,
            attributes: CreateImageAttributes {
                file_name: file_name.to_string(),
                file_size,
            },
            relationships: Some(CreateImageRelationships {
                subscription: ToOne {
                    data: ResourceId {
                        resource_type: RESOURCE_TYPE,
                        id: subscription_id.to_string(),
                    },
                },
            }),
        },
    }
}

#[async_trait]
pub trait SubscriptionsApi {
    /// `GET /v1/apps/{id}/subscriptionGroups`.
    async fn list_subscription_groups(&self, app_id: &str) -> Result<Vec<SubscriptionGroup>>;

    /// `GET /v1/subscriptionGroups/{id}/subscriptions`.
    async fn list_group_subscriptions(&self, group_id: &str) -> Result<Vec<Subscription>>;

    /// Every subscription of an app, across all its groups.
    async fn list_app_subscriptions(&self, app_id: &str) -> Result<Vec<GroupedSubscription>>;

    /// `GET /v1/subscriptions/{id}/images`.
    async fn list_subscription_images(
        &self,
        subscription_id: &str,
    ) -> Result<Vec<SubscriptionImage>>;

    /// `DELETE /v1/subscriptionImages/{id}`.
    async fn delete_subscription_image(&self, id: &str) -> Result<()>;

    /// Reserve, upload and commit `bytes` as a subscription's image.
    async fn upload_subscription_image(
        &self,
        subscription_id: &str,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<SubscriptionImage>;
}

#[async_trait]
impl SubscriptionsApi for Client {
    async fn list_subscription_groups(&self, app_id: &str) -> Result<Vec<SubscriptionGroup>> {
        let path = format!("/v1/apps/{app_id}/subscriptionGroups");
        self.list_all::<SubscriptionGroupAttributes>(&path, &[("limit", "200")])
            .await
    }

    async fn list_group_subscriptions(&self, group_id: &str) -> Result<Vec<Subscription>> {
        let path = format!("/v1/subscriptionGroups/{group_id}/subscriptions");
        self.list_all::<SubscriptionAttributes>(&path, &[("limit", "200")])
            .await
    }

    async fn list_app_subscriptions(&self, app_id: &str) -> Result<Vec<GroupedSubscription>> {
        let mut rows = Vec::new();
        for group in self.list_subscription_groups(app_id).await? {
            for subscription in self.list_group_subscriptions(&group.id).await? {
                rows.push(GroupedSubscription {
                    group_id: group.id.clone(),
                    group_name: group.attributes.reference_name.clone(),
                    subscription,
                });
            }
        }
        rows.sort_by_key(|row| row.subscription.attributes.group_level.unwrap_or(u32::MAX));
        Ok(rows)
    }

    async fn list_subscription_images(
        &self,
        subscription_id: &str,
    ) -> Result<Vec<SubscriptionImage>> {
        let path = format!("/v1/subscriptions/{subscription_id}/images");
        self.list_all::<SubscriptionImageAttributes>(&path, &[("limit", "200")])
            .await
    }

    async fn delete_subscription_image(&self, id: &str) -> Result<()> {
        let path = format!("/v1/subscriptionImages/{id}");
        self.request_no_content::<()>(Method::DELETE, &path, &[], None)
            .await
    }

    async fn upload_subscription_image(
        &self,
        subscription_id: &str,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<SubscriptionImage> {
        let checksum = format!("{:x}", md5::compute(&bytes));
        let body = create_image_body(subscription_id, &file_name, bytes.len() as u64);
        let reserved: Document<SubscriptionImageAttributes> = self
            .request(Method::POST, "/v1/subscriptionImages", &[], Some(&body))
            .await?;
        let reserved = reserved.data;

        upload_operations(self, &reserved.attributes.upload_operations, &bytes).await?;

        let path = format!("/v1/subscriptionImages/{}", reserved.id);
        let commit = UpdateBody {
            data: UpdateData {
                resource_type: IMAGE_RESOURCE_TYPE,
                id: reserved.id.clone(),
                attributes: CommitAttributes {
                    uploaded: true,
                    source_file_checksum: checksum,
                },
            },
        };
        let doc: Document<SubscriptionImageAttributes> = self
            .request(Method::PATCH, &path, &[], Some(&commit))
            .await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_image_reserve_body_names_the_subscription_relationship() {
        let v = serde_json::to_value(create_image_body("SUB1", "pro.png", 42)).unwrap();
        assert_eq!(v["data"]["type"], "subscriptionImages");
        assert_eq!(v["data"]["attributes"]["fileName"], "pro.png");
        assert_eq!(v["data"]["attributes"]["fileSize"], 42);
        assert_eq!(
            v["data"]["relationships"]["subscription"]["data"]["type"],
            "subscriptions"
        );
        assert_eq!(
            v["data"]["relationships"]["subscription"]["data"]["id"],
            "SUB1"
        );
    }

    #[test]
    fn a_subscription_row_deserializes_and_flattens_with_its_group() {
        let sub: Subscription = serde_json::from_value(serde_json::json!({
            "id": "S1", "type": "subscriptions",
            "attributes": { "name": "Business Monthly", "productId": "business_business_monthly",
              "state": "APPROVED", "subscriptionPeriod": "ONE_MONTH", "groupLevel": 2 }
        }))
        .unwrap();
        let row = GroupedSubscription {
            group_id: "G1".into(),
            group_name: Some("smbCloud Mail Pro".into()),
            subscription: sub,
        };
        let v = serde_json::to_value(&row).unwrap();
        assert_eq!(v["groupName"], "smbCloud Mail Pro");
        assert_eq!(v["id"], "S1");
        assert_eq!(v["attributes"]["productId"], "business_business_monthly");
        assert_eq!(v["attributes"]["groupLevel"], 2);
    }

    #[test]
    fn a_committed_subscription_image_with_null_operations_deserializes() {
        let doc: Document<SubscriptionImageAttributes> =
            serde_json::from_value(serde_json::json!({
                "data": { "id": "I1", "type": IMAGE_RESOURCE_TYPE,
                  "attributes": { "fileName": "pro.png", "fileSize": 10,
                    "uploadOperations": null, "state": "UPLOAD_COMPLETE" } }
            }))
            .unwrap();
        assert!(doc.data.attributes.upload_operations.is_empty());
    }
}
