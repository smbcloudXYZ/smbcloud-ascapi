use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read the OAuth client file {path}: {source}")]
    ClientRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Deliberately carries no part of the file: a parse error could
    /// otherwise quote the client secret.
    #[error("the OAuth client JSON is not valid or lacks client_id/client_secret (use a Desktop app client)")]
    ClientInvalid,

    #[error("no cached AdMob sign-in at {path}: run `asc admob login` first")]
    NotSignedIn { path: String },

    #[error("failed to read or write the token cache {path}: {source}")]
    TokenCache {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("the token cache {path} is not valid JSON; delete it and run `asc admob login`")]
    TokenCacheInvalid { path: String },

    #[error("sign-in failed: {0}")]
    Authorization(String),

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to decode JSON response: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Google OAuth token exchange returned {status}: {detail}")]
    Token { status: u16, detail: String },

    #[error(
        "Google rejected the cached sign-in (invalid_grant: revoked or expired). Delete {path} and \
         run `asc admob login` again"
    )]
    SignInExpired { path: String },

    #[error("AdMob API returned {status}: {detail}")]
    Api { status: u16, detail: String },

    #[error(
        "AdMob refused to create the ad unit (403). adUnits.create is limited access: Google must \
         grant it to your AdMob account. Create the unit in the console instead, then run \
         `asc admob adunits list` to read its ID. API detail: {detail}"
    )]
    CreateNotPermitted { detail: String },
}

pub type Result<T> = std::result::Result<T, Error>;
