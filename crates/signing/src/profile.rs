//! Provisioning profiles: `/v1/profiles`.
//!
//! A profile is the document that ties three things together and says
//! Apple approves of the combination: *which app* (a bundle ID), *signed by
//! whom* (one or more certificates), and *allowed to run where* (a device
//! list, for the profile types that use one). Code signing consults it to
//! decide whether the entitlements a binary claims are ones the App ID is
//! actually authorised for.
//!
//! That last part is why this module exists alongside [`crate::certificate`]
//! rather than in the metadata crate. A certificate proves identity and
//! nothing else; it says nothing about entitlements. Enabling a capability
//! on an App ID does not reach an already-issued profile either — profiles
//! are snapshots, so a capability added today is invisible to a profile
//! created yesterday. The fix is always to create a new one, which is what
//! [`ProfilesApi::create_profile`] is for.
//!
//! Two operational notes that are easy to learn the expensive way:
//!
//! - **Apple will happily hand back a stale profile.** A profile stays
//!   `ACTIVE` until its certificate expires or is revoked, so a capability
//!   change leaves an outdated-but-valid profile in place. Nothing in the
//!   API marks it as superseded. Recreate rather than re-download.
//! - **`profileContent` is the file.** It is base64 of the binary
//!   `.mobileprovision`/`.provisionprofile` that goes into the bundle;
//!   there is no separate download endpoint.
//!
//! App Groups are the common reason a profile needs regenerating and are
//! also the one thing this API cannot help with: there is no `appGroups`
//! resource, so assigning a group to an App ID is Developer Portal web UI
//! work. Once assigned, a new profile picks it up.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{
    CreateBody, CreateData, Document, ListDocument, Resource, ResourceId, ToMany, ToOne,
};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "profiles";

/// The kind of profile, which fixes both the distribution channel and
/// whether a device list applies.
///
/// The `*Development` and `*AdHoc` variants are device-scoped: a build
/// signed with one runs only on hardware in the profile's device list.
/// The `*AppStore` and `*InHouse` variants are not, and Apple rejects a
/// create request that sends devices for them.
///
/// [`Self::MacAppStore`] is the one to use for a Mac App Store submission;
/// [`Self::MacAppDirect`] is Developer ID distribution outside the store
/// and is a different signing path entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileType {
    #[serde(rename = "IOS_APP_DEVELOPMENT")]
    IosAppDevelopment,
    #[serde(rename = "IOS_APP_STORE")]
    IosAppStore,
    #[serde(rename = "IOS_APP_ADHOC")]
    IosAppAdHoc,
    #[serde(rename = "IOS_APP_INHOUSE")]
    IosAppInHouse,
    #[serde(rename = "MAC_APP_DEVELOPMENT")]
    MacAppDevelopment,
    /// Mac App Store distribution — the profile a `productbuild` submission
    /// embeds as `Contents/embedded.provisionprofile`.
    #[serde(rename = "MAC_APP_STORE")]
    MacAppStore,
    #[serde(rename = "MAC_APP_DIRECT")]
    MacAppDirect,
    #[serde(rename = "TVOS_APP_DEVELOPMENT")]
    TvOsAppDevelopment,
    #[serde(rename = "TVOS_APP_STORE")]
    TvOsAppStore,
    #[serde(rename = "TVOS_APP_ADHOC")]
    TvOsAppAdHoc,
    #[serde(rename = "TVOS_APP_INHOUSE")]
    TvOsAppInHouse,
    #[serde(rename = "MAC_CATALYST_APP_DEVELOPMENT")]
    MacCatalystAppDevelopment,
    #[serde(rename = "MAC_CATALYST_APP_STORE")]
    MacCatalystAppStore,
    #[serde(rename = "MAC_CATALYST_APP_DIRECT")]
    MacCatalystAppDirect,
}

impl ProfileType {
    /// The string App Store Connect uses, for building
    /// `filter[profileType]` query values without going through serde.
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::IosAppDevelopment => "IOS_APP_DEVELOPMENT",
            Self::IosAppStore => "IOS_APP_STORE",
            Self::IosAppAdHoc => "IOS_APP_ADHOC",
            Self::IosAppInHouse => "IOS_APP_INHOUSE",
            Self::MacAppDevelopment => "MAC_APP_DEVELOPMENT",
            Self::MacAppStore => "MAC_APP_STORE",
            Self::MacAppDirect => "MAC_APP_DIRECT",
            Self::TvOsAppDevelopment => "TVOS_APP_DEVELOPMENT",
            Self::TvOsAppStore => "TVOS_APP_STORE",
            Self::TvOsAppAdHoc => "TVOS_APP_ADHOC",
            Self::TvOsAppInHouse => "TVOS_APP_INHOUSE",
            Self::MacCatalystAppDevelopment => "MAC_CATALYST_APP_DEVELOPMENT",
            Self::MacCatalystAppStore => "MAC_CATALYST_APP_STORE",
            Self::MacCatalystAppDirect => "MAC_CATALYST_APP_DIRECT",
        }
    }

    /// Whether Apple expects a device list for this profile type.
    ///
    /// Sending devices for a store profile is an error rather than a
    /// harmless extra, so callers building a create request need to know
    /// this before they assemble relationships.
    pub fn takes_devices(self) -> bool {
        matches!(
            self,
            Self::IosAppDevelopment
                | Self::IosAppAdHoc
                | Self::MacAppDevelopment
                | Self::TvOsAppDevelopment
                | Self::TvOsAppAdHoc
                | Self::MacCatalystAppDevelopment
        )
    }
}

