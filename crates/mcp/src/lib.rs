//! App Store Connect MCP contract.
//!
//! Defines the canonical tool set exposed by `ascapi --mcp`. Each name is
//! supplied by the embedder through [`crate::ascapi_mcp_tools`], so the
//! standalone binary and any host that re-exposes this contract share one
//! implementation and one public surface. Same arrangement `xcrs` uses in
//! `smbcloud-cli`.
//!
//! # Two rules this server is built around
//!
//! **stdout belongs to the protocol.** The stdio transport carries
//! JSON-RPC on stdout, so a stray `println!` anywhere beneath a tool call
//! corrupts the stream, and the failure surfaces as a parse error in the
//! client a long way from its cause. Nothing in this path may write to
//! stdout; diagnostics go to stderr.
//!
//! **Tool results are read by a model.** Private keys and raw certificate
//! bodies never appear in a result; see
//! [`smbcloud_ascapi_frontend`] for where that is enforced.
//!
//! # What is deliberately absent
//!
//! Certificate revocation. `ascapi certificates revoke` exists on the
//! command line and is intentionally not a tool: revoking a distribution
//! certificate invalidates every provisioning profile embedding it, for
//! every teammate and every CI job, at once and irreversibly. That is not
//! an action a language model should take from a prompt, and no
//! confirmation string typed by the model rather than the human makes it
//! safer. Deletes that are scoped and recreatable (a version, a
//! localization, a screenshot) are exposed, annotated destructive.

pub mod requests;
pub mod server;

pub use requests::*;
pub use server::{serve, AscapiMcpServer};

