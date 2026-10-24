use serde::Deserialize;

/// The image buckets of a listing that this crate manages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    PhoneScreenshots,
    SevenInchScreenshots,
    TenInchScreenshots,
    FeatureGraphic,
    Icon,
}

impl ImageType {
    /// The path segment Play uses for this bucket.
    pub fn api_name(self) -> &'static str {
        match self {
            Self::PhoneScreenshots => "phoneScreenshots",
            Self::SevenInchScreenshots => "sevenInchScreenshots",
            Self::TenInchScreenshots => "tenInchScreenshots",
            Self::FeatureGraphic => "featureGraphic",
            Self::Icon => "icon",
        }
    }

    pub fn is_screenshot(self) -> bool {
        !matches!(self, Self::FeatureGraphic | Self::Icon)
    }

    /// Check an image's pixel size against Play's rules for this bucket.
    pub fn check_dimensions(self, width: u32, height: u32) -> std::result::Result<(), String> {
        match self {
            Self::FeatureGraphic if (width, height) != (1024, 500) => Err(format!(
                "feature graphic must be exactly 1024x500, got {width}x{height}"
            )),
            Self::Icon if (width, height) != (512, 512) => Err(format!(
                "icon must be exactly 512x512, got {width}x{height}"
            )),
            _ if self.is_screenshot() => {
                let (short, long) = (width.min(height), width.max(height));
                if short < 320 || long > 3840 {
                    Err(format!(
                        "screenshot sides must be between 320 and 3840 pixels, got {width}x{height}"
                    ))
                } else if long > short * 2 {
                    Err(format!(
                        "screenshot long side may be at most twice the short side, got {width}x{height}"
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

/// Width and height from a PNG's IHDR chunk, or `None` for anything else.
pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

/// One uploaded image as Play lists it.
#[derive(Debug, Clone, Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct Image {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn png_dimensions_read_the_ihdr_chunk() {
        assert_eq!(png_dimensions(&png_header(1080, 2400)), Some((1080, 2400)));
        assert_eq!(png_dimensions(b"not a png at all, but long enough"), None);
    }

    #[test]
    fn api_names_match_the_play_path_segments() {
        assert_eq!(ImageType::PhoneScreenshots.api_name(), "phoneScreenshots");
        assert_eq!(
            ImageType::SevenInchScreenshots.api_name(),
            "sevenInchScreenshots"
        );
        assert_eq!(
            ImageType::TenInchScreenshots.api_name(),
            "tenInchScreenshots"
        );
        assert_eq!(ImageType::FeatureGraphic.api_name(), "featureGraphic");
        assert_eq!(ImageType::Icon.api_name(), "icon");
    }

    #[test]
    fn feature_graphic_and_icon_need_exact_sizes() {
        assert!(ImageType::FeatureGraphic
            .check_dimensions(1024, 500)
            .is_ok());
        assert!(ImageType::FeatureGraphic
            .check_dimensions(1024, 501)
            .is_err());
        assert!(ImageType::Icon.check_dimensions(512, 512).is_ok());
        assert!(ImageType::Icon.check_dimensions(511, 512).is_err());
    }

    #[test]
    fn screenshots_are_bounded_in_size_and_aspect() {
        let phone = ImageType::PhoneScreenshots;
        assert!(phone.check_dimensions(1080, 2160).is_ok());
        assert!(phone.check_dimensions(1080, 2161).is_err());
        assert!(phone.check_dimensions(319, 600).is_err());
        assert!(phone.check_dimensions(1000, 3841).is_err());
    }
}
