//! The `ascapi --mcp` server.
//!
//! Holds no state: every tool resolves its App Store Connect credentials
//! from the environment per call, so an unconfigured server still starts
//! and still answers `tools/list`, which is how a client discovers what
//! configuration it needs.
//!
//! The tool names are supplied here rather than baked into the contract
//! module, so a host that re-exposes these tools can namespace them
//! without forking the implementations. Same arrangement as `xcrs` in
//! `smbcloud-cli`.

use crate::ascapi_mcp_tools;
use anyhow::{anyhow, Result};
use rmcp::{
    model::{Implementation, ServerCapabilities, ServerInfo},
    transport::stdio,
    ServerHandler, ServiceExt,
};

#[derive(Debug, Clone, Default)]
pub struct AscapiMcpServer;

impl AscapiMcpServer {
    pub fn new() -> Self {
        Self
    }
}

ascapi_mcp_tools!(
    AscapiMcpServer,
    "app_list",
    "app_get",
    "app_update",
    "app_info_list",
    "build_list",
    "bundle_id_list",
    "bundle_id_create",
    "app_store_version_list",
    "app_store_version_get",
    "app_store_version_create",
    "app_store_version_delete",
    "app_store_version_set_build",
    "app_info_localization_list",
    "app_info_localization_create",
    "app_info_localization_update",
    "app_info_localization_delete",
    "app_store_version_localization_list",
    "app_store_version_localization_create",
    "app_store_version_localization_update",
    "app_store_version_localization_delete",
    "app_screenshot_set_list",
    "app_screenshot_set_create",
    "app_screenshot_set_delete",
    "app_screenshot_list",
    "app_screenshot_upload",
    "app_screenshot_delete",
    "certificate_list",
    "certificate_create"
);

#[rmcp::tool_handler(router = Self::ascapi_tool_router())]
impl ServerHandler for AscapiMcpServer {
    fn get_info(&self) -> ServerInfo {
        let mut implementation = Implementation::from_build_env();
        implementation.name = "ascapi".to_string();
        implementation.version = env!("CARGO_PKG_VERSION").to_string();

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(implementation)
            .with_instructions(
                "ascapi exposes App Store Connect signing certificates (certificate_list, \
                 certificate_create). Certificates are issued by generating an RSA 2048 key \
                 pair locally and sending only a signing request to Apple, so the private key \
                 never leaves this machine and Apple keeps no copy of it: a lost key file \
                 makes its certificate permanently unusable. Revocation is deliberately not \
                 offered here, because revoking a distribution certificate invalidates every \
                 provisioning profile embedding it for the whole team at once; run \
                 `ascapi certificates revoke` from a terminal instead. Requires ASC_API_KEY, \
                 ASC_ISSUER_ID, and a readable .p8 key.",
            )
    }
}

