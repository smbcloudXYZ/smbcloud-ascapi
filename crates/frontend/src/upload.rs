//! Uploading an `.ipa`/`.pkg` to App Store Connect through `xcrun altool`.
//!
//! The App Store Connect REST API has no binary-upload endpoint a client
//! can drive end to end, so this shells out to Apple's own uploader. What
//! this module adds over calling altool by hand is authentication parity:
//! altool is handed the exact `.p8` the rest of the CLI resolved, via
//! `--p8-file-path`, instead of searching `~/.appstoreconnect/private_keys`
//! and friends for an `AuthKey_<id>.p8`. A search can find a stale key with
//! the same id that the API then rejects with a 401, which is
//! indistinguishable at the call site from a revoked key.
//!
//! Once the upload succeeds, look the build up with
//! [`BuildsApi::list_builds`](smbcloud_ascapi_aso::build::BuildsApi::list_builds)
//! filtered by build number, or pass `wait` to have altool block until App
//! Store Connect finishes processing.

use serde::Serialize;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The API key altool authenticates with. Paths and ids only: altool's
/// `--auth-string` would take the key itself, which would then sit in the
/// process table for any local user to read, so it is never used.
#[derive(Debug, Clone)]
pub struct AltoolAuth {
    pub key_id: String,
    pub issuer_id: String,
    pub private_key_path: PathBuf,
}

/// The result of one altool upload, as reported to callers.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct UploadOutcome {
    pub package: String,
    pub success: bool,
    /// altool's exit status; `None` if it was killed by a signal.
    pub exit_code: Option<i32>,
    /// The delivery UUID altool reports for a successful upload, if its
    /// output carried one. `altool --build-status --delivery-id` takes it.
    pub delivery_id: Option<String>,
    /// altool's `--output-format json` body when it parsed, otherwise its
    /// stdout as a string.
    pub output: serde_json::Value,
}

/// The arguments after `xcrun`. Separate from [`upload_package`] so the
/// exact command line is testable and `--dry-run` can print it.
pub fn altool_args(package: &Path, auth: &AltoolAuth, wait: bool) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "altool".into(),
        "--upload-package".into(),
        package.as_os_str().to_owned(),
        "--api-key".into(),
        auth.key_id.clone().into(),
        "--api-issuer".into(),
        auth.issuer_id.clone().into(),
        "--p8-file-path".into(),
        auth.private_key_path.as_os_str().to_owned(),
        "--output-format".into(),
        "json".into(),
    ];
    if wait {
        args.push("--wait".into());
    }
    args
}

/// Check the package before spending minutes in altool on it.
pub fn validate_package(package: &Path) -> Result<(), String> {
    let extension = package
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("ipa") | Some("pkg") => {}
        _ => {
            return Err(format!(
                "{} is not an .ipa or .pkg; export the archive first \
                 (xcodebuild -exportArchive)",
                package.display()
            ))
        }
    }
    if !package.is_file() {
        return Err(format!("{} does not exist", package.display()));
    }
    Ok(())
}

/// Run `xcrun altool --upload-package` and report what happened.
///
/// Blocks for the length of the upload (and processing, with `wait`), so
/// async callers should run it on a blocking thread. altool's stderr is
/// passed through so its progress stays visible; stdout is captured for
/// the JSON result.
///
/// A failed upload is an `Ok` with `success: false`, not an `Err`: altool's
/// own error body is the useful part and the caller should see it. `Err`
/// means altool could not be run at all.
pub fn upload_package(
    package: &Path,
    auth: &AltoolAuth,
    wait: bool,
) -> Result<UploadOutcome, String> {
    validate_package(package)?;
    if !auth.private_key_path.is_file() {
        return Err(format!(
            "could not read the App Store Connect private key at {}",
            auth.private_key_path.display()
        ));
    }

    let output = Command::new("xcrun")
        .args(altool_args(package, auth, wait))
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("running xcrun altool (is Xcode installed?): {error}"))?;

    Ok(outcome(package, output.status.code(), &output.stdout))
}

fn outcome(package: &Path, exit_code: Option<i32>, stdout: &[u8]) -> UploadOutcome {
    let output = serde_json::from_slice::<serde_json::Value>(stdout).unwrap_or_else(|_| {
        serde_json::Value::String(String::from_utf8_lossy(stdout).trim().to_string())
    });
    UploadOutcome {
        package: package.display().to_string(),
        success: exit_code == Some(0),
        exit_code,
        delivery_id: find_delivery_id(&output),
        output,
    }
}

/// altool has spelled this key differently across releases
/// (`delivery-uuid` in the legacy tool), so search the body for any of
/// the spellings rather than pinning one path.
fn find_delivery_id(value: &serde_json::Value) -> Option<String> {
    const KEYS: [&str; 4] = ["delivery-uuid", "deliveryUuid", "delivery-id", "deliveryId"];
    match value {
        serde_json::Value::Object(map) => KEYS
            .iter()
            .find_map(|key| map.get(*key).and_then(|v| v.as_str()).map(str::to_string))
            .or_else(|| map.values().find_map(find_delivery_id)),
        serde_json::Value::Array(items) => items.iter().find_map(find_delivery_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> AltoolAuth {
        AltoolAuth {
            key_id: "ABC123".to_string(),
            issuer_id: "57246542-96fe-1a63-e053-0824d011072a".to_string(),
            private_key_path: PathBuf::from("/keys/AuthKey_ABC123.p8"),
        }
    }

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect()
    }

    #[test]
    fn passes_the_resolved_key_file_rather_than_letting_altool_search() {
        let args = strings(altool_args(Path::new("/out/Klepon.ipa"), &auth(), false));
        assert_eq!(
            args,
            [
                "altool",
                "--upload-package",
                "/out/Klepon.ipa",
                "--api-key",
                "ABC123",
                "--api-issuer",
                "57246542-96fe-1a63-e053-0824d011072a",
                "--p8-file-path",
                "/keys/AuthKey_ABC123.p8",
                "--output-format",
                "json",
            ]
        );
    }

    #[test]
    fn never_puts_key_material_on_the_command_line() {
        let args = strings(altool_args(Path::new("/out/Klepon.ipa"), &auth(), true));
        assert!(!args.iter().any(|arg| arg == "--auth-string"));
        assert_eq!(args.last().map(String::as_str), Some("--wait"));
    }

    #[test]
    fn rejects_an_unexported_archive() {
        let error = validate_package(Path::new("/out/Klepon.xcarchive")).unwrap_err();
        assert!(error.contains("exportArchive"), "unhelpful error: {error}");
    }

    #[test]
    fn reads_a_delivery_id_from_nested_json() {
        let body = br#"{"tool-version":"9.0","details":{"delivery-uuid":"d1e2f3"}}"#;
        let outcome = outcome(Path::new("/out/Klepon.ipa"), Some(0), body);
        assert!(outcome.success);
        assert_eq!(outcome.delivery_id.as_deref(), Some("d1e2f3"));
    }

    #[test]
    fn keeps_non_json_output_as_text_and_reports_failure() {
        let outcome = outcome(
            Path::new("/out/Klepon.ipa"),
            Some(1),
            b"ERROR: bad bundle\n",
        );
        assert!(!outcome.success);
        assert_eq!(outcome.output, serde_json::json!("ERROR: bad bundle"));
        assert_eq!(outcome.delivery_id, None);
    }
}
