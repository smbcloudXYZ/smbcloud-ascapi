//! Operations shared by the `ascapi` command line and its MCP server.
//!
//! Both surfaces need the same things: resolve credentials, call the App
//! Store Connect client, and shape the result into something worth showing
//! a human or handing a model. Putting that here means the two front ends
//! cannot drift, and in particular that the ordering guarantee in
//! [`certificates::issue_certificate`] (private key written before the
//! network call) has exactly one implementation.
//!
//! Two rules the result types are built around, because one of the
//! consumers is a language model:
//!
//! - **No key material in a result.** [`certificates::IssuedCertificate`]
//!   carries the path a key was written to, never the key. Enforced by
//!   construction rather than filtered at the edge, since a filter is one
//!   refactor away from being bypassed.
//! - **No certificate bodies either.** They are large, never needed to
//!   decide what to do next, and would otherwise land in a transcript.

pub mod certificates;
pub mod enums;
pub mod env;
pub mod time;

pub use certificates::{
    certificate_type_from_str, issue_certificate, CertificateSummary, IssuedCertificate,
};
pub use enums::{bundle_id_platform_from_str, display_type_from_str, platform_from_str};
pub use env::api_key_from_env;
pub use time::{is_expired, now_iso8601};
