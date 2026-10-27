//! Publishing API calls for listing text.
//!
//! Endpoints (all under `https://connect-api.cloud.huawei.com/api`):
//! `GET /publish/v2/appid-list`, `GET /publish/v2/app-info` and
//! `PUT /publish/v2/app-language-info`. Every reply carries
//! `ret: {code, msg}`; a non-zero code is [`Error::Result`].

use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::client::AppGalleryClient;
use crate::error::{Error, Result};

pub const LANG_MAX_LEN: usize = 64;
pub const APP_NAME_MAX_LEN: usize = 64;
pub const APP_DESC_MAX_LEN: usize = 8000;
/// The update reference allows 80; the query reference documents 60 for the
/// same field. Validate against the update limit and let the server decide.
pub const BRIEF_INFO_MAX_LEN: usize = 80;
/// The update reference allows 500 (the query reference documents 1000).
pub const NEW_FEATURES_MAX_LEN: usize = 500;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RetCode {
    pub code: i64,
    #[serde(default)]
    pub msg: String,
}

impl RetCode {
    fn check(self) -> Result<()> {
        if self.code == 0 {
            Ok(())
        } else {
            Err(Error::Result {
                code: self.code,
                msg: self.msg,
            })
        }
    }
}

/// App basic information: the fields listing work reads.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// 0 released, 1 rejected, 3 releasing, 4 reviewing, 7 draft, and so on.
    pub release_state: Option<i64>,
    pub default_lang: Option<String>,
    pub privacy_policy: Option<String>,
    pub version_number: Option<String>,
    pub version_code: Option<i64>,
    pub on_shelf_version_number: Option<String>,
    pub publish_country: Option<String>,
}

/// One language's listing text.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageInfo {
    pub lang: Option<String>,
    pub app_name: Option<String>,
    pub app_desc: Option<String>,
    pub brief_info: Option<String>,
    pub new_features: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoResponse {
    pub app_info: Option<AppInfo>,
    /// The reviewer's overall comment, when there is one.
    pub audit_info: Option<AuditInfo>,
    #[serde(default)]
    pub languages: Vec<LanguageInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditInfo {
    pub audit_opinion: Option<String>,
}

/// The body of `PUT app-language-info`. Unset fields are left as they are.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageFields {
    pub lang: String,
    /// Required when the language is new.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_desc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brief_info: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_features: Option<String>,
}

fn check_len(field: &'static str, value: &Option<String>, max: usize) -> Result<()> {
    match value {
        Some(v) if v.chars().count() > max => Err(Error::TooLong {
            field,
            len: v.chars().count(),
            max,
        }),
        _ => Ok(()),
    }
}

impl LanguageFields {
    /// Check the documented limits before anything is sent.
    pub fn validate(&self) -> Result<()> {
        if self.lang.is_empty() {
            return Err(Error::EmptyLang);
        }
        check_len("lang", &Some(self.lang.clone()), LANG_MAX_LEN)?;
        check_len("appName", &self.app_name, APP_NAME_MAX_LEN)?;
        check_len("appDesc", &self.app_desc, APP_DESC_MAX_LEN)?;
        check_len("briefInfo", &self.brief_info, BRIEF_INFO_MAX_LEN)?;
        check_len("newFeatures", &self.new_features, NEW_FEATURES_MAX_LEN)
    }
}

#[derive(Deserialize)]
struct RetOnly {
    ret: RetCode,
}

#[derive(Deserialize)]
struct AppInfoEnvelope {
    ret: RetCode,
    #[serde(flatten)]
    body: AppInfoResponse,
}

#[derive(Deserialize)]
struct AppIdEnvelope {
    ret: RetCode,
    #[serde(default)]
    appids: Vec<AppIdPair>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppIdPair {
    /// The app name.
    pub key: String,
    /// The app id.
    pub value: String,
}

fn parse_ret(bytes: &[u8]) -> Result<()> {
    serde_json::from_slice::<RetOnly>(bytes)?.ret.check()
}

fn parse_app_info(bytes: &[u8]) -> Result<AppInfoResponse> {
    let envelope: AppInfoEnvelope = serde_json::from_slice(bytes)?;
    envelope.ret.check()?;
    Ok(envelope.body)
}

fn parse_app_ids(bytes: &[u8]) -> Result<Vec<AppIdPair>> {
    let envelope: AppIdEnvelope = serde_json::from_slice(bytes)?;
    envelope.ret.check()?;
    Ok(envelope.appids)
}

impl AppGalleryClient {
    /// App ids for up to 50 package names (comma separated on the wire).
    pub async fn app_ids_for_packages(&self, packages: &[&str]) -> Result<Vec<AppIdPair>> {
        let path = format!("/publish/v2/appid-list?packageName={}", packages.join(","));
        let bytes = self.send(Method::GET, &path, None).await?.bytes().await?;
        parse_app_ids(&bytes)
    }

