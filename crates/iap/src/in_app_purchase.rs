//! `inAppPurchases` (v2): the product record.
//!
//! Lists come from `GET /v1/apps/{id}/inAppPurchasesV2`; create, read and
//! update go through `/v2/inAppPurchases`. The v1 resource is deprecated and
//! not used here.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, Resource, ResourceId, ToOne, UpdateBody, UpdateData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "inAppPurchases";

/// Apple's limit on the reference name (seen only in App Store Connect).
pub const NAME_MAX_LEN: usize = 64;
/// Apple's limit on the note for the reviewer.
pub const REVIEW_NOTE_MAX_LEN: usize = 4000;

/// `inAppPurchaseType`. Subscriptions that renew are a different resource
/// (`subscriptions`) and are not listed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InAppPurchaseType {
    Consumable,
    NonConsumable,
    NonRenewingSubscription,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InAppPurchaseAttributes {
    /// Reference name, shown only in App Store Connect.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub product_id: Option<String>,
    #[serde(default)]
    pub in_app_purchase_type: Option<InAppPurchaseType>,
    /// e.g. `MISSING_METADATA`, `READY_TO_SUBMIT`, `WAITING_FOR_REVIEW`,
    /// `APPROVED`. Left as a string: Apple adds states.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub review_note: Option<String>,
    #[serde(default)]
    pub family_sharable: Option<bool>,
    #[serde(default)]
    pub content_hosting: Option<bool>,
}

pub type InAppPurchase = Resource<InAppPurchaseAttributes>;

/// The fields of a new product.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InAppPurchaseCreateAttributes {
    pub name: String,
    pub product_id: String,
    pub in_app_purchase_type: InAppPurchaseType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_sharable: Option<bool>,
}

/// A partial update: only `Some` fields are sent. The product id and type
/// cannot change after creation.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InAppPurchaseUpdateAttributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_sharable: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct CreateRelationships {
    app: ToOne,
}

/// Build the `POST /v2/inAppPurchases` body. Public so the CLI can print it
/// for `--dry-run` without sending anything.
pub fn create_body(
    app_id: &str,
    attributes: InAppPurchaseCreateAttributes,
) -> impl Serialize + Send + Sync {
    CreateBody {
        data: CreateData {
            resource_type: RESOURCE_TYPE,
            attributes,
            relationships: Some(CreateRelationships {
                app: ToOne {
                    data: ResourceId {
                        resource_type: "apps",
                        id: app_id.to_string(),
                    },
                },
            }),
        },
    }
}

/// Build the `PATCH /v2/inAppPurchases/{id}` body.
pub fn update_body(
    id: &str,
    attributes: InAppPurchaseUpdateAttributes,
) -> impl Serialize + Send + Sync {
    UpdateBody {
        data: UpdateData {
            resource_type: RESOURCE_TYPE,
            id: id.to_string(),
            attributes,
        },
    }
}

/// The product record of an in-app purchase.
#[async_trait]
pub trait InAppPurchasesApi {
    /// `GET /v1/apps/{app_id}/inAppPurchasesV2`: every product of the app.
    async fn list_in_app_purchases(&self, app_id: &str) -> Result<Vec<InAppPurchase>>;

    /// `GET /v2/inAppPurchases/{id}`.
    async fn get_in_app_purchase(&self, id: &str) -> Result<InAppPurchase>;

    /// `POST /v2/inAppPurchases`. Apple refuses a product id already used
    /// by this developer account, even on another app or after deletion.
    async fn create_in_app_purchase(
        &self,
        app_id: &str,
        attributes: InAppPurchaseCreateAttributes,
    ) -> Result<InAppPurchase>;

    /// `PATCH /v2/inAppPurchases/{id}`.
    async fn update_in_app_purchase(
        &self,
        id: &str,
        attributes: InAppPurchaseUpdateAttributes,
    ) -> Result<InAppPurchase>;
}

#[async_trait]
impl InAppPurchasesApi for Client {
    async fn list_in_app_purchases(&self, app_id: &str) -> Result<Vec<InAppPurchase>> {
        let path = format!("/v1/apps/{app_id}/inAppPurchasesV2");
        self.list_all::<InAppPurchaseAttributes>(&path, &[("limit", "200")])
            .await
    }

    async fn get_in_app_purchase(&self, id: &str) -> Result<InAppPurchase> {
        let path = format!("/v2/inAppPurchases/{id}");
        let doc: Document<InAppPurchaseAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn create_in_app_purchase(
        &self,
        app_id: &str,
        attributes: InAppPurchaseCreateAttributes,
    ) -> Result<InAppPurchase> {
        let body = create_body(app_id, attributes);
        let doc: Document<InAppPurchaseAttributes> = self
            .request(Method::POST, "/v2/inAppPurchases", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    async fn update_in_app_purchase(
        &self,
        id: &str,
        attributes: InAppPurchaseUpdateAttributes,
    ) -> Result<InAppPurchase> {
        let path = format!("/v2/inAppPurchases/{id}");
        let body = update_body(id, attributes);
        let doc: Document<InAppPurchaseAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smbcloud_ascapi_core::jsonapi::ListDocument;

    #[test]
    fn the_create_body_points_at_the_app() {
        let body = create_body(
            "6828625369",
            InAppPurchaseCreateAttributes {
                name: "Onde".into(),
                product_id: "onde.unlock".into(),
                in_app_purchase_type: InAppPurchaseType::NonConsumable,
                review_note: None,
                family_sharable: Some(false),
            },
        );
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["data"]["type"], "inAppPurchases");
        assert_eq!(v["data"]["attributes"]["productId"], "onde.unlock");
        assert_eq!(
            v["data"]["attributes"]["inAppPurchaseType"],
            "NON_CONSUMABLE"
        );
        assert_eq!(v["data"]["attributes"]["familySharable"], false);
        assert!(v["data"]["attributes"].get("reviewNote").is_none());
        assert_eq!(v["data"]["relationships"]["app"]["data"]["type"], "apps");
        assert_eq!(
            v["data"]["relationships"]["app"]["data"]["id"],
            "6828625369"
        );
    }

    #[test]
    fn an_update_sends_only_what_changed() {
        let body = update_body(
            "IAP1",
            InAppPurchaseUpdateAttributes {
                review_note: Some("Paywall after 15 answers.".into()),
                ..Default::default()
            },
        );
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["data"]["id"], "IAP1");
        let attrs = v["data"]["attributes"].as_object().unwrap();
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs["reviewNote"], "Paywall after 15 answers.");
    }

    #[test]
    fn a_list_deserializes() {
        let body = serde_json::json!({"data": [{
            "id": "6700000001", "type": "inAppPurchases",
            "attributes": {
                "name": "Onde", "productId": "onde.unlock",
                "inAppPurchaseType": "NON_CONSUMABLE", "state": "MISSING_METADATA",
                "reviewNote": null, "familySharable": false, "contentHosting": false
            }
        }]});
        let doc: ListDocument<InAppPurchaseAttributes> = serde_json::from_value(body).unwrap();
        let a = &doc.data[0].attributes;
        assert_eq!(a.product_id.as_deref(), Some("onde.unlock"));
        assert_eq!(
            a.in_app_purchase_type,
            Some(InAppPurchaseType::NonConsumable)
        );
        assert_eq!(a.state.as_deref(), Some("MISSING_METADATA"));
    }
}
