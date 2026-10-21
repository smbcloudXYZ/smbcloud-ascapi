//! Attaching a build to an App Store Version, and confirming it took.
//!
//! The PATCH response is the version resource, whose `build` relationship
//! holds only links, so it cannot say which build is attached. Reading the
//! build back is what catches a wrong attachment before App Review does.

use serde::Serialize;
use smbcloud_ascapi_aso::build::Build;
use smbcloud_ascapi_aso::prelude::*;
use smbcloud_ascapi_core::Client;

/// The outcome of [`attach_build`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedBuild {
    pub version_id: String,
    /// The build read back from App Store Connect: its id, `version`,
    /// `processingState` and `usesNonExemptEncryption`. `None` only when
    /// verification was skipped.
    pub build: Option<Build>,
}

/// Point `version_id` at `build_id`, then (when `verify`) read the attached
/// build back and fail unless it is the one requested.
pub async fn attach_build(
    client: &Client,
    version_id: &str,
    build_id: &str,
    verify: bool,
) -> Result<AttachedBuild, String> {
    client
        .set_app_store_version_build(version_id, build_id)
        .await
        .map_err(|error| error.to_string())?;
    if !verify {
        return Ok(AttachedBuild {
            version_id: version_id.to_string(),
            build: None,
        });
    }
    let attached = client
        .get_app_store_version_build(version_id)
        .await
        .map_err(|error| error.to_string())?;
    check_attached(build_id, attached.as_ref())?;
    Ok(AttachedBuild {
        version_id: version_id.to_string(),
        build: attached,
    })
}

/// The read-back check, split out so it can be tested without a network.
fn check_attached(expected: &str, attached: Option<&Build>) -> Result<(), String> {
    match attached {
        None => Err(format!(
            "attached build {expected}, but the version reports no build"
        )),
        Some(build) if build.id != expected => Err(format!(
            "asked to attach build {expected}, but the version reports build {}",
            build.id
        )),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smbcloud_ascapi_aso::build::BuildAttributes;

    fn build(id: &str) -> Build {
        Build {
            id: id.to_string(),
            resource_type: "builds".to_string(),
            attributes: BuildAttributes {
                version: Some("1".to_string()),
                uploaded_date: None,
                expired: None,
                processing_state: None,
                min_os_version: None,
                uses_non_exempt_encryption: None,
            },
            relationships: None,
        }
    }

    #[test]
    fn a_matching_build_passes() {
        assert!(check_attached("b1", Some(&build("b1"))).is_ok());
    }

    #[test]
    fn no_build_is_an_error_naming_the_expected_id() {
        let error = check_attached("b1", None).unwrap_err();
        assert!(error.contains("b1"));
    }

    #[test]
    fn a_different_build_names_both_ids() {
        let error = check_attached("b1", Some(&build("b2"))).unwrap_err();
        assert!(error.contains("b1") && error.contains("b2"));
    }
}