    /// App info in one language, or in every language when `lang` is `None`.
    pub async fn app_info(&self, app_id: &str, lang: Option<&str>) -> Result<AppInfoResponse> {
        let mut path = format!("/publish/v2/app-info?appId={app_id}");
        if let Some(lang) = lang {
            path.push_str(&format!("&lang={lang}"));
        }
        let bytes = self.send(Method::GET, &path, None).await?.bytes().await?;
        parse_app_info(&bytes)
    }

    /// Update one language's text, or add the language when it is new.
    /// `release_type` is 1 (entire network, the default) or 3 (by phase).
    pub async fn update_language_info(
        &self,
        app_id: &str,
        fields: &LanguageFields,
        release_type: Option<u8>,
    ) -> Result<()> {
        fields.validate()?;
        let mut path = format!("/publish/v2/app-language-info?appId={app_id}");
        if let Some(release_type) = release_type {
            path.push_str(&format!("&releaseType={release_type}"));
        }
        let body = serde_json::to_value(fields)?;
        let bytes = self
            .send(Method::PUT, &path, Some(&body))
            .await?
            .bytes()
            .await?;
        parse_ret(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_update_body_uses_the_documented_names_and_skips_unset_fields() {
        let fields = LanguageFields {
            lang: "en-US".into(),
            app_name: Some("siGit".into()),
            new_features: Some("First release".into()),
            ..Default::default()
        };
        let json = serde_json::to_value(&fields).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"lang":"en-US","appName":"siGit","newFeatures":"First release"})
        );
    }

    #[test]
    fn limits_are_counted_in_characters_and_named_in_the_error() {
        let ok = LanguageFields {
            lang: "en-US".into(),
            app_desc: Some("é".repeat(APP_DESC_MAX_LEN)),
            ..Default::default()
        };
        assert!(ok.validate().is_ok());

        let too_long = LanguageFields {
            lang: "en-US".into(),
            new_features: Some("x".repeat(NEW_FEATURES_MAX_LEN + 1)),
            ..Default::default()
        };
        let err = too_long.validate().unwrap_err().to_string();
        assert!(err.contains("newFeatures") && err.contains("500"), "{err}");

        let brief = LanguageFields {
            lang: "en-US".into(),
            brief_info: Some("x".repeat(BRIEF_INFO_MAX_LEN + 1)),
            ..Default::default()
        };
        assert!(brief.validate().is_err());
    }

    #[test]
    fn an_empty_lang_is_rejected() {
        assert!(matches!(
            LanguageFields::default().validate(),
            Err(Error::EmptyLang)
        ));
    }

    #[test]
    fn app_info_parses_the_documented_response() {
        let json = br#"{
            "ret": {"code": 0, "msg": "success"},
            "appInfo": {"releaseState": 7, "defaultLang": "en-US", "versionNumber": "1.2.0",
                        "versionCode": 2026102700, "privacyPolicy": "https://sigit.si/privacy"},
            "auditInfo": {"auditOpinion": ""},
            "languages": [{"lang": "en-US", "appName": "siGit", "briefInfo": "b",
                           "newFeatures": "n", "appDesc": "d", "icon": "https://x/i.png"}]
        }"#;
        let parsed = parse_app_info(json).unwrap();
        let info = parsed.app_info.unwrap();
        assert_eq!(info.release_state, Some(7));
        assert_eq!(info.version_code, Some(2026102700));
        assert_eq!(parsed.languages.len(), 1);
        assert_eq!(parsed.languages[0].app_name.as_deref(), Some("siGit"));
    }

    #[test]
    fn a_nonzero_ret_code_is_an_error_even_on_http_200() {
        let err = parse_ret(br#"{"ret":{"code":204144660,"msg":"no permission"}}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("204144660") && err.contains("no permission"));
        assert!(parse_app_info(br#"{"ret":{"code":1,"msg":"x"},"languages":[]}"#).is_err());
        assert!(parse_ret(br#"{"ret":{"code":0,"msg":"success"}}"#).is_ok());
    }

    #[test]
    fn app_ids_parse_into_name_and_id() {
        let json =
            br#"{"ret":{"code":0,"msg":"success"},"appids":[{"key":"siGit","value":"1099"}]}"#;
        let ids = parse_app_ids(json).unwrap();
        assert_eq!(ids[0].key, "siGit");
        assert_eq!(ids[0].value, "1099");
    }
}
