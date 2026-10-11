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
    /// output carried one. It is also the build's App Store Connect id, so
    /// `asc apps build <delivery_id>` reads the build back.
    pub delivery_id: Option<String>,
    /// altool's `--output-format json` body when it parsed (an array of
    /// both documents with `--wait`), otherwise its stdout as a string.
    pub output: serde_json::Value,
}

/// Which altool operation to run against the package: the pre-flight
/// `--validate-app` or the real `--upload-package`. They take the same
/// authentication and the same output format, so they share one argv
/// builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltoolOp {
    /// `--validate-app`: run App Store Connect's checks without delivering
    /// the binary. Catches most of what a failed upload would, in seconds.
    Validate,
    /// `--upload-package`: deliver the binary for real.
    Upload,
}

impl AltoolOp {
    fn flag(self) -> &'static str {
        match self {
            AltoolOp::Validate => "--validate-app",
            AltoolOp::Upload => "--upload-package",
        }
    }
}

/// The arguments after `xcrun`. Separate from [`upload_package`] so the
/// exact command line is testable and `--dry-run` can print it.
pub fn altool_args(package: &Path, auth: &AltoolAuth, op: AltoolOp, wait: bool) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "altool".into(),
        op.flag().into(),
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
    // --wait only means anything for an upload; validation is already
    // synchronous, so don't pass a flag altool would reject there.
    if wait && op == AltoolOp::Upload {
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
        // A `.app` is a directory, so the generic "not a file" check below
        // would mislabel the most common mistake — pointing altool at the
        // built app instead of the exported archive. Name it precisely.
        Some("app") => {
            return Err(format!(
                "{} is an .app bundle, not an uploadable package; export the \
                 archive first (xcodebuild -exportArchive) to get an .ipa/.pkg",
                package.display()
            ))
        }
        _ => {
            return Err(format!(
                "{} is not an .ipa or .pkg; export the archive first \
                 (xcodebuild -exportArchive)",
                package.display()
            ))
        }
    }
    // Distinguish "nothing here" from "here, but a directory" so the error
    // points at the real problem instead of a generic "does not exist".
    if !package.exists() {
        return Err(format!("{} does not exist", package.display()));
    }
    if !package.is_file() {
        return Err(format!(
            "{} is a directory, not a package file",
            package.display()
        ));
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
    run_altool(package, auth, AltoolOp::Upload, wait)
}

/// Run `xcrun altool --validate-app` and report what happened.
///
/// A pre-flight check that runs App Store Connect's validation without
/// delivering the binary: it catches most of what a failed upload would
/// (bad entitlements, missing icons, bundle-id mismatches) in seconds
/// instead of minutes, before anything is uploaded. Same success/`Err`
/// contract as [`upload_package`].
pub fn validate_app(package: &Path, auth: &AltoolAuth) -> Result<UploadOutcome, String> {
    // --wait is upload-only; validation is already synchronous.
    run_altool(package, auth, AltoolOp::Validate, false)
}

fn run_altool(
    package: &Path,
    auth: &AltoolAuth,
    op: AltoolOp,
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
        .args(altool_args(package, auth, op, wait))
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("running xcrun altool (is Xcode installed?): {error}"))?;

    Ok(outcome(package, output.status.code(), &output.stdout))
}

fn outcome(package: &Path, exit_code: Option<i32>, stdout: &[u8]) -> UploadOutcome {
    let output = parse_output(stdout);
    // A zero exit is necessary but not sufficient: altool has been seen to
    // exit 0 while reporting errors in its JSON body (e.g. the
    // `product-errors` array). Trust the exit code only when the body
    // doesn't contradict it.
    let success = exit_code == Some(0) && !has_errors(&output);
    UploadOutcome {
        package: package.display().to_string(),
        success,
        exit_code,
        delivery_id: find_delivery_id(&output),
        output,
    }
}

