//! String to enum mapping for values that arrive as text from a command
//! line or a tool call.
//!
//! Each mapper's error lists the accepted values. That matters more for
//! the MCP surface than the CLI one: clap rejects a bad value before the
//! program runs, but a model that guessed wrong only learns from the
//! error text, and a bare "invalid value" costs it another round trip.

use smbcloud_ascapi_aso::app_screenshot_set::ScreenshotDisplayType;
use smbcloud_ascapi_aso::app_store_version::Platform;
use smbcloud_ascapi_aso::bundle_id::BundleIdPlatform;

pub fn platform_from_str(value: &str) -> Result<Platform, String> {
    match value.to_ascii_lowercase().as_str() {
        "ios" => Ok(Platform::Ios),
        "mac_os" | "macos" => Ok(Platform::MacOs),
        "tv_os" | "tvos" => Ok(Platform::TvOs),
        "vision_os" | "visionos" => Ok(Platform::VisionOs),
        other => Err(format!(
            "unknown platform {other:?}; expected one of ios, mac_os, tv_os, vision_os"
        )),
    }
}

pub fn bundle_id_platform_from_str(value: &str) -> Result<BundleIdPlatform, String> {
    match value.to_ascii_lowercase().as_str() {
        // visionOS shares the iOS identifier namespace, so there is no
        // separate value to offer here.
        "ios" => Ok(BundleIdPlatform::Ios),
        "mac_os" | "macos" => Ok(BundleIdPlatform::MacOs),
        "universal" => Ok(BundleIdPlatform::Universal),
        // Accepted so a round trip through this mapper does not lose a
        // value the API itself returns. Registering one is a different
        // flow than an app bundle ID, and Apple rejects it here.
        "services" => Ok(BundleIdPlatform::Services),
        other => Err(format!(
            "unknown bundle ID platform {other:?}; expected one of ios, mac_os, universal"
        )),
    }
}

/// App Store Connect's display types are already screaming snake case, so
/// this accepts them verbatim and only normalizes the casing.
pub fn display_type_from_str(value: &str) -> Result<ScreenshotDisplayType, String> {
    let upper = value.to_ascii_uppercase();
    serde_json::from_value::<ScreenshotDisplayType>(serde_json::Value::String(upper.clone()))
        .map_err(|_| {
            format!(
                "unknown screenshot display type {value:?}; expected an App Store Connect \
                 display type such as APP_IPHONE_67, APP_IPAD_PRO_129, APP_APPLE_VISION_PRO, \
                 or APP_DESKTOP"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_accepts_both_spellings() {
        assert_eq!(platform_from_str("visionos").unwrap(), Platform::VisionOs);
        assert_eq!(platform_from_str("VISION_OS").unwrap(), Platform::VisionOs);
    }

    #[test]
    fn errors_name_the_accepted_values() {
        // A model that guessed wrong has to be able to fix itself from the
        // error alone, without another call.
        let error = platform_from_str("watchos").unwrap_err();
        assert!(
            error.contains("ios") && error.contains("vision_os"),
            "{error}"
        );

        let error = display_type_from_str("iphone").unwrap_err();
        assert!(error.contains("APP_IPHONE_67"), "{error}");

        let error = bundle_id_platform_from_str("visionos").unwrap_err();
        assert!(error.contains("universal"), "{error}");
    }

    #[test]
    fn display_type_round_trips_a_real_value() {
        assert_eq!(
            display_type_from_str("app_iphone_67").unwrap(),
            ScreenshotDisplayType::Iphone67
        );
    }
}
