//! Uploading images and packages: get a pre-signed URL, `PUT` the bytes to
//! it, then bind the returned object id to the app with `app-file-info`.

use std::collections::HashMap;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::client::AppGalleryClient;
use crate::error::{Error, Result};
use crate::publish::parse_ret;

/// `fileType` in `PUT app-file-info`; only the kinds this crate uploads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Icon,
    Screenshots,
    /// An APK, AAB or RPK.
    Package,
}

impl FileType {
    pub fn code(self) -> u8 {
        match self {
            Self::Icon => 0,
            Self::Screenshots => 2,
            Self::Package => 5,
        }
    }
}

/// `imgShowType`: how screenshots are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOrientation {
    Portrait,
    Landscape,
}

impl ImageOrientation {
    pub fn code(self) -> u8 {
        match self {
            Self::Portrait => 0,
            Self::Landscape => 1,
        }
    }
}

/// Where and how to `PUT` one file. The URL lives about five minutes.
#[derive(Debug, Clone, Deserialize)]
pub struct UploadUrl {
    #[serde(rename = "objectId")]
    pub object_id: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Deserialize)]
struct UploadUrlEnvelope {
    ret: crate::publish::RetCode,
    #[serde(rename = "urlInfo")]
    url_info: Option<UploadUrl>,
}

fn parse_upload_url(bytes: &[u8]) -> Result<UploadUrl> {
    let envelope: UploadUrlEnvelope = serde_json::from_slice(bytes)?;
    envelope.ret.check()?;
    envelope.url_info.ok_or(Error::NoUploadUrl)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    file_name: Option<&'a str>,
    file_dest_url: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileInfoBody<'a> {
    file_type: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    lang: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    img_show_type: Option<u8>,
    files: Vec<FileEntry<'a>>,
}

impl AppGalleryClient {
    /// `GET upload-url/for-obs`. `chinese_mainland` is sent as
    /// `chineseMainlandFlag`; the API makes it mandatory when the developer
    /// registered outside the Chinese mainland.
    pub async fn upload_url(
        &self,
        app_id: &str,
        file_name: &str,
        bytes: &[u8],
        chinese_mainland: Option<bool>,
    ) -> Result<UploadUrl> {
        let mut path = format!(
            "/publish/v2/upload-url/for-obs?appId={app_id}&fileName={}&contentLength={}&sha256={}",
            encode(file_name),
            bytes.len(),
            sha256_hex(bytes),
        );
        if let Some(flag) = chinese_mainland {
            path.push_str(&format!("&chineseMainlandFlag={}", u8::from(flag)));
        }
        let body = self.send(Method::GET, &path, None).await?.bytes().await?;
        parse_upload_url(&body)
    }

    /// `PUT` the bytes to a pre-signed URL with the headers it came with.
    /// Our JWT is not sent: the URL carries its own signature.
    pub async fn put_upload(&self, target: &UploadUrl, bytes: Vec<u8>) -> Result<()> {
        let mut request = self.http().put(&target.url);
        for (name, value) in &target.headers {
            // The host comes from the URL itself, and the length from the body.
            if name.eq_ignore_ascii_case("host") || name.eq_ignore_ascii_case("content-length") {
                continue;
            }
            request = request.header(name, value);
        }
        let response = request.body(bytes).send().await?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(Error::Api {
                status: status.as_u16(),
                detail: response.text().await.unwrap_or_default(),
            })
        }
    }

    /// `PUT app-file-info`: bind uploaded objects to the app. `lang` is
    /// required for images; `orientation` for screenshots. `files` pairs an
    /// optional file name (packages need it) with the object id.
    pub async fn bind_files(
        &self,
        app_id: &str,
        file_type: FileType,
        lang: Option<&str>,
        orientation: Option<ImageOrientation>,
        files: &[(Option<&str>, &str)],
    ) -> Result<()> {
        let body = serde_json::to_value(FileInfoBody {
            file_type: file_type.code(),
            lang,
            img_show_type: orientation.map(ImageOrientation::code),
            files: files
                .iter()
                .map(|(name, object)| FileEntry {
                    file_name: *name,
                    file_dest_url: object,
                })
                .collect(),
        })?;
        let path = format!("/publish/v2/app-file-info?appId={app_id}");
        let bytes = self
            .send(Method::PUT, &path, Some(&body))
            .await?
            .bytes()
            .await?;
        parse_ret(&bytes)
    }
}

/// Percent-encode a file name for a query string.
fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_type_and_orientation_codes_match_the_reference() {
        assert_eq!(FileType::Icon.code(), 0);
        assert_eq!(FileType::Screenshots.code(), 2);
        assert_eq!(FileType::Package.code(), 5);
        assert_eq!(ImageOrientation::Portrait.code(), 0);
        assert_eq!(ImageOrientation::Landscape.code(), 1);
    }

    #[test]
    fn sha256_hex_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn file_names_are_percent_encoded() {
        assert_eq!(encode("icon 216.png"), "icon%20216.png");
        assert_eq!(encode("a&b=c.png"), "a%26b%3Dc.png");
    }

    #[test]
    fn upload_url_parses_object_id_url_and_headers() {
        let json = br#"{"ret":{"code":0,"msg":"success"},"urlInfo":{"objectId":"abc.png",
            "url":"https://obs.example/abc.png","method":"PUT",
            "headers":{"Authorization":"x","Host":"obs.example"}}}"#;
        let url = parse_upload_url(json).unwrap();
        assert_eq!(url.object_id, "abc.png");
        assert_eq!(url.headers["Authorization"], "x");
        assert!(parse_upload_url(br#"{"ret":{"code":0,"msg":"ok"}}"#).is_err());
        assert!(parse_upload_url(br#"{"ret":{"code":9,"msg":"no"}}"#).is_err());
    }

    #[test]
    fn the_bind_body_carries_type_lang_orientation_and_object_ids() {
        let body = serde_json::to_value(FileInfoBody {
            file_type: 2,
            lang: Some("en-US"),
            img_show_type: Some(0),
            files: vec![FileEntry {
                file_name: None,
                file_dest_url: "obj1",
            }],
        })
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({"fileType":2,"lang":"en-US","imgShowType":0,
                               "files":[{"fileDestUrl":"obj1"}]})
        );
    }
}
