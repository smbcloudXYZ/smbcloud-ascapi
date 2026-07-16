//! An `AppScreenshotSet` groups the screenshots for one device/display
//! class (e.g. Apple Vision Pro, iPhone 6.7") within one localization of an
//! `AppStoreVersion`. Screenshots themselves are a separate resource
//! ([`crate::app_screenshot`]) that belongs to a set.

use crate::client::Client;
use crate::error::Result;
use crate::jsonapi::{CreateBody, CreateData, Document, ListDocument, Resource, ResourceId, ToOne};
use reqwest::Method;
use serde::{Deserialize, Serialize};

pub const RESOURCE_TYPE: &str = "appScreenshotSets";

/// The device/display class a screenshot set targets. Apple documents the
/// full set at
/// <https://developer.apple.com/documentation/appstoreconnectapi/screenshotdisplaytype>;
/// this covers the App Store (non-iMessage) display classes in current use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenshotDisplayType {
    #[serde(rename = "APP_APPLE_VISION_PRO")]
    AppleVisionPro,
    #[serde(rename = "APP_IPHONE_67")]
    Iphone67,
    #[serde(rename = "APP_IPHONE_65")]
    Iphone65,
    #[serde(rename = "APP_IPHONE_61")]
    Iphone61,
    #[serde(rename = "APP_IPHONE_58")]
    Iphone58,
    #[serde(rename = "APP_IPHONE_55")]
    Iphone55,
    #[serde(rename = "APP_IPHONE_47")]
    Iphone47,
    #[serde(rename = "APP_IPHONE_40")]
    Iphone40,
    #[serde(rename = "APP_IPHONE_35")]
    Iphone35,
    #[serde(rename = "APP_IPAD_PRO_3GEN_129")]
    IpadPro3Gen129,
    #[serde(rename = "APP_IPAD_PRO_3GEN_11")]
    IpadPro3Gen11,
    #[serde(rename = "APP_IPAD_PRO_129")]
    IpadPro129,
    #[serde(rename = "APP_IPAD_105")]
    Ipad105,
    #[serde(rename = "APP_IPAD_97")]
    Ipad97,
    #[serde(rename = "APP_DESKTOP")]
    Desktop,
    #[serde(rename = "APP_APPLE_TV")]
    AppleTv,
    #[serde(rename = "APP_WATCH_ULTRA")]
    WatchUltra,
    #[serde(rename = "APP_WATCH_SERIES_10")]
    WatchSeries10,
    #[serde(rename = "APP_WATCH_SERIES_7")]
    WatchSeries7,
    #[serde(rename = "APP_WATCH_SERIES_4")]
    WatchSeries4,
    #[serde(rename = "APP_WATCH_SERIES_3")]
    WatchSeries3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScreenshotSetAttributes {
    pub screenshot_display_type: ScreenshotDisplayType,
}

pub type AppScreenshotSet = Resource<AppScreenshotSetAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScreenshotSetCreateAttributes {
    pub screenshot_display_type: ScreenshotDisplayType,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppScreenshotSetRelationships {
    pub app_store_version_localization: ToOne,
}

impl Client {
    /// `GET /v1/appStoreVersionLocalizations/{id}/appScreenshotSets`.
    pub async fn list_app_screenshot_sets(
        &self,
        app_store_version_localization_id: &str,
    ) -> Result<Vec<AppScreenshotSet>> {
        let path = format!(
            "/v1/appStoreVersionLocalizations/{app_store_version_localization_id}/appScreenshotSets"
        );
        let doc: ListDocument<AppScreenshotSetAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
        Ok(doc.data)
    }

    /// `POST /v1/appScreenshotSets` — adds a device/display class's
    /// screenshot bucket to a localization. Screenshots are then added to
    /// the returned set with [`Client::create_app_screenshot`] /
    /// [`Client::upload_app_screenshot`].
    pub async fn create_app_screenshot_set(
        &self,
        app_store_version_localization_id: &str,
        attributes: AppScreenshotSetCreateAttributes,
    ) -> Result<AppScreenshotSet> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes,
                relationships: Some(AppScreenshotSetRelationships {
                    app_store_version_localization: ToOne {
                        data: ResourceId {
                            resource_type: crate::app_store_version_localization::RESOURCE_TYPE,
                            id: app_store_version_localization_id.to_string(),
                        },
                    },
                }),
            },
        };
        let doc: Document<AppScreenshotSetAttributes> = self
            .request(Method::POST, "/v1/appScreenshotSets", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    /// `DELETE /v1/appScreenshotSets/{id}`.
    pub async fn delete_app_screenshot_set(&self, id: &str) -> Result<()> {
        let path = format!("/v1/appScreenshotSets/{id}");
        self.request_no_content::<()>(Method::DELETE, &path, &[], None)
            .await
    }
}
