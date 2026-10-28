use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use smbcloud_ascapi_appgallery::{
    AppInfoFields as AppGalleryInfoFields, FileType as AppGalleryFileType,
    ImageOrientation as AppGalleryOrientation, LanguageFields as AppGalleryLanguageFields,
    PhasedRelease as AppGalleryPhasedRelease, ReleaseTimeChange as AppGalleryReleaseTimeChange,
    Submit as AppGallerySubmit,
};
use smbcloud_ascapi_aso::app::AppUpdateAttributes;
use smbcloud_ascapi_aso::app_info_localization::{
    AppInfoLocalizationCreateAttributes, AppInfoLocalizationUpdateAttributes,
};
use smbcloud_ascapi_aso::app_screenshot_set::{
    AppScreenshotSetCreateAttributes, ScreenshotDisplayType,
};
use smbcloud_ascapi_aso::app_store_review_detail::{AppStoreReviewDetailWrite, NOTES_MAX_LEN};
use smbcloud_ascapi_aso::app_store_version::{
    AppStoreVersionCreateAttributes, AppStoreVersionUpdateFields, Platform,
};
use smbcloud_ascapi_aso::app_store_version_localization::{
    AppStoreVersionLocalizationAttributes, AppStoreVersionLocalizationCreateAttributes,
    AppStoreVersionLocalizationFields,
};
use smbcloud_ascapi_aso::beta_app_review_detail::BetaAppReviewDetailUpdate;
use smbcloud_ascapi_aso::build::{BuildFilter, BuildUpdateAttributes};
use smbcloud_ascapi_aso::bundle_id::{BundleIdCreateAttributes, BundleIdPlatform};
use smbcloud_ascapi_aso::prelude::*;
use smbcloud_ascapi_core::{ApiKey, Client};
use smbcloud_ascapi_frontend::attach_build::attach_build;
use smbcloud_ascapi_frontend::await_processing::{await_processing, AwaitConfig};
use smbcloud_ascapi_frontend::review;
use smbcloud_ascapi_frontend::upload::{self, upload_package, validate_app, AltoolAuth, AltoolOp};
use smbcloud_ascapi_msstore::ListingFields as MsStoreListingFields;
use smbcloud_ascapi_play::{ImageType as PlayImageType, ListingFields as PlayListingFields};
use smbcloud_ascapi_pricing::app_price::PriceKind;
use smbcloud_ascapi_pricing::prelude::*;
use smbcloud_ascapi_reports::prelude::*;
use smbcloud_ascapi_reports::SalesReportRequest;
use smbcloud_ascapi_signing::certificate::{CertificateCreateAttributes, CertificateType};
use smbcloud_ascapi_signing::csr::generate_certificate_request;
use smbcloud_ascapi_signing::prelude::*;
use smbcloud_ascapi_signing::profile::{
    ProfileCreateAttributes, ProfileCreateRelationships, ProfileType,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

/// Add/update App Store Connect app metadata (apps, app infos, app store
/// versions, and their localizations) from the terminal or a script.
#[derive(Parser)]
#[command(name = "asc", version, about)]
struct Cli {
    /// App Store Connect API key ID (Users and Access → Integrations → App
    /// Store Connect API). Not required with `--mcp`, which resolves
    /// credentials per tool call so an unconfigured server can still list
    /// its tools.
    #[arg(long, env = "ASC_API_KEY")]
    key_id: Option<String>,

    /// App Store Connect API issuer ID (same page as the key).
    #[arg(long, env = "ASC_ISSUER_ID")]
    issuer_id: Option<String>,

    /// Run as an MCP server over stdio instead of executing a subcommand.
    ///
    /// stdout carries the JSON-RPC stream in this mode, so nothing else
    /// may be written to it.
    #[arg(long)]
    mcp: bool,

    /// Path to the key's .p8 private key file. Defaults to
    /// `~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8`, matching
    /// Xcode's own convention.
    #[arg(long, env = "ASC_PRIVATE_KEY_PATH", global = true)]
    private_key_path: Option<PathBuf>,

    /// Print the request body that would be sent instead of sending it.
    /// Only affects commands that create or modify data.
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum PlayCommand {
    /// Localized store listing text (title, short and full description).
    Listings {
        #[command(subcommand)]
        command: PlayListingsCommand,
    },
    /// Listing images: screenshots, feature graphic, icon.
    Images {
        #[command(subcommand)]
        command: PlayImagesCommand,
    },
    /// Release notes on a track release.
    Tracks {
        #[command(subcommand)]
        command: PlayTracksCommand,
    },
}

// The update commands carry many flags; boxing a clap subcommand field is not
// possible.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum AppGalleryCommand {
    /// Look up the AppGallery app id for a package name.
    AppId { package: String },
    /// Localized listing text: name, description, brief and release notes.
    Listings {
        #[command(subcommand)]
        command: AppGalleryListingsCommand,
    },
    /// Basic information: privacy links, category, countries, price and flags.
    Info {
        #[command(subcommand)]
        command: AppGalleryInfoCommand,
    },
    /// Upload and bind files: icon, screenshots, videos, promo images,
    /// certificates, VR assets, or the APK/AAB/RPK.
    #[command(alias = "images")]
    Files {
        #[command(subcommand)]
        command: AppGalleryFilesCommand,
    },
    /// Uploaded packages and whether they parsed.
    Packages {
        #[command(subcommand)]
        command: AppGalleryPackagesCommand,
    },
    /// Report whether the app depends on Google Mobile Services.
    Gms {
        app: String,
        /// true if the app needs GMS, false if it runs without it.
        #[arg(long, action = clap::ArgAction::Set)]
        needs_gms: bool,
    },
    /// Submit the app for review. Once approved it goes live (at
    /// --release-time if given). Check everything with --dry-run first.
    Submit {
        app: String,
        /// yyyy-MM-ddTHH:mm:ss+HHMM; released right after approval when unset.
        #[arg(long)]
        release_time: Option<String>,
        /// Note for the reviewer, 10-300 characters.
        #[arg(long, conflicts_with = "remark_file")]
        remark: Option<String>,
        #[arg(long)]
        remark_file: Option<PathBuf>,
        #[command(flatten)]
        phased: PhasedArgs,
    },
    /// Change a running phased release, or turn it into a full release.
    PhasedRelease {
        app: String,
        /// Release to the entire network instead; ignores the other flags.
        #[arg(long)]
        to_full: bool,
        /// SUSPEND or RELEASE.
        #[arg(long)]
        state: Option<String>,
        #[command(flatten)]
        phased: PhasedArgs,
    },
    /// Change when a version in Releasing state goes live.
    ReleaseTime {
        app: String,
        #[arg(long, conflicts_with = "at", required_unless_present = "at")]
        immediately: bool,
        /// yyyy-MM-ddTHH:mm:ss+HHMM
        #[arg(long)]
        at: Option<String>,
    },
}

/// The phased-release window. Setting any of these on `submit` makes it a
/// phased release, which needs a version already released.
#[derive(clap::Args)]
struct PhasedArgs {
    /// yyyy-MM-ddTHH:mm:ss+HHMM
    #[arg(long)]
    phased_start: Option<String>,
    /// yyyy-MM-ddTHH:mm:ss+HHMM
    #[arg(long)]
    phased_end: Option<String>,
    /// 0.00-100.00, no percent sign.
    #[arg(long)]
    phased_percent: Option<String>,
    /// Up to 500 characters.
    #[arg(long)]
    phased_description: Option<String>,
}

impl PhasedArgs {
    fn into_release(self, state: Option<String>) -> Option<AppGalleryPhasedRelease> {
        let any = self.phased_start.is_some()
            || self.phased_end.is_some()
            || self.phased_percent.is_some()
            || self.phased_description.is_some()
            || state.is_some();
        any.then_some(AppGalleryPhasedRelease {
            state,
            phased_release_start_time: self.phased_start,
            phased_release_end_time: self.phased_end,
            phased_release_percent: self.phased_percent,
            phased_release_description: self.phased_description,
        })
    }
}

#[derive(Subcommand)]
enum AppGalleryInfoCommand {
    /// Update basic information. The API needs --privacy-policy on every
    /// call; every other flag is left unchanged when omitted. Reads the
    /// privacy link back. With `--dry-run` nothing is sent.
    Update {
        app: String,
        #[arg(long)]
        privacy_policy: String,
        /// Default language, e.g. en-US.
        #[arg(long)]
        default_lang: Option<String>,
        /// Level-2 category id, e.g. 10000000 (Tools).
        #[arg(long)]
        category: Option<u32>,
        /// Level-3 category (tag) id, e.g. 15000005.
        #[arg(long)]
        category_tag: Option<u32>,
        /// Comma-separated country/region codes. Leave CN out to keep the
        /// app off the Chinese mainland.
        #[arg(long)]
        publish_country: Option<String>,
        /// 1 free, 0 paid (needs --price and --price-detail). Fixed after release.
        #[arg(long)]
        is_free: Option<u8>,
        /// {"countryCode":"price"}
        #[arg(long)]
        price: Option<String>,
        /// JSON array of {country, currency, price}.
        #[arg(long)]
        price_detail: Option<String>,
        /// Comma-separated in-app purchase types (1 activation ... 8 membership).
        #[arg(long)]
        tariff_type: Option<String>,
        #[arg(long)]
        developer_website: Option<String>,
        /// URL where users view, change and delete their personal data.
        #[arg(long)]
        privacy_rights_url: Option<String>,
        #[arg(long)]
        user_agreement_url: Option<String>,
        /// JSON, e.g. [{"deviceType":4,"appAdapters":"4,5"}] (4 phone, 5 tablet).
        #[arg(long)]
        device_types: Option<String>,
        /// 0 or 1.
        #[arg(long)]
        forced_update: Option<u8>,
        /// 0 or 1: let AppGallery fetch the package from the Internet.
        #[arg(long)]
        auto_download: Option<u8>,
        /// 0 or 1.
        #[arg(long)]
        family_share: Option<u8>,
        /// 0 or 1; the API defaults to 1.
        #[arg(long)]
        encrypted: Option<u8>,
        /// Games: 1 standalone, 2 online.
        #[arg(long)]
        net_type: Option<u8>,
        /// Games: 0 or 1.
        #[arg(long)]
        web_game: Option<u8>,
        /// Games: publication approval number.
        #[arg(long)]
        publication_number: Option<String>,
        /// Games, China only: Internet culture operation license number.
        #[arg(long)]
        culture_record_number: Option<String>,
        /// Trustlisted developers only.
        #[arg(long)]
        developer_name_cn: Option<String>,
        /// Trustlisted developers only.
        #[arg(long)]
        developer_name_en: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum CliAppGalleryFileType {
    Icon,
    Screenshots,
    IntroVideo,
    PromoVideo,
    PromoImage,
    Package,
    AgentCertificate,
    ElectronicCertificate,
    CultureLicenseScreenshot,
    CultureLicense,
    VrCover,
    VrScreenshots,
    VrPromoImage,
    VrCoverLayered,
    #[value(name = "vr-promo-4x3")]
    VrPromo4x3,
    #[value(name = "vr-promo-1x1")]
    VrPromo1x1,
    VrPanorama,
}

impl From<CliAppGalleryFileType> for AppGalleryFileType {
    fn from(value: CliAppGalleryFileType) -> Self {
        match value {
            CliAppGalleryFileType::Icon => Self::Icon,
            CliAppGalleryFileType::Screenshots => Self::Screenshots,
            CliAppGalleryFileType::IntroVideo => Self::IntroVideo,
            CliAppGalleryFileType::PromoVideo => Self::PromoVideo,
            CliAppGalleryFileType::PromoImage => Self::PromoImage,
            CliAppGalleryFileType::Package => Self::Package,
            CliAppGalleryFileType::AgentCertificate => Self::AgentCertificate,
            CliAppGalleryFileType::ElectronicCertificate => Self::ElectronicCertificate,
            CliAppGalleryFileType::CultureLicenseScreenshot => Self::CultureLicenseScreenshot,
            CliAppGalleryFileType::CultureLicense => Self::CultureLicense,
            CliAppGalleryFileType::VrCover => Self::VrCover,
            CliAppGalleryFileType::VrScreenshots => Self::VrScreenshots,
            CliAppGalleryFileType::VrPromoImage => Self::VrPromoImage,
            CliAppGalleryFileType::VrCoverLayered => Self::VrCoverLayered,
            CliAppGalleryFileType::VrPromo4x3 => Self::VrPromo4x3,
            CliAppGalleryFileType::VrPromo1x1 => Self::VrPromo1x1,
            CliAppGalleryFileType::VrPanorama => Self::VrPanorama,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum CliAppGalleryOrientation {
    Portrait,
    Landscape,
}

impl From<CliAppGalleryOrientation> for AppGalleryOrientation {
    fn from(value: CliAppGalleryOrientation) -> Self {
        match value {
            CliAppGalleryOrientation::Portrait => Self::Portrait,
            CliAppGalleryOrientation::Landscape => Self::Landscape,
        }
    }
}

#[derive(Subcommand)]
enum AppGalleryFilesCommand {
    /// Upload files of one type, in display order, and bind them. Images
    /// and videos need --lang; that language's text is checked and written
    /// back if binding cleared it. With `--dry-run` it lists each file's size
    /// and SHA-256 and sends nothing.
    Upload {
        app: String,
        #[arg(long = "type", alias = "kind", value_enum)]
        file_type: CliAppGalleryFileType,
        #[arg(long)]
        lang: Option<String>,
        /// Screenshots: portrait or landscape.
        #[arg(long, value_enum)]
        orientation: Option<CliAppGalleryOrientation>,
        /// Videos: portrait or landscape.
        #[arg(long, value_enum)]
        video_orientation: Option<CliAppGalleryOrientation>,
        /// Packages: sensitive permission description.
        #[arg(long)]
        sensitive_permission_desc: Option<String>,
        /// Device type for visual assets on a multi-device app (4 phone).
        #[arg(long)]
        device_type: Option<u8>,
        /// Distribute the package to the Chinese mainland. Off by default.
        #[arg(long)]
        chinese_mainland: bool,
        /// The files. Videos alternate with their posters: video, poster, ...
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AppGalleryPackagesCommand {
    /// Packages uploaded for the app, newest first.
    List {
        app: String,
        /// First record, from 1.
        #[arg(long)]
        from: Option<u32>,
        /// Up to 100 (the API's default is 10).
        #[arg(long)]
        max: Option<u32>,
    },
    /// Parse status of packages: 0 normal, 1 parsing, 2 failed.
    Status {
        app: String,
        /// Package ids (pkgVersion from `packages list` or the upload).
        #[arg(required = true)]
        pkg_ids: Vec<String>,
    },
}

#[derive(Subcommand)]
enum AppGalleryListingsCommand {
    /// App info and every language's text and assets, or one language's.
    /// `APP` is the numeric app id or the package name.
    List {
        app: String,
        #[arg(long)]
        lang: Option<String>,
    },
    /// Update one language's text (or add the language), then read it back.
    /// With `--dry-run` nothing is sent.
    Update {
        app: String,
        /// Language code such as en-US, zh-CN or id.
        #[arg(long)]
        lang: String,
        /// App name, up to 64 characters (required for a new language).
        #[arg(long, conflicts_with = "name_file")]
        name: Option<String>,
        #[arg(long)]
        name_file: Option<PathBuf>,
        /// Full description, up to 8000 characters.
        #[arg(long, conflicts_with = "description_file")]
        description: Option<String>,
        /// Read the description from a UTF-8 file; one trailing newline is dropped.
        #[arg(long)]
        description_file: Option<PathBuf>,
        /// Brief introduction, up to 80 characters.
        #[arg(long, conflicts_with = "brief_file")]
        brief: Option<String>,
        #[arg(long)]
        brief_file: Option<PathBuf>,
        /// Release notes, up to 500 characters.
        #[arg(long, conflicts_with = "release_notes_file")]
        release_notes: Option<String>,
        #[arg(long)]
        release_notes_file: Option<PathBuf>,
        /// 1 for the entire network (default) or 3 for a phased release.
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
        release_type: Option<u8>,
    },
    /// Delete one language's listing. The default language cannot be deleted.
    Delete {
        app: String,
        #[arg(long)]
        lang: String,
    },
}

#[derive(Subcommand)]
enum PlayListingsCommand {
    /// Every localized listing of an app.
    List { package: String },
    /// Patch one locale's listing, then read it back. With `--dry-run` the
    /// edit is opened, the body printed, and the edit deleted.
    Update {
        package: String,
        #[arg(long)]
        locale: String,
        /// Title, up to 30 characters.
        #[arg(long, conflicts_with = "title_file")]
        title: Option<String>,
        /// Read the title from a UTF-8 file; one trailing newline is dropped.
        #[arg(long)]
        title_file: Option<PathBuf>,
        /// Short description, up to 80 characters.
        #[arg(long, conflicts_with = "short_description_file")]
        short_description: Option<String>,
        /// Read the short description from a UTF-8 file.
        #[arg(long)]
        short_description_file: Option<PathBuf>,
        /// Full description, up to 4000 characters.
        #[arg(long, conflicts_with = "full_description_file")]
        full_description: Option<String>,
        /// Read the full description from a UTF-8 file.
        #[arg(long)]
        full_description_file: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum CliPlayImageType {
    PhoneScreenshots,
    SevenInchScreenshots,
    TenInchScreenshots,
    FeatureGraphic,
    Icon,
}

impl From<CliPlayImageType> for PlayImageType {
    fn from(value: CliPlayImageType) -> Self {
        match value {
            CliPlayImageType::PhoneScreenshots => Self::PhoneScreenshots,
            CliPlayImageType::SevenInchScreenshots => Self::SevenInchScreenshots,
            CliPlayImageType::TenInchScreenshots => Self::TenInchScreenshots,
            CliPlayImageType::FeatureGraphic => Self::FeatureGraphic,
            CliPlayImageType::Icon => Self::Icon,
        }
    }
}

#[derive(Subcommand)]
enum PlayImagesCommand {
    /// Images in one bucket of one locale.
    List {
        package: String,
        #[arg(long)]
        locale: String,
        #[arg(long = "type")]
        image_type: CliPlayImageType,
    },
    /// Replace a bucket with the given files, in order, in one edit: the old
    /// set is deleted and the new one uploaded, so a failed upload keeps the
    /// old set. Sizes are checked before anything is sent.
    Replace {
        package: String,
        #[arg(long)]
        locale: String,
        #[arg(long = "type")]
        image_type: CliPlayImageType,
        /// PNG or JPEG file; repeat for each image, in display order.
        #[arg(long = "file", required = true)]
        files: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum PlayTracksCommand {
    /// A track and its releases, with their release notes.
    Get {
        package: String,
        #[arg(long)]
        track: String,
    },
    /// Set one locale's release notes (up to 500 characters) on the release
    /// carrying a version code, then read the track back.
    SetNotes {
        package: String,
        #[arg(long)]
        track: String,
        #[arg(long)]
        version_code: String,
        #[arg(long)]
        locale: String,
        #[arg(
            long,
            conflicts_with = "notes_file",
            required_unless_present = "notes_file"
        )]
        notes: Option<String>,
        /// Read the notes from a UTF-8 file; one trailing newline is dropped.
        #[arg(long)]
        notes_file: Option<PathBuf>,
    },
    /// Promote a version code from one track to another, for example
    /// internal to production, in one edit. A staged rollout is the default
    /// shape: pass `--user-fraction 1.0` to release to everyone. With
    /// `--dry-run` the edit is deleted and the planned track is printed.
    Promote {
        package: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        version_code: String,
        /// Share of users to roll out to, above 0 and up to 1.0.
        #[arg(long, default_value_t = 1.0)]
        user_fraction: f64,
        /// Commit with changesNotSentForReview, for apps that have rejected
        /// changes pending.
        #[arg(long)]
        changes_not_sent_for_review: bool,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum MsStoreCommand {
    /// The app record, including its pending submission when a draft exists.
    App {
        /// Store product id; defaults to MSSTORE_PRODUCT_ID.
        #[arg(long)]
        product_id: Option<String>,
    },
    /// Submissions of the app.
    Submissions {
        #[command(subcommand)]
        command: MsStoreSubmissionsCommand,
    },
    /// Localized store listing text.
    Listings {
        #[command(subcommand)]
        command: MsStoreListingsCommand,
    },
}

#[derive(Subcommand)]
enum MsStoreSubmissionsCommand {
    /// Id and status of the pending submission, if any.
    Status {
        #[arg(long)]
        product_id: Option<String>,
    },
    /// The packages and listing locales of a submission: the pending one,
    /// else the last published one, unless `--submission-id` names one.
    Get {
        #[arg(long)]
        product_id: Option<String>,
        #[arg(long)]
        submission_id: Option<String>,
    },
    /// Send the pending submission to certification, then wait for the
    /// status to leave CommitStarted. A submission already committed is a
    /// no-op. With `--dry-run` the plan is printed and nothing is sent.
    Commit {
        #[arg(long)]
        product_id: Option<String>,
        /// Refuse unless a shipping package has this version, e.g. 1.3.2.0.
        #[arg(long)]
        expect_package_version: Option<String>,
        /// How long to wait for the status to leave CommitStarted.
        #[arg(long, default_value_t = 120)]
        wait_seconds: u64,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum MsStoreListingsCommand {
    /// One locale's listing text, from the pending submission or, with no
    /// draft, the last published one.
    Get {
        #[arg(long)]
        product_id: Option<String>,
        #[arg(long)]
        locale: String,
    },
    /// Write listing text into the pending submission (one is created only
    /// when none exists), then read it back. Never commits to certification.
    /// With `--dry-run` the diff is printed and nothing is written.
    Update {
        #[arg(long)]
        product_id: Option<String>,
        #[arg(long)]
        locale: String,
        /// Title, up to 256 characters.
        #[arg(long, conflicts_with = "title_file")]
        title: Option<String>,
        /// Read the title from a UTF-8 file; one trailing newline is dropped.
        #[arg(long)]
        title_file: Option<PathBuf>,
        /// Short description, up to 1000 characters.
        #[arg(long, conflicts_with = "short_description_file")]
        short_description: Option<String>,
        #[arg(long)]
        short_description_file: Option<PathBuf>,
        /// Description, up to 10000 characters.
        #[arg(long, conflicts_with = "description_file")]
        description: Option<String>,
        #[arg(long)]
        description_file: Option<PathBuf>,
        /// Release notes, up to 1500 characters.
        #[arg(long, conflicts_with = "release_notes_file")]
        release_notes: Option<String>,
        #[arg(long)]
        release_notes_file: Option<PathBuf>,
        /// UTF-8 file with one feature per line (up to 20, 200 characters each).
        #[arg(long)]
        features_file: Option<PathBuf>,
        /// UTF-8 file with one search term per line (up to 7, 30 characters
        /// and 21 words in total).
        #[arg(long)]
        keywords_file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum Command {
    /// Huawei AppGallery listing text. Uses an AppGallery Connect Service
    /// Account, not the App Store Connect key.
    #[command(name = "appgallery")]
    AppGallery {
        /// Path to the Service Account credential file (`*private.json`).
        /// Falls back to `APPGALLERY_SERVICE_ACCOUNT_JSON_PATH`, then to the
        /// contents in `APPGALLERY_SERVICE_ACCOUNT_JSON`.
        #[arg(long, global = true)]
        service_account: Option<PathBuf>,
        /// Sent as the `client_id` header when set. Falls back to
        /// `APPGALLERY_CLIENT_ID`.
        #[arg(long, global = true)]
        client_id: Option<String>,
        #[command(subcommand)]
        command: AppGalleryCommand,
    },
    /// Google Play store listings: text, images, and release notes. Uses a
    /// Play service account, not the App Store Connect key.
    Play {
        /// Path to the Play service-account JSON key. Falls back to
        /// `PLAY_SERVICE_ACCOUNT_JSON_PATH`, then to the key contents in
        /// `PLAY_SERVICE_ACCOUNT_JSON`.
        #[arg(long, global = true)]
        play_service_account: Option<PathBuf>,
        #[command(subcommand)]
        command: PlayCommand,
    },
    /// Microsoft Store listings: text on the app's pending submission. Uses
    /// Partner Center API credentials, not the App Store Connect key.
    Msstore {
        /// Dotenv file with the MSSTORE_* credentials. Falls back to
        /// `MSSTORE_ENV_PATH`, then to the MSSTORE_* variables themselves.
        #[arg(long, global = true)]
        msstore_env: Option<PathBuf>,
        #[command(subcommand)]
        command: MsStoreCommand,
    },
    /// Look up apps by bundle ID, or a specific app by its ASC app ID.
    Apps {
        #[command(subcommand)]
        command: AppsCommand,
    },
    /// Register or look up bundle identifiers.
    BundleIds {
        #[command(subcommand)]
        command: BundleIdsCommand,
    },
    /// Per-platform App Store Version records for an app — create one to
    /// add a new platform (e.g. visionOS) to an existing app.
    AppStoreVersions {
        #[command(subcommand)]
        command: AppStoreVersionsCommand,
    },
    /// The App Review Information of an App Store Version: contact, demo
    /// account and notes for the reviewer.
    AppStoreReviewDetails {
        #[command(subcommand)]
        command: AppStoreReviewDetailsCommand,
    },
    /// Send an App Store Version to App Review, see what's waiting, or
    /// withdraw a submission that hasn't been reviewed yet.
    ReviewSubmissions {
        #[command(subcommand)]
        command: ReviewSubmissionsCommand,
    },
    /// Localized name/subtitle for an app's AppInfo.
    AppInfoLocalizations {
        #[command(subcommand)]
        command: AppInfoLocalizationsCommand,
    },
    /// Localized description/keywords/etc for an App Store Version.
    AppStoreVersionLocalizations {
        #[command(subcommand)]
        command: AppStoreVersionLocalizationsCommand,
    },
    /// Per-device-class screenshot buckets on an App Store Version
    /// Localization — create one, then add images to it with
    /// `app-screenshots upload`.
    AppScreenshotSets {
        #[command(subcommand)]
        command: AppScreenshotSetsCommand,
    },
    /// Screenshot image binaries within an AppScreenshotSet.
    AppScreenshots {
        #[command(subcommand)]
        command: AppScreenshotsCommand,
    },
    /// What an app costs, per territory — the manual price a human set
    /// and the automatic ones Apple converted it into.
    AppPrices {
        #[command(subcommand)]
        command: AppPricesCommand,
    },
    /// Signing certificates: list what the team holds, issue a new one, or
    /// revoke an old one.
    Certificates {
        #[command(subcommand)]
        command: CertificatesCommand,
    },
    /// Provisioning profiles: what a bundle ID is allowed to claim, and
    /// which certificates may sign it. Create a new one after changing an
    /// App ID's capabilities — existing profiles never pick the change up.
    Profiles {
        #[command(subcommand)]
        command: ProfilesCommand,
    },
    /// Download account-level App Store sales reports as JSON.
    SalesReports {
        #[command(subcommand)]
        command: SalesReportsCommand,
    },
    /// Manage TestFlight beta testing: groups, review details, and review submissions.
    Testflight {
        #[command(subcommand)]
        command: TestflightCommand,
    },
    /// Local signing health checks that compare this machine against App
    /// Store Connect. macOS only — they read the login keychain.
    #[cfg(target_os = "macos")]
    Signing {
        #[command(subcommand)]
        command: SigningCommand,
    },
}

async fn run_appgallery(
    service_account: Option<&std::path::Path>,
    client_id: Option<&str>,
    command: AppGalleryCommand,
    dry_run: bool,
) -> Result<()> {
    use smbcloud_ascapi_frontend::appgallery;
    let client = appgallery::appgallery_client(service_account, client_id)
        .map_err(|e| anyhow::anyhow!(e))?;
    let fail = |e: String| anyhow::anyhow!(e);
    let flag = |label: &str, dry: bool| {
        if dry {
            println!("# dry run: nothing was sent; {label}:");
        }
    };
    match command {
        AppGalleryCommand::AppId { package } => print_json(
            &client
                .app_ids_for_packages(&[package.as_str()])
                .await
                .map_err(|e| anyhow::anyhow!(e))?,
        ),
        AppGalleryCommand::Info { command } => match command {
            AppGalleryInfoCommand::Update {
                app,
                privacy_policy,
                default_lang,
                category,
                category_tag,
                publish_country,
                is_free,
                price,
                price_detail,
                tariff_type,
                developer_website,
                privacy_rights_url,
                user_agreement_url,
                device_types,
                forced_update,
                auto_download,
                family_share,
                encrypted,
                net_type,
                web_game,
                publication_number,
                culture_record_number,
                developer_name_cn,
                developer_name_en,
            } => {
                let device_types = device_types
                    .map(|json| serde_json::from_str(&json))
                    .transpose()
                    .context("--device-types must be JSON")?;
                let fields = AppGalleryInfoFields {
                    privacy_policy,
                    default_lang,
                    child_type: category,
                    grand_child_type: category_tag,
                    app_net_type: net_type,
                    is_free,
                    price,
                    price_detail,
                    publish_country,
                    is_app_forced_update: forced_update,
                    hispace_auto_down: auto_download,
                    app_tariff_type: tariff_type,
                    publication_number,
                    culture_record_number,
                    developer_name_cn,
                    developer_name_en,
                    developer_website,
                    family_share_tag: family_share,
                    device_types,
                    web_game_flag: web_game,
                    privacy_rights_url,
                    user_agreement_url,
                    encrypted,
                };
                let result = appgallery::update_basic_info(&client, &app, fields, None, dry_run)
                    .await
                    .map_err(fail)?;
                flag("body that would be sent", result.dry_run);
                print_json(&result)
            }
        },
        AppGalleryCommand::Files { command } => match command {
            AppGalleryFilesCommand::Upload {
                app,
                file_type,
                lang,
                orientation,
                video_orientation,
                sensitive_permission_desc,
                device_type,
                chinese_mainland,
                files,
            } => {
                let options = appgallery::UploadOptions {
                    lang,
                    image_orientation: orientation.map(Into::into),
                    video_orientation: video_orientation.map(Into::into),
                    sensitive_permission_desc,
                    device_type,
                    chinese_mainland,
                };
                let result = appgallery::upload_files(
                    &client,
                    &app,
                    file_type.into(),
                    &files,
                    &options,
                    dry_run,
                )
                .await
                .map_err(fail)?;
                flag("files that would be uploaded", result.dry_run);
                print_json(&result)
            }
        },
        AppGalleryCommand::Packages { command } => match command {
            AppGalleryPackagesCommand::List { app, from, max } => print_json(
                &appgallery::package_list(&client, &app, from, max)
                    .await
                    .map_err(fail)?,
            ),
            AppGalleryPackagesCommand::Status { app, pkg_ids } => print_json(
                &appgallery::package_status(&client, &app, &pkg_ids)
                    .await
                    .map_err(fail)?,
            ),
        },
        AppGalleryCommand::Gms { app, needs_gms } => {
            let result = appgallery::set_gms_dependency(&client, &app, needs_gms, dry_run)
                .await
                .map_err(fail)?;
            flag("body that would be sent", result.dry_run);
            print_json(&result)
        }
        AppGalleryCommand::Submit {
            app,
            release_time,
            remark,
            remark_file,
            phased,
        } => {
            let options = AppGallerySubmit {
                release_time,
                remark: text_arg(remark, remark_file, "--remark-file")?,
                phased: phased.into_release(None),
            };
            let result = appgallery::submit(&client, &app, options, dry_run)
                .await
                .map_err(fail)?;
            flag("submission that would be sent", result.dry_run);
            print_json(&result)
        }
        AppGalleryCommand::PhasedRelease {
            app,
            to_full,
            state,
            phased,
        } => {
            let release = phased.into_release(state).unwrap_or_default();
            let result =
                appgallery::update_phased_release(&client, &app, release, to_full, dry_run)
                    .await
                    .map_err(fail)?;
            flag("body that would be sent", result.dry_run);
            print_json(&result)
        }
        AppGalleryCommand::ReleaseTime {
            app,
            immediately,
            at,
        } => {
            let change = match (immediately, at) {
                (true, _) => AppGalleryReleaseTimeChange::Immediately,
                (false, Some(at)) => AppGalleryReleaseTimeChange::At(at),
                (false, None) => anyhow::bail!("pass --immediately or --at"),
            };
            let result = appgallery::update_release_time(&client, &app, change, dry_run)
                .await
                .map_err(fail)?;
            flag("body that would be sent", result.dry_run);
            print_json(&result)
        }
        AppGalleryCommand::Listings { command } => match command {
            AppGalleryListingsCommand::List { app, lang } => print_json(
                &appgallery::read_listings(&client, &app, lang.as_deref())
                    .await
                    .map_err(fail)?,
            ),
            AppGalleryListingsCommand::Update {
                app,
                lang,
                name,
                name_file,
                description,
                description_file,
                brief,
                brief_file,
                release_notes,
                release_notes_file,
                release_type,
            } => {
                let fields = AppGalleryLanguageFields {
                    lang,
                    app_name: text_arg(name, name_file, "--name-file")?,
                    app_desc: text_arg(description, description_file, "--description-file")?,
                    brief_info: text_arg(brief, brief_file, "--brief-file")?,
                    new_features: text_arg(
                        release_notes,
                        release_notes_file,
                        "--release-notes-file",
                    )?,
                };
                let result =
                    appgallery::update_language(&client, &app, fields, release_type, dry_run)
                        .await
                        .map_err(fail)?;
                flag("body that would be sent", result.dry_run);
                print_json(&result)
            }
            AppGalleryListingsCommand::Delete { app, lang } => {
                let result = appgallery::delete_language(&client, &app, &lang, dry_run)
                    .await
                    .map_err(fail)?;
                print_json(&result)
            }
        },
    }
}

/// One non-empty trimmed line per entry, from a UTF-8 file.
fn lines_arg(file: Option<PathBuf>, flag: &str) -> Result<Option<Vec<String>>> {
    let Some(path) = file else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("{flag}: cannot read {} as UTF-8 text", path.display()))?;
    Ok(Some(
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
    ))
}

async fn run_msstore(
    env_path: Option<&std::path::Path>,
    command: MsStoreCommand,
    dry_run: bool,
) -> Result<()> {
    use smbcloud_ascapi_frontend::msstore;
    let client = msstore::msstore_client(env_path).map_err(|e| anyhow::anyhow!(e))?;
    let fail = |e: String| anyhow::anyhow!(e);
    match command {
        MsStoreCommand::App { product_id } => print_json(
            &msstore::read_app(&client, product_id.as_deref())
                .await
                .map_err(fail)?,
        ),
        MsStoreCommand::Submissions { command } => match command {
            MsStoreSubmissionsCommand::Status { product_id } => print_json(
                &msstore::submission_status(&client, product_id.as_deref())
                    .await
                    .map_err(fail)?,
            ),
            MsStoreSubmissionsCommand::Commit {
                product_id,
                expect_package_version,
                wait_seconds,
            } => {
                let result = msstore::commit_pending(
                    &client,
                    product_id.as_deref(),
                    expect_package_version.as_deref(),
                    wait_seconds,
                    dry_run,
                )
                .await
                .map_err(fail)?;
                if result.dry_run {
                    println!("# dry run: nothing was committed; plan:");
                }
                print_json(&result)
            }
            MsStoreSubmissionsCommand::Get {
                product_id,
                submission_id,
            } => print_json(
                &msstore::read_submission(&client, product_id.as_deref(), submission_id.as_deref())
                    .await
                    .map_err(fail)?,
            ),
        },
        MsStoreCommand::Listings { command } => match command {
            MsStoreListingsCommand::Get { product_id, locale } => print_json(
                &msstore::read_listing(&client, product_id.as_deref(), &locale)
                    .await
                    .map_err(fail)?,
            ),
            MsStoreListingsCommand::Update {
                product_id,
                locale,
                title,
                title_file,
                short_description,
                short_description_file,
                description,
                description_file,
                release_notes,
                release_notes_file,
                features_file,
                keywords_file,
            } => {
                let fields = MsStoreListingFields {
                    title: text_arg(title, title_file, "--title-file")?,
                    short_description: text_arg(
                        short_description,
                        short_description_file,
                        "--short-description-file",
                    )?,
                    description: text_arg(description, description_file, "--description-file")?,
                    release_notes: text_arg(
                        release_notes,
                        release_notes_file,
                        "--release-notes-file",
                    )?,
                    features: lines_arg(features_file, "--features-file")?,
                    keywords: lines_arg(keywords_file, "--keywords-file")?,
                };
                let result = msstore::update_listing(
                    &client,
                    product_id.as_deref(),
                    &locale,
                    fields,
                    dry_run,
                )
                .await
                .map_err(fail)?;
                for warning in &result.warnings {
                    eprintln!("warning: {warning}");
                }
                if result.dry_run {
                    println!("# dry run: nothing was written; changes that would be made:");
                }
                print_json(&result)
            }
        },
    }
}

async fn run_play(
    service_account: Option<&std::path::Path>,
    command: PlayCommand,
    dry_run: bool,
) -> Result<()> {
    use smbcloud_ascapi_frontend::play;
    let client = play::play_client(service_account).map_err(|e| anyhow::anyhow!(e))?;
    let fail = |e: String| anyhow::anyhow!(e);
    match command {
        PlayCommand::Listings { command } => match command {
            PlayListingsCommand::List { package } => {
                print_json(&play::read_listings(&client, &package).await.map_err(fail)?)
            }
            PlayListingsCommand::Update {
                package,
                locale,
                title,
                title_file,
                short_description,
                short_description_file,
                full_description,
                full_description_file,
            } => {
                let fields = PlayListingFields {
                    title: text_arg(title, title_file, "--title-file")?,
                    short_description: text_arg(
                        short_description,
                        short_description_file,
                        "--short-description-file",
                    )?,
                    full_description: text_arg(
                        full_description,
                        full_description_file,
                        "--full-description-file",
                    )?,
                };
                let result = play::update_listing(&client, &package, &locale, fields, dry_run)
                    .await
                    .map_err(fail)?;
                if result.dry_run {
                    println!("# dry run: nothing was committed; body that would be sent:");
                }
                print_json(&result)
            }
        },
        PlayCommand::Images { command } => match command {
            PlayImagesCommand::List {
                package,
                locale,
                image_type,
            } => print_json(
                &play::read_images(&client, &package, &locale, image_type.into())
                    .await
                    .map_err(fail)?,
            ),
            PlayImagesCommand::Replace {
                package,
                locale,
                image_type,
                files,
            } => {
                let result = play::replace_images(
                    &client,
                    &package,
                    &locale,
                    image_type.into(),
                    &files,
                    dry_run,
                )
                .await
                .map_err(fail)?;
                if result.dry_run {
                    println!("# dry run: nothing was committed; images that would be uploaded:");
                }
                print_json(&result)
            }
        },
        PlayCommand::Tracks { command } => match command {
            PlayTracksCommand::Get { package, track } => print_json(
                &play::read_track(&client, &package, &track)
                    .await
                    .map_err(fail)?,
            ),
            PlayTracksCommand::SetNotes {
                package,
                track,
                version_code,
                locale,
                notes,
                notes_file,
            } => {
                let notes = text_arg(notes, notes_file, "--notes-file")?
                    .context("--notes or --notes-file is required")?;
                let result = play::set_track_notes(
                    &client,
                    &package,
                    &track,
                    &version_code,
                    &locale,
                    &notes,
                    dry_run,
                )
                .await
                .map_err(fail)?;
                if result.dry_run {
                    println!("# dry run: nothing was committed; track as it would be:");
                }
                print_json(&result)
            }
            PlayTracksCommand::Promote {
                package,
                from,
                to,
                version_code,
                user_fraction,
                changes_not_sent_for_review,
            } => {
                let result = play::promote_track(
                    &client,
                    &package,
                    &from,
                    &to,
                    &version_code,
                    user_fraction,
                    changes_not_sent_for_review,
                    dry_run,
                )
                .await
                .map_err(fail)?;
                if result.dry_run {
                    println!("# dry run: the edit was deleted; target track as it would be:");
                }
                print_json(&result)
            }
        },
    }
}

/// macOS-only signing diagnostics.
#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum SigningCommand {
    /// Cross-check the login keychain against Apple's certificates and
    /// report which of Apple's certificates this machine can actually sign
    /// with — i.e. which ones have their private key present locally.
    ///
    /// Read-only: it lists certificates and inspects the keychain, and
    /// never writes or transmits key material.
    Doctor,
}

#[derive(Subcommand)]
enum TestflightCommand {
    /// Manage TestFlight beta groups.
    Groups {
        #[command(subcommand)]
        command: BetaGroupsCommand,
    },
    /// Manage the Beta App Review contact details for an app.
    AppReviewDetail {
        #[command(subcommand)]
        command: BetaAppReviewDetailCommand,
    },
    /// Manage Beta App Review submissions for a build.
    AppReviewSubmissions {
        #[command(subcommand)]
        command: BetaAppReviewSubmissionsCommand,
    },
    /// Manage the per-locale "What to Test" notes testers see on a build.
    BuildLocalizations {
        #[command(subcommand)]
        command: BetaBuildLocalizationsCommand,
    },
}

#[derive(Subcommand)]
enum BetaBuildLocalizationsCommand {
    /// `GET /v1/betaBuildLocalizations?filter[build]={build_id}` — every
    /// locale's note for a build (find the build with `apps builds`).
    List {
        /// The ASC build ID.
        build_id: String,
    },
    /// `POST /v1/betaBuildLocalizations` — add a locale's note to a build.
    /// A build holds one note per locale; use `update` if the locale already
    /// has one.
    #[command(group(clap::ArgGroup::new("text").required(true).args(["whats_new", "whats_new_file"])))]
    Create {
        /// The ASC build ID.
        build_id: String,
        /// BCP 47 locale tag, e.g. `en-US` or `id`.
        #[arg(long)]
        locale: String,
        /// The "What to Test" text (up to 4000 characters).
        #[arg(long)]
        whats_new: Option<String>,
        /// Read the "What to Test" text from a UTF-8 file instead of
        /// `--whats-new`; one trailing newline is dropped.
        #[arg(long)]
        whats_new_file: Option<PathBuf>,
    },
    /// `PATCH /v1/betaBuildLocalizations/{id}` — replace the text of an
    /// existing note (the id comes from `list`).
    #[command(group(clap::ArgGroup::new("text").required(true).args(["whats_new", "whats_new_file"])))]
    Update {
        /// The ASC beta build localization ID.
        id: String,
        /// The "What to Test" text (up to 4000 characters).
        #[arg(long)]
        whats_new: Option<String>,
        /// Read the "What to Test" text from a UTF-8 file instead of
        /// `--whats-new`; one trailing newline is dropped.
        #[arg(long)]
        whats_new_file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum BetaGroupsCommand {
    /// List all beta groups for an app.
    List {
        /// The ASC app ID to list groups for.
        app_id: String,
    },
    /// Fetch a single beta group by its ASC ID.
    Get {
        /// The ASC beta group ID.
        group_id: String,
    },
    /// Add one or more builds to a beta group.
    ///
    /// Adding a build to an external group may trigger Beta App Review if the
    /// app has not received approval for that group yet. This command is
    /// CLI-only for that reason.
    AddBuild {
        /// The ASC beta group ID.
        group_id: String,
        /// One or more ASC build IDs to add to the group.
        #[arg(long = "build-id", required = true, num_args = 1..)]
        build_ids: Vec<String>,
    },
}

#[derive(Subcommand)]
enum AppStoreReviewDetailsCommand {
    /// Show the review information of a version, or `null` when it has none.
    Get {
        /// The App Store Version id.
        app_store_version_id: String,
    },
    /// Set the review information of a version. Creates it when the version
    /// has none. Only supplied flags are sent. Each `--<field>-file` conflicts
    /// with its inline flag.
    Set(Box<ReviewDetailSetArgs>),
}

#[derive(clap::Args)]
struct ReviewDetailSetArgs {
    /// The App Store Version id.
    app_store_version_id: String,
    #[arg(long)]
    contact_first_name: Option<String>,
    #[arg(long)]
    contact_last_name: Option<String>,
    #[arg(long)]
    contact_phone: Option<String>,
    #[arg(long)]
    contact_email: Option<String>,
    /// Demo account username.
    #[arg(long)]
    demo_account_name: Option<String>,
    /// Demo account password. Prefer `--demo-account-password-file` so it
    /// stays out of shell history.
    #[arg(long, conflicts_with = "demo_account_password_file")]
    demo_account_password: Option<String>,
    #[arg(long)]
    demo_account_password_file: Option<std::path::PathBuf>,
    /// Whether the reviewer needs the demo account to sign in.
    #[arg(long)]
    demo_account_required: Option<bool>,
    /// Notes for the reviewer (max 4000 characters).
    #[arg(long, conflicts_with = "notes_file")]
    notes: Option<String>,
    #[arg(long)]
    notes_file: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum BetaAppReviewDetailCommand {
    /// Fetch the Beta App Review contact details for an app.
    Get {
        /// The ASC app ID.
        app_id: String,
    },
    /// Update the Beta App Review contact and demo-account details for an app.
    ///
    /// Only supplied flags are updated; omitted fields are left unchanged.
    Update {
        /// The ASC app ID.
        app_id: String,
        /// Contact email address for Beta App Review questions.
        #[arg(long)]
        contact_email: Option<String>,
        /// Contact first name.
        #[arg(long)]
        contact_first_name: Option<String>,
        /// Contact last name.
        #[arg(long)]
        contact_last_name: Option<String>,
        /// Contact phone number (international format recommended).
        #[arg(long)]
        contact_phone: Option<String>,
        /// Demo account username (if the app requires login to review).
        #[arg(long)]
        demo_account_name: Option<String>,
        /// Demo account password.
        #[arg(long)]
        demo_account_password: Option<String>,
        /// Whether a demo account is required to review the app.
        #[arg(long)]
        demo_account_required: Option<bool>,
        /// Free-form notes for the reviewer (max 4000 chars).
        #[arg(long)]
        notes: Option<String>,
    },
}

#[derive(Subcommand)]
enum BetaAppReviewSubmissionsCommand {
    /// List Beta App Review submissions for a build.
    List {
        /// The ASC build ID whose submission record to retrieve.
        #[arg(long)]
        build_id: String,
    },
    /// Submit a build to Beta App Review.
    ///
    /// This is irreversible — it immediately sends the build to Apple's
    /// review queue. Use `--dry-run` to confirm which build would be submitted
    /// before committing.
    Submit {
        /// The ASC build ID to submit for review.
        #[arg(long)]
        build_id: String,
    },
}

#[derive(Subcommand)]
enum SalesReportsCommand {
    /// Download, decompress, and convert one Apple sales report to JSON.
    Download {
        /// App Store Connect vendor number, from Payments and Financial
        /// Reports. May also be set through ASC_VENDOR_NUMBER.
        #[arg(long, env = "ASC_VENDOR_NUMBER")]
        vendor_number: String,
        /// Apple report frequency, such as DAILY, WEEKLY, MONTHLY, or YEARLY.
        #[arg(long)]
        frequency: String,
        /// Report date in Apple's required format for the selected frequency.
        #[arg(long)]
        report_date: String,
        /// Apple report type. Defaults to SALES for paid-app and IAP sales.
        #[arg(long, default_value = "SALES")]
        report_type: String,
        /// Apple report subtype. Defaults to SUMMARY.
        #[arg(long, default_value = "SUMMARY")]
        report_subtype: String,
        /// Optional Apple report format version, for report types that use one.
        #[arg(long)]
        version: Option<String>,
        /// Write the JSON array to this file instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ProfilesCommand {
    /// List the team's profiles, newest expiry last.
    ///
    /// `profileContent` is deliberately not shown; use `download` for the
    /// file itself.
    List {
        /// Only show profiles with this exact name. Apple allows
        /// duplicates, so this can still match several.
        #[arg(long)]
        name: Option<String>,
        /// Only show one type.
        #[arg(long, value_enum)]
        r#type: Option<CliProfileType>,
        /// Add each profile's bundle ID (`bundle_id`, `bundle_identifier`).
        /// One extra request per profile.
        #[arg(long)]
        with_bundle_id: bool,
    },
    /// Create a profile over a bundle ID and one or more certificates.
    ///
    /// This is the only way to pick up a capability added to the App ID
    /// after an existing profile was made: profiles are snapshots, and
    /// Apple keeps serving the stale one as ACTIVE until its certificate
    /// dies. Apple does not treat an equivalent profile as a duplicate, so
    /// repeated runs accumulate profiles rather than replacing one.
    Create {
        /// Label shown in the developer portal.
        #[arg(long)]
        name: String,
        #[arg(long, value_enum)]
        r#type: CliProfileType,
        /// Resource id of the bundle ID (from `bundle-ids list`), not the
        /// reverse-DNS identifier.
        #[arg(long)]
        bundle_id: String,
        /// Certificate resource id (from `certificates list`). Repeat for
        /// several.
        #[arg(long = "certificate", required = true)]
        certificates: Vec<String>,
        /// Device resource id. Only meaningful for development and ad hoc
        /// types; ignored for store profiles, which Apple rejects if sent
        /// devices.
        #[arg(long = "device")]
        devices: Vec<String>,
    },
    /// Write a profile's bytes to a file.
    ///
    /// The output is the binary `.provisionprofile`/`.mobileprovision` a
    /// bundle embeds, not JSON.
    Download {
        id: String,
        /// Where to write the profile.
        #[arg(long)]
        output: PathBuf,
    },
    /// Delete a profile.
    ///
    /// Narrower than revoking a certificate: builds already signed with it
    /// keep working, but new signing against it fails.
    Delete {
        id: String,
        /// Required. Pass the profile id again to confirm.
        #[arg(long)]
        confirm: String,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum CliProfileType {
    /// iOS development — device-scoped.
    IosAppDevelopment,
    /// iOS App Store distribution.
    IosAppStore,
    /// iOS ad hoc — device-scoped.
    IosAppAdhoc,
    /// iOS in-house (Enterprise) distribution.
    IosAppInhouse,
    /// macOS development — device-scoped.
    MacAppDevelopment,
    /// Mac App Store distribution — the embedded.provisionprofile a MAS
    /// submission carries.
    MacAppStore,
    /// Developer ID — distribution outside the App Store.
    MacAppDirect,
    /// tvOS development — device-scoped.
    TvosAppDevelopment,
    /// tvOS App Store distribution.
    TvosAppStore,
    /// tvOS ad hoc — device-scoped.
    TvosAppAdhoc,
    /// tvOS in-house (Enterprise) distribution.
    TvosAppInhouse,
    /// Mac Catalyst development — device-scoped.
    MacCatalystAppDevelopment,
    /// Mac Catalyst App Store distribution.
    MacCatalystAppStore,
    /// Mac Catalyst Developer ID distribution.
    MacCatalystAppDirect,
}

impl From<CliProfileType> for ProfileType {
    fn from(value: CliProfileType) -> Self {
        match value {
            CliProfileType::IosAppDevelopment => ProfileType::IosAppDevelopment,
            CliProfileType::IosAppStore => ProfileType::IosAppStore,
            CliProfileType::IosAppAdhoc => ProfileType::IosAppAdHoc,
            CliProfileType::IosAppInhouse => ProfileType::IosAppInHouse,
            CliProfileType::MacAppDevelopment => ProfileType::MacAppDevelopment,
            CliProfileType::MacAppStore => ProfileType::MacAppStore,
            CliProfileType::MacAppDirect => ProfileType::MacAppDirect,
            CliProfileType::TvosAppDevelopment => ProfileType::TvOsAppDevelopment,
            CliProfileType::TvosAppStore => ProfileType::TvOsAppStore,
            CliProfileType::TvosAppAdhoc => ProfileType::TvOsAppAdHoc,
            CliProfileType::TvosAppInhouse => ProfileType::TvOsAppInHouse,
            CliProfileType::MacCatalystAppDevelopment => ProfileType::MacCatalystAppDevelopment,
            CliProfileType::MacCatalystAppStore => ProfileType::MacCatalystAppStore,
            CliProfileType::MacCatalystAppDirect => ProfileType::MacCatalystAppDirect,
        }
    }
}

#[derive(Subcommand)]
enum CertificatesCommand {
    /// List the team's certificates, newest expiry last. Apple returns
    /// expired ones too; `expired` in the output says which.
    List {
        /// Only show one type.
        #[arg(long, value_enum)]
        r#type: Option<CliCertificateType>,
    },
    /// Generate an RSA 2048 key pair, ask Apple to certify it, and write
    /// both halves to disk.
    ///
    /// The private key is written first and is the part that matters:
    /// Apple keeps no copy, so losing it makes the certificate permanently
    /// useless. Back up the whole output directory somewhere encrypted.
    Create {
        #[arg(long, value_enum)]
        r#type: CliCertificateType,
        /// Where to write `<name>.key.pem` and `<name>.cer`.
        #[arg(long, default_value = ".")]
        out_dir: PathBuf,
        /// Subject common name for the CSR. Apple replaces it on the
        /// issued certificate, so this only labels the request itself.
        #[arg(long, default_value = "smbcloud-ascapi")]
        common_name: String,
    },
    /// Revoke a certificate.
    ///
    /// Irreversible, and it breaks every provisioning profile embedding
    /// this certificate for the whole team at once. Use it to free a slot
    /// at Apple's per-type cap, or when a private key is compromised.
    Revoke {
        id: String,
        /// Required. Pass the certificate id again to confirm.
        #[arg(long)]
        confirm: String,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum CliCertificateType {
    /// Apple Development — all platforms.
    Development,
    /// Apple Distribution — App Store and Ad Hoc, all platforms.
    Distribution,
    /// 3rd Party Mac Developer Application — the .app in a Mac App Store build.
    MacAppDistribution,
    /// 3rd Party Mac Developer Installer — the .pkg `productbuild --sign` needs.
    MacInstallerDistribution,
    /// Developer ID Application — distribution outside the App Store.
    DeveloperIdApplication,
}

impl From<CliCertificateType> for CertificateType {
    fn from(value: CliCertificateType) -> Self {
        match value {
            CliCertificateType::Development => CertificateType::Development,
            CliCertificateType::Distribution => CertificateType::Distribution,
            CliCertificateType::MacAppDistribution => CertificateType::MacAppDistribution,
            CliCertificateType::MacInstallerDistribution => {
                CertificateType::MacInstallerDistribution
            }
            CliCertificateType::DeveloperIdApplication => CertificateType::DeveloperIdApplication,
        }
    }
}

#[derive(Subcommand)]
enum AppPricesCommand {
    /// `GET /v1/appPriceSchedules/{app_id}` — the base territory Apple
    /// converts every other storefront from.
    Schedule { app_id: String },
    /// `GET /v1/appPriceSchedules/{app_id}/manualPrices`, joined against
    /// the price points and territories so the money is actually visible.
    ///
    /// Defaults to the manual prices — what a human set, usually one row
    /// in the base territory. `--automatic` instead lists what Apple
    /// converted that into for all ~178 storefronts, which is where a
    /// scheduled conversion shows up as a near-future `endDate`.
    List {
        app_id: String,
        /// List Apple's converted per-storefront prices rather than the
        /// manually set ones.
        #[arg(long)]
        automatic: bool,
        /// Narrow to one territory code, e.g. IDN. Applied by Apple as
        /// `filter[territory]`, so it is much cheaper than fetching every
        /// storefront.
        #[arg(long)]
        territory: Option<String>,
    },
}

#[derive(Subcommand)]
enum AppsCommand {
    /// `GET /v1/apps`.
    List {
        #[arg(long)]
        bundle_id: Option<String>,
    },
    /// `GET /v1/apps/{id}`.
    Get { app_id: String },
    /// `PATCH /v1/apps/{id}`.
    Update {
        app_id: String,
        #[arg(long)]
        primary_locale: Option<String>,
    },
    /// `GET /v1/apps/{id}/appInfos`.
    Infos { app_id: String },
    /// `GET /v1/builds?filter[app]={id}` — newest first, so the
    /// most recently uploaded build (e.g. after a fixplist re-upload) is
    /// first. Check `processingState` (VALID/INVALID/PROCESSING/FAILED).
    Builds {
        app_id: String,
        /// `filter[version]` — the build (upload) number, e.g. `42`.
        #[arg(long)]
        version: Option<String>,
        /// `filter[preReleaseVersion.version]` — the marketing version the
        /// build sits under, e.g. `1.1.0`.
        #[arg(long)]
        pre_release_version: Option<String>,
        /// `filter[processingState]` — PROCESSING, FAILED, INVALID, or VALID.
        #[arg(long)]
        processing_state: Option<String>,
    },
    /// `GET /v1/builds/{id}` — a single build by its App Store Connect id.
    Build { id: String },
    /// `PATCH /v1/builds/{id}` `usesNonExemptEncryption` — answer export
    /// compliance for a build that shows "Missing Compliance" (it was uploaded
    /// without `ITSAppUsesNonExemptEncryption` in every bundle's Info.plist).
    /// App Review rejects a version whose build has no answer.
    SetEncryption {
        /// The build's App Store Connect id (from `apps builds`), not its build number.
        build_id: String,
        /// `false`: no encryption beyond what Apple's OS provides (HTTPS via
        /// URLSession or Security.framework counts), i.e. "None of the
        /// algorithms mentioned above". `true`: non-exempt encryption.
        #[arg(long, action = clap::ArgAction::Set)]
        uses_non_exempt_encryption: bool,
    },
    /// `GET /v1/preReleaseVersions?filter[app]={id}` — the marketing version strings
    /// (e.g. `1.1.0`) TestFlight groups builds under.
    PreReleaseVersions {
        app_id: String,
        /// Restrict to one platform.
        #[arg(long, value_enum)]
        platform: Option<CliPlatform>,
    },
    /// Pre-flight an exported `.ipa`/`.pkg` with `xcrun altool --validate-app`:
    /// run App Store Connect's checks without delivering the binary. Catches
    /// most of what a failed upload would, in seconds. Run this before `upload`.
    Validate {
        /// The exported package (`xcodebuild -exportArchive` output).
        package: PathBuf,
    },
    /// Upload an exported `.ipa`/`.pkg` with `xcrun altool --upload-package`,
    /// authenticated with the same key and `.p8` as every other command.
    /// Afterwards, find the build with `apps builds <APP_ID> --version <N>`.
    Upload {
        /// The exported package (`xcodebuild -exportArchive` output).
        package: PathBuf,
        /// Block until App Store Connect finishes processing the build.
        #[arg(long)]
        wait: bool,
    },
    /// Poll `GET /v1/builds?filter[app]={id}` until the newest matching build
    /// finishes processing (VALID) or fails (INVALID/FAILED), or the timeout
    /// elapses. This is the signal that confirms an upload or a
    /// binary-rejection fix worked. Narrow with `--version` /
    /// `--pre-release-version` when more than one upload is in flight.
    AwaitProcessing {
        app_id: String,
        /// `filter[version]` — the build (upload) number, e.g. `42`.
        #[arg(long)]
        version: Option<String>,
        /// `filter[preReleaseVersion.version]` — the marketing version the
        /// build sits under, e.g. `1.1.0`.
        #[arg(long)]
        pre_release_version: Option<String>,
        /// Seconds between polls.
        #[arg(long, default_value_t = 30)]
        interval: u64,
        /// Give up after this many seconds without a terminal state.
        #[arg(long, default_value_t = 30 * 60)]
        timeout: u64,
    },
}

#[derive(Subcommand)]
enum BundleIdsCommand {
    /// `GET /v1/bundleIds`.
    List {
        #[arg(long)]
        identifier: Option<String>,
    },
    /// `POST /v1/bundleIds`.
    Create {
        #[arg(long)]
        identifier: String,
        #[arg(long)]
        name: String,
        #[arg(long, value_enum, default_value_t = CliBundleIdPlatform::Ios)]
        platform: CliBundleIdPlatform,
    },
}

#[derive(Subcommand)]
enum AppStoreVersionsCommand {
    /// `GET /v1/apps/{app_id}/appStoreVersions`.
    List {
        app_id: String,
        #[arg(long, value_enum)]
        platform: Option<CliPlatform>,
    },
    /// `GET /v1/appStoreVersions/{id}`.
    Get { id: String },
    /// `POST /v1/appStoreVersions`.
    Create {
        app_id: String,
        #[arg(long, value_enum)]
        platform: CliPlatform,
        #[arg(long)]
        version_string: String,
        #[arg(long)]
        copyright: Option<String>,
    },
    /// `PATCH /v1/appStoreVersions/{id}` — edit a version's own attributes,
    /// e.g. rename the platform's editable version (`--version-string
    /// 1.1.0`). The version must be in an editable state. Apple has no
    /// separate rename action and such a version can't be deleted once a
    /// build exists, so this is the way to bump its number.
    Update {
        id: String,
        #[arg(long)]
        version_string: Option<String>,
        #[arg(long)]
        copyright: Option<String>,
    },
    /// `DELETE /v1/appStoreVersions/{id}`.
    Delete { id: String },
    /// `PATCH /v1/appStoreVersions/{id}` — attach a `Build` (see
    /// `apps builds`) to this version, e.g. after fixing an
    /// `INVALID_BINARY` version with a re-uploaded build.
    SetBuild {
        id: String,
        #[arg(long)]
        build_id: String,
        /// Skip the read-back check that the version now reports this build.
        #[arg(long)]
        no_verify: bool,
    },
    /// `GET /v1/appStoreVersions/{id}/build` — the build attached to this
    /// version, or `null` when there is none. Read-only; run it before
    /// `review-submissions submit`.
    Build { id: String },
}

#[derive(Subcommand)]
enum ReviewSubmissionsCommand {
    /// `GET /v1/reviewSubmissions?filter[app]={app_id}`.
    List {
        app_id: String,
        #[arg(long, value_enum)]
        platform: Option<CliPlatform>,
        /// Only submissions in this state, e.g. READY_FOR_REVIEW,
        /// WAITING_FOR_REVIEW, IN_REVIEW, UNRESOLVED_ISSUES, COMPLETE.
        #[arg(long)]
        state: Option<String>,
    },
    /// Submit an App Store Version for review: reuse the platform's open
    /// submission or create one, add the version to it, then submit.
    /// `--dry-run` reads everything and prints the plan without writing.
    Submit {
        app_id: String,
        /// The App Store Version to submit (see `app-store-versions list`).
        /// It needs a build attached (`app-store-versions set-build`;
        /// check with `app-store-versions build`).
        #[arg(long)]
        version_id: String,
    },
    /// `PATCH /v1/reviewSubmissions/{id}` with `canceled: true` — withdraw
    /// a submission before App Review picks it up.
    Cancel { id: String },
}

#[derive(Subcommand)]
enum AppInfoLocalizationsCommand {
    List {
        app_info_id: String,
    },
    Create {
        app_info_id: String,
        #[arg(long)]
        locale: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        subtitle: Option<String>,
    },
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        subtitle: Option<String>,
    },
    Delete {
        id: String,
    },
}

#[derive(Subcommand)]
enum AppStoreVersionLocalizationsCommand {
    List {
        app_store_version_id: String,
    },
    Create {
        app_store_version_id: String,
        #[arg(long)]
        locale: String,
        #[arg(long)]
        description: Option<String>,
        /// Read `description` from a UTF-8 file; one trailing newline is dropped.
        #[arg(long, conflicts_with = "description")]
        description_file: Option<PathBuf>,
        #[arg(long)]
        keywords: Option<String>,
        /// Read `keywords` from a UTF-8 file; one trailing newline is dropped.
        #[arg(long, conflicts_with = "keywords")]
        keywords_file: Option<PathBuf>,
        #[arg(long)]
        whats_new: Option<String>,
        /// Read `whatsNew` from a UTF-8 file; one trailing newline is dropped.
        #[arg(long, conflicts_with = "whats_new")]
        whats_new_file: Option<PathBuf>,
        #[arg(long)]
        promotional_text: Option<String>,
        /// Read `promotionalText` from a UTF-8 file; one trailing newline is dropped.
        #[arg(long, conflicts_with = "promotional_text")]
        promotional_text_file: Option<PathBuf>,
        #[arg(long)]
        marketing_url: Option<String>,
        #[arg(long)]
        support_url: Option<String>,
        /// Skip the read-back check that compares the stored values with
        /// what was sent.
        #[arg(long)]
        no_verify: bool,
    },
    Update {
        id: String,
        #[arg(long)]
        description: Option<String>,
        /// Read `description` from a UTF-8 file; one trailing newline is dropped.
        #[arg(long, conflicts_with = "description")]
        description_file: Option<PathBuf>,
        #[arg(long)]
        keywords: Option<String>,
        /// Read `keywords` from a UTF-8 file; one trailing newline is dropped.
        #[arg(long, conflicts_with = "keywords")]
        keywords_file: Option<PathBuf>,
        #[arg(long)]
        whats_new: Option<String>,
        /// Read `whatsNew` from a UTF-8 file; one trailing newline is dropped.
        #[arg(long, conflicts_with = "whats_new")]
        whats_new_file: Option<PathBuf>,
        #[arg(long)]
        promotional_text: Option<String>,
        /// Read `promotionalText` from a UTF-8 file; one trailing newline is dropped.
        #[arg(long, conflicts_with = "promotional_text")]
        promotional_text_file: Option<PathBuf>,
        #[arg(long)]
        marketing_url: Option<String>,
        #[arg(long)]
        support_url: Option<String>,
        /// Skip the read-back check that compares the stored values with
        /// what was sent.
        #[arg(long)]
        no_verify: bool,
    },
    Delete {
        id: String,
    },
}

#[derive(Subcommand)]
enum AppScreenshotSetsCommand {
    /// `GET /v1/appStoreVersionLocalizations/{id}/appScreenshotSets`.
    List {
        app_store_version_localization_id: String,
    },
    /// `POST /v1/appScreenshotSets`.
    Create {
        app_store_version_localization_id: String,
        #[arg(long, value_enum)]
        display_type: CliScreenshotDisplayType,
    },
    /// `DELETE /v1/appScreenshotSets/{id}`.
    Delete { id: String },
}

#[derive(Subcommand)]
enum AppScreenshotsCommand {
    /// `GET /v1/appScreenshotSets/{id}/appScreenshots`.
    List { app_screenshot_set_id: String },
    /// Reserve, PUT, and commit an image file in one call — the full
    /// `POST /v1/appScreenshots` → upload → `PATCH …uploaded:true` flow.
    Upload {
        app_screenshot_set_id: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// `DELETE /v1/appScreenshots/{id}`.
    Delete { id: String },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliScreenshotDisplayType {
    AppleVisionPro,
    Iphone67,
    Iphone65,
    Iphone61,
    Iphone58,
    Iphone55,
    Iphone47,
    Iphone40,
    Iphone35,
    IpadPro3Gen129,
    IpadPro3Gen11,
    IpadPro129,
    Ipad105,
    Ipad97,
    Desktop,
    AppleTv,
    WatchUltra,
    WatchSeries10,
    WatchSeries7,
    WatchSeries4,
    WatchSeries3,
}

impl From<CliScreenshotDisplayType> for ScreenshotDisplayType {
    fn from(value: CliScreenshotDisplayType) -> Self {
        match value {
            CliScreenshotDisplayType::AppleVisionPro => ScreenshotDisplayType::AppleVisionPro,
            CliScreenshotDisplayType::Iphone67 => ScreenshotDisplayType::Iphone67,
            CliScreenshotDisplayType::Iphone65 => ScreenshotDisplayType::Iphone65,
            CliScreenshotDisplayType::Iphone61 => ScreenshotDisplayType::Iphone61,
            CliScreenshotDisplayType::Iphone58 => ScreenshotDisplayType::Iphone58,
            CliScreenshotDisplayType::Iphone55 => ScreenshotDisplayType::Iphone55,
            CliScreenshotDisplayType::Iphone47 => ScreenshotDisplayType::Iphone47,
            CliScreenshotDisplayType::Iphone40 => ScreenshotDisplayType::Iphone40,
            CliScreenshotDisplayType::Iphone35 => ScreenshotDisplayType::Iphone35,
            CliScreenshotDisplayType::IpadPro3Gen129 => ScreenshotDisplayType::IpadPro3Gen129,
            CliScreenshotDisplayType::IpadPro3Gen11 => ScreenshotDisplayType::IpadPro3Gen11,
            CliScreenshotDisplayType::IpadPro129 => ScreenshotDisplayType::IpadPro129,
            CliScreenshotDisplayType::Ipad105 => ScreenshotDisplayType::Ipad105,
            CliScreenshotDisplayType::Ipad97 => ScreenshotDisplayType::Ipad97,
            CliScreenshotDisplayType::Desktop => ScreenshotDisplayType::Desktop,
            CliScreenshotDisplayType::AppleTv => ScreenshotDisplayType::AppleTv,
            CliScreenshotDisplayType::WatchUltra => ScreenshotDisplayType::WatchUltra,
            CliScreenshotDisplayType::WatchSeries10 => ScreenshotDisplayType::WatchSeries10,
            CliScreenshotDisplayType::WatchSeries7 => ScreenshotDisplayType::WatchSeries7,
            CliScreenshotDisplayType::WatchSeries4 => ScreenshotDisplayType::WatchSeries4,
            CliScreenshotDisplayType::WatchSeries3 => ScreenshotDisplayType::WatchSeries3,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliPlatform {
    Ios,
    MacOs,
    TvOs,
    VisionOs,
}

impl From<CliPlatform> for Platform {
    fn from(value: CliPlatform) -> Self {
        match value {
            CliPlatform::Ios => Platform::Ios,
            CliPlatform::MacOs => Platform::MacOs,
            CliPlatform::TvOs => Platform::TvOs,
            CliPlatform::VisionOs => Platform::VisionOs,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliBundleIdPlatform {
    Ios,
    MacOs,
    Universal,
}

impl From<CliBundleIdPlatform> for BundleIdPlatform {
    fn from(value: CliBundleIdPlatform) -> Self {
        match value {
            CliBundleIdPlatform::Ios => BundleIdPlatform::Ios,
            CliBundleIdPlatform::MacOs => BundleIdPlatform::MacOs,
            CliBundleIdPlatform::Universal => BundleIdPlatform::Universal,
        }
    }
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Print `value` and return without sending anything, when `--dry-run` is
/// set. Every mutating command checks this before touching the network.
fn dry_run_guard(dry_run: bool, value: &impl serde::Serialize) -> Result<bool> {
    if dry_run {
        println!("# dry run — request body that would be sent:");
        print_json(value)?;
    }
    Ok(dry_run)
}

/// Resolve a text option given either inline or as a file. The file is read
/// as UTF-8 and one trailing newline (`\n` or `\r\n`) is dropped, the way
/// `$(cat file)` does, so file-backed and inline values behave the same.
fn text_arg(inline: Option<String>, file: Option<PathBuf>, flag: &str) -> Result<Option<String>> {
    let Some(path) = file else {
        return Ok(inline);
    };
    let mut text = std::fs::read_to_string(&path)
        .with_context(|| format!("{flag}: cannot read {} as UTF-8 text", path.display()))?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    Ok(Some(text))
}

/// Run the shared length lint: warnings go to stderr, a violation aborts
/// with the field's message rewritten to name the CLI flag.
fn lint_localization_fields(fields: &AppStoreVersionLocalizationFields) -> Result<()> {
    match fields.validate() {
        Ok(warnings) => {
            for warning in warnings {
                eprintln!("warning: {warning}");
            }
            Ok(())
        }
        Err(message) => {
            let (field, rest) = message.split_once(' ').unwrap_or((&message, ""));
            let mut flag = String::from("--");
            for c in field.chars() {
                if c.is_ascii_uppercase() {
                    flag.push('-');
                    flag.push(c.to_ascii_lowercase());
                } else {
                    flag.push(c);
                }
            }
            anyhow::bail!("{flag} {rest}. Shorten the text and try again.")
        }
    }
}

/// Names of the fields in `sent` whose stored value is not an exact match,
/// with the index of the first differing character (a missing value counts
/// as the empty string).
fn diff_fields(
    sent: &AppStoreVersionLocalizationFields,
    stored: &AppStoreVersionLocalizationAttributes,
) -> Vec<&'static str> {
    [
        ("description", &sent.description, &stored.description),
        ("keywords", &sent.keywords, &stored.keywords),
        ("marketingUrl", &sent.marketing_url, &stored.marketing_url),
        (
            "promotionalText",
            &sent.promotional_text,
            &stored.promotional_text,
        ),
        ("supportUrl", &sent.support_url, &stored.support_url),
        ("whatsNew", &sent.whats_new, &stored.whats_new),
    ]
    .into_iter()
    .filter(|(_, sent, stored)| match sent {
        Some(sent) => stored.as_deref() != Some(sent.as_str()),
        None => false,
    })
    .map(|(name, _, _)| name)
    .collect()
}

fn first_diff_index(a: &str, b: &str) -> usize {
    a.chars()
        .zip(b.chars())
        .position(|(x, y)| x != y)
        .unwrap_or_else(|| a.chars().count().min(b.chars().count()))
}

/// Read the localization back and fail if any field that was sent is not
/// stored byte for byte.
async fn verify_localization(
    client: &Client,
    id: &str,
    sent: &AppStoreVersionLocalizationFields,
    no_verify: bool,
) -> Result<()> {
    if no_verify {
        return Ok(());
    }
    let stored = client.get_app_store_version_localization(id).await?;
    let mismatched = diff_fields(sent, &stored.attributes);
    if mismatched.is_empty() {
        return Ok(());
    }
    let details = mismatched
        .iter()
        .map(|name| {
            let (sent, stored) = match *name {
                "description" => (&sent.description, &stored.attributes.description),
                "keywords" => (&sent.keywords, &stored.attributes.keywords),
                "marketingUrl" => (&sent.marketing_url, &stored.attributes.marketing_url),
                "promotionalText" => (&sent.promotional_text, &stored.attributes.promotional_text),
                "supportUrl" => (&sent.support_url, &stored.attributes.support_url),
                _ => (&sent.whats_new, &stored.attributes.whats_new),
            };
            let at = first_diff_index(
                sent.as_deref().unwrap_or(""),
                stored.as_deref().unwrap_or(""),
            );
            format!("{name} (first difference at char {at})")
        })
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!("read-back mismatch on {id}: {details}")
}

/// Validate a `--version-string` before sending it. Apple takes one to
/// three period-separated integers (`1.1` and `1.1.0` are both fine) and
/// answers anything else with an opaque error, so the shape is checked
/// here where the message can name the offending flag. Shared by create
/// and update because both write the same attribute.
fn validate_version_string(version_string: &str) -> Result<()> {
    let parts = version_string.split('.').collect::<Vec<_>>();
    let well_formed = parts.len() <= 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if !well_formed {
        anyhow::bail!(
            "invalid --version-string `{version_string}`: expected one to three \
             period-separated numbers, e.g. `1.1.0`"
        );
    }
    Ok(())
}

use smbcloud_ascapi_aso::app_store_version_localization::WHATS_NEW_MAX_LEN;

/// Validate a `--whats-new` text before sending it. App Store Connect caps
/// the field at [`WHATS_NEW_MAX_LEN`] characters and answers with an opaque
/// error otherwise, so the length is checked here where the message can
/// name the offending flag. Shared by `build-localizations create` and
/// `update` because both write the same attribute, and called before the
/// dry-run print so `--dry-run` catches the limit too.
fn validate_whats_new(whats_new: &str) -> Result<()> {
    let len = whats_new.chars().count();
    if len > WHATS_NEW_MAX_LEN {
        anyhow::bail!(
            "--whats-new is {len} characters; App Store Connect accepts at most \
             {WHATS_NEW_MAX_LEN}. Shorten the note and try again."
        );
    }
    Ok(())
}

/// Convert Apple's gzip-compressed TSV report to JSON without normalizing
/// columns: report layouts vary by report type and Apple can add columns.
fn sales_report_as_json(gzip: &[u8]) -> Result<Vec<serde_json::Value>> {
    use flate2::read::GzDecoder;
    use serde_json::{Map, Value};

    let decoder = GzDecoder::new(gzip);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(true)
        .flexible(true)
        .from_reader(decoder);

    let raw_headers = reader
        .headers()
        .context("reading the TSV header from Apple's sales report")?
        .iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if raw_headers.is_empty() {
        anyhow::bail!("Apple's sales report has no TSV headers")
    }

    // Apple may add blank or repeated columns. JSON objects require unique,
    // non-empty keys, so retain valid headers and assign stable fallbacks to
    // the rest instead of discarding an otherwise usable report.
    let mut used_headers = HashSet::new();
    let headers = raw_headers
        .into_iter()
        .enumerate()
        .map(|(index, header)| {
            let base = if header.is_empty() {
                format!("column_{}", index + 1)
            } else {
                header
            };
            let mut name = base.clone();
            let mut suffix = 2;
            while !used_headers.insert(name.clone()) {
                name = format!("{base}_{suffix}");
                suffix += 1;
            }
            name
        })
        .collect::<Vec<_>>();

    let mut rows = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let record = record.with_context(|| format!("parsing TSV row {}", index + 2))?;
        let mut row_headers = headers.clone();
        let mut used_row_headers = row_headers.iter().cloned().collect::<HashSet<_>>();
        for column in row_headers.len()..record.len() {
            let base = format!("extra_column_{}", column + 1);
            let mut name = base.clone();
            let mut suffix = 2;
            while !used_row_headers.insert(name.clone()) {
                name = format!("{base}_{suffix}");
                suffix += 1;
            }
            row_headers.push(name);
        }
        let object = row_headers
            .iter()
            .zip(record.iter())
            .map(|(header, value)| (header.clone(), Value::String(value.to_owned())))
            .collect::<Map<_, _>>();
        rows.push(Value::Object(object));
    }
    Ok(rows)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.mcp {
        return smbcloud_ascapi_mcp::serve().await;
    }

    if let Some(Command::AppGallery {
        service_account,
        client_id,
        command,
    }) = cli.command
    {
        return run_appgallery(
            service_account.as_deref(),
            client_id.as_deref(),
            command,
            cli.dry_run,
        )
        .await;
    }

    if let Some(Command::Play {
        play_service_account,
        command,
    }) = cli.command
    {
        return run_play(play_service_account.as_deref(), command, cli.dry_run).await;
    }

    if let Some(Command::Msstore {
        msstore_env,
        command,
    }) = cli.command
    {
        return run_msstore(msstore_env.as_deref(), command, cli.dry_run).await;
    }

    let key_id = cli
        .key_id
        .clone()
        .context("--key-id is required (or set ASC_API_KEY)")?;
    let issuer_id = cli
        .issuer_id
        .clone()
        .context("--issuer-id is required (or set ASC_ISSUER_ID)")?;

    let private_key_path = cli.private_key_path.clone().unwrap_or_else(|| {
        let mut path = dirs_home();
        path.push(".appstoreconnect");
        path.push("private_keys");
        path.push(format!("AuthKey_{key_id}.p8"));
        path
    });

    let altool_auth = AltoolAuth {
        key_id: key_id.clone(),
        issuer_id: issuer_id.clone(),
        private_key_path: private_key_path.clone(),
    };
    let api_key = ApiKey::from_p8_file(&key_id, &issuer_id, &private_key_path)
        .with_context(|| format!("loading App Store Connect API key from {private_key_path:?}"))?;
    let client = Client::new(api_key);

    let Some(command) = cli.command else {
        anyhow::bail!("no subcommand given; run `asc --help`, or `asc --mcp` for the MCP server")
    };

    match command {
        Command::AppGallery { .. } | Command::Msstore { .. } | Command::Play { .. } => {
            unreachable!("handled before App Store Connect credentials")
        }
        Command::Apps { command } => run_apps(&client, &altool_auth, command, cli.dry_run).await,
        Command::BundleIds { command } => run_bundle_ids(&client, command, cli.dry_run).await,
        Command::AppStoreVersions { command } => {
            run_app_store_versions(&client, command, cli.dry_run).await
        }
        Command::AppStoreReviewDetails { command } => {
            run_app_store_review_details(&client, command, cli.dry_run).await
        }
        Command::AppInfoLocalizations { command } => {
            run_app_info_localizations(&client, command, cli.dry_run).await
        }
        Command::AppStoreVersionLocalizations { command } => {
            run_app_store_version_localizations(&client, command, cli.dry_run).await
        }
        Command::AppScreenshotSets { command } => {
            run_app_screenshot_sets(&client, command, cli.dry_run).await
        }
        Command::AppScreenshots { command } => {
            run_app_screenshots(&client, command, cli.dry_run).await
        }
        Command::AppPrices { command } => run_app_prices(&client, command).await,
        Command::Certificates { command } => run_certificates(&client, command, cli.dry_run).await,
        Command::Profiles { command } => run_profiles(&client, command, cli.dry_run).await,
        Command::ReviewSubmissions { command } => {
            run_review_submissions(&client, command, cli.dry_run).await
        }
        Command::SalesReports { command } => run_sales_reports(&client, command).await,
        Command::Testflight { command } => run_testflight(&client, command, cli.dry_run).await,
        #[cfg(target_os = "macos")]
        Command::Signing { command } => run_signing(&client, command).await,
    }
}

/// macOS-only signing diagnostics. Kept out of MCP: a tool call doesn't
/// need to inspect the operator's login keychain.
#[cfg(target_os = "macos")]
async fn run_signing(client: &Client, command: SigningCommand) -> Result<()> {
    match command {
        SigningCommand::Doctor => {
            let report = smbcloud_ascapi_frontend::run_doctor(client)
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
            print_json(&report)
        }
    }
}

fn dirs_home() -> PathBuf {
    // Avoids pulling in the `dirs` crate for a single lookup; $HOME is set
    // on every platform this CLI targets (macOS/Linux CI runners).
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

async fn run_sales_reports(client: &Client, command: SalesReportsCommand) -> Result<()> {
    match command {
        SalesReportsCommand::Download {
            vendor_number,
            frequency,
            report_date,
            report_type,
            report_subtype,
            version,
            output,
        } => {
            let report = client
                .download_sales_report(&SalesReportRequest {
                    vendor_number,
                    frequency,
                    report_date,
                    report_type,
                    report_subtype,
                    version,
                })
                .await?;
            let rows = sales_report_as_json(&report)?;
            if let Some(path) = output {
                let json = serde_json::to_vec_pretty(&rows)?;
                std::fs::write(&path, json)
                    .with_context(|| format!("writing sales report JSON to {}", path.display()))?;
            } else {
                print_json(&rows)?;
            }
            Ok(())
        }
    }
}

async fn run_testflight(client: &Client, command: TestflightCommand, dry_run: bool) -> Result<()> {
    match command {
        TestflightCommand::Groups { command } => match command {
            BetaGroupsCommand::List { app_id } => {
                let groups = client.list_beta_groups(&app_id).await?;
                print_json(&groups)
            }
            BetaGroupsCommand::Get { group_id } => {
                let group = client.get_beta_group(&group_id).await?;
                print_json(&group)
            }
            BetaGroupsCommand::AddBuild {
                group_id,
                build_ids,
            } => {
                if dry_run {
                    println!(
                        "[dry-run] would add {} build(s) to beta group {group_id}: {}",
                        build_ids.len(),
                        build_ids.join(", ")
                    );
                    return Ok(());
                }
                let ids: Vec<&str> = build_ids.iter().map(String::as_str).collect();
                client.add_builds_to_beta_group(&group_id, &ids).await?;
                println!("Added {} build(s) to group {group_id}.", build_ids.len());
                Ok(())
            }
        },
        TestflightCommand::AppReviewDetail { command } => match command {
            BetaAppReviewDetailCommand::Get { app_id } => {
                let detail = client.get_beta_app_review_detail(&app_id).await?;
                print_json(&detail)
            }
            BetaAppReviewDetailCommand::Update {
                app_id,
                contact_email,
                contact_first_name,
                contact_last_name,
                contact_phone,
                demo_account_name,
                demo_account_password,
                demo_account_required,
                notes,
            } => {
                let update = BetaAppReviewDetailUpdate {
                    contact_email,
                    contact_first_name,
                    contact_last_name,
                    contact_phone,
                    demo_account_name,
                    demo_account_password,
                    demo_account_required,
                    notes,
                };
                if dry_run {
                    println!("[dry-run] would update Beta App Review detail for app {app_id}:");
                    print_json(&update)?;
                    return Ok(());
                }
                let detail = client
                    .update_beta_app_review_detail(&app_id, update)
                    .await?;
                print_json(&detail)
            }
        },
        TestflightCommand::AppReviewSubmissions { command } => match command {
            BetaAppReviewSubmissionsCommand::List { build_id } => {
                let submissions = client.list_beta_app_review_submissions(&build_id).await?;
                print_json(&submissions)
            }
            BetaAppReviewSubmissionsCommand::Submit { build_id } => {
                if dry_run {
                    println!("[dry-run] would submit build {build_id} to Beta App Review.");
                    return Ok(());
                }
                let submission = client.submit_build_for_beta_review(&build_id).await?;
                print_json(&submission)
            }
        },
        TestflightCommand::BuildLocalizations { command } => match command {
            BetaBuildLocalizationsCommand::List { build_id } => {
                print_json(&client.list_beta_build_localizations(&build_id).await?)
            }
            BetaBuildLocalizationsCommand::Create {
                build_id,
                locale,
                whats_new,
                whats_new_file,
            } => {
                let whats_new = text_arg(whats_new, whats_new_file, "--whats-new-file")?
                    .context("one of --whats-new or --whats-new-file is required")?;
                validate_whats_new(&whats_new)?;
                if dry_run {
                    println!(
                        "[dry-run] would create the {locale} \"What to Test\" note on build {build_id}:"
                    );
                    return print_json(&serde_json::json!({ "whatsNew": whats_new }));
                }
                let created = client
                    .create_beta_build_localization(&build_id, &locale, Some(&whats_new))
                    .await?;
                print_json(&created)
            }
            BetaBuildLocalizationsCommand::Update {
                id,
                whats_new,
                whats_new_file,
            } => {
                let whats_new = text_arg(whats_new, whats_new_file, "--whats-new-file")?
                    .context("one of --whats-new or --whats-new-file is required")?;
                validate_whats_new(&whats_new)?;
                if dry_run {
                    println!("[dry-run] would update \"What to Test\" note {id}:");
                    return print_json(&serde_json::json!({ "whatsNew": whats_new }));
                }
                print_json(
                    &client
                        .update_beta_build_localization(&id, &whats_new)
                        .await?,
                )
            }
        },
    }
}

async fn run_apps(
    client: &Client,
    altool_auth: &AltoolAuth,
    command: AppsCommand,
    dry_run: bool,
) -> Result<()> {
    match command {
        AppsCommand::List { bundle_id } => {
            let apps = client.list_apps(bundle_id.as_deref()).await?;
            print_json(&apps)
        }
        AppsCommand::Get { app_id } => print_json(&client.get_app(&app_id).await?),
        AppsCommand::Update {
            app_id,
            primary_locale,
        } => {
            let attributes = AppUpdateAttributes {
                primary_locale,
                content_rights_declaration: None,
            };
            if dry_run_guard(dry_run, &attributes)? {
                return Ok(());
            }
            print_json(&client.update_app(&app_id, attributes).await?)
        }
        AppsCommand::Infos { app_id } => print_json(&client.list_app_infos(&app_id).await?),
        AppsCommand::Builds {
            app_id,
            version,
            pre_release_version,
            processing_state,
        } => {
            let filter = BuildFilter {
                version,
                pre_release_version,
                processing_state,
            };
            print_json(&client.list_builds(&app_id, &filter).await?)
        }
        AppsCommand::Build { id } => print_json(&client.get_build(&id).await?),
        AppsCommand::SetEncryption {
            build_id,
            uses_non_exempt_encryption,
        } => {
            let attributes = BuildUpdateAttributes {
                uses_non_exempt_encryption: Some(uses_non_exempt_encryption),
            };
            if dry_run_guard(dry_run, &attributes)? {
                return Ok(());
            }
            print_json(&client.update_build(&build_id, attributes).await?)
        }
        AppsCommand::PreReleaseVersions { app_id, platform } => {
            let platform = platform.map(Platform::from);
            print_json(&client.list_pre_release_versions(&app_id, platform).await?)
        }
        AppsCommand::Validate { package } => {
            if dry_run {
                let mut argv = vec!["xcrun".to_string()];
                argv.extend(
                    upload::altool_args(&package, altool_auth, AltoolOp::Validate, false)
                        .iter()
                        .map(|arg| arg.to_string_lossy().into_owned()),
                );
                return print_json(&serde_json::json!({ "dry_run": true, "command": argv }));
            }
            let auth = altool_auth.clone();
            let outcome = tokio::task::spawn_blocking(move || validate_app(&package, &auth))
                .await?
                .map_err(anyhow::Error::msg)?;
            print_json(&outcome)?;
            match outcome.exit_code {
                _ if outcome.success => {}
                Some(code) => anyhow::bail!("altool validation failed with exit code {code}"),
                None => anyhow::bail!("altool validation was killed by a signal"),
            }
            Ok(())
        }
        AppsCommand::Upload { package, wait } => {
            if dry_run {
                let mut argv = vec!["xcrun".to_string()];
                argv.extend(
                    upload::altool_args(&package, altool_auth, AltoolOp::Upload, wait)
                        .iter()
                        .map(|arg| arg.to_string_lossy().into_owned()),
                );
                return print_json(&serde_json::json!({ "dry_run": true, "command": argv }));
            }
            let auth = altool_auth.clone();
            let outcome =
                tokio::task::spawn_blocking(move || upload_package(&package, &auth, wait))
                    .await?
                    .map_err(anyhow::Error::msg)?;
            print_json(&outcome)?;
            match outcome.exit_code {
                _ if outcome.success => {}
                Some(code) => anyhow::bail!("altool upload failed with exit code {code}"),
                None => anyhow::bail!("altool upload was killed by a signal"),
            }
            Ok(())
        }
        AppsCommand::AwaitProcessing {
            app_id,
            version,
            pre_release_version,
            interval,
            timeout,
        } => {
            let filter = BuildFilter {
                version,
                pre_release_version,
                processing_state: None,
            };
            let config = AwaitConfig {
                interval: Duration::from_secs(interval),
                timeout: Duration::from_secs(timeout),
            };
            let outcome = await_processing(client, &app_id, &filter, config)
                .await
                .map_err(anyhow::Error::msg)?;
            print_json(&outcome)?;
            if outcome.timed_out {
                anyhow::bail!(
                    "timed out after {}s waiting for processing",
                    outcome.waited_secs
                );
            }
            if !outcome.valid {
                anyhow::bail!(
                    "build finished processing but is not valid (state: {})",
                    outcome.processing_state.as_deref().unwrap_or("unknown")
                );
            }
            Ok(())
        }
    }
}

async fn run_bundle_ids(client: &Client, command: BundleIdsCommand, dry_run: bool) -> Result<()> {
    match command {
        BundleIdsCommand::List { identifier } => {
            print_json(&client.list_bundle_ids(identifier.as_deref()).await?)
        }
        BundleIdsCommand::Create {
            identifier,
            name,
            platform,
        } => {
            let attributes = BundleIdCreateAttributes {
                identifier,
                name,
                platform: platform.into(),
            };
            if dry_run_guard(dry_run, &attributes)? {
                return Ok(());
            }
            print_json(&client.create_bundle_id(attributes).await?)
        }
    }
}

async fn run_app_store_review_details(
    client: &Client,
    command: AppStoreReviewDetailsCommand,
    dry_run: bool,
) -> Result<()> {
    match command {
        AppStoreReviewDetailsCommand::Get {
            app_store_version_id,
        } => {
            let detail = client
                .get_app_store_review_detail(&app_store_version_id)
                .await?;
            print_json(&detail)
        }
        AppStoreReviewDetailsCommand::Set(args) => {
            let ReviewDetailSetArgs {
                app_store_version_id,
                contact_first_name,
                contact_last_name,
                contact_phone,
                contact_email,
                demo_account_name,
                demo_account_password,
                demo_account_password_file,
                demo_account_required,
                notes,
                notes_file,
            } = *args;
            let write = AppStoreReviewDetailWrite {
                contact_first_name,
                contact_last_name,
                contact_phone,
                contact_email,
                demo_account_name,
                demo_account_password: text_arg(
                    demo_account_password,
                    demo_account_password_file,
                    "--demo-account-password-file",
                )?,
                demo_account_required,
                notes: text_arg(notes, notes_file, "--notes-file")?,
            };
            if let Some(notes) = &write.notes {
                if notes.chars().count() > NOTES_MAX_LEN {
                    anyhow::bail!("notes are over {NOTES_MAX_LEN} characters");
                }
            }
            let existing = client
                .get_app_store_review_detail(&app_store_version_id)
                .await?;
            if dry_run {
                let mut shown = serde_json::to_value(&write)?;
                // The password is never printed.
                if shown.get("demoAccountPassword").is_some() {
                    shown["demoAccountPassword"] = "<hidden>".into();
                }
                println!(
                    "[dry-run] would {} the review information of version {app_store_version_id}:",
                    if existing.is_some() {
                        "update"
                    } else {
                        "create"
                    }
                );
                return print_json(&shown);
            }
            let saved = match existing {
                Some(detail) => {
                    client
                        .update_app_store_review_detail(&detail.id, write)
                        .await?
                }
                None => {
                    client
                        .create_app_store_review_detail(&app_store_version_id, write)
                        .await?
                }
            };
            print_json(&saved)
        }
    }
}

async fn run_app_store_versions(
    client: &Client,
    command: AppStoreVersionsCommand,
    dry_run: bool,
) -> Result<()> {
    match command {
        AppStoreVersionsCommand::List { app_id, platform } => {
            let versions = client
                .list_app_store_versions(&app_id, platform.map(Into::into))
                .await?;
            print_json(&versions)
        }
        AppStoreVersionsCommand::Get { id } => {
            print_json(&client.get_app_store_version(&id).await?)
        }
        AppStoreVersionsCommand::Create {
            app_id,
            platform,
            version_string,
            copyright,
        } => {
            validate_version_string(&version_string)?;
            let attributes = AppStoreVersionCreateAttributes {
                platform: platform.into(),
                version_string,
                copyright,
            };
            if dry_run_guard(dry_run, &attributes)? {
                return Ok(());
            }
            print_json(&client.create_app_store_version(&app_id, attributes).await?)
        }
        AppStoreVersionsCommand::Update {
            id,
            version_string,
            copyright,
        } => {
            if version_string.is_none() && copyright.is_none() {
                anyhow::bail!("nothing to update: pass --version-string and/or --copyright");
            }
            if let Some(version_string) = version_string.as_deref() {
                validate_version_string(version_string)?;
            }
            let fields = AppStoreVersionUpdateFields {
                version_string,
                copyright,
            };
            if dry_run_guard(dry_run, &fields)? {
                return Ok(());
            }
            print_json(&client.update_app_store_version(&id, fields).await?)
        }
        AppStoreVersionsCommand::Delete { id } => {
            if dry_run {
                println!("# dry run — would DELETE /v1/appStoreVersions/{id}");
                return Ok(());
            }
            client.delete_app_store_version(&id).await?;
            println!("deleted {id}");
            Ok(())
        }
        AppStoreVersionsCommand::Build { id } => {
            let build = client.get_app_store_version_build(&id).await?;
            if build.is_none() {
                eprintln!("no build attached");
            }
            print_json(&build)
        }
        AppStoreVersionsCommand::SetBuild {
            id,
            build_id,
            no_verify,
        } => {
            if dry_run {
                println!(
                    "# dry run — would PATCH /v1/appStoreVersions/{id} relationships.build → {build_id}"
                );
                return Ok(());
            }
            let attached = attach_build(client, &id, &build_id, !no_verify)
                .await
                .map_err(anyhow::Error::msg)?;
            print_json(&attached)
        }
    }
}

async fn run_review_submissions(
    client: &Client,
    command: ReviewSubmissionsCommand,
    dry_run: bool,
) -> Result<()> {
    match command {
        ReviewSubmissionsCommand::List {
            app_id,
            platform,
            state,
        } => {
            let states: Vec<&str> = state.as_deref().into_iter().collect();
            let submissions = client
                .list_review_submissions(&app_id, platform.map(Into::into), &states)
                .await?;
            print_json(&submissions)
        }
        ReviewSubmissionsCommand::Submit { app_id, version_id } => {
            let plan = review::plan_submission(client, &app_id, &version_id)
                .await
                .map_err(anyhow::Error::msg)?;
            if dry_run {
                match &plan.blocked {
                    Some(reason) => println!("# dry run — would not submit: {reason}"),
                    None => println!("# dry run — would submit this plan for review:"),
                }
                return print_json(&plan);
            }
            let submitted = review::submit_for_review(client, plan)
                .await
                .map_err(anyhow::Error::msg)?;
            print_json(&submitted)
        }
        ReviewSubmissionsCommand::Cancel { id } => {
            if dry_run {
                println!("# dry run — would PATCH /v1/reviewSubmissions/{id} canceled: true");
                return Ok(());
            }
            print_json(&client.cancel_review_submission(&id).await?)
        }
    }
}

async fn run_app_info_localizations(
    client: &Client,
    command: AppInfoLocalizationsCommand,
    dry_run: bool,
) -> Result<()> {
    match command {
        AppInfoLocalizationsCommand::List { app_info_id } => {
            print_json(&client.list_app_info_localizations(&app_info_id).await?)
        }
        AppInfoLocalizationsCommand::Create {
            app_info_id,
            locale,
            name,
            subtitle,
        } => {
            let attributes = AppInfoLocalizationCreateAttributes {
                locale,
                name,
                subtitle,
                privacy_policy_url: None,
            };
            if dry_run_guard(dry_run, &attributes)? {
                return Ok(());
            }
            print_json(
                &client
                    .create_app_info_localization(&app_info_id, attributes)
                    .await?,
            )
        }
        AppInfoLocalizationsCommand::Update { id, name, subtitle } => {
            let attributes = AppInfoLocalizationUpdateAttributes {
                name,
                subtitle,
                privacy_policy_url: None,
            };
            if dry_run_guard(dry_run, &attributes)? {
                return Ok(());
            }
            print_json(&client.update_app_info_localization(&id, attributes).await?)
        }
        AppInfoLocalizationsCommand::Delete { id } => {
            if dry_run {
                println!("# dry run — would DELETE /v1/appInfoLocalizations/{id}");
                return Ok(());
            }
            client.delete_app_info_localization(&id).await?;
            println!("deleted {id}");
            Ok(())
        }
    }
}

async fn run_app_store_version_localizations(
    client: &Client,
    command: AppStoreVersionLocalizationsCommand,
    dry_run: bool,
) -> Result<()> {
    match command {
        AppStoreVersionLocalizationsCommand::List {
            app_store_version_id,
        } => print_json(
            &client
                .list_app_store_version_localizations(&app_store_version_id)
                .await?,
        ),
        AppStoreVersionLocalizationsCommand::Create {
            app_store_version_id,
            locale,
            description,
            description_file,
            keywords,
            keywords_file,
            whats_new,
            whats_new_file,
            promotional_text,
            promotional_text_file,
            marketing_url,
            support_url,
            no_verify,
        } => {
            let fields = AppStoreVersionLocalizationFields {
                description: text_arg(description, description_file, "--description-file")?,
                keywords: text_arg(keywords, keywords_file, "--keywords-file")?,
                whats_new: text_arg(whats_new, whats_new_file, "--whats-new-file")?,
                promotional_text: text_arg(
                    promotional_text,
                    promotional_text_file,
                    "--promotional-text-file",
                )?,
                marketing_url,
                support_url,
            };
            lint_localization_fields(&fields)?;
            let attributes = AppStoreVersionLocalizationCreateAttributes {
                locale,
                fields: fields.clone(),
            };
            if dry_run_guard(dry_run, &attributes)? {
                return Ok(());
            }
            let created = client
                .create_app_store_version_localization(&app_store_version_id, attributes)
                .await?;
            verify_localization(client, &created.id, &fields, no_verify).await?;
            print_json(&created)
        }
        AppStoreVersionLocalizationsCommand::Update {
            id,
            description,
            description_file,
            keywords,
            keywords_file,
            whats_new,
            whats_new_file,
            promotional_text,
            promotional_text_file,
            marketing_url,
            support_url,
            no_verify,
        } => {
            let fields = AppStoreVersionLocalizationFields {
                description: text_arg(description, description_file, "--description-file")?,
                keywords: text_arg(keywords, keywords_file, "--keywords-file")?,
                whats_new: text_arg(whats_new, whats_new_file, "--whats-new-file")?,
                promotional_text: text_arg(
                    promotional_text,
                    promotional_text_file,
                    "--promotional-text-file",
                )?,
                marketing_url,
                support_url,
            };
            lint_localization_fields(&fields)?;
            if dry_run_guard(dry_run, &fields)? {
                return Ok(());
            }
            let updated = client
                .update_app_store_version_localization(&id, fields.clone())
                .await?;
            verify_localization(client, &updated.id, &fields, no_verify).await?;
            print_json(&updated)
        }
        AppStoreVersionLocalizationsCommand::Delete { id } => {
            if dry_run {
                println!("# dry run — would DELETE /v1/appStoreVersionLocalizations/{id}");
                return Ok(());
            }
            client.delete_app_store_version_localization(&id).await?;
            println!("deleted {id}");
            Ok(())
        }
    }
}

async fn run_app_screenshot_sets(
    client: &Client,
    command: AppScreenshotSetsCommand,
    dry_run: bool,
) -> Result<()> {
    match command {
        AppScreenshotSetsCommand::List {
            app_store_version_localization_id,
        } => print_json(
            &client
                .list_app_screenshot_sets(&app_store_version_localization_id)
                .await?,
        ),
        AppScreenshotSetsCommand::Create {
            app_store_version_localization_id,
            display_type,
        } => {
            let attributes = AppScreenshotSetCreateAttributes {
                screenshot_display_type: display_type.into(),
            };
            if dry_run_guard(dry_run, &attributes)? {
                return Ok(());
            }
            print_json(
                &client
                    .create_app_screenshot_set(&app_store_version_localization_id, attributes)
                    .await?,
            )
        }
        AppScreenshotSetsCommand::Delete { id } => {
            if dry_run {
                println!("# dry run — would DELETE /v1/appScreenshotSets/{id}");
                return Ok(());
            }
            client.delete_app_screenshot_set(&id).await?;
            println!("deleted {id}");
            Ok(())
        }
    }
}

async fn run_app_screenshots(
    client: &Client,
    command: AppScreenshotsCommand,
    dry_run: bool,
) -> Result<()> {
    match command {
        AppScreenshotsCommand::List {
            app_screenshot_set_id,
        } => print_json(&client.list_app_screenshots(&app_screenshot_set_id).await?),
        AppScreenshotsCommand::Upload {
            app_screenshot_set_id,
            file,
        } => {
            let file_name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .with_context(|| format!("{file:?} has no file name"))?;
            let bytes = std::fs::read(&file)
                .with_context(|| format!("reading screenshot file {file:?}"))?;

            if dry_run {
                println!(
                    "# dry run — would upload {file_name} ({} bytes) to appScreenshotSet {app_screenshot_set_id}",
                    bytes.len()
                );
                return Ok(());
            }

            print_json(
                &client
                    .upload_app_screenshot(&app_screenshot_set_id, file_name, bytes)
                    .await?,
            )
        }
        AppScreenshotsCommand::Delete { id } => {
            if dry_run {
                println!("# dry run — would DELETE /v1/appScreenshots/{id}");
                return Ok(());
            }
            client.delete_app_screenshot(&id).await?;
            println!("deleted {id}");
            Ok(())
        }
    }
}

/// Human-readable summary of a certificate, with the fields that decide
/// whether it is usable pulled to the top. Deliberately omits
/// `certificateContent`: it is large, and printing it invites pasting
/// certificate bodies into places they should not go.
#[derive(serde::Serialize)]
struct CertificateSummary {
    id: String,
    name: Option<String>,
    r#type: Option<String>,
    serial_number: Option<String>,
    expiration_date: Option<String>,
    /// Computed here rather than by Apple, which offers no such filter.
    expired: Option<bool>,
}

fn summarize(
    certificate: &smbcloud_ascapi_signing::certificate::Certificate,
) -> CertificateSummary {
    let expiration_date = certificate.attributes.expiration_date.clone();
    // Lexicographic comparison is sound for ISO-8601 UTC timestamps, which
    // is what Apple returns, and avoids a date-parsing dependency for a
    // field that is only ever displayed.
    let now = now_iso8601();
    let expired = expiration_date
        .as_ref()
        .map(|at| at.as_str() < now.as_str());

    CertificateSummary {
        id: certificate.id.clone(),
        name: certificate.attributes.name.clone(),
        r#type: certificate
            .attributes
            .certificate_type
            .map(|t| t.as_api_str().to_string()),
        serial_number: certificate.attributes.serial_number.clone(),
        expiration_date,
        expired,
    }
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SS`, for comparing against
/// Apple's ISO-8601 expiry strings without pulling in a date library.
fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Days since the epoch to a civil date, per Howard Hinnant's
    // days_from_civil inverse. Shifting the era to March makes the leap
    // day the last day of the year, which removes every special case.
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        y,
        m,
        d,
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Read-only throughout, so unlike its siblings it takes no `dry_run`:
/// there is no request body to preview.
async fn run_app_prices(client: &Client, command: AppPricesCommand) -> Result<()> {
    match command {
        AppPricesCommand::Schedule { app_id } => {
            print_json(&client.get_app_price_schedule(&app_id).await?)
        }
        AppPricesCommand::List {
            app_id,
            automatic,
            territory,
        } => {
            let kind = if automatic {
                PriceKind::Automatic
            } else {
                PriceKind::Manual
            };
            let prices = client
                .list_app_prices(&app_id, kind, territory.as_deref())
                .await?;
            print_json(&prices)
        }
    }
}

async fn run_certificates(
    client: &Client,
    command: CertificatesCommand,
    dry_run: bool,
) -> Result<()> {
    match command {
        CertificatesCommand::List { r#type } => {
            let mut certificates = client.list_certificates(r#type.map(Into::into)).await?;
            certificates.sort_by(|a, b| {
                a.attributes
                    .expiration_date
                    .cmp(&b.attributes.expiration_date)
            });
            let summaries: Vec<_> = certificates.iter().map(summarize).collect();
            print_json(&summaries)
        }

        CertificatesCommand::Create {
            r#type,
            out_dir,
            common_name,
        } => {
            let certificate_type: CertificateType = r#type.into();

            if dry_run {
                // Stop before generating a key: an unused RSA key pair on
                // disk is a small liability, and a dry run should have no
                // side effects at all.
                println!("# dry run — would generate an RSA 2048 key pair and request:");
                print_json(&serde_json::json!({
                    "certificateType": certificate_type.as_api_str(),
                    "commonName": common_name,
                    "outDir": out_dir,
                }))?;
                return Ok(());
            }

            let request = generate_certificate_request(&common_name)
                .context("generating the RSA 2048 key pair and CSR")?;

            std::fs::create_dir_all(&out_dir)
                .with_context(|| format!("creating {}", out_dir.display()))?;

            // Write the private key BEFORE calling Apple. If the request
            // fails we are left with an unused key, which is harmless; the
            // other order risks Apple issuing a certificate whose key we
            // then fail to persist, which is unrecoverable.
            let stem = certificate_type.as_api_str().to_lowercase();
            let key_path = out_dir.join(format!("{stem}.key.pem"));
            write_private_key(&key_path, request.private_key_pem())
                .with_context(|| format!("writing the private key to {}", key_path.display()))?;

            let attributes = CertificateCreateAttributes {
                csr_content: request.csr_pem().to_string(),
                certificate_type,
            };
            let certificate = client.create_certificate(attributes).await?;

            let cer_path = out_dir.join(format!("{stem}.cer"));
            if let Some(content) = certificate.attributes.certificate_content.as_deref() {
                let der =
                    base64_decode(content).context("decoding the certificate Apple returned")?;
                std::fs::write(&cer_path, der)
                    .with_context(|| format!("writing {}", cer_path.display()))?;
            }

            print_json(&serde_json::json!({
                "certificate": summarize(&certificate),
                "privateKey": key_path,
                "certificatePath": cer_path,
                "nextSteps": [
                    format!(
                        "openssl pkcs12 -export -inkey {} -in {} -out {}.p12",
                        key_path.display(), cer_path.display(), stem
                    ),
                    format!("security import {stem}.p12 -k ~/Library/Keychains/login.keychain-db -T /usr/bin/codesign"),
                    "security find-identity -v -p codesigning".to_string(),
                ],
            }))
        }

        CertificatesCommand::Revoke { id, confirm } => {
            if confirm != id {
                anyhow::bail!(
                    "refusing to revoke: pass --confirm {id} to acknowledge that this breaks \
                     every provisioning profile embedding this certificate, for the whole team"
                );
            }
            if dry_run {
                println!("# dry run — would revoke certificate {id}");
                return Ok(());
            }
            client.revoke_certificate(&id).await?;
            print_json(&serde_json::json!({ "revoked": id }))
        }
    }
}

async fn run_profiles(client: &Client, command: ProfilesCommand, dry_run: bool) -> Result<()> {
    use smbcloud_ascapi_frontend::profiles::{download_profile, ProfileSummary};
    use smbcloud_ascapi_frontend::time::now_iso8601;

    match command {
        ProfilesCommand::List {
            name,
            r#type,
            with_bundle_id,
        } => {
            let mut profiles = client
                .list_profiles(name.as_deref(), r#type.map(Into::into))
                .await?;
            profiles.sort_by(|a, b| {
                a.attributes
                    .expiration_date
                    .cmp(&b.attributes.expiration_date)
            });
            let now = now_iso8601();
            let mut summaries: Vec<_> = profiles
                .iter()
                .map(|profile| ProfileSummary::from_resource(profile, &now))
                .collect();
            if with_bundle_id {
                for summary in &mut summaries {
                    let bundle = client.get_profile_bundle_id(&summary.id).await?;
                    summary.bundle_identifier = bundle.attributes.identifier;
                    summary.bundle_id = Some(bundle.id);
                }
            }
            print_json(&summaries)
        }

        ProfilesCommand::Create {
            name,
            r#type,
            bundle_id,
            certificates,
            devices,
        } => {
            let profile_type: ProfileType = r#type.into();

            // Say so rather than dropping them silently: a caller who
            // passed devices to a store profile has misunderstood
            // something, and the created profile will not be what they
            // expect.
            if !devices.is_empty() && !profile_type.takes_devices() {
                eprintln!(
                    "note: {} does not take devices; ignoring the {} passed",
                    profile_type.as_api_str(),
                    devices.len()
                );
            }

            let relationships =
                ProfileCreateRelationships::new(profile_type, &bundle_id, &certificates, &devices);
            let attributes = ProfileCreateAttributes {
                name: name.clone(),
                profile_type,
            };

            if dry_run {
                print_json(&serde_json::json!({
                    "data": {
                        "type": "profiles",
                        "attributes": attributes,
                        "relationships": relationships,
                    }
                }))?;
                return Ok(());
            }

            let profile = client.create_profile(attributes, relationships).await?;
            print_json(&ProfileSummary::from_resource(&profile, &now_iso8601()))
        }

        ProfilesCommand::Download { id, output } => {
            if dry_run {
                println!(
                    "# dry run — would write profile {id} to {}",
                    output.display()
                );
                return Ok(());
            }
            let downloaded = download_profile(client, &id, &output)
                .await
                .map_err(anyhow::Error::msg)?;
            print_json(&downloaded)
        }

        ProfilesCommand::Delete { id, confirm } => {
            if confirm != id {
                anyhow::bail!(
                    "refusing to delete: pass --confirm {id} to acknowledge that signing \
                     against this profile stops working until a replacement exists"
                );
            }
            if dry_run {
                println!("# dry run — would delete profile {id}");
                return Ok(());
            }
            client.delete_profile(&id).await?;
            print_json(&serde_json::json!({ "deleted": id }))
        }
    }
}

/// Write a private key with owner-only permissions, set at creation time
/// rather than chmod'ed afterwards so the key is never briefly readable.
fn write_private_key(path: &std::path::Path, pem: &str) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(pem.as_bytes())?;
    file.sync_all()
}

fn base64_decode(input: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.decode(input.trim())?)
}

#[cfg(test)]
mod tests {
    use super::sales_report_as_json;
    use super::validate_version_string;
    use super::validate_whats_new;
    use super::WHATS_NEW_MAX_LEN;
    use super::{diff_fields, lint_localization_fields, text_arg, Cli};
    use clap::Parser;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use smbcloud_ascapi_aso::app_store_version_localization::{
        AppStoreVersionLocalizationAttributes, AppStoreVersionLocalizationFields,
    };
    use std::io::Write;

    fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("asc-test-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn text_arg_strips_one_trailing_newline_and_keeps_the_rest() {
        let path = temp_file("strip", "a & b\n\nline two\n");
        let text = text_arg(None, Some(path.clone()), "--description-file").unwrap();
        assert_eq!(text.as_deref(), Some("a & b\n\nline two"));
        std::fs::write(&path, "crlf\r\n").unwrap();
        let text = text_arg(None, Some(path.clone()), "--description-file").unwrap();
        assert_eq!(text.as_deref(), Some("crlf"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn text_arg_passes_inline_through_and_names_flag_on_error() {
        assert_eq!(
            text_arg(Some("x\n".into()), None, "--f")
                .unwrap()
                .as_deref(),
            Some("x\n")
        );
        let error = text_arg(None, Some("/nonexistent/asc.txt".into()), "--keywords-file")
            .unwrap_err()
            .to_string();
        assert!(error.contains("--keywords-file") && error.contains("/nonexistent/asc.txt"));
    }

    #[test]
    fn file_and_inline_flags_conflict() {
        let result = Cli::try_parse_from([
            "asc",
            "app-store-version-localizations",
            "update",
            "id",
            "--description",
            "a",
            "--description-file",
            "b.txt",
        ]);
        assert!(result.is_err());
        let result =
            Cli::try_parse_from(["asc", "testflight", "build-localizations", "update", "id"]);
        assert!(
            result.is_err(),
            "one of the two whats-new flags is required"
        );
        let result = Cli::try_parse_from([
            "asc",
            "testflight",
            "build-localizations",
            "update",
            "id",
            "--whats-new",
            "a",
            "--whats-new-file",
            "b.txt",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn lint_names_the_flag() {
        let fields = AppStoreVersionLocalizationFields {
            promotional_text: Some("a".repeat(171)),
            ..Default::default()
        };
        let error = lint_localization_fields(&fields).unwrap_err().to_string();
        assert!(error.starts_with("--promotional-text is 171"), "{error}");
    }

    fn stored(description: Option<&str>) -> AppStoreVersionLocalizationAttributes {
        AppStoreVersionLocalizationAttributes {
            locale: "en-US".into(),
            description: description.map(Into::into),
            keywords: None,
            marketing_url: None,
            promotional_text: None,
            support_url: None,
            whats_new: None,
        }
    }

    #[test]
    fn diff_fields_matches_and_mismatches() {
        let sent = AppStoreVersionLocalizationFields {
            description: Some("abc".into()),
            ..Default::default()
        };
        assert!(diff_fields(&sent, &stored(Some("abc"))).is_empty());
        assert_eq!(
            diff_fields(&sent, &stored(Some("abd"))),
            vec!["description"]
        );
        assert_eq!(diff_fields(&sent, &stored(None)), vec!["description"]);
        // Fields that were not sent are never compared.
        assert!(diff_fields(
            &AppStoreVersionLocalizationFields::default(),
            &stored(Some("x"))
        )
        .is_empty());
    }

    fn gzip(tsv: &str) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(tsv.as_bytes()).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn sales_report_preserves_headers_and_string_values() {
        let rows = sales_report_as_json(&gzip(
            "Provider Country\tUnits\tTitle\nUS\t12\t\"An app\twith tab\"\n",
        ))
        .unwrap();

        assert_eq!(
            rows,
            vec![serde_json::json!({
                "Provider Country": "US",
                "Units": "12",
                "Title": "An app\twith tab",
            })]
        );
    }

    #[test]
    fn sales_report_accepts_rows_with_missing_or_extra_fields() {
        let rows = sales_report_as_json(&gzip("Country\tUnits\nUS\nCA\t3\textra\n")).unwrap();

        assert_eq!(
            rows,
            vec![
                serde_json::json!({ "Country": "US" }),
                serde_json::json!({
                    "Country": "CA",
                    "Units": "3",
                    "extra_column_3": "extra",
                }),
            ]
        );
    }

    #[test]
    fn sales_report_rejects_invalid_gzip() {
        let error = sales_report_as_json(b"not a gzip file").unwrap_err();
        assert!(error
            .to_string()
            .contains("reading the TSV header from Apple's sales report"));
    }

    #[test]
    fn version_string_accepts_one_to_three_numbers() {
        for version_string in ["1", "1.1", "1.1.0", "12.345.6789"] {
            assert!(
                validate_version_string(version_string).is_ok(),
                "{version_string} should be accepted"
            );
        }
    }

    #[test]
    fn version_string_rejects_non_numeric_or_too_many_parts() {
        for version_string in [
            "", "1.1.0.0", "1..0", "1.1.", ".1", "v1.1", "1.1-beta", "1,1",
        ] {
            let error = validate_version_string(version_string)
                .expect_err(&format!("{version_string} should be rejected"));
            assert!(
                error.to_string().starts_with("invalid --version-string"),
                "{error}"
            );
        }
    }

    #[test]
    fn sales_report_disambiguates_blank_and_duplicate_headers() {
        let rows = sales_report_as_json(&gzip("Country\t\tCountry\nUS\t3\tCA\n")).unwrap();

        assert_eq!(
            rows,
            vec![serde_json::json!({
                "Country": "US",
                "column_2": "3",
                "Country_2": "CA",
            })]
        );
    }

    #[test]
    fn whats_new_accepts_empty_and_up_to_the_limit() {
        // Apple accepts the field at create time as null/empty, and a
        // note exactly at the cap is the boundary that must pass.
        assert!(validate_whats_new("").is_ok());
        let at_limit = "a".repeat(WHATS_NEW_MAX_LEN);
        assert!(validate_whats_new(&at_limit).is_ok(), "at-limit must pass");
    }

    #[test]
    fn whats_new_rejects_one_char_over_the_limit() {
        let too_long = "a".repeat(WHATS_NEW_MAX_LEN + 1);
        let error = validate_whats_new(&too_long).expect_err("over-limit must be rejected");
        let message = error.to_string();
        assert!(
            message.contains("--whats-new"),
            "should name the flag: {message}"
        );
        assert!(
            message.contains(&WHATS_NEW_MAX_LEN.to_string()),
            "should quote the limit: {message}"
        );
    }

    #[test]
    fn whats_new_counts_characters_not_bytes() {
        // "あ" is 3 bytes in UTF-8 but 1 character, so 4000 of them fits
        // the character-based limit even though it would blow a byte
        // budget. Make sure the validator agrees with ASC's character
        // semantics.
        let multibyte = "あ".repeat(WHATS_NEW_MAX_LEN);
        assert!(
            validate_whats_new(&multibyte).is_ok(),
            "char count, not byte count"
        );
    }
}