/// altool reports failures in its JSON body under a handful of keys
/// (`product-errors` / `product_errors` for upload, `errors` for
/// validation). A non-empty array under any of them means the operation
/// failed regardless of the exit code.
fn has_errors(value: &serde_json::Value) -> bool {
    const ERROR_KEYS: [&str; 3] = ["product-errors", "product_errors", "errors"];
    match value {
        serde_json::Value::Object(map) => {
            ERROR_KEYS.iter().any(|key| {
                matches!(map.get(*key), Some(serde_json::Value::Array(errors)) if !errors.is_empty())
            }) || map.values().any(has_errors)
        }
        serde_json::Value::Array(items) => items.iter().any(has_errors),
        _ => false,
    }
}

/// With `--wait`, altool writes two JSON documents back to back: the build's
/// processing status, then the upload result. Read them as a stream and
/// return an array when there is more than one. Anything that isn't JSON
/// is kept as text.
fn parse_output(stdout: &[u8]) -> serde_json::Value {
    let documents: Result<Vec<serde_json::Value>, _> = serde_json::Deserializer::from_slice(stdout)
        .into_iter()
        .collect();
    match documents {
        Ok(mut documents) if documents.len() == 1 => documents.remove(0),
        Ok(documents) if !documents.is_empty() => serde_json::Value::Array(documents),
        _ => serde_json::Value::String(String::from_utf8_lossy(stdout).trim().to_string()),
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
        let args = strings(altool_args(
            Path::new("/out/Klepon.ipa"),
            &auth(),
            AltoolOp::Upload,
            false,
        ));
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
        let args = strings(altool_args(
            Path::new("/out/Klepon.ipa"),
            &auth(),
            AltoolOp::Upload,
            true,
        ));
        assert!(!args.iter().any(|arg| arg == "--auth-string"));
        assert_eq!(args.last().map(String::as_str), Some("--wait"));
    }

    #[test]
    fn validation_uses_validate_app_and_ignores_wait() {
        let args = strings(altool_args(
            Path::new("/out/Klepon.ipa"),
            &auth(),
            AltoolOp::Validate,
            true,
        ));
        assert_eq!(args[1], "--validate-app");
        assert!(
            !args.iter().any(|arg| arg == "--wait"),
            "--wait is upload-only: {args:?}"
        );
    }

    #[test]
    fn points_at_the_archive_when_given_an_app_bundle() {
        let error = validate_package(Path::new("/out/Klepon.app")).unwrap_err();
        assert!(error.contains(".app"), "unhelpful error: {error}");
        assert!(error.contains("exportArchive"), "unhelpful error: {error}");
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

    /// The shape Xcode 27's altool prints with `--wait`, trimmed from a real
    /// smbcloud-mailx visionOS upload: a status document, then the upload
    /// document, with no separator between them.
    #[test]
    fn reads_both_documents_altool_prints_with_wait() {
        let body = br#"{
  "build-status" : "VALID",
  "bundle-short-version-string" : "1.1.0",
  "bundle-version" : "1791527508",
  "delivery-uuid" : "3a12861d-5afd-4fc1-8b2d-96c80e055b1c"
}
{
  "details" : {
    "delivery-uuid" : "3a12861d-5afd-4fc1-8b2d-96c80e055b1c"
  },
  "success-message" : "No errors uploading archive"
}
"#;
        let outcome = outcome(Path::new("/out/MailXVision.ipa"), Some(0), body);
        assert_eq!(
            outcome.delivery_id.as_deref(),
            Some("3a12861d-5afd-4fc1-8b2d-96c80e055b1c")
        );
        let documents = outcome.output.as_array().expect("two documents");
        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0]["build-status"], "VALID");
    }

    #[test]
    fn reports_failure_when_altool_exits_zero_but_the_body_has_errors() {
        let body = br#"{"product-errors":[{"message":"Invalid bundle"}]}"#;
        let outcome = outcome(Path::new("/out/Klepon.ipa"), Some(0), body);
        assert!(
            !outcome.success,
            "a non-empty product-errors array means failure even on exit 0"
        );
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
