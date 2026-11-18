//! `inAppPurchaseLocalizations`: the display name and description of a
//! product in one locale, as the store's purchase sheet shows them.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, Resource, ResourceId, ToOne, UpdateBody, UpdateData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "inAppPurchaseLocalizations";

/// Apple's limits, in characters.
pub const NAME_MAX_LEN: usize = 30;
pub const DESCRIPTION_MAX_LEN: usize = 45;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InAppPurchaseLocalizationAttributes {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

pub type InAppPurchaseLocalization = Resource<InAppPurchaseLocalizationAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateAttributes {
    locale: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateRelationships {
    in_app_purchase_v2: ToOne,
}

/// What a localization update sends; only `Some` fields.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The `POST /v1/inAppPurchaseLocalizations` body.
pub fn create_body(
    in_app_purchase_id: &str,
    locale: &str,
    name: &str,
    description: Option<&str>,
) -> impl Serialize + Send + Sync {
    CreateBody {
        data: CreateData {
            resource_type: RESOURCE_TYPE,
            attributes: CreateAttributes {
                locale: locale.to_string(),
                name: name.to_string(),
                description: description.map(str::to_string),
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

/// The `PATCH /v1/inAppPurchaseLocalizations/{id}` body.
pub fn update_body(id: &str, update: LocalizationUpdate) -> impl Serialize + Send + Sync {
    UpdateBody {
        data: UpdateData {
            resource_type: RESOURCE_TYPE,
            id: id.to_string(),
            attributes: update,
        },
    }
}

#[async_trait]
pub trait InAppPurchaseLocalizationsApi {
    /// `GET /v2/inAppPurchases/{id}/inAppPurchaseLocalizations`.
    async fn list_in_app_purchase_localizations(
        &self,
        in_app_purchase_id: &str,
    ) -> Result<Vec<InAppPurchaseLocalization>>;

    /// `POST /v1/inAppPurchaseLocalizations`.
    async fn create_in_app_purchase_localization(
        &self,
        in_app_purchase_id: &str,
        locale: &str,
        name: &str,
        description: Option<&str>,
    ) -> Result<InAppPurchaseLocalization>;

    /// `PATCH /v1/inAppPurchaseLocalizations/{id}`.
    async fn update_in_app_purchase_localization(
        &self,
        id: &str,
        update: LocalizationUpdate,
    ) -> Result<InAppPurchaseLocalization>;
}

#[async_trait]
impl InAppPurchaseLocalizationsApi for Client {
    async fn list_in_app_purchase_localizations(
        &self,
        in_app_purchase_id: &str,
    ) -> Result<Vec<InAppPurchaseLocalization>> {
        let path = format!("/v2/inAppPurchases/{in_app_purchase_id}/inAppPurchaseLocalizations");
        self.list_all::<InAppPurchaseLocalizationAttributes>(&path, &[("limit", "200")])
            .await
    }

    async fn create_in_app_purchase_localization(
        &self,
        in_app_purchase_id: &str,
        locale: &str,
        name: &str,
        description: Option<&str>,
    ) -> Result<InAppPurchaseLocalization> {
        let body = create_body(in_app_purchase_id, locale, name, description);
        let doc: Document<InAppPurchaseLocalizationAttributes> = self
            .request(
                Method::POST,
                "/v1/inAppPurchaseLocalizations",
                &[],
                Some(&body),
            )
            .await?;
        Ok(doc.data)
    }

    async fn update_in_app_purchase_localization(
        &self,
        id: &str,
        update: LocalizationUpdate,
    ) -> Result<InAppPurchaseLocalization> {
        let path = format!("/v1/inAppPurchaseLocalizations/{id}");
        let body = update_body(id, update);
        let doc: Document<InAppPurchaseLocalizationAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_create_body_names_the_v2_relationship() {
        let v = serde_json::to_value(create_body(
            "IAP1",
            "en-US",
            "Onde",
            Some("Unlimited answers"),
        ))
        .unwrap();
        assert_eq!(v["data"]["type"], "inAppPurchaseLocalizations");
        assert_eq!(v["data"]["attributes"]["locale"], "en-US");
        assert_eq!(v["data"]["attributes"]["name"], "Onde");
        let rel = &v["data"]["relationships"]["inAppPurchaseV2"]["data"];
        assert_eq!(rel["type"], "inAppPurchases");
        assert_eq!(rel["id"], "IAP1");
    }

    #[test]
    fn a_missing_description_is_left_out() {
        let v = serde_json::to_value(create_body("IAP1", "en-US", "Onde", None)).unwrap();
        assert!(v["data"]["attributes"].get("description").is_none());
    }
}
