//! Provisioning profile operations shared by both front ends.

use serde::Serialize;
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_signing::prelude::*;
use smbcloud_ascapi_signing::profile::{Profile, ProfileType};

use crate::time::{is_expired, now_iso8601};

/// Map a CLI/MCP profile-type string onto the API enum.
///
/// Accepts the App Store Connect spelling with either casing, so a caller
/// that copied `MAC_APP_STORE` out of an API response does not have to
/// translate it.
pub fn profile_type_from_str(value: &str) -> Result<ProfileType, String> {
    match value.to_ascii_lowercase().as_str() {
        "ios_app_development" => Ok(ProfileType::IosAppDevelopment),
        "ios_app_store" => Ok(ProfileType::IosAppStore),
        "ios_app_adhoc" => Ok(ProfileType::IosAppAdHoc),
        "ios_app_inhouse" => Ok(ProfileType::IosAppInHouse),
        "mac_app_development" => Ok(ProfileType::MacAppDevelopment),
        "mac_app_store" => Ok(ProfileType::MacAppStore),
        "mac_app_direct" => Ok(ProfileType::MacAppDirect),
        "tvos_app_development" => Ok(ProfileType::TvOsAppDevelopment),
        "tvos_app_store" => Ok(ProfileType::TvOsAppStore),
        "tvos_app_adhoc" => Ok(ProfileType::TvOsAppAdHoc),
        "tvos_app_inhouse" => Ok(ProfileType::TvOsAppInHouse),
        "mac_catalyst_app_development" => Ok(ProfileType::MacCatalystAppDevelopment),
        "mac_catalyst_app_store" => Ok(ProfileType::MacCatalystAppStore),
        "mac_catalyst_app_direct" => Ok(ProfileType::MacCatalystAppDirect),
        other => Err(format!(
            "unknown profile type {other:?}; expected one of ios_app_development, \
             ios_app_store, ios_app_adhoc, ios_app_inhouse, mac_app_development, \
             mac_app_store, mac_app_direct, tvos_app_development, tvos_app_store, \
             tvos_app_adhoc, tvos_app_inhouse, mac_catalyst_app_development, \
             mac_catalyst_app_store, mac_catalyst_app_direct"
        )),
    }
}

/// A profile as reported to callers.
///
/// Note what is missing: `profileContent`. It is the multi-kilobyte binary
/// profile, it is never needed to decide what to do next, and keeping it
/// out means a tool result can never carry one into a model's context.
/// `profiles download` writes it to a path instead, the same bargain
/// [`crate::certificates::IssuedCertificate`] makes for key material.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ProfileSummary {
    pub id: String,
    pub name: Option<String>,
    pub profile_type: Option<String>,
    pub profile_state: Option<String>,
    pub platform: Option<String>,
    /// The UUID inside the profile, which is what `codesign` and Xcode
    /// diagnostics quote. Different from `id`.
    pub uuid: Option<String>,
    pub created_date: Option<String>,
    pub expiration_date: Option<String>,
    /// Computed locally: App Store Connect offers no filter for it, and a
    /// profile can also be unusable while unexpired if its certificate was
    /// revoked — check `profile_state` for that case.
    pub expired: Option<bool>,
}

impl ProfileSummary {
    pub fn from_resource(profile: &Profile, now: &str) -> Self {
        let expiration_date = profile.attributes.expiration_date.clone();
        let expired = expiration_date
            .as_ref()
            .map(|at| is_expired(at.as_str(), now));

        Self {
            id: profile.id.clone(),
            name: profile.attributes.name.clone(),
            profile_type: profile
                .attributes
                .profile_type
                .map(|t| t.as_api_str().to_string()),
            profile_state: profile
                .attributes
                .profile_state
                .map(|s| s.as_api_str().to_string()),
            platform: profile.attributes.platform.clone(),
            uuid: profile.attributes.uuid.clone(),
            created_date: profile.attributes.created_date.clone(),
            expiration_date,
            expired,
        }
    }
}

/// The result of writing a profile to disk.
///
/// Carries the path, never the profile bytes.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct DownloadedProfile {
    pub profile: ProfileSummary,
    pub path: String,
    pub bytes: usize,
}

/// Fetch a profile's content and write it to `path`.
///
/// Shared by the CLI and the MCP tool so the "ask for the field
/// explicitly" detail has one implementation: a plain resource fetch omits
/// `profileContent`, and the absence is indistinguishable from a profile
/// that genuinely has none.
pub async fn download_profile(
    client: &Client,
    id: &str,
    path: &std::path::Path,
) -> Result<DownloadedProfile, String> {
    let profile = client
        .get_profile_content(id)
        .await
        .map_err(|error| error.to_string())?;

    let content = profile
        .attributes
        .profile_content
        .as_deref()
        .ok_or_else(|| {
            format!("profile {id} came back without profileContent, so there is nothing to write")
        })?;

    let bytes = base64_decode(content)?;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("creating {}: {error}", parent.display()))?;
        }
    }
    std::fs::write(path, &bytes).map_err(|error| format!("writing {}: {error}", path.display()))?;

    Ok(DownloadedProfile {
        profile: ProfileSummary::from_resource(&profile, &now_iso8601()),
        path: path.display().to_string(),
        bytes: bytes.len(),
    })
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(input.trim())
        .map_err(|error| format!("decoding the profile Apple returned: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_type_accepts_the_api_spelling_in_either_casing() {
        assert_eq!(
            profile_type_from_str("MAC_APP_STORE").unwrap(),
            ProfileType::MacAppStore
        );
        assert_eq!(
            profile_type_from_str("mac_app_store").unwrap(),
            ProfileType::MacAppStore
        );
    }

    #[test]
    fn errors_name_the_accepted_values() {
        // A model that guessed wrong has to correct itself from the error
        // alone, without another round trip.
        let error = profile_type_from_str("mac_appstore").unwrap_err();
        assert!(error.contains("mac_app_store"), "unhelpful error: {error}");
        assert!(error.contains("ios_app_store"), "unhelpful error: {error}");
    }

    #[test]
    fn summary_never_serializes_the_profile_body() {
        let summary = ProfileSummary {
            id: "ABC123".to_string(),
            name: Some("smbCloud Browser MAS Distribution Profile".to_string()),
            profile_type: Some("MAC_APP_STORE".to_string()),
            profile_state: Some("ACTIVE".to_string()),
            platform: Some("MAC_OS".to_string()),
            uuid: Some("1a2b3c".to_string()),
            created_date: Some("2026-08-26T20:03:07".to_string()),
            expiration_date: Some("2027-08-06T19:49:55".to_string()),
            expired: Some(false),
        };
        let json = serde_json::to_string(&summary).expect("serializes");
        assert!(!json.contains("profileContent"));
        assert!(!json.contains("profile_content"));
    }

    #[test]
    fn downloaded_profile_reports_a_path_not_bytes() {
        let downloaded = DownloadedProfile {
            profile: ProfileSummary {
                id: "ABC123".to_string(),
                name: None,
                profile_type: None,
                profile_state: None,
                platform: None,
                uuid: None,
                created_date: None,
                expiration_date: None,
                expired: None,
            },
            path: "/tmp/out/app.provisionprofile".to_string(),
            bytes: 12577,
        };
        let json = serde_json::to_string(&downloaded).expect("serializes");
        assert!(json.contains("app.provisionprofile"));
        assert!(!json.contains("profileContent"));
    }
}