/// Whether Apple still considers the profile usable.
///
/// `Invalid` is not a separate deletion state: it is what a profile
/// becomes when something it depends on goes away, most often a revoked or
/// expired certificate. The profile stays listed so you can see why a
/// build that used to sign no longer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileState {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "INVALID")]
    Invalid,
}

impl ProfileState {
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Invalid => "INVALID",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileAttributes {
    pub name: Option<String>,
    pub platform: Option<String>,
    pub profile_type: Option<ProfileType>,
    pub profile_state: Option<ProfileState>,
    /// The UUID embedded in the profile itself, which is what Xcode and
    /// `codesign` diagnostics refer to. Distinct from the resource `id`
    /// this API uses.
    pub uuid: Option<String>,
    pub created_date: Option<String>,
    /// ISO-8601. Bounded by the signing certificate's own expiry, so a
    /// profile never outlives the certificate it embeds.
    pub expiration_date: Option<String>,
    /// Base64 of the binary profile file. Omitted from list responses
    /// unless asked for via `fields[profiles]`; see
    /// [`ProfilesApi::get_profile_content`].
    pub profile_content: Option<String>,
}

pub type Profile = Resource<ProfileAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCreateAttributes {
    /// Free-form label shown in the developer portal. Apple permits
    /// duplicates, so a team can accumulate several same-named profiles
    /// that differ only by creation date — worth making distinctive.
    pub name: String,
    pub profile_type: ProfileType,
}

/// The relationships a create request must carry.
///
/// Built explicitly rather than derived from ids alone, because `devices`
/// has to be *absent* rather than empty for store profiles — see
/// [`ProfileType::takes_devices`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCreateRelationships {
    pub bundle_id: ToOne,
    pub certificates: ToMany,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<ToMany>,
}

impl ProfileCreateRelationships {
    /// Assemble relationships for `profile_type`, dropping `devices` when
    /// the type does not take them.
    pub fn new(
        profile_type: ProfileType,
        bundle_id: &str,
        certificate_ids: &[String],
        device_ids: &[String],
    ) -> Self {
        let devices = if profile_type.takes_devices() && !device_ids.is_empty() {
            Some(ToMany {
                data: device_ids
                    .iter()
                    .map(|id| ResourceId {
                        resource_type: "devices",
                        id: id.clone(),
                    })
                    .collect(),
            })
        } else {
            None
        };

        Self {
            bundle_id: ToOne {
                data: ResourceId {
                    resource_type: "bundleIds",
                    id: bundle_id.to_string(),
                },
            },
            certificates: ToMany {
                data: certificate_ids
                    .iter()
                    .map(|id| ResourceId {
                        resource_type: "certificates",
                        id: id.clone(),
                    })
                    .collect(),
            },
            devices,
        }
    }
}

/// Provisioning profiles.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait ProfilesApi {
    /// `GET /v1/profiles`, optionally filtered by name and type.
    ///
    /// `profile_content` is left out of these results by Apple. That is
    /// usually what you want — the content is a multi-kilobyte blob and
    /// listing is for deciding *which* profile you need. Fetch the bytes
    /// with [`ProfilesApi::get_profile_content`] once you know.
    async fn list_profiles(
        &self,
        filter_name: Option<&str>,
        filter_type: Option<ProfileType>,
    ) -> Result<Vec<Profile>>;

    /// `GET /v1/profiles/{id}` asking explicitly for `profileContent`.
    ///
    /// Separate from listing because the field has to be requested: a
    /// plain `GET` of the same resource returns metadata with
    /// `profileContent` absent, which reads as "this profile has no
    /// content" rather than "you did not ask for it".
    async fn get_profile_content(&self, id: &str) -> Result<Profile>;

    /// `POST /v1/profiles` — creates a profile over a bundle ID and
    /// certificates.
    ///
    /// This is the only way to pick up a capability that was added to the
    /// App ID after an existing profile was made. Apple does not reject
    /// the request as a duplicate when an equivalent profile already
    /// exists; you get a second one, and both stay active.
    async fn create_profile(
        &self,
        attributes: ProfileCreateAttributes,
        relationships: ProfileCreateRelationships,
    ) -> Result<Profile>;

    /// `DELETE /v1/profiles/{id}`.
    ///
    /// Narrower than revoking a certificate: it invalidates only this
    /// profile, and any build already signed with it keeps working. New
    /// signing that referenced it fails until a replacement exists.
    async fn delete_profile(&self, id: &str) -> Result<()>;
}

