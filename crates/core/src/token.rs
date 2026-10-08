//! The bearer-token abstraction the [`Client`](crate::Client) mints from.
//!
//! App Store Connect wants a short-lived ES256 JWT on every request. The
//! usual way to produce one is an [`ApiKey`](crate::ApiKey) signing its own
//! claims, but it isn't the only way: CI often injects a token minted
//! elsewhere, and tests want a fixed string. [`TokenSource`] is the seam
//! that lets all three flow through the same client without the client
//! caring which it holds.
//!
//! [`Client::new`](crate::Client::new) takes `impl IntoTokenSource`, so
//! `Client::new(api_key)` keeps working unchanged while a
//! [`StaticToken`] or any custom source is equally accepted.

use crate::auth::ApiKey;
use crate::error::Result;
use std::fmt;
use std::time::Duration;

/// A freshly minted bearer token and how long it stays valid.
///
/// The client caches the `value` and re-mints shortly before `lifetime`
/// elapses, so a source is free to return a long or short window as suits
/// it. Apple caps real JWTs at 20 minutes; a static token can claim more.
#[derive(Clone)]
pub struct MintedToken {
    /// The bearer token string, sent verbatim as `Authorization: Bearer`.
    pub value: String,
    /// How long the token remains valid from the moment it was minted.
    pub lifetime: Duration,
}

impl MintedToken {
    /// Convenience constructor from a token string and a lifetime in seconds.
    pub fn new(value: impl Into<String>, lifetime_secs: u64) -> Self {
        Self {
            value: value.into(),
            lifetime: Duration::from_secs(lifetime_secs),
        }
    }
}

impl fmt::Debug for MintedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the token material itself.
        f.debug_struct("MintedToken")
            .field("value", &"<redacted>")
            .field("lifetime", &self.lifetime)
            .finish()
    }
}

/// Anything that can produce a bearer token for the client.
///
/// Implementors must be `Send + Sync` because the client caches tokens
/// behind a mutex and is shared across tasks.
pub trait TokenSource: Send + Sync {
    /// Mint a fresh token. The client calls this lazily and caches the
    /// result until shortly before it expires, so this need not be cheap.
    fn mint(&self) -> Result<MintedToken>;

    /// A short, secret-free description for diagnostics and MCP
    /// transcripts. Must never contain key or token material — it may be
    /// logged or surfaced to a user. Defaults to the type's shape.
    fn describe(&self) -> String {
        "token source".to_string()
    }
}

impl fmt::Debug for dyn TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSource")
            .field("describe", &self.describe())
            .finish()
    }
}

impl TokenSource for ApiKey {
    fn mint(&self) -> Result<MintedToken> {
        Ok(MintedToken {
            value: self.token()?,
            lifetime: Duration::from_secs(self.lifetime_secs()),
        })
    }

    fn describe(&self) -> String {
        // key_id is an identifier, not a secret, and is invaluable when
        // diagnosing which of several keys a request was signed with.
        format!("api key {}", self.key_id())
    }
}

/// A fixed, already-minted bearer token.
///
/// For CI that injects a token from elsewhere, or tests that want a known
/// string. Reports a generous default lifetime so the client doesn't try
/// to re-mint it; override with [`StaticToken::with_lifetime_secs`].
#[derive(Clone)]
pub struct StaticToken {
    value: String,
    lifetime: Duration,
}

impl StaticToken {
    /// Wrap a pre-minted token string. Defaults to a 20 minute lifetime,
    /// matching Apple's ceiling for real JWTs.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            lifetime: Duration::from_secs(20 * 60),
        }
    }

    /// Override how long the client treats the token as valid.
    pub fn with_lifetime_secs(mut self, secs: u64) -> Self {
        self.lifetime = Duration::from_secs(secs);
        self
    }
}

impl fmt::Debug for StaticToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StaticToken")
            .field("value", &"<redacted>")
            .field("lifetime", &self.lifetime)
            .finish()
    }
}

impl TokenSource for StaticToken {
    fn mint(&self) -> Result<MintedToken> {
        Ok(MintedToken {
            value: self.value.clone(),
            lifetime: self.lifetime,
        })
    }

    fn describe(&self) -> String {
        "static token".to_string()
    }
}

/// Conversion into a boxed [`TokenSource`], so [`Client::new`](crate::Client::new)
/// can accept an [`ApiKey`], a [`StaticToken`], a `Box<dyn TokenSource>`, or
/// any custom source with one signature.
pub trait IntoTokenSource {
    fn into_token_source(self) -> Box<dyn TokenSource>;
}

impl<T: TokenSource + 'static> IntoTokenSource for T {
    fn into_token_source(self) -> Box<dyn TokenSource> {
        Box::new(self)
    }
}

impl IntoTokenSource for Box<dyn TokenSource> {
    fn into_token_source(self) -> Box<dyn TokenSource> {
        self
    }
}