#[macro_export]
macro_rules! ascapi_mcp_tools {
    (
        $server:ty,
        $app_list_name:literal,
        $app_get_name:literal,
        $app_update_name:literal,
        $app_info_list_name:literal,
        $build_list_name:literal,
        $bundle_id_list_name:literal,
        $bundle_id_create_name:literal,
        $app_store_version_list_name:literal,
        $app_store_version_get_name:literal,
        $app_store_version_create_name:literal,
        $app_store_version_delete_name:literal,
        $app_store_version_set_build_name:literal,
        $app_info_localization_list_name:literal,
        $app_info_localization_create_name:literal,
        $app_info_localization_update_name:literal,
        $app_info_localization_delete_name:literal,
        $app_store_version_localization_list_name:literal,
        $app_store_version_localization_create_name:literal,
        $app_store_version_localization_update_name:literal,
        $app_store_version_localization_delete_name:literal,
        $app_screenshot_set_list_name:literal,
        $app_screenshot_set_create_name:literal,
        $app_screenshot_set_delete_name:literal,
        $app_screenshot_list_name:literal,
        $app_screenshot_upload_name:literal,
        $app_screenshot_delete_name:literal,
        $certificate_list_name:literal,
        $certificate_create_name:literal
    ) => {
        #[::rmcp::tool_router(router = ascapi_tool_router, vis = "pub(crate)")]
        impl $server {
            fn client() -> ::std::result::Result<::smbcloud_ascapi_core::Client, ::rmcp::model::ErrorData> {
                let api_key = ::smbcloud_ascapi_frontend::api_key_from_env()
                    .map_err(|error| ::rmcp::model::ErrorData::invalid_request(error, None))?;
                Ok(::smbcloud_ascapi_core::Client::new(api_key))
            }

            #[::rmcp::tool(
                name = $app_list_name,
                title = "List apps",
                annotations(title = "List apps", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: find the App Store Connect apps on this account, and translate a bundle identifier into the numeric app id every other tool wants. When to use vs siblings: call this first when you only know a bundle identifier; use app_get once you have an id and want one app's full attributes. Behavior: returns each app's id, name, bundle identifier, SKU, and primary locale, optionally filtered to one bundle identifier. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: returns apps only, not their versions, builds, or localizations, each of which has its own tool."
            )]
            async fn app_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppListRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .list_apps(request.bundle_id.as_deref())
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_get_name,
                title = "Get one app",
                annotations(title = "Get one app", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: fetch one app's full attributes by its App Store Connect id. When to use vs siblings: use app_list when you have a bundle identifier rather than an id, and app_info_list when you want the AppInfo records that hold localized names. Behavior: returns the single app resource with its attributes. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: attributes only; related resources such as versions and builds are fetched with their own tools."
            )]
            async fn app_get(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppIdRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .get_app(&request.app_id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_update_name,
                title = "Update app attributes",
                annotations(title = "Update app attributes", read_only_hint = false, destructive_hint = false, idempotent_hint = true),
                description = "Purpose: change an app's primary locale or content rights declaration. When to use vs siblings: this edits the app record itself; localized names and subtitles live on AppInfo localizations and are edited with app_info_localization_update. Behavior: sends only the fields provided, leaving the rest untouched, and returns the updated app. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: only these two attributes are editable through this endpoint; most app metadata belongs to a version or a localization instead."
            )]
            async fn app_update(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppUpdateRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let attributes = ::smbcloud_ascapi_aso::app::AppUpdateAttributes {
                    primary_locale: request.primary_locale,
                    content_rights_declaration: request.content_rights_declaration,
                };
                let value = client
                    .update_app(&request.app_id, attributes)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_info_list_name,
                title = "List app infos",
                annotations(title = "List app infos", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: list an app's AppInfo records, which are the containers for localized names, subtitles, and privacy policy URLs. When to use vs siblings: call this to get the app_info_id that app_info_localization_list and app_info_localization_create need. Behavior: returns each AppInfo with its state and id. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: an app typically has one editable AppInfo plus historical ones for shipped versions, and this returns all of them without saying which is editable."
            )]
            async fn app_info_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppIdRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .list_app_infos(&request.app_id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $build_list_name,
                title = "List builds",
                annotations(title = "List builds", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: list the builds uploaded for an app, so one can be attached to a version. When to use vs siblings: pair with app_store_version_set_build, which needs a build id from here. Behavior: returns each build's id, version string, upload date, and processing state. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: a build that is still processing cannot be attached yet, and this reports that state rather than waiting for it."
            )]
            async fn build_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppIdRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .list_builds(&request.app_id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $bundle_id_list_name,
                title = "List bundle IDs",
                annotations(title = "List bundle IDs", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: list the bundle identifiers registered to the team, or check whether one exists before trying to register it. When to use vs siblings: call this before bundle_id_create, which fails on an identifier that already exists. Behavior: returns each bundle ID's id, identifier, name, and platform, optionally filtered to an exact identifier. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: registration state only; whether an app exists for the identifier is a separate question answered by app_list."
            )]
            async fn bundle_id_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::BundleIdListRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .list_bundle_ids(request.identifier.as_deref())
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $bundle_id_create_name,
                title = "Register a bundle ID",
                annotations(title = "Register a bundle ID", read_only_hint = false, destructive_hint = false, idempotent_hint = false),
                description = "Purpose: register a new reverse-DNS bundle identifier with the developer account. When to use vs siblings: call bundle_id_list first, since registering an identifier that already exists fails. Behavior: registers the identifier under the given platform namespace and returns the created resource. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: identifiers cannot be renamed or deleted through this API once registered, so a typo here is permanent; visionOS apps register as ios because visionOS shares the iOS namespace."
            )]
            async fn bundle_id_create(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::BundleIdCreateRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let platform = ::smbcloud_ascapi_frontend::bundle_id_platform_from_str(&request.platform)
                    .map_err(|error| ::rmcp::model::ErrorData::invalid_request(error, None))?;
                let attributes = ::smbcloud_ascapi_aso::bundle_id::BundleIdCreateAttributes {
                    identifier: request.identifier,
                    name: request.name,
                    platform,
                };
                let value = client
                    .create_bundle_id(attributes)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_store_version_list_name,
                title = "List app store versions",
                annotations(title = "List app store versions", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: list an app's per-platform version records, which are what the store actually reviews and releases. When to use vs siblings: use this to find the version id that the localization and build tools need. Behavior: returns each version's id, platform, version string, and app store state. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: versions are per platform, so one app can have several concurrent editable versions and this returns all of them."
            )]
            async fn app_store_version_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppStoreVersionListRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let platform = match request.platform.as_deref() {
                    Some(value) => Some(
                        ::smbcloud_ascapi_frontend::platform_from_str(value)
                            .map_err(|error| ::rmcp::model::ErrorData::invalid_request(error, None))?,
                    ),
                    None => None,
                };
                let value = client
                    .list_app_store_versions(&request.app_id, platform)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_store_version_get_name,
                title = "Get one app store version",
                annotations(title = "Get one app store version", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: fetch one App Store Version's attributes by id. When to use vs siblings: use app_store_version_list when you have an app id rather than a version id. Behavior: returns the single version resource with its platform, version string, and release state. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: attributes only; its localizations and attached build are fetched with their own tools."
            )]
            async fn app_store_version_get(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::IdRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .get_app_store_version(&request.id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_store_version_create_name,
                title = "Create an app store version",
                annotations(title = "Create an app store version", read_only_hint = false, destructive_hint = false, idempotent_hint = false),
                description = "Purpose: create a new version record for an app on a given platform, which is how a new platform such as visionOS is added to an existing app. When to use vs siblings: call app_store_version_list first, because an app can hold only one editable version per platform at a time. Behavior: creates the version and returns it. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: creating a version does not submit anything; localizations, screenshots, and a build still have to be attached before review."
            )]
            async fn app_store_version_create(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppStoreVersionCreateRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let platform = ::smbcloud_ascapi_frontend::platform_from_str(&request.platform)
                    .map_err(|error| ::rmcp::model::ErrorData::invalid_request(error, None))?;
                let attributes = ::smbcloud_ascapi_aso::app_store_version::AppStoreVersionCreateAttributes {
                    platform,
                    version_string: request.version_string,
                    copyright: request.copyright,
                };
                let value = client
                    .create_app_store_version(&request.app_id, attributes)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_store_version_delete_name,
                title = "Delete an app store version",
                annotations(title = "Delete an app store version", read_only_hint = false, destructive_hint = true, idempotent_hint = true),
                description = "Purpose: delete an editable App Store Version record. When to use vs siblings: use this to discard a version created by mistake, typically after app_store_version_list shows a duplicate. Behavior: deletes the version and everything attached to it, including its localizations and screenshots, then returns the deleted id. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: destructive and not undoable, though the version can be recreated; a version already submitted or released cannot be deleted and Apple rejects the attempt."
            )]
            async fn app_store_version_delete(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::IdRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                client
                    .delete_app_store_version(&request.id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&::serde_json::json!({ "deleted": request.id }))?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_store_version_set_build_name,
                title = "Attach a build to a version",
                annotations(title = "Attach a build to a version", read_only_hint = false, destructive_hint = false, idempotent_hint = true),
                description = "Purpose: attach an uploaded build to an App Store Version, which a version needs before it can be submitted. When to use vs siblings: get the build id from build_list and the version id from app_store_version_list. Behavior: points the version's build relationship at the given build, replacing whatever was attached. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: the build must have finished processing, and its platform must match the version's; Apple rejects the pairing otherwise."
            )]
            async fn app_store_version_set_build(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::SetBuildRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                client
                    .set_app_store_version_build(&request.version_id, &request.build_id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&::serde_json::json!({ "versionId": request.version_id, "buildId": request.build_id }))?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_info_localization_list_name,
                title = "List app info localizations",
                annotations(title = "List app info localizations", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: list the localized names and subtitles attached to an AppInfo. When to use vs siblings: this covers name, subtitle, and privacy policy URL; description, keywords, and release notes live on version localizations instead. Behavior: returns each localization's id, locale, name, and subtitle. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: only the editable AppInfo accepts changes, and this does not distinguish editable from historical records."
            )]
            async fn app_info_localization_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppInfoLocalizationListRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .list_app_info_localizations(&request.app_info_id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_info_localization_create_name,
                title = "Add an app info localization",
                annotations(title = "Add an app info localization", read_only_hint = false, destructive_hint = false, idempotent_hint = false),
                description = "Purpose: add a localized app name and subtitle for a new locale. When to use vs siblings: use app_info_localization_update when the locale already exists, since creating a duplicate fails. Behavior: creates the localization under the AppInfo and returns it. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: the subtitle is capped at 30 characters and the name at 30 by App Store Connect, which rejects longer values rather than truncating them."
            )]
            async fn app_info_localization_create(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppInfoLocalizationCreateRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let attributes = ::smbcloud_ascapi_aso::app_info_localization::AppInfoLocalizationCreateAttributes {
                    locale: request.locale,
                    name: request.name,
                    subtitle: request.subtitle,
                    privacy_policy_url: request.privacy_policy_url,
                };
                let value = client
                    .create_app_info_localization(&request.app_info_id, attributes)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_info_localization_update_name,
                title = "Edit an app info localization",
                annotations(title = "Edit an app info localization", read_only_hint = false, destructive_hint = false, idempotent_hint = true),
                description = "Purpose: change the localized name, subtitle, or privacy policy URL for a locale that already exists. When to use vs siblings: use app_info_localization_create for a locale that has no record yet. Behavior: sends only the fields provided and leaves the others untouched, then returns the updated localization. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: the locale itself cannot be changed; delete and recreate to move copy to a different locale."
            )]
            async fn app_info_localization_update(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::AppInfoLocalizationUpdateRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let attributes = ::smbcloud_ascapi_aso::app_info_localization::AppInfoLocalizationUpdateAttributes {
                    name: request.name,
                    subtitle: request.subtitle,
                    privacy_policy_url: request.privacy_policy_url,
                };
                let value = client
                    .update_app_info_localization(&request.id, attributes)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_info_localization_delete_name,
                title = "Delete an app info localization",
                annotations(title = "Delete an app info localization", read_only_hint = false, destructive_hint = true, idempotent_hint = true),
                description = "Purpose: remove a locale's app name and subtitle. When to use vs siblings: use this to drop a language from the product page; use app_info_localization_update to change copy rather than remove it. Behavior: deletes the localization and returns the deleted id. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: destructive but recreatable with app_info_localization_create; the app's primary locale cannot be deleted and Apple rejects the attempt."
            )]
            async fn app_info_localization_delete(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::IdRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                client
                    .delete_app_info_localization(&request.id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&::serde_json::json!({ "deleted": request.id }))?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_store_version_localization_list_name,
                title = "List version localizations",
                annotations(title = "List version localizations", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: list the per-locale description, keywords, promotional text, and release notes for a version. When to use vs siblings: this is the version-scoped copy; the app name and subtitle live on AppInfo localizations instead. Behavior: returns each localization's id, locale, and copy fields. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: screenshots hang off these localizations but are listed separately with app_screenshot_set_list."
            )]
            async fn app_store_version_localization_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::VersionLocalizationListRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .list_app_store_version_localizations(&request.version_id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_store_version_localization_create_name,
                title = "Add a version localization",
                annotations(title = "Add a version localization", read_only_hint = false, destructive_hint = false, idempotent_hint = false),
                description = "Purpose: add the description, keywords, and release notes for a locale on a specific version. When to use vs siblings: use the update tool when the locale already exists on this version. Behavior: creates the localization with whichever fields are provided and returns it. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: App Store Connect enforces field limits, notably 4000 characters of description, 100 for keywords, and 170 for promotional text, and rejects longer values outright."
            )]
            async fn app_store_version_localization_create(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::VersionLocalizationCreateRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let fields = ::smbcloud_ascapi_aso::app_store_version_localization::AppStoreVersionLocalizationFields {
                    description: request.description,
                    keywords: request.keywords,
                    marketing_url: request.marketing_url,
                    promotional_text: request.promotional_text,
                    support_url: request.support_url,
                    whats_new: request.whats_new,
                };
                let attributes = ::smbcloud_ascapi_aso::app_store_version_localization::AppStoreVersionLocalizationCreateAttributes {
                    locale: request.locale,
                    fields,
                };
                let value = client
                    .create_app_store_version_localization(&request.version_id, attributes)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_store_version_localization_update_name,
                title = "Edit a version localization",
                annotations(title = "Edit a version localization", read_only_hint = false, destructive_hint = false, idempotent_hint = true),
                description = "Purpose: change the description, keywords, promotional text, or release notes for an existing locale on a version. When to use vs siblings: promotional text is the one field editable without shipping a new build, which makes this the tool for timely copy changes. Behavior: sends only the fields provided and leaves the rest untouched. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: which fields are editable depends on the version's review state, and Apple rejects edits to a version under review."
            )]
            async fn app_store_version_localization_update(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::VersionLocalizationUpdateRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let fields = ::smbcloud_ascapi_aso::app_store_version_localization::AppStoreVersionLocalizationFields {
                    description: request.description,
                    keywords: request.keywords,
                    marketing_url: request.marketing_url,
                    promotional_text: request.promotional_text,
                    support_url: request.support_url,
                    whats_new: request.whats_new,
                };
                let value = client
                    .update_app_store_version_localization(&request.id, fields)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_store_version_localization_delete_name,
                title = "Delete a version localization",
                annotations(title = "Delete a version localization", read_only_hint = false, destructive_hint = true, idempotent_hint = true),
                description = "Purpose: remove a locale's copy from a version. When to use vs siblings: use the update tool to change copy; use this only to drop a language from this version entirely. Behavior: deletes the localization along with its screenshot sets and returns the deleted id. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: destructive and takes the locale's screenshots with it, though everything can be recreated; the primary locale cannot be removed."
            )]
            async fn app_store_version_localization_delete(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::IdRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                client
                    .delete_app_store_version_localization(&request.id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&::serde_json::json!({ "deleted": request.id }))?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_screenshot_set_list_name,
                title = "List screenshot sets",
                annotations(title = "List screenshot sets", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: list the per-device-class screenshot buckets on a version localization. When to use vs siblings: each set holds the images for one display type, so call this to find the set id that app_screenshot_list and app_screenshot_upload need. Behavior: returns each set's id and display type. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: reports the sets, not how many images each holds or whether the set satisfies Apple's per-device requirements."
            )]
            async fn app_screenshot_set_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::ScreenshotSetListRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .list_app_screenshot_sets(&request.localization_id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_screenshot_set_create_name,
                title = "Create a screenshot set",
                annotations(title = "Create a screenshot set", read_only_hint = false, destructive_hint = false, idempotent_hint = false),
                description = "Purpose: create the bucket that holds screenshots for one device class on a version localization. When to use vs siblings: call app_screenshot_set_list first, since a localization holds at most one set per display type. Behavior: creates the set for the given display type and returns it, ready for app_screenshot_upload. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: the display type must be one App Store Connect recognizes, and image dimensions are validated later at upload rather than here."
            )]
            async fn app_screenshot_set_create(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::ScreenshotSetCreateRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let display_type = ::smbcloud_ascapi_frontend::display_type_from_str(&request.display_type)
                    .map_err(|error| ::rmcp::model::ErrorData::invalid_request(error, None))?;
                let attributes = ::smbcloud_ascapi_aso::app_screenshot_set::AppScreenshotSetCreateAttributes {
                    screenshot_display_type: display_type,
                };
                let value = client
                    .create_app_screenshot_set(&request.localization_id, attributes)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_screenshot_set_delete_name,
                title = "Delete a screenshot set",
                annotations(title = "Delete a screenshot set", read_only_hint = false, destructive_hint = true, idempotent_hint = true),
                description = "Purpose: remove a device class's screenshot bucket and every image in it. When to use vs siblings: use app_screenshot_delete to remove a single image instead of the whole set. Behavior: deletes the set and its screenshots, then returns the deleted id. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: destructive and takes every image with it, though the set and its images can be recreated by uploading again."
            )]
            async fn app_screenshot_set_delete(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::IdRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                client
                    .delete_app_screenshot_set(&request.id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&::serde_json::json!({ "deleted": request.id }))?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_screenshot_list_name,
                title = "List screenshots",
                annotations(title = "List screenshots", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: list the images in a screenshot set, with their upload and processing state. When to use vs siblings: call this after app_screenshot_upload to confirm an image finished processing, since upload returns before Apple has validated the file. Behavior: returns each screenshot's id, file name, size, and asset delivery state. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: an image that failed Apple's dimension checks appears here with an error state rather than being absent, so the state is worth reading."
            )]
            async fn app_screenshot_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::ScreenshotListRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let value = client
                    .list_app_screenshots(&request.screenshot_set_id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_screenshot_upload_name,
                title = "Upload a screenshot",
                annotations(title = "Upload a screenshot", read_only_hint = false, destructive_hint = false, idempotent_hint = false),
                description = "Purpose: upload one image into a screenshot set, running the whole three-step reservation, upload, and commit dance in a single call. When to use vs siblings: create the set with app_screenshot_set_create first, then verify with app_screenshot_list, since Apple validates asynchronously. Behavior: reads the file from this machine, reserves the asset, PUTs the bytes to the pre-signed URL, and commits it with its checksum. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: an unreadable path fails before any network call; a wrong image size is accepted here and rejected later, surfacing in app_screenshot_list. Limitations: the path is read by this process, so it must be reachable from wherever the server runs, not from the client's machine."
            )]
            async fn app_screenshot_upload(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::ScreenshotUploadRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let path = ::std::path::PathBuf::from(&request.file_path);
                let bytes = ::std::fs::read(&path).map_err(|error| {
                    ::rmcp::model::ErrorData::invalid_request(
                        format!("reading {}: {error}", path.display()),
                        None,
                    )
                })?;
                let file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .ok_or_else(|| {
                        ::rmcp::model::ErrorData::invalid_request(
                            format!("{} has no file name", path.display()),
                            None,
                        )
                    })?;
                let value = client
                    .upload_app_screenshot(&request.screenshot_set_id, file_name, bytes)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&value)?,
                ]))
            }

            #[::rmcp::tool(
                name = $app_screenshot_delete_name,
                title = "Delete a screenshot",
                annotations(title = "Delete a screenshot", read_only_hint = false, destructive_hint = true, idempotent_hint = true),
                description = "Purpose: remove one uploaded screenshot. When to use vs siblings: use app_screenshot_set_delete to clear an entire device class at once. Behavior: deletes the image and returns the deleted id. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: destructive but trivially recoverable by uploading the file again, which makes it the safest of the delete tools."
            )]
            async fn app_screenshot_delete(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::IdRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                client
                    .delete_app_screenshot(&request.id)
                    .await
                    .map_err(|error| ::rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&::serde_json::json!({ "deleted": request.id }))?,
                ]))
            }

            #[::rmcp::tool(
                name = $certificate_list_name,
                title = "List signing certificates",
                annotations(title = "List signing certificates", read_only_hint = true, idempotent_hint = true),
                description = "Purpose: report every signing certificate the team holds, with the expiry of each, so an agent can tell what is usable before a build is attempted. When to use vs siblings: call this first to see whether a valid certificate already exists; use certificate_create only when none of the required type exists, or the existing one has expired. Behavior: returns id, name, type, serial number, expiry, and a computed expired flag, soonest expiry first. The flag is computed here because App Store Connect offers no filter for it and returns expired certificates alongside valid ones. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: missing or unreadable credentials return an invalid-request error naming what is absent; App Store Connect errors are returned verbatim with their status. Limitations: certificate bodies are omitted deliberately, and this reports certificates only, not provisioning profiles, so a valid certificate here does not by itself mean a build will sign."
            )]
            async fn certificate_list(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::CertificateListRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let filter = match request.certificate_type.as_deref() {
                    Some(value) => Some(
                        ::smbcloud_ascapi_frontend::certificate_type_from_str(value)
                            .map_err(|error| ::rmcp::model::ErrorData::invalid_request(error, None))?,
                    ),
                    None => None,
                };
                let mut certificates = client.list_certificates(filter).await.map_err(|error| {
                    ::rmcp::model::ErrorData::internal_error(error.to_string(), None)
                })?;
                certificates.sort_by(|a, b| {
                    a.attributes.expiration_date.cmp(&b.attributes.expiration_date)
                });
                let now = ::smbcloud_ascapi_frontend::now_iso8601();
                let summaries: Vec<_> = certificates
                    .iter()
                    .map(|certificate| ::smbcloud_ascapi_frontend::CertificateSummary::from_resource(certificate, &now))
                    .collect();
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&summaries)?,
                ]))
            }

            #[::rmcp::tool(
                name = $certificate_create_name,
                title = "Issue a signing certificate",
                annotations(title = "Issue a signing certificate", read_only_hint = false, destructive_hint = false, idempotent_hint = false),
                description = "Purpose: issue a new Apple signing certificate end to end, generating the key pair locally and writing both halves to disk, so a machine can sign without anyone visiting the developer portal. When to use vs siblings: call certificate_list first and use this only when no valid certificate of the needed type exists; an expired certificate cannot be renewed, so replacing one also means calling this. Behavior: generates an RSA 2048 key pair, the only kind Apple accepts, writes the private key with owner-only permissions, sends a signing request, then writes the issued certificate beside the key and returns both paths. The key is written before the request is sent, so a failure mid-flight leaves an unused key rather than a certificate whose key was never saved. Prerequisites: ASC_API_KEY, ASC_ISSUER_ID, and a readable .p8 key at ASC_PRIVATE_KEY_PATH or ~/.appstoreconnect/private_keys/AuthKey_<key-id>.p8. Failure modes: Apple caps how many certificates of each type a team may hold and returns an error at the cap, which is resolved by revoking one from the command line, never from here. Limitations: the response carries paths, never key material; Apple keeps no copy of the private key, so losing the file makes the certificate permanently useless."
            )]
            async fn certificate_create(
                &self,
                parameters: ::rmcp::handler::server::wrapper::Parameters<$crate::CertificateCreateRequest>,
            ) -> ::std::result::Result<::rmcp::model::CallToolResult, ::rmcp::model::ErrorData> {
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_aso::prelude::*;
                #[allow(unused_imports)]
                use ::smbcloud_ascapi_signing::prelude::*;
                let client = Self::client()?;
                let request = parameters.0;
                let certificate_type = ::smbcloud_ascapi_frontend::certificate_type_from_str(&request.certificate_type)
                    .map_err(|error| ::rmcp::model::ErrorData::invalid_request(error, None))?;
                let common_name = request
                    .common_name
                    .unwrap_or_else(|| "smbcloud-ascapi".to_string());
                let out_dir = ::std::path::PathBuf::from(&request.out_dir);
                let outcome = ::smbcloud_ascapi_frontend::issue_certificate(
                    &client,
                    certificate_type,
                    &common_name,
                    &out_dir,
                )
                .await
                .map_err(|error| ::rmcp::model::ErrorData::internal_error(error, None))?;
                Ok(::rmcp::model::CallToolResult::success(vec![
                    ::rmcp::model::ContentBlock::json(&outcome)?,
                ]))
            }
        }
    };
}
