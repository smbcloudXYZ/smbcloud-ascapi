//! Apple code signing through App Store Connect.
//!
//! Two halves that only make sense together: [`csr`] generates an RSA key
//! pair and a signing request locally, and [`certificate`] asks Apple to
//! certify it.
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

pub mod prelude {
    pub use crate::certificate::CertificatesApi;
}
