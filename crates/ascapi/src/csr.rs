//! Certificate Signing Requests, and the key pairs behind them.
//!
//! A CSR is the only thing you ever send Apple when asking for a signing
//! certificate. It carries the *public* half of a freshly generated key
//! pair plus a subject name, self-signed to prove you hold the matching
//! private key. Apple checks that signature, then issues a certificate
//! binding your public key to your team identity.
//!
//! The private half never leaves this process except through
//! [`CertificateRequest::private_key_pem`], and it is the part that actually matters:
//! a certificate whose private key is lost is dead, permanently, because
//! Apple never had a copy to give back.
//!
//! # Why RSA 2048
//!
//! Apple's certificate service accepts RSA 2048 only. An ECDSA CSR, which
//! is what most modern tooling produces by default, is rejected at
//! submission. That single constraint dictates the whole implementation
//! here: [`rcgen`] cannot generate RSA keys, so the key comes from the
//! [`rsa`] crate and is handed to rcgen as an externally supplied key
//! pair purely for the CSR's self-signature.

use crate::error::{Error, Result};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair as RcgenKeyPair};
use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rsa::RsaPrivateKey;
use zeroize::Zeroizing;

/// Apple rejects anything else. Not a tunable.
const RSA_MODULUS_BITS: usize = 2048;

/// A freshly generated RSA key pair and the CSR that asks Apple to certify
/// it.
///
/// Keep these two together. The CSR is worthless without the key, the
/// certificate Apple returns is worthless without the key, and the key is
/// unrecoverable if dropped.
pub struct CertificateRequest {
    /// PEM-encoded PKCS#10, ready for
    /// [`CertificateCreateAttributes::csr_content`](crate::certificate::CertificateCreateAttributes::csr_content).
    csr_pem: String,
    /// PEM-encoded PKCS#8. Wrapped in [`Zeroizing`] so the bytes are
    /// wiped when dropped rather than left in freed heap for whatever
    /// allocates next.
    private_key_pem: Zeroizing<String>,
}

impl CertificateRequest {
    /// The CSR to send Apple, as PEM text including the
    /// `-----BEGIN CERTIFICATE REQUEST-----` armour.
    pub fn csr_pem(&self) -> &str {
        &self.csr_pem
    }

    /// The private key, PKCS#8 PEM. This is the secret: write it somewhere
    /// encrypted, and never log it, print it to stdout, or return it from
    /// an API a language model can read.
    pub fn private_key_pem(&self) -> &Zeroizing<String> {
        &self.private_key_pem
    }
}

/// Deliberately opaque: printing a key pair, even by accident through a
/// derived `Debug` on some struct three layers up, is how private keys end
/// up in CI logs.
impl std::fmt::Debug for CertificateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CertificateRequest")
            .field("csr_pem", &"<csr>")
            .field("private_key_pem", &"<redacted>")
            .finish()
    }
}

/// Generate an RSA 2048 key pair and a PKCS#10 CSR over it.
///
/// `common_name` becomes the CSR subject's CN. Apple overwrites the
/// subject on the certificate it issues (with `Apple Distribution: Team
/// Name (TEAMID)` or similar), so this value is not what you will see in
/// your keychain afterwards. It is still worth setting to something
/// identifying, because the CSR itself may be archived.
///
/// Generation is CPU-bound and takes a noticeable moment: finding two
/// 1024-bit primes is not free. Call it off the async executor if you are
/// in one.
pub fn generate_certificate_request(common_name: &str) -> Result<CertificateRequest> {
    let mut rng = rand::thread_rng();

    let private_key = RsaPrivateKey::new(&mut rng, RSA_MODULUS_BITS)
        .map_err(|source| Error::KeyGeneration(source.to_string()))?;

    let private_key_pem = private_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|source| Error::KeyGeneration(source.to_string()))?;

    // rcgen re-parses the PEM rather than taking the rsa crate's type
    // directly: the two crates share no key representation, and PKCS#8 PEM
    // is the interchange format both agree on.
    let rcgen_key = RcgenKeyPair::from_pem(&private_key_pem)
        .map_err(|source| Error::CsrGeneration(source.to_string()))?;

    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;

    let csr_pem = params
        .serialize_request(&rcgen_key)
        .map_err(|source| Error::CsrGeneration(source.to_string()))?
        .pem()
        .map_err(|source| Error::CsrGeneration(source.to_string()))?;

    Ok(CertificateRequest {
        csr_pem,
        private_key_pem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    /// Run `openssl` over a PEM on stdin and return stdout **and** stderr
    /// joined, or `None` if openssl isn't on this machine.
    ///
    /// Both streams, deliberately: macOS ships LibreSSL, which prints the
    /// `verify OK` banner to stderr while OpenSSL proper puts it on
    /// stdout. Reading only one stream makes this test pass or fail based
    /// on which implementation the machine happens to have.
    fn openssl(args: &[&str], stdin_pem: &str) -> Option<String> {
        let mut child = Command::new("openssl")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        child
            .stdin
            .as_mut()
            .expect("stdin piped")
            .write_all(stdin_pem.as_bytes())
            .expect("writes");
        let out = child.wait_with_output().expect("waits");
        assert!(
            out.status.success(),
            "openssl {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }

    #[test]
    fn csr_is_rsa_2048_and_self_signature_verifies() {
        let request = generate_certificate_request("smbCloud signing test").expect("generates");

        // Structural checks that hold with or without openssl present.
        assert!(request
            .csr_pem()
            .starts_with("-----BEGIN CERTIFICATE REQUEST-----"));
        assert!(request
            .private_key_pem()
            .starts_with("-----BEGIN PRIVATE KEY-----"));

        // The parts that actually decide whether Apple accepts this: the
        // key must be RSA 2048, and the CSR's own signature must verify.
        // Asserting on our own library's output would be circular, so this
        // asks a completely independent implementation.
        let Some(text) = openssl(&["req", "-noout", "-text", "-verify"], request.csr_pem()) else {
            eprintln!("openssl not available; skipped the cryptographic assertions");
            return;
        };

        assert!(
            text.contains("Public-Key: (2048 bit)"),
            "expected an RSA 2048 key, got:\n{text}"
        );
        assert!(
            text.contains("rsaEncryption"),
            "expected an RSA key; Apple rejects ECDSA CSRs. Got:\n{text}"
        );
        // openssl prints this only when the CSR's self-signature checks
        // out, which is the proof we hold the matching private key.
        assert!(
            text.contains("Certificate request self-signature verify OK")
                || text.contains("verify OK"),
            "CSR self-signature did not verify:\n{text}"
        );
        assert!(
            text.contains("smbCloud signing test"),
            "common name missing from subject:\n{text}"
        );
    }

    #[test]
    fn each_request_gets_a_fresh_key() {
        // Reusing a key across certificates means one compromise revokes
        // everything, and it is an easy mistake to make when adding
        // caching later.
        let a = generate_certificate_request("a").expect("generates");
        let b = generate_certificate_request("b").expect("generates");
        assert_ne!(
            a.private_key_pem().as_str(),
            b.private_key_pem().as_str(),
            "two requests shared a private key"
        );
    }

    #[test]
    fn debug_never_prints_key_material() {
        let request = generate_certificate_request("redaction check").expect("generates");
        let rendered = format!("{request:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(
            !rendered.contains("BEGIN PRIVATE KEY"),
            "Debug leaked the private key"
        );
    }
}
