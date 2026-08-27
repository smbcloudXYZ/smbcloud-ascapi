//! Apple code signing through App Store Connect.
//!
//! Three parts. [`csr`] generates an RSA key pair and a signing request
//! locally, [`certificate`] asks Apple to certify it, and [`profile`] binds
//! a certificate to an App ID so signing knows which entitlements are
//! allowed.
//!
//! The division of labour is the whole point. Apple never receives, and
//! never returns, a private key. You keep it; they vouch for its public
//! half. A certificate whose key file is lost is permanently unusable, and
//! an expired certificate cannot be renewed, only replaced by a new key
//! pair and a new request.
//!
//! The API calls are an extension trait on
//! [`Client`](smbcloud_ascapi_core::Client), since that type belongs to
//! the core crate. Import [`prelude`] to call them.

pub mod certificate;
pub mod csr;
pub mod profile;

pub mod prelude {
    pub use crate::certificate::CertificatesApi;
    pub use crate::profile::ProfilesApi;
}
