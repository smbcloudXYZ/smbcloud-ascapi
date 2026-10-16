use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, Resource, ResourceId, ToOne, UpdateBody, UpdateData,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "appStoreVersionLocalizations";

/// Maximum `description` length App Store Connect accepts, in characters.
pub const DESCRIPTION_MAX_LEN: usize = 4000;
/// Maximum `keywords` length, in characters, commas included.
pub const KEYWORDS_MAX_LEN: usize = 100;
/// Maximum `promotionalText` length, in characters.
pub const PROMOTIONAL_TEXT_MAX_LEN: usize = 170;
/// Maximum `whatsNew` length, in characters. Beta "What to Test" notes
/// share the same limit.
pub const WHATS_NEW_MAX_LEN: usize = 4000;

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

impl AppStoreVersionLocalizationFields {
    /// Check the fields against App Store Connect's length limits before
    /// anything is sent; Apple answers an over-long value with an opaque
    /// 400. Lengths are counted in characters, as Apple counts them.
    ///
    /// Returns `Err` naming the first field over its limit (with its length
    /// and the limit), or `Ok` with advisory warnings: a space after a
    /// keyword comma and empty keyword entries both waste the 100-character
    /// budget without changing what the store indexes.
    pub fn validate(&self) -> std::result::Result<Vec<String>, String> {
        let limits = [
            ("description", &self.description, DESCRIPTION_MAX_LEN),
            ("keywords", &self.keywords, KEYWORDS_MAX_LEN),
            (
                "promotionalText",
                &self.promotional_text,
                PROMOTIONAL_TEXT_MAX_LEN,
            ),
            ("whatsNew", &self.whats_new, WHATS_NEW_MAX_LEN),
        ];
        for (name, value, max) in limits {
            let len = value.as_deref().map_or(0, |v| v.chars().count());
            if len > max {
                return Err(format!(
                    "{name} is {len} characters; App Store Connect accepts at most {max}"
                ));
            }
        }
        let mut warnings = Vec::new();
        if let Some(keywords) = &self.keywords {
            if keywords.contains(", ") {
                warnings.push(
                    "keywords: a space after a comma wastes a character; use `a,b` not `a, b`"
                        .to_string(),
                );
            }
            if keywords.split(',').any(|k| k.trim().is_empty()) && !keywords.is_empty() {
                warnings
                    .push("keywords: contains an empty entry (`,,` or a stray comma)".to_string());
            }
        }
        Ok(warnings)
    }
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

    /// `GET /v1/appStoreVersionLocalizations/{id}` — read one localization
    /// back, e.g. to verify a write stored what was sent.
    async fn get_app_store_version_localization(
        &self,
        id: &str,
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
        self.list_all::<AppStoreVersionLocalizationAttributes>(&path, &[])
            .await
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

    async fn get_app_store_version_localization(
        &self,
        id: &str,
    ) -> Result<AppStoreVersionLocalization> {
        let path = format!("/v1/appStoreVersionLocalizations/{id}");
        let doc: Document<AppStoreVersionLocalizationAttributes> =
            self.request(Method::GET, &path, &[], None::<&()>).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn only(
        f: impl FnOnce(&mut AppStoreVersionLocalizationFields),
    ) -> AppStoreVersionLocalizationFields {
        let mut fields = AppStoreVersionLocalizationFields::default();
        f(&mut fields);
        fields
    }

    #[test]
    fn each_field_over_its_limit_is_rejected_by_name() {
        let cases = [
            (
                "description",
                only(|f| f.description = Some("a".repeat(DESCRIPTION_MAX_LEN + 1))),
            ),
            (
                "keywords",
                only(|f| f.keywords = Some("a".repeat(KEYWORDS_MAX_LEN + 1))),
            ),
            (
                "promotionalText",
                only(|f| f.promotional_text = Some("a".repeat(PROMOTIONAL_TEXT_MAX_LEN + 1))),
            ),
            (
                "whatsNew",
                only(|f| f.whats_new = Some("a".repeat(WHATS_NEW_MAX_LEN + 1))),
            ),
        ];
        for (name, fields) in cases {
            let message = fields.validate().expect_err("over-limit must be rejected");
            assert!(message.starts_with(name), "{message}");
            assert!(message.contains("at most"), "{message}");
        }
    }

    #[test]
    fn fields_at_their_limit_pass() {
        let fields = AppStoreVersionLocalizationFields {
            description: Some("a".repeat(DESCRIPTION_MAX_LEN)),
            keywords: Some("a".repeat(KEYWORDS_MAX_LEN)),
            promotional_text: Some("a".repeat(PROMOTIONAL_TEXT_MAX_LEN)),
            whats_new: Some("a".repeat(WHATS_NEW_MAX_LEN)),
            ..Default::default()
        };
        assert_eq!(fields.validate(), Ok(vec![]));
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        // `•` is 3 bytes in UTF-8 but one character.
        let fields = only(|f| f.promotional_text = Some("•".repeat(PROMOTIONAL_TEXT_MAX_LEN)));
        assert!(fields.validate().is_ok());
        let over = only(|f| f.promotional_text = Some("•".repeat(PROMOTIONAL_TEXT_MAX_LEN + 1)));
        assert!(over.validate().is_err());
    }

    #[test]
    fn keyword_spacing_and_empty_entries_warn() {
        let spaced = only(|f| f.keywords = Some("a, b".into()));
        assert_eq!(spaced.validate().unwrap().len(), 1);
        let empty = only(|f| f.keywords = Some("a,,b".into()));
        let warnings = empty.validate().unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("empty"));
        let clean = only(|f| f.keywords = Some("a,b".into()));
        assert!(clean.validate().unwrap().is_empty());
    }
}
