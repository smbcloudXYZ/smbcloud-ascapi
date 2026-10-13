//! Cross-checking the local login keychain against App Store Connect.
//!
//! `asc signing doctor` answers one question a `list_certificates` call
//! alone cannot: of the certificates Apple holds, which ones can this
//! machine actually sign with? A certificate is only usable if its private
//! key sits in a local keychain — Apple keeps no copy — so the check reads
//! the serial numbers of the certificates in the login keychain and
//! intersects them with what Apple reports.
//!
//! macOS only. `security` and the login keychain do not exist elsewhere,
//! so the whole module is gated and the CLI exposes the command only on
//! macOS.
//!
//! The implementation shells out the same way the CSR tests do: `security`
//! to export the keychain certificates as PEM, `openssl x509` to read each
//! serial. Serial formatting differs between OpenSSL 3 and macOS
//! LibreSSL, so every serial is normalised (uppercase hex, no separators,
//! no leading zeros) before comparison rather than compared as printed.

use serde::Serialize;
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_signing::prelude::*;
use std::process::Command;

use crate::certificates::CertificateSummary;
use crate::time::now_iso8601;

/// One certificate Apple holds, annotated with whether this machine can
/// sign with it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct DoctorCertificate {
    #[serde(flatten)]
    pub certificate: CertificateSummary,
    /// True when a certificate with this serial is present in the local
    /// login keychain, which is the only place its private key can live.
    pub private_key_present: bool,
}

/// The result of `asc signing doctor`.
///
/// Carries no key or certificate material: `DoctorCertificate` reuses
/// [`CertificateSummary`], whose shape already forbids it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct DoctorReport {
    /// Every certificate Apple holds, each flagged with whether its key is
    /// present locally. Expired certificates are included — they are still
    /// what a shipped build was signed with — and carry `expired: true`.
    pub certificates: Vec<DoctorCertificate>,
    /// Serial numbers found in the local keychain that Apple does not
    /// report. Usually a certificate revoked at Apple while its key lingers
    /// locally, or a certificate from a different team.
    pub local_only_serials: Vec<String>,
    /// How many of Apple's certificates are usable here: not expired and
    /// backed by a local private key.
    pub usable_count: usize,
}

