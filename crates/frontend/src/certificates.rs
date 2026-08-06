//! Certificate operations shared by both front ends.

use serde::Serialize;
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_signing::certificate::CertificateType;
use smbcloud_ascapi_signing::prelude::*;

use crate::time::{is_expired, now_iso8601};

/// Map a CLI/MCP certificate-type string onto the API enum.
pub fn certificate_type_from_str(value: &str) -> Result<CertificateType, String> {
    use CertificateType;
    match value {
        "development" => Ok(CertificateType::Development),
        "distribution" => Ok(CertificateType::Distribution),
        "mac_app_distribution" => Ok(CertificateType::MacAppDistribution),
        "mac_installer_distribution" => Ok(CertificateType::MacInstallerDistribution),
        "developer_id_application" => Ok(CertificateType::DeveloperIdApplication),
        other => Err(format!(
            "unknown certificate type {other:?}; expected one of development, distribution, \
             mac_app_distribution, mac_installer_distribution, developer_id_application"
        )),
    }
}

/// Defines the MCP tool implementations on `$server`, with each tool's
/// wire name supplied by the embedder.
///
/// Names are parameters rather than literals so a host that re-exposes
/// this contract can namespace them without forking the bodies, exactly as
/// `xcrs_mcp_tools!` allows in `smbcloud-cli`.
/// A certificate as reported to callers.
///
/// Note what is missing: `certificateContent`. It is large, it is never
/// needed to decide what to do next, and keeping it out means a tool
/// result can never carry a certificate body into a model's context.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CertificateSummary {
    pub id: String,
    pub name: Option<String>,
    pub certificate_type: Option<String>,
    pub serial_number: Option<String>,
    pub expiration_date: Option<String>,
    /// Computed locally: App Store Connect offers no filter for it and
    /// returns expired certificates alongside valid ones.
    pub expired: Option<bool>,
}

impl CertificateSummary {
    pub fn from_resource(
        certificate: &smbcloud_ascapi_signing::certificate::Certificate,
        now: &str,
    ) -> Self {
        let expiration_date = certificate.attributes.expiration_date.clone();
        let expired = expiration_date
            .as_ref()
            .map(|at| is_expired(at.as_str(), now));

        Self {
            id: certificate.id.clone(),
            name: certificate.attributes.name.clone(),
            certificate_type: certificate
                .attributes
                .certificate_type
                .map(|t| t.as_api_str().to_string()),
            serial_number: certificate.attributes.serial_number.clone(),
            expiration_date,
            expired,
        }
    }
}

/// The result of issuing a certificate.
///
/// Carries paths, never key material.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct IssuedCertificate {
    pub certificate: CertificateSummary,
    /// Path the private key was written to. The key itself is never
    /// included: this is the whole point of returning a path.
    pub private_key_path: String,
    pub certificate_path: String,
    /// Copy-pasteable commands to get the pair into a keychain.
    pub next_steps: Vec<String>,
}

/// Generate a key pair, have Apple certify it, and write both halves.
///
/// Shared by the CLI and the MCP tool so the ordering guarantee (key
/// written before the request is sent) has exactly one implementation.
pub async fn issue_certificate(
    client: &Client,
    certificate_type: CertificateType,
    common_name: &str,
    out_dir: &std::path::Path,
) -> Result<IssuedCertificate, String> {
    use smbcloud_ascapi_signing::certificate::CertificateCreateAttributes;

    let request = smbcloud_ascapi_signing::csr::generate_certificate_request(common_name)
        .map_err(|error| format!("generating the RSA 2048 key pair and CSR: {error}"))?;

    std::fs::create_dir_all(out_dir)
        .map_err(|error| format!("creating {}: {error}", out_dir.display()))?;

    let stem = certificate_type.as_api_str().to_lowercase();
    let key_path = out_dir.join(format!("{stem}.key.pem"));

    // Before the network call, deliberately. A failure after Apple has
    // issued the certificate would leave a certificate whose key was never
    // persisted, which is unrecoverable and has consumed one of the team's
    // limited slots. A failure here leaves an unused key, which costs
    // nothing.
    write_private_key(&key_path, request.private_key_pem())
        .map_err(|error| format!("writing the private key to {}: {error}", key_path.display()))?;

    let certificate = client
        .create_certificate(CertificateCreateAttributes {
            csr_content: request.csr_pem().to_string(),
            certificate_type,
        })
        .await
        .map_err(|error| error.to_string())?;

    let cer_path = out_dir.join(format!("{stem}.cer"));
    if let Some(content) = certificate.attributes.certificate_content.as_deref() {
        let der = base64_decode(content)?;
        std::fs::write(&cer_path, der)
            .map_err(|error| format!("writing {}: {error}", cer_path.display()))?;
    }

    Ok(IssuedCertificate {
        certificate: CertificateSummary::from_resource(&certificate, &now_iso8601()),
        private_key_path: key_path.display().to_string(),
        certificate_path: cer_path.display().to_string(),
        next_steps: vec![
            format!(
                "openssl pkcs12 -export -inkey {} -in {} -out {stem}.p12",
                key_path.display(),
                cer_path.display()
            ),
            format!(
                "security import {stem}.p12 -k ~/Library/Keychains/login.keychain-db -T /usr/bin/codesign"
            ),
            "security find-identity -v -p codesigning".to_string(),
        ],
    })
}

