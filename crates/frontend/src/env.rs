//! Credential resolution from the environment.

use smbcloud_ascapi_core::ApiKey;
use smbcloud_ascapi_core::Error;

/// Resolve an App Store Connect API key from the environment.
///
/// Deliberately per-call rather than at startup: an MCP server that
/// refuses to start without credentials cannot answer `tools/list`, which
/// is the first thing every client asks and the only way a user discovers
/// what configuration is missing. Unconfigured servers should list their
/// tools and fail with a useful message when one is called.
pub fn api_key_from_env() -> Result<ApiKey, String> {
    let key_id = std::env::var("ASC_API_KEY")
        .map_err(|_| "ASC_API_KEY is not set (App Store Connect API key ID)".to_string())?;
    let issuer_id = std::env::var("ASC_ISSUER_ID")
        .map_err(|_| "ASC_ISSUER_ID is not set (App Store Connect issuer ID)".to_string())?;

    let path = match std::env::var("ASC_PRIVATE_KEY_PATH") {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            let home = std::env::var("HOME").map_err(|_| {
                "neither ASC_PRIVATE_KEY_PATH nor HOME is set, so the .p8 key cannot be located"
                    .to_string()
            })?;
            std::path::PathBuf::from(home)
                .join(".appstoreconnect")
                .join("private_keys")
                .join(format!("AuthKey_{key_id}.p8"))
        }
    };

    ApiKey::from_p8_file(&key_id, &issuer_id, &path).map_err(|error| match error {
        // The most common misconfiguration by a wide margin, and the
        // default-path message needs to name the path it guessed.
        Error::PrivateKeyRead { .. } => {
            format!(
                "could not read the App Store Connect private key at {}",
                path.display()
            )
        }
        other => other.to_string(),
    })
}
