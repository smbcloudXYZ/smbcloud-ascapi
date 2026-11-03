use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read the credentials file {path}: {source}")]
    EnvRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Names the key only, never a value.
    #[error("missing or empty credential {0}")]
    MissingCredential(&'static str),

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to decode JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Entra token request returned {status}: {detail}")]
    Token { status: u16, detail: String },

    #[error("Microsoft Store API returned {status}: {detail}")]
    Api { status: u16, detail: String },

    #[error("unexpected submission shape: {0}")]
    Shape(String),
}

pub type Result<T> = std::result::Result<T, Error>;
