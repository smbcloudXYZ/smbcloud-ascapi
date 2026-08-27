use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read private key at {path}: {source}")]
    PrivateKeyRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to sign JWT: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to decode JSON response: {0}")]
    Json(#[from] serde_json::Error),

    #[error("App Store Connect API returned {status}: {detail}")]
    Api { status: u16, detail: String },

    #[error("failed to generate an RSA key pair: {0}")]
    KeyGeneration(String),

    #[error("failed to build the certificate signing request: {0}")]
    CsrGeneration(String),
}

pub type Result<T> = std::result::Result<T, Error>;
