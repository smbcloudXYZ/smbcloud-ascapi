//! Signing certificates: `/v1/certificates`.
//!
//! The one thing to understand before using this module: **Apple never
//! gives you a private key.** You generate an RSA key pair locally, send
//! Apple only the public half inside a Certificate Signing Request, and
//! Apple returns a certificate that vouches for it. The private key never
//! leaves the machine that made it.
//!
//! Two consequences follow, and both are the reason a tool like this
//! exists:
//!
//! - Losing the private key makes its certificate permanently useless.
//!   Re-downloading from the developer portal cannot recover it, because
//!   Apple never had it.
//! - An expired certificate cannot be renewed. There is no such endpoint.
//!   You create a new one, which means a new key pair and a new CSR, and
//!   every provisioning profile embedding the old certificate has to be
//!   regenerated.
//!
//! See [`crate::csr`] for producing the `csr_content` that
//! [`CertificatesApi::create_certificate`] wants.

use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use smbcloud_ascapi_core::jsonapi::{CreateBody, CreateData, Document, ListDocument, Resource};
use smbcloud_ascapi_core::Client;
use smbcloud_ascapi_core::Result;

pub const RESOURCE_TYPE: &str = "certificates";

/// The kinds of certificate App Store Connect can issue.
///
/// `Development` and `Distribution` are the modern, platform-agnostic
/// pair: one Apple Development certificate covers iOS, macOS, tvOS, and
/// visionOS development, and one Apple Distribution certificate covers
/// distribution for all of them. The `Ios*` and `Mac*` variants are the
/// older per-platform types, still issued and still required in two
/// cases worth knowing:
///
/// - [`Self::MacInstallerDistribution`] signs the installer package for a
///   Mac App Store submission (`productbuild --sign`). Nothing else can,
///   so a Mac App Store release needs it alongside a distribution
///   certificate for the app bundle itself.
/// - [`Self::DeveloperIdApplication`] and [`Self::DeveloperIdKext`] are for
///   distribution outside the App Store, and are capped per team in a way
///   the others are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CertificateType {
    /// Apple Development: one certificate for development across every
    /// Apple platform.
    #[serde(rename = "DEVELOPMENT")]
    Development,
    /// Apple Distribution: one certificate for App Store and Ad Hoc
    /// distribution across every Apple platform.
    #[serde(rename = "DISTRIBUTION")]
    Distribution,
    #[serde(rename = "IOS_DEVELOPMENT")]
    IosDevelopment,
    #[serde(rename = "IOS_DISTRIBUTION")]
    IosDistribution,
    #[serde(rename = "MAC_APP_DEVELOPMENT")]
    MacAppDevelopment,
    /// "3rd Party Mac Developer Application": signs the `.app` bundle of a
    /// Mac App Store submission.
    #[serde(rename = "MAC_APP_DISTRIBUTION")]
    MacAppDistribution,
    /// "3rd Party Mac Developer Installer": signs the `.pkg` a Mac App
    /// Store submission is delivered in. Required by `productbuild
    /// --sign`; no other certificate type substitutes for it.
    #[serde(rename = "MAC_INSTALLER_DISTRIBUTION")]
    MacInstallerDistribution,
    #[serde(rename = "DEVELOPER_ID_APPLICATION")]
    DeveloperIdApplication,
    #[serde(rename = "DEVELOPER_ID_KEXT")]
    DeveloperIdKext,
}

