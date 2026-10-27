//! Uploading images and packages: get a pre-signed URL, `PUT` the bytes to
//! it, then bind the returned object id to the app with `app-file-info`.

use std::collections::HashMap;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::client::AppGalleryClient;
use crate::error::{Error, Result};

/// `fileType` in `PUT app-file-info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// 0
    Icon,
    /// 1: up to three introduction videos, each followed by its poster.
    IntroVideo,
    /// 2
    Screenshots,
    /// 3: a promotion video followed by its poster.
    PromoVideo,
    /// 4
    PromoImage,
    /// 5: an APK, AAB or RPK.
    Package,
    /// 6: agent certificate or agent copyright image.
    AgentCertificate,
    /// 7: electronic certificate PDF, up to 5 MB.
    ElectronicCertificate,
    /// 8: Internet culture operation license screenshot (China only).
    CultureLicenseScreenshot,
    /// 9: Internet culture operation license image or PDF (games only).
    CultureLicense,
    /// 10
    VrCover,
    /// 11
    VrScreenshots,
    /// 12
    VrPromoImage,
    /// 13
    VrCoverLayered,
    /// 14
    VrPromo4x3,
    /// 15
    VrPromo1x1,
    /// 16
    VrPanorama,
}

impl FileType {
    pub const ALL: [FileType; 17] = [
        Self::Icon,
        Self::IntroVideo,
        Self::Screenshots,
        Self::PromoVideo,
        Self::PromoImage,
        Self::Package,
        Self::AgentCertificate,
        Self::ElectronicCertificate,
        Self::CultureLicenseScreenshot,
        Self::CultureLicense,
        Self::VrCover,
        Self::VrScreenshots,
        Self::VrPromoImage,
        Self::VrCoverLayered,
        Self::VrPromo4x3,
        Self::VrPromo1x1,
        Self::VrPanorama,
    ];

    pub fn code(self) -> u8 {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0) as u8
    }

    /// Images and videos belong to a language; packages and certificates
    /// do not.
    pub fn needs_lang(self) -> bool {
        !matches!(
            self,
            Self::Package
                | Self::AgentCertificate
                | Self::ElectronicCertificate
                | Self::CultureLicenseScreenshot
                | Self::CultureLicense
        )
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
    #[serde(skip_serializing_if = "Option::is_none")]
    video_show_type: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sensitive_permission_desc: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_type: Option<u8>,
    files: Vec<FileEntry<'a>>,
}

/// What `PUT app-file-info` binds. `files` pairs an optional file name
/// (packages need it) with the object id from [`UploadUrl`], in order.
#[derive(Debug, Clone)]
pub struct FileBind<'a> {
    pub file_type: FileType,
    /// Required for images and videos.
    pub lang: Option<&'a str>,
    /// Required for screenshots.
    pub image_orientation: Option<ImageOrientation>,
    /// Required for videos.
    pub video_orientation: Option<ImageOrientation>,
    pub sensitive_permission_desc: Option<&'a str>,
    /// Needed for visual assets when the app supports several device types
    /// (4 phone, 6 VR, 7 watch, 8 Vision, 9 router, 10 car).
    pub device_type: Option<u8>,
    pub files: Vec<(Option<&'a str>, &'a str)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindEnvelope {
    ret: crate::publish::RetCode,
    #[serde(default)]
    pkg_version: Vec<String>,
}

fn parse_bind(bytes: &[u8]) -> Result<Vec<String>> {
    let envelope: BindEnvelope = serde_json::from_slice(bytes)?;
    envelope.ret.check()?;
    Ok(envelope.pkg_version)
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

    /// `PUT app-file-info`: bind uploaded objects to the app. Returns the
    /// package id (`pkgVersion`) when a package was added. Binding an image
    /// or video for a language resets that language's text (seen
    /// 2026-10-27), so write text after images.
    pub async fn bind_files(&self, app_id: &str, bind: &FileBind<'_>) -> Result<Vec<String>> {
        if bind.files.is_empty() {
            return Err(Error::Missing("files"));
        }
        if bind.file_type.needs_lang() && bind.lang.is_none() {
            return Err(Error::Missing("lang"));
        }
        if bind.file_type == FileType::Screenshots && bind.image_orientation.is_none() {
            return Err(Error::Missing("imgShowType"));
        }
        if matches!(bind.file_type, FileType::IntroVideo | FileType::PromoVideo)
            && bind.video_orientation.is_none()
        {
            return Err(Error::Missing("videoShowType"));
        }
        if bind.file_type == FileType::Package && bind.files.iter().any(|(n, _)| n.is_none()) {
            return Err(Error::Missing("fileName"));
        }
        let body = serde_json::to_value(FileInfoBody {
            file_type: bind.file_type.code(),
            lang: bind.lang,
            img_show_type: bind.image_orientation.map(ImageOrientation::code),
            video_show_type: bind.video_orientation.map(ImageOrientation::code),
            sensitive_permission_desc: bind.sensitive_permission_desc,
            device_type: bind.device_type,
            files: bind
                .files
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
        parse_bind(&bytes)
    }
}

/// Percent-encode a file name for a query string.
pub(crate) fn encode(value: &str) -> String {
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
        assert_eq!(FileType::IntroVideo.code(), 1);
        assert_eq!(FileType::VrPanorama.code(), 16);
        assert!(!FileType::Package.needs_lang());
        assert!(FileType::PromoImage.needs_lang());
        assert_eq!(ImageOrientation::Portrait.code(), 0);
        assert_eq!(ImageOrientation::Landscape.code(), 1);
    }

    #[test]
    fn a_package_bind_returns_its_package_id() {
        let ids =
            parse_bind(br#"{"ret":{"code":0,"msg":"ok"},"pkgVersion":["10004151"]}"#).unwrap();
        assert_eq!(ids, vec!["10004151"]);
        assert!(parse_bind(br#"{"ret":{"code":0,"msg":"ok"}}"#)
            .unwrap()
            .is_empty());
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
            video_show_type: None,
            sensitive_permission_desc: None,
            device_type: None,
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