#[async_trait]
impl ProfilesApi for Client {
    async fn list_profiles(
        &self,
        filter_name: Option<&str>,
        filter_type: Option<ProfileType>,
    ) -> Result<Vec<Profile>> {
        let mut query = Vec::new();
        if let Some(name) = filter_name {
            query.push(("filter[name]", name));
        }
        if let Some(profile_type) = filter_type {
            query.push(("filter[profileType]", profile_type.as_api_str()));
        }
        // Apple's default page size is 20, which a team carrying a profile
        // per app per platform will exceed.
        query.push(("limit", "200"));

        let doc: ListDocument<ProfileAttributes> = self
            .request(Method::GET, "/v1/profiles", &query, None::<&()>)
            .await?;
        Ok(doc.data)
    }

    async fn get_profile_content(&self, id: &str) -> Result<Profile> {
        let query = [(
            "fields[profiles]",
            "name,platform,profileType,profileState,uuid,createdDate,expirationDate,profileContent",
        )];
        let doc: Document<ProfileAttributes> = self
            .request(
                Method::GET,
                &format!("/v1/profiles/{id}"),
                &query,
                None::<&()>,
            )
            .await?;
        Ok(doc.data)
    }

    async fn create_profile(
        &self,
        attributes: ProfileCreateAttributes,
        relationships: ProfileCreateRelationships,
    ) -> Result<Profile> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes,
                relationships: Some(relationships),
            },
        };
        let doc: Document<ProfileAttributes> = self
            .request(Method::POST, "/v1/profiles", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    async fn delete_profile(&self, id: &str) -> Result<()> {
        self.request_no_content(
            Method::DELETE,
            &format!("/v1/profiles/{id}"),
            &[],
            None::<&()>,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_type_round_trips_through_the_api_wire_format() {
        // The serde rename and as_api_str() are two separate spellings of
        // the same contract, and a filter built from the wrong one fails
        // silently by matching nothing.
        for profile_type in [
            ProfileType::IosAppDevelopment,
            ProfileType::IosAppStore,
            ProfileType::IosAppAdHoc,
            ProfileType::IosAppInHouse,
            ProfileType::MacAppDevelopment,
            ProfileType::MacAppStore,
            ProfileType::MacAppDirect,
            ProfileType::TvOsAppDevelopment,
            ProfileType::TvOsAppStore,
            ProfileType::TvOsAppAdHoc,
            ProfileType::TvOsAppInHouse,
            ProfileType::MacCatalystAppDevelopment,
            ProfileType::MacCatalystAppStore,
            ProfileType::MacCatalystAppDirect,
        ] {
            let json = serde_json::to_string(&profile_type).expect("serializes");
            let unquoted = json.trim_matches('"');
            assert_eq!(
                unquoted,
                profile_type.as_api_str(),
                "serde and as_api_str disagree for {profile_type:?}"
            );

            let parsed: ProfileType = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(parsed, profile_type);
        }
    }

    #[test]
    fn profile_state_round_trips_through_the_api_wire_format() {
        for state in [ProfileState::Active, ProfileState::Invalid] {
            let json = serde_json::to_string(&state).expect("serializes");
            assert_eq!(json.trim_matches('"'), state.as_api_str());
            let parsed: ProfileState = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(parsed, state);
        }
    }

    #[test]
    fn store_profiles_omit_devices_entirely() {
        // An empty `devices` array is not the same as no `devices` key:
        // Apple rejects the former on a store profile. Serializing has to
        // drop the field, not send `[]`.
        let relationships = ProfileCreateRelationships::new(
            ProfileType::MacAppStore,
            "P59W5V5953",
            &["CERT1".to_string()],
            &["DEVICE1".to_string()],
        );
        let json = serde_json::to_value(&relationships).expect("serializes");
        assert!(
            json.get("devices").is_none(),
            "a store profile must not carry devices: {json}"
        );
        assert_eq!(json["bundleId"]["data"]["type"], "bundleIds");
        assert_eq!(json["certificates"]["data"][0]["type"], "certificates");
    }

    #[test]
    fn development_profiles_keep_their_devices() {
        let relationships = ProfileCreateRelationships::new(
            ProfileType::IosAppDevelopment,
            "P59W5V5953",
            &["CERT1".to_string()],
            &["DEVICE1".to_string(), "DEVICE2".to_string()],
        );
        let json = serde_json::to_value(&relationships).expect("serializes");
        assert_eq!(json["devices"]["data"].as_array().expect("array").len(), 2);
        assert_eq!(json["devices"]["data"][0]["type"], "devices");
    }

    #[test]
    fn a_development_profile_with_no_devices_omits_the_key() {
        // Rather than sending `{"data": []}`, which Apple reads as an
        // explicit empty set and rejects.
        let relationships = ProfileCreateRelationships::new(
            ProfileType::IosAppDevelopment,
            "P59W5V5953",
            &["CERT1".to_string()],
            &[],
        );
        let json = serde_json::to_value(&relationships).expect("serializes");
        assert!(json.get("devices").is_none());
    }
}
