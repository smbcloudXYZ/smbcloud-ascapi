use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read the service account file {path}: {source}")]
    ServiceAccountRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Deliberately carries no part of the file: a parse error could
    /// otherwise quote key material.
    #[error("the service account JSON is not valid or lacks client_email/private_key")]
    ServiceAccountInvalid,

    #[error("failed to sign the service account JWT: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to decode JSON response: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Google OAuth token exchange returned {status}: {detail}")]
    Token { status: u16, detail: String },

    #[error("Google Play API returned {status}: {detail}")]
    Api { status: u16, detail: String },
}

pub type Result<T> = std::result::Result<T, Error>;
