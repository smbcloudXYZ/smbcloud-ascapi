use crate::client::Client;
use crate::error::Result;
use crate::jsonapi::{
    CreateBody, CreateData, Document, ListDocument, Resource, ResourceId, ToOne, UpdateBody,
    UpdateData,
};
use reqwest::Method;
use serde::{Deserialize, Serialize};

pub const RESOURCE_TYPE: &str = "appInfoLocalizations";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoLocalizationAttributes {
    pub locale: String,
    pub name: Option<String>,
    pub subtitle: Option<String>,
    pub privacy_policy_url: Option<String>,
    pub privacy_choices_url: Option<String>,
    pub privacy_policy_text: Option<String>,
}

pub type AppInfoLocalization = Resource<AppInfoLocalizationAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoLocalizationCreateAttributes {
    pub locale: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy_policy_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoLocalizationUpdateAttributes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy_policy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoLocalizationRelationships {
    pub app_info: ToOne,
}

impl Client {
    pub async fn list_app_info_localizations(
        &self,
        app_info_id: &str,
    ) -> Result<Vec<AppInfoLocalization>> {
        let path = format!("/v1/appInfos/{app_info_id}/appInfoLocalizations");
        let doc: ListDocument<AppInfoLocalizationAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    /// `POST /v1/appInfoLocalizations` — adds a locale's name/subtitle to an
    /// `AppInfo`.
    pub async fn create_app_info_localization(
        &self,
        app_info_id: &str,
        attributes: AppInfoLocalizationCreateAttributes,
    ) -> Result<AppInfoLocalization> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes,
                relationships: Some(AppInfoLocalizationRelationships {
                    app_info: ToOne {
                        data: ResourceId {
                            resource_type: crate::app_info::RESOURCE_TYPE,
                            id: app_info_id.to_string(),
                        },
                    },
                }),
            },
        };
        let doc: Document<AppInfoLocalizationAttributes> = self
            .request(Method::POST, "/v1/appInfoLocalizations", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    pub async fn update_app_info_localization(
        &self,
        id: &str,
        attributes: AppInfoLocalizationUpdateAttributes,
    ) -> Result<AppInfoLocalization> {
        let path = format!("/v1/appInfoLocalizations/{id}");
        let body = UpdateBody {
            data: UpdateData {
                resource_type: RESOURCE_TYPE,
                id: id.to_string(),
                attributes,
            },
        };
        let doc: Document<AppInfoLocalizationAttributes> =
            self.request(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(doc.data)
    }

    pub async fn delete_app_info_localization(&self, id: &str) -> Result<()> {
        let path = format!("/v1/appInfoLocalizations/{id}");
        self.request_no_content::<()>(Method::DELETE, &path, &[], None)
            .await
    }
}