/// Write a private key with owner-only permissions, set at creation time
/// rather than chmod'ed afterwards so the key is never briefly readable by
/// anyone else.
pub fn write_private_key(path: &std::path::Path, pem: &str) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(pem.as_bytes())?;
    file.sync_all()
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(input.trim())
        .map_err(|error| format!("decoding the certificate Apple returned: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_comparison_matches_calendar_order() {
        assert!(is_expired("2026-08-05T20:56:53", "2026-08-06T04:32:19"));
        assert!(!is_expired("2027-02-01T22:12:15", "2026-08-06T04:32:19"));
        // Same day, hours apart: the case that decides whether a
        // certificate expiring later today reads as already dead.
        assert!(!is_expired("2026-08-06T04:37:57", "2026-08-06T04:32:19"));
        assert!(is_expired("2026-08-06T04:31:00", "2026-08-06T04:32:19"));
    }

    #[test]
    fn now_is_a_fixed_width_sortable_timestamp() {
        let now = now_iso8601();
        assert_eq!(now.len(), 19, "expected YYYY-MM-DDTHH:MM:SS, got {now:?}");
        assert_eq!(&now[4..5], "-");
        assert_eq!(&now[10..11], "T");
        // Sanity bound: a wildly wrong civil-date conversion would land
        // outside this window and silently mark every certificate expired.
        assert!(
            now.as_str() > "2024-01-01T00:00:00",
            "clock looks wrong: {now}"
        );
        assert!(
            now.as_str() < "2100-01-01T00:00:00",
            "clock looks wrong: {now}"
        );
    }

    #[test]
    fn certificate_type_strings_are_the_documented_set() {
        for value in [
            "development",
            "distribution",
            "mac_app_distribution",
            "mac_installer_distribution",
            "developer_id_application",
        ] {
            certificate_type_from_str(value)
                .unwrap_or_else(|_| panic!("{value} should be accepted"));
        }
        let error = certificate_type_from_str("apple_distribution").expect_err("should reject");
        // The error has to list the valid values: a model that guessed
        // wrong needs to correct itself without another round trip.
        assert!(error.contains("distribution"), "unhelpful error: {error}");
    }

    #[test]
    fn issued_certificate_never_serializes_key_material() {
        let issued = IssuedCertificate {
            certificate: CertificateSummary {
                id: "ABC123".to_string(),
                name: Some("Apple Distribution: Example".to_string()),
                certificate_type: Some("DISTRIBUTION".to_string()),
                serial_number: Some("1A2B3C".to_string()),
                expiration_date: Some("2027-08-06T00:00:00".to_string()),
                expired: Some(false),
            },
            private_key_path: "/tmp/out/distribution.key.pem".to_string(),
            certificate_path: "/tmp/out/distribution.cer".to_string(),
            next_steps: vec!["security find-identity -v -p codesigning".to_string()],
        };

        let json = serde_json::to_string(&issued).expect("serializes");
        assert!(!json.contains("BEGIN PRIVATE KEY"));
        assert!(!json.contains("BEGIN CERTIFICATE"));
        // The path is the point: it tells the caller where to look without
        // putting the secret into a transcript.
        assert!(json.contains("distribution.key.pem"));
    }
}
