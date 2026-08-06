//! Tool parameter types.
//!
//! Every field carries a doc comment, because rustdoc comments become the
//! JSON Schema `description` a model reads when deciding how to call a
//! tool, and a parameter without one is a guess waiting to happen. The
//! contract test in [`crate::server`] fails the build if any is missing.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct AppListRequest {
    /// Only return the app with this bundle identifier, e.g.
    /// xyz.smbcloud.mailx. Omit to list every app on the account.
    #[serde(default)]
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AppIdRequest {
    /// App Store Connect app id, the numeric string from the app's URL in
    /// App Store Connect. Not the bundle identifier; use app_list to
    /// translate one into the other.
    pub app_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AppUpdateRequest {
    /// App Store Connect app id.
    pub app_id: String,
    /// BCP-47 locale to make the app's primary language, e.g. en-US.
    #[serde(default)]
    pub primary_locale: Option<String>,
    /// Content rights declaration, e.g. DOES_NOT_USE_THIRD_PARTY_CONTENT
    /// or USES_THIRD_PARTY_CONTENT.
    #[serde(default)]
    pub content_rights_declaration: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct BundleIdListRequest {
    /// Only return the bundle ID matching this identifier exactly. Omit to
    /// list every registered identifier.
    #[serde(default)]
    pub identifier: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct BundleIdCreateRequest {
    /// The reverse-DNS identifier to register, e.g. xyz.smbcloud.mailx.
    pub identifier: String,
    /// Human-readable name shown in the developer portal.
    pub name: String,
    /// Platform namespace: ios, mac_os, or universal. visionOS apps
    /// register as ios, because visionOS shares the iOS namespace.
    pub platform: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AppStoreVersionListRequest {
    /// App Store Connect app id whose versions to list.
    pub app_id: String,
    /// Only return versions for this platform: ios, mac_os, tv_os, or
    /// vision_os. Omit to list every platform's versions.
    #[serde(default)]
    pub platform: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct IdRequest {
    /// App Store Connect resource id.
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AppStoreVersionCreateRequest {
    /// App Store Connect app id to add the version to.
    pub app_id: String,
    /// Platform for this version: ios, mac_os, tv_os, or vision_os. Adding
    /// a platform to an existing app means creating a version for it here.
    pub platform: String,
    /// Marketing version string, e.g. 1.2.0.
    pub version_string: String,
    /// Copyright line, e.g. "2026 Splitfire AB". Optional.
    #[serde(default)]
    pub copyright: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct SetBuildRequest {
    /// App Store Version id to attach the build to.
    pub version_id: String,
    /// Build id to attach. Use build_list to find one for the app.
    pub build_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AppInfoLocalizationListRequest {
    /// AppInfo id whose localizations to list. Use app_info_list to find
    /// the AppInfo for an app.
    pub app_info_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AppInfoLocalizationCreateRequest {
    /// AppInfo id to add the localization to.
    pub app_info_id: String,
    /// BCP-47 locale, e.g. sv-SE or id-ID.
    pub locale: String,
    /// Localized app name shown on the product page.
    pub name: String,
    /// Localized subtitle, up to 30 characters.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Localized privacy policy URL.
    #[serde(default)]
    pub privacy_policy_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AppInfoLocalizationUpdateRequest {
    /// AppInfoLocalization id to update.
    pub id: String,
    /// New localized app name. Omit to leave unchanged.
    #[serde(default)]
    pub name: Option<String>,
    /// New localized subtitle. Omit to leave unchanged.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// New privacy policy URL. Omit to leave unchanged.
    #[serde(default)]
    pub privacy_policy_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct VersionLocalizationListRequest {
    /// App Store Version id whose localizations to list.
    pub version_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct VersionLocalizationCreateRequest {
    /// App Store Version id to add the localization to.
    pub version_id: String,
    /// BCP-47 locale, e.g. sv-SE or id-ID.
    pub locale: String,
    /// Long description shown on the product page, up to 4000 characters.
    #[serde(default)]
    pub description: Option<String>,
    /// Comma-separated keyword field, up to 100 characters. Apple indexes
    /// it for search and never shows it.
    #[serde(default)]
    pub keywords: Option<String>,
    /// Marketing URL.
    #[serde(default)]
    pub marketing_url: Option<String>,
    /// Promotional text, up to 170 characters. Editable without a new
    /// build, unlike the description.
    #[serde(default)]
    pub promotional_text: Option<String>,
    /// Support URL.
    #[serde(default)]
    pub support_url: Option<String>,
    /// Release notes for this version.
    #[serde(default)]
    pub whats_new: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct VersionLocalizationUpdateRequest {
    /// AppStoreVersionLocalization id to update.
    pub id: String,
    /// New description. Omit to leave unchanged.
    #[serde(default)]
    pub description: Option<String>,
    /// New keyword field. Omit to leave unchanged.
    #[serde(default)]
    pub keywords: Option<String>,
    /// New marketing URL. Omit to leave unchanged.
    #[serde(default)]
    pub marketing_url: Option<String>,
    /// New promotional text. Omit to leave unchanged.
    #[serde(default)]
    pub promotional_text: Option<String>,
    /// New support URL. Omit to leave unchanged.
    #[serde(default)]
    pub support_url: Option<String>,
    /// New release notes. Omit to leave unchanged.
    #[serde(default)]
    pub whats_new: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ScreenshotSetListRequest {
    /// AppStoreVersionLocalization id whose screenshot sets to list.
    pub localization_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ScreenshotSetCreateRequest {
    /// AppStoreVersionLocalization id to create the set under.
    pub localization_id: String,
    /// Device class for this set, e.g. APP_IPHONE_67, APP_IPAD_PRO_129,
    /// APP_APPLE_VISION_PRO, or APP_DESKTOP.
    pub display_type: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ScreenshotListRequest {
    /// AppScreenshotSet id whose screenshots to list.
    pub screenshot_set_id: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ScreenshotUploadRequest {
    /// AppScreenshotSet id to upload into.
    pub screenshot_set_id: String,
    /// Path to the image file on this machine. Read from disk by the
    /// server, so it must be a path this process can open.
    pub file_path: String,
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct CertificateListRequest {
    /// Only list certificates of this type. One of development,
    /// distribution, mac_app_distribution, mac_installer_distribution,
    /// developer_id_application. Omit to list every type.
    #[serde(default)]
    pub certificate_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CertificateCreateRequest {
    /// The kind of certificate to issue. One of development,
    /// distribution, mac_app_distribution, mac_installer_distribution,
    /// developer_id_application.
    pub certificate_type: String,
    /// Directory to write the private key and issued certificate into. It
    /// is created if missing; the key is written with owner-only
    /// permissions.
    pub out_dir: String,
    /// Subject common name for the signing request. Apple replaces the
    /// subject on the certificate it issues, so this only labels the
    /// request itself. Defaults to smbcloud-ascapi.
    #[serde(default)]
    pub common_name: Option<String>,
}
