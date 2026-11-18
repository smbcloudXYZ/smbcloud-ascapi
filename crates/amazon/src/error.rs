use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read the credentials file {path}: {source}")]
    CredentialsRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Deliberately carries no part of the file: a parse error could
    /// otherwise quote the secret.
    #[error("the credentials JSON is not valid or lacks client_id/client_secret")]
    CredentialsInvalid,

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to decode JSON response: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Login with Amazon returned {status}: {detail}")]
    Token { status: u16, detail: String },

    #[error("Amazon Appstore API returned {status}: {detail}")]
    Api { status: u16, detail: String },

    #[error("reading {path}: {source}")]
    FileRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("the {0} response had no ETag header")]
    NoEtag(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;
