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

    /// `DeveloperId` and `UserId` must be numbers; GetApps answers 999 otherwise.
    #[error("credential {0} must be a number")]
    NotNumeric(&'static str),

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to decode JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// A non-2xx HTTP status.
    #[error("GetApps returned HTTP {status}: {detail}")]
    HttpStatus { status: u16, detail: String },

    /// HTTP 200 with a `code` other than 200 in the envelope. 999 is a header
    /// or permission problem, 1001/1002/3001/3002 are signature problems.
    #[error("GetApps returned code {code}: {msg}")]
    Api { code: i64, msg: String },
}

pub type Result<T> = std::result::Result<T, Error>;