/// Normalise a serial number for comparison across toolchains.
///
/// OpenSSL 3 prints `serial=1A2B3C`, LibreSSL may print colon-separated
/// bytes, and Apple returns its own casing. Reducing each to uppercase hex
/// digits with leading zeros stripped makes them comparable regardless of
/// how they were rendered.
fn normalise_serial(serial: &str) -> String {
    let hex: String = serial
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_uppercase();
    let trimmed = hex.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Read the serial numbers of every certificate in the login keychain.
///
/// Returns normalised serials. Errors only when `security` itself cannot
/// be run; an empty keychain, or one `openssl` cannot parse an entry in,
/// yields the serials it could read rather than failing the whole command.
fn local_keychain_serials() -> Result<Vec<String>, String> {
    // `-a -p` dumps every certificate in the keychain as concatenated PEM.
    let output = Command::new("security")
        .args(["find-certificate", "-a", "-p"])
        .output()
        .map_err(|error| format!("running `security find-certificate`: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "`security find-certificate` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let pem = String::from_utf8_lossy(&output.stdout);
    let mut serials = Vec::new();

    for block in split_pem_certificates(&pem) {
        if let Some(serial) = openssl_serial(&block) {
            let normalised = normalise_serial(&serial);
            if !normalised.is_empty() && !serials.contains(&normalised) {
                serials.push(normalised);
            }
        }
    }

    Ok(serials)
}

/// Split a concatenated PEM dump into individual certificate blocks.
fn split_pem_certificates(pem: &str) -> Vec<String> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";

    let mut blocks = Vec::new();
    let mut rest = pem;
    while let Some(start) = rest.find(BEGIN) {
        let after_start = &rest[start..];
        if let Some(end_rel) = after_start.find(END) {
            let end = end_rel + END.len();
            blocks.push(after_start[..end].to_string());
            rest = &after_start[end..];
        } else {
            break;
        }
    }
    blocks
}

/// Read one PEM certificate's serial via `openssl x509 -noout -serial`.
fn openssl_serial(pem: &str) -> Option<String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("openssl")
        .args(["x509", "-noout", "-serial"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    child.stdin.take()?.write_all(pem.as_bytes()).ok()?;

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    // Output looks like `serial=1A2B3C`.
    text.split('=').nth(1).map(|s| s.trim().to_string())
}

/// Build the doctor report: list Apple's certificates, flag each with
/// whether its private key is present in the local keychain.
pub async fn run_doctor(client: &Client) -> Result<DoctorReport, String> {
    let certificates = client
        .list_certificates(None)
        .await
        .map_err(|error| error.to_string())?;

    let local_serials = local_keychain_serials()?;
    let now = now_iso8601();

    let mut doctor_certificates = Vec::with_capacity(certificates.len());
    let mut matched_local = std::collections::HashSet::new();
    let mut usable_count = 0;

    for certificate in &certificates {
        let summary = CertificateSummary::from_resource(certificate, &now);
        let normalised = summary
            .serial_number
            .as_deref()
            .map(normalise_serial)
            .filter(|s| !s.is_empty());

        let private_key_present = match &normalised {
            Some(serial) => local_serials.contains(serial),
            None => false,
        };
        if private_key_present {
            if let Some(serial) = &normalised {
                matched_local.insert(serial.clone());
            }
            if summary.expired != Some(true) {
                usable_count += 1;
            }
        }

        doctor_certificates.push(DoctorCertificate {
            certificate: summary,
            private_key_present,
        });
    }

    let local_only_serials = local_serials
        .into_iter()
        .filter(|serial| !matched_local.contains(serial))
        .collect();

    Ok(DoctorReport {
        certificates: doctor_certificates,
        local_only_serials,
        usable_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_serial_folds_casing_separators_and_leading_zeros() {
        assert_eq!(normalise_serial("1a2b3c"), "1A2B3C");
        assert_eq!(normalise_serial("1A:2B:3C"), "1A2B3C");
        assert_eq!(normalise_serial("0x1a2b3c"), "1A2B3C");
        assert_eq!(normalise_serial("001A2B3C"), "1A2B3C");
    }

    #[test]
    fn normalise_serial_keeps_a_single_zero() {
        assert_eq!(normalise_serial("00"), "0");
        assert_eq!(normalise_serial("0"), "0");
    }

    #[test]
    fn split_pem_separates_concatenated_certificates() {
        let pem = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n\
                   -----BEGIN CERTIFICATE-----\nBBBB\n-----END CERTIFICATE-----\n";
        let blocks = split_pem_certificates(pem);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].contains("AAAA"));
        assert!(blocks[1].contains("BBBB"));
    }

    #[test]
    fn split_pem_ignores_an_unterminated_trailing_block() {
        let pem = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n\
                   -----BEGIN CERTIFICATE-----\nBBBB\n";
        let blocks = split_pem_certificates(pem);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("AAAA"));
    }

    #[test]
    fn doctor_report_never_serializes_certificate_material() {
        let report = DoctorReport {
            certificates: vec![DoctorCertificate {
                certificate: CertificateSummary {
                    id: "ABC123".to_string(),
                    name: Some("Apple Distribution: Example".to_string()),
                    certificate_type: Some("DISTRIBUTION".to_string()),
                    serial_number: Some("1A2B3C".to_string()),
                    expiration_date: Some("2027-08-06T00:00:00".to_string()),
                    expired: Some(false),
                },
                private_key_present: true,
            }],
            local_only_serials: vec!["DEADBEEF".to_string()],
            usable_count: 1,
        };

        let json = serde_json::to_string(&report).expect("serializes");
        assert!(!json.contains("BEGIN CERTIFICATE"));
        assert!(!json.contains("BEGIN PRIVATE KEY"));
        // The flattened summary should surface serial and the presence flag.
        assert!(json.contains("1A2B3C"));
        assert!(json.contains("private_key_present"));
    }
}
