use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, ListDocument, Resource, ResourceId, ToOne, UpdateBody,
    UpdateData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "appStoreVersionLocalizations";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreVersionLocalizationAttributes {
    pub locale: String,
    pub description: Option<String>,
    pub keywords: Option<String>,
    pub marketing_url: Option<String>,
    pub promotional_text: Option<String>,
    pub support_url: Option<String>,
    pub whats_new: Option<String>,
}

pub type AppStoreVersionLocalization = Resource<AppStoreVersionLocalizationAttributes>;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreVersionLocalizationFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marketing_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotional_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whats_new: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreVersionLocalizationCreateAttributes {
    pub locale: String,
    #[serde(flatten)]
    pub fields: AppStoreVersionLocalizationFields,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoreVersionLocalizationRelationships {
    pub app_store_version: ToOne,
}

/// Per-locale description, keywords, and release notes.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait AppStoreVersionLocalizationsApi {
    async fn list_app_store_version_localizations(
        &self,
        app_store_version_id: &str,
    ) -> Result<Vec<AppStoreVersionLocalization>>;

    /// `POST /v1/appStoreVersionLocalizations` — adds a locale's
    /// description/keywords/etc to an `AppStoreVersion`.
    async fn create_app_store_version_localization(
        &self,
        app_store_version_id: &str,
        attributes: AppStoreVersionLocalizationCreateAttributes,
    ) -> Result<AppStoreVersionLocalization>;

    async fn update_app_store_version_localization(
        &self,
        id: &str,
        fields: AppStoreVersionLocalizationFields,
    ) -> Result<AppStoreVersionLocalization>;

    async fn delete_app_store_version_localization(&self, id: &str) -> Result<()>;
}

#[async_trait]
impl AppStoreVersionLocalizationsApi for Client {
    async fn list_app_store_version_localizations(
        &self,
        app_store_version_id: &str,
    ) -> Result<Vec<AppStoreVersionLocalization>> {
        let path =
            format!("/v1/appStoreVersions/{app_store_version_id}/appStoreVersionLocalizations");
        let doc: ListDocument<AppStoreVersionLocalizationAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    async fn create_app_store_version_localization(
        &self,
        app_store_version_id: &str,
        attributes: AppStoreVersionLocalizationCreateAttributes,
    ) -> Result<AppStoreVersionLocalization> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes,
                relationships: Some(AppStoreVersionLocalizationRelationships {
                    app_store_version: ToOne {
                        data: ResourceId {
                            resource_type: crate::app_store_version::RESOURCE_TYPE,
                            id: app_store_version_id.to_string(),
                        },
                    },
                }),
            },
        };
        let doc: Document<AppStoreVersionLocalizationAttributes> = self
            .request(
                Method::POST,
                "/v1/appStoreVersionLocalizations",
                &[],
                Some(&body),
            )
            .await?;
        Ok(doc.data)
    }

    async fn update_app_store_version_localization(
        &self,
        id: &str,
        fields: AppStoreVersionLocalizationFields,
    ) -> Result<AppStoreVersionLocalization> {
        let path = format!("/v1/appStoreVersionLocalizations/{id}");
        let body = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: id.to_string(),
                attributes: fields,
            },
        };
        let doc: Document<AppStoreVersionLocalizationAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }

    async fn delete_app_store_version_localization(&self, id: &str) -> Result<()> {
        let path = format!("/v1/appStoreVersionLocalizations/{id}");
        self.request_no_content::<()>(Method::DELETE, &path, &[], None)
            .await
    }
}
