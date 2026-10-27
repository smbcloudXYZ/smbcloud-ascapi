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
    #[error("the service account JSON is not valid or lacks key_id/private_key/sub_account")]
    ServiceAccountInvalid,

    #[error("failed to sign the service account JWT: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("failed to decode JSON response: {0}")]
    Json(#[from] serde_json::Error),

    #[error("AppGallery Connect API returned {status}: {detail}")]
    Api { status: u16, detail: String },

    /// HTTP 200 with a non-zero `ret.code` in the body.
    #[error("AppGallery Connect result code {code}: {msg}")]
    Result { code: i64, msg: String },

    #[error("{field} is {len} characters; the limit is {max}")]
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },

    #[error("{0} is required")]
    Missing(&'static str),

    #[error("reading {path}: {source}")]
    FileRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("the upload URL reply had no urlInfo")]
    NoUploadUrl,

    #[error("lang must not be empty")]
    EmptyLang,
}

pub type Result<T> = std::result::Result<T, Error>;
