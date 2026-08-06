use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use smbcloud_ascapi_aso::app::AppUpdateAttributes;
use smbcloud_ascapi_aso::app_info_localization::{
    AppInfoLocalizationCreateAttributes, AppInfoLocalizationUpdateAttributes,
};
use smbcloud_ascapi_aso::app_screenshot_set::{
    AppScreenshotSetCreateAttributes, ScreenshotDisplayType,
};
use smbcloud_ascapi_aso::app_store_version::{AppStoreVersionCreateAttributes, Platform};
use smbcloud_ascapi_aso::app_store_version_localization::{
    AppStoreVersionLocalizationCreateAttributes, AppStoreVersionLocalizationFields,
};
use smbcloud_ascapi_aso::bundle_id::{BundleIdCreateAttributes, BundleIdPlatform};
use smbcloud_ascapi_aso::prelude::*;
use smbcloud_ascapi_core::{ApiKey, Client};
use smbcloud_ascapi_signing::certificate::{CertificateCreateAttributes, CertificateType};
use smbcloud_ascapi_signing::csr::generate_certificate_request;
use smbcloud_ascapi_signing::prelude::*;
use std::path::PathBuf;

/// Add/update App Store Connect app metadata (apps, app infos, app store
/// versions, and their localizations) from the terminal or a script.
#[derive(Parser)]
#[command(name = "ascapi", version, about)]
struct Cli {
    /// App Store Connect API key ID (Users and Access → Integrations → App
    /// Store Connect API). Not required with `--mcp`, which resolves
    /// credentials per tool call so an unconfigured server can still list
    /// its tools.
    #[arg(long, env = "ASC_API_KEY", required_unless_present = "mcp")]
    key_id: Option<String>,

    /// App Store Connect API issuer ID (same page as the key).
    #[arg(long, env = "ASC_ISSUER_ID", required_unless_present = "mcp")]
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
enum Command {
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
    /// Signing certificates: list what the team holds, issue a new one, or
    /// revoke an old one.
    Certificates {
        #[command(subcommand)]
        command: CertificatesCommand,
    },
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
    /// `GET /v1/apps/{id}/builds` — newest first, so the
    /// most recently uploaded build (e.g. after a fixplist re-upload) is
    /// first. Check `processingState` (VALID/INVALID/PROCESSING/FAILED).
    Builds { app_id: String },
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
    /// `DELETE /v1/appStoreVersions/{id}`.
    Delete { id: String },
    /// `PATCH /v1/appStoreVersions/{id}` — attach a `Build` (see
    /// `apps builds`) to this version, e.g. after fixing an
    /// `INVALID_BINARY` version with a re-uploaded build.
    SetBuild {
        id: String,
        #[arg(long)]
        build_id: String,
    },
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
        #[arg(long)]
        keywords: Option<String>,
        #[arg(long)]
        whats_new: Option<String>,
        #[arg(long)]
        promotional_text: Option<String>,
        #[arg(long)]
        marketing_url: Option<String>,
        #[arg(long)]
        support_url: Option<String>,
    },
    Update {
        id: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        keywords: Option<String>,
        #[arg(long)]
        whats_new: Option<String>,
        #[arg(long)]
        promotional_text: Option<String>,
        #[arg(long)]
        marketing_url: Option<String>,
        #[arg(long)]
        support_url: Option<String>,
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.mcp {
        return smbcloud_ascapi_mcp::serve().await;
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

    let api_key = ApiKey::from_p8_file(&key_id, &issuer_id, &private_key_path)
        .with_context(|| format!("loading App Store Connect API key from {private_key_path:?}"))?;
    let client = Client::new(api_key);

    let Some(command) = cli.command else {
        anyhow::bail!(
            "no subcommand given; run `ascapi --help`, or `ascapi --mcp` for the MCP server"
        )
    };

    match command {
        Command::Apps { command } => run_apps(&client, command, cli.dry_run).await,
        Command::BundleIds { command } => run_bundle_ids(&client, command, cli.dry_run).await,
        Command::AppStoreVersions { command } => {
            run_app_store_versions(&client, command, cli.dry_run).await
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
        Command::Certificates { command } => run_certificates(&client, command, cli.dry_run).await,
    }
}

fn dirs_home() -> PathBuf {
    // Avoids pulling in the `dirs` crate for a single lookup; $HOME is set
    // on every platform this CLI targets (macOS/Linux CI runners).
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

async fn run_apps(client: &Client, command: AppsCommand, dry_run: bool) -> Result<()> {
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
        AppsCommand::Builds { app_id } => print_json(&client.list_builds(&app_id).await?),
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
        AppStoreVersionsCommand::Delete { id } => {
            if dry_run {
                println!("# dry run — would DELETE /v1/appStoreVersions/{id}");
                return Ok(());
            }
            client.delete_app_store_version(&id).await?;
            println!("deleted {id}");
            Ok(())
        }
        AppStoreVersionsCommand::SetBuild { id, build_id } => {
            if dry_run {
                println!(
                    "# dry run — would PATCH /v1/appStoreVersions/{id} relationships.build → {build_id}"
                );
                return Ok(());
            }
            print_json(&client.set_app_store_version_build(&id, &build_id).await?)
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
            keywords,
            whats_new,
            promotional_text,
            marketing_url,
            support_url,
        } => {
            let attributes = AppStoreVersionLocalizationCreateAttributes {
                locale,
                fields: AppStoreVersionLocalizationFields {
                    description,
                    keywords,
                    whats_new,
                    promotional_text,
                    marketing_url,
                    support_url,
                },
            };
            if dry_run_guard(dry_run, &attributes)? {
                return Ok(());
            }
            print_json(
                &client
                    .create_app_store_version_localization(&app_store_version_id, attributes)
                    .await?,
            )
        }
        AppStoreVersionLocalizationsCommand::Update {
            id,
            description,
            keywords,
            whats_new,
            promotional_text,
            marketing_url,
            support_url,
        } => {
            let fields = AppStoreVersionLocalizationFields {
                description,
                keywords,
                whats_new,
                promotional_text,
                marketing_url,
                support_url,
            };
            if dry_run_guard(dry_run, &fields)? {
                return Ok(());
            }
            print_json(
                &client
                    .update_app_store_version_localization(&id, fields)
                    .await?,
            )
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