/// Serve over stdio until the client disconnects.
///
/// stdout carries the JSON-RPC stream from here on, so nothing in this
/// path may print to it. Diagnostics belong on stderr.
pub async fn serve() -> Result<()> {
    let running = AscapiMcpServer::new()
        .serve(stdio())
        .await
        .map_err(|error| anyhow!("Failed to start ascapi MCP server: {error}"))?;
    running
        .waiting()
        .await
        .map_err(|error| anyhow!("ascapi MCP server stopped unexpectedly: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical tool names. Adding or renaming one is a
    /// public-contract change, and this list is what makes that
    /// deliberate rather than accidental.
    const CANONICAL_TOOL_NAMES: [&str; 28] = [
        "app_list",
        "app_get",
        "app_update",
        "app_info_list",
        "build_list",
        "bundle_id_list",
        "bundle_id_create",
        "app_store_version_list",
        "app_store_version_get",
        "app_store_version_create",
        "app_store_version_delete",
        "app_store_version_set_build",
        "app_info_localization_list",
        "app_info_localization_create",
        "app_info_localization_update",
        "app_info_localization_delete",
        "app_store_version_localization_list",
        "app_store_version_localization_create",
        "app_store_version_localization_update",
        "app_store_version_localization_delete",
        "app_screenshot_set_list",
        "app_screenshot_set_create",
        "app_screenshot_set_delete",
        "app_screenshot_list",
        "app_screenshot_upload",
        "app_screenshot_delete",
        "certificate_list",
        "certificate_create",
    ];

    /// Never a tool, at any point, for any caller.
    ///
    /// Revoking a signing certificate invalidates every provisioning
    /// profile embedding it, for every teammate and every CI job, at once
    /// and irreversibly. No confirmation string a model types on a human's
    /// behalf makes that safe. It lives on the command line only, where a
    /// person is the one at the keyboard.
    ///
    /// The scoped deletes below are a different matter and *are* exposed:
    /// a version, a localization, a screenshot set, or an image can all be
    /// recreated by re-running the tool that made them. They carry
    /// `destructive_hint` so a client can gate them on its own terms.
    const FORBIDDEN_TOOL_NAMES: [&str; 4] = [
        "certificate_revoke",
        "certificate_delete",
        "certificate_nuke",
        "app_delete",
    ];

    #[test]
    fn tool_router_exposes_exactly_the_canonical_tools() {
        let tools = AscapiMcpServer::ascapi_tool_router().list_all();

        assert_eq!(
            tools.len(),
            CANONICAL_TOOL_NAMES.len(),
            "expected exactly {} canonical tools, found {}",
            CANONICAL_TOOL_NAMES.len(),
            tools.len()
        );

        let mut actual: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        actual.sort_unstable();
        let mut expected = CANONICAL_TOOL_NAMES;
        expected.sort_unstable();
        assert_eq!(actual, expected);
    }

    #[test]
    fn no_irreversible_tool_is_ever_exposed() {
        let tools = AscapiMcpServer::ascapi_tool_router().list_all();
        for tool in &tools {
            assert!(
                !FORBIDDEN_TOOL_NAMES.contains(&tool.name.as_ref()),
                "{} is irreversible or team-wide and must not be reachable over MCP",
                tool.name
            );
        }
    }

    #[test]
    fn every_delete_tool_is_annotated_destructive() {
        // The naming convention is load-bearing: a client deciding whether
        // to prompt reads the annotation, not the name, so the two must
        // agree or a delete slips through a confirmation gate.
        let tools = AscapiMcpServer::ascapi_tool_router().list_all();
        let deletes: Vec<_> = tools
            .iter()
            .filter(|tool| tool.name.ends_with("_delete"))
            .collect();
        assert!(
            !deletes.is_empty(),
            "expected some delete tools; if they were removed, drop this test with them"
        );
        for tool in deletes {
            let annotations = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{} is missing annotations", tool.name));
            assert_eq!(
                annotations.destructive_hint,
                Some(true),
                "{} deletes something and must be annotated destructive",
                tool.name
            );
        }
    }

    #[test]
    fn read_only_tools_are_never_annotated_destructive() {
        let tools = AscapiMcpServer::ascapi_tool_router().list_all();
        for tool in &tools {
            let Some(annotations) = tool.annotations.as_ref() else {
                continue;
            };
            if annotations.read_only_hint == Some(true) {
                assert_ne!(
                    annotations.destructive_hint,
                    Some(true),
                    "{} claims to be read-only and destructive at once",
                    tool.name
                );
            }
        }
    }

    #[test]
    fn tool_names_carry_no_server_prefix() {
        let tools = AscapiMcpServer::ascapi_tool_router().list_all();
        for tool in &tools {
            assert!(
                !tool.name.starts_with("ascapi_") && !tool.name.starts_with("smb_"),
                "{} should not repeat a server prefix",
                tool.name
            );
        }
    }

    #[test]
    fn every_tool_has_a_meaningful_concise_title() {
        let tools = AscapiMcpServer::ascapi_tool_router().list_all();
        for tool in &tools {
            let title = tool
                .title
                .as_deref()
                .unwrap_or_else(|| panic!("{} is missing a top-level title", tool.name));
            assert!(!title.trim().is_empty(), "{} has a blank title", tool.name);
            assert!(
                title.len() <= 40,
                "{} title should be concise (<= 40 chars), got {title:?}",
                tool.name
            );
            assert_ne!(
                title,
                tool.name.as_ref(),
                "{} title should be a human-readable label, not the raw tool name",
                tool.name
            );
        }
    }

    #[test]
    fn every_tool_has_a_front_loaded_transparent_description() {
        let tools = AscapiMcpServer::ascapi_tool_router().list_all();
        for tool in &tools {
            let description = tool
                .description
                .as_deref()
                .unwrap_or_else(|| panic!("{} is missing a description", tool.name));

            for required in [
                "Purpose:",
                "When to use",
                "Behavior:",
                "Prerequisites:",
                "Failure modes:",
                "Limitations:",
            ] {
                assert!(
                    description.contains(required),
                    "{} description is missing the {required:?} section",
                    tool.name
                );
            }
            assert!(
                description.starts_with("Purpose:"),
                "{} description should front-load its purpose",
                tool.name
            );
            // Loosely bounded rather than snapshotted, so wording can
            // evolve without a brittle exact-text assertion.
            assert!(
                description.len() >= 150,
                "{} description is too terse for behavioral transparency ({} chars)",
                tool.name,
                description.len()
            );
        }
    }

    #[test]
    fn every_tool_has_internally_consistent_annotations() {
        let tools = AscapiMcpServer::ascapi_tool_router().list_all();
        for tool in &tools {
            let annotations = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{} is missing annotations", tool.name));
            let annotations_title = annotations
                .title
                .as_deref()
                .unwrap_or_else(|| panic!("{} annotations are missing a title", tool.name));
            assert_eq!(
                Some(annotations_title),
                tool.title.as_deref(),
                "{} top-level title and annotations title must agree",
                tool.name
            );
            assert!(
                !(annotations.read_only_hint == Some(true)
                    && annotations.destructive_hint == Some(true)),
                "{} annotations contradict: read_only_hint and destructive_hint are both true",
                tool.name
            );
        }
    }

    #[test]
    fn every_input_schema_property_has_a_description() {
        let tools = AscapiMcpServer::ascapi_tool_router().list_all();
        for tool in &tools {
            let Some(properties) = tool
                .input_schema
                .get("properties")
                .and_then(|value| value.as_object())
            else {
                continue;
            };
            for (property_name, property_schema) in properties {
                let description = property_schema
                    .get("description")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                assert!(
                    !description.trim().is_empty(),
                    "{}.{} is missing a parameter description",
                    tool.name,
                    property_name
                );
            }
        }
    }
}