impl CertificateType {
    /// The string App Store Connect uses, for building `filter[certificateType]`
    /// query values without going through serde.
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Development => "DEVELOPMENT",
            Self::Distribution => "DISTRIBUTION",
            Self::IosDevelopment => "IOS_DEVELOPMENT",
            Self::IosDistribution => "IOS_DISTRIBUTION",
            Self::MacAppDevelopment => "MAC_APP_DEVELOPMENT",
            Self::MacAppDistribution => "MAC_APP_DISTRIBUTION",
            Self::MacInstallerDistribution => "MAC_INSTALLER_DISTRIBUTION",
            Self::DeveloperIdApplication => "DEVELOPER_ID_APPLICATION",
            Self::DeveloperIdKext => "DEVELOPER_ID_KEXT",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateAttributes {
    /// The certificate's common name, e.g.
    /// `Apple Distribution: Splitfire AB (2TQF86ZACD)`. This is what
    /// `codesign --sign` matches when given a name rather than a hash.
    pub name: Option<String>,
    pub certificate_type: Option<CertificateType>,
    pub display_name: Option<String>,
    pub serial_number: Option<String>,
    pub platform: Option<String>,
    /// ISO-8601. Apple reports this directly, so callers never need to
    /// parse the DER to find out when a certificate dies.
    pub expiration_date: Option<String>,
    /// Base64-encoded DER of the certificate itself (the public half).
    /// Absent from some list responses; always present on create.
    pub certificate_content: Option<String>,
}

pub type Certificate = Resource<CertificateAttributes>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateCreateAttributes {
    /// PEM-encoded PKCS#10 Certificate Signing Request. Apple requires the
    /// key to be RSA 2048; see [`crate::csr`].
    pub csr_content: String,
    pub certificate_type: CertificateType,
}

/// Signing certificates.
///
/// An extension trait rather than inherent methods, because `Client`
/// lives in the core crate and Rust only allows inherent impls in the
/// crate that defines the type. Import it, or the crate's `prelude`,
/// to call these on a `Client`.
#[async_trait]
pub trait CertificatesApi {
    /// `GET /v1/certificates`, optionally filtered by type.
    ///
    /// Apple returns expired certificates too, and gives no filter for
    /// "valid" — read [`CertificateAttributes::expiration_date`] and decide
    /// for yourself. That is deliberate on their side: an expired
    /// certificate is still meaningful for identifying what a previously
    /// shipped build was signed with.
    async fn list_certificates(
        &self,
        filter_type: Option<CertificateType>,
    ) -> Result<Vec<Certificate>>;

    /// `POST /v1/certificates` — issues a new certificate for a CSR.
    ///
    /// The response carries `certificate_content`, the only copy of the
    /// signed certificate you are handed at creation time. It can be
    /// fetched again later with [`CertificatesApi::list_certificates`], but the
    /// private key it belongs to cannot, so store the two together
    /// immediately.
    ///
    /// Apple caps how many certificates of each type a team may hold at
    /// once (two for distribution). At the cap this returns a 409; revoke
    /// something with [`CertificatesApi::revoke_certificate`] first.
    async fn create_certificate(
        &self,
        attributes: CertificateCreateAttributes,
    ) -> Result<Certificate>;

    /// `DELETE /v1/certificates/{id}` — revokes a certificate.
    ///
    /// Irreversible, and wider-reaching than it looks: every provisioning
    /// profile that embeds this certificate stops working for everyone on
    /// the team at once, including builds already in CI. Revoking is for
    /// reclaiming a slot at Apple's per-type cap or for a key you believe
    /// is compromised, not for tidying up.
    async fn revoke_certificate(&self, id: &str) -> Result<()>;
}

#[async_trait]
impl CertificatesApi for Client {
    async fn list_certificates(
        &self,
        filter_type: Option<CertificateType>,
    ) -> Result<Vec<Certificate>> {
        let mut query = Vec::new();
        if let Some(certificate_type) = filter_type {
            query.push(("filter[certificateType]", certificate_type.as_api_str()));
        }
        // Apple's default page size is 20, which a team with several
        // platforms and a few years of history will exceed.
        query.push(("limit", "200"));

        let doc: ListDocument<CertificateAttributes> = self
            .request(Method::GET, "/v1/certificates", &query, None::<&()>)
            .await?;
        Ok(doc.data)
    }

    async fn create_certificate(
        &self,
        attributes: CertificateCreateAttributes,
    ) -> Result<Certificate> {
        let body = CreateBody {
            data: CreateData {
                resource_type: RESOURCE_TYPE,
                attributes,
                relationships: None::<()>,
            },
        };
        let doc: Document<CertificateAttributes> = self
            .request(Method::POST, "/v1/certificates", &[], Some(&body))
            .await?;
        Ok(doc.data)
    }

    async fn revoke_certificate(&self, id: &str) -> Result<()> {
        self.request_no_content(
            Method::DELETE,
            &format!("/v1/certificates/{id}"),
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
    fn certificate_type_round_trips_through_the_api_wire_format() {
        // The serde rename and as_api_str() are two separate spellings of
        // the same contract, and a filter built from the wrong one fails
        // silently by matching nothing.
        for certificate_type in [
            CertificateType::Development,
            CertificateType::Distribution,
            CertificateType::IosDevelopment,
            CertificateType::IosDistribution,
            CertificateType::MacAppDevelopment,
            CertificateType::MacAppDistribution,
            CertificateType::MacInstallerDistribution,
            CertificateType::DeveloperIdApplication,
            CertificateType::DeveloperIdKext,
        ] {
            let json = serde_json::to_string(&certificate_type).expect("serializes");
            let unquoted = json.trim_matches('"');
            assert_eq!(
                unquoted,
                certificate_type.as_api_str(),
                "serde and as_api_str disagree for {certificate_type:?}"
            );

            let parsed: CertificateType = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(parsed, certificate_type);
        }
    }
}
