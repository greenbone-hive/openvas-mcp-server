//! Assets toolset: host assets, operating-system assets and the generic
//! type-scoped asset catalog.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::error::gateway_tool_error;
use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, GetByIdParams, ListParams, create_resource, delete_resource_plain, get_passthrough,
    get_passthrough_query, json_result, list_summarized, list_summarized_query, update_resource,
};

const HOST_ROW_KEYS: &[&str] = &["id", "name", "ip", "hostname", "severity", "os"];
const OS_ROW_KEYS: &[&str] = &[
    "id",
    "name",
    "title",
    "hostsCount",
    "latestSeverity",
    "highestSeverity",
    "averageSeverity",
];
const GENERIC_ROW_KEYS: &[&str] = &[
    "id",
    "name",
    "type",
    "value",
    "severity",
    "ip",
    "hostname",
    "os",
    "hostsCount",
];

#[tool_router(router = assets_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List host assets discovered by scans, with highest severity and OS.
    #[tool(
        name = "openvas_list_asset_hosts",
        annotations(title = "List host assets", read_only_hint = true)
    )]
    pub async fn list_asset_hosts(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "hosts",
            "hosts",
            HOST_ROW_KEYS,
            &params,
            "listing host assets",
        )
        .await
    }

    /// Get one host asset by UUID.
    #[tool(
        name = "openvas_get_asset_host",
        annotations(title = "Get host asset", read_only_hint = true)
    )]
    pub async fn get_asset_host(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["hosts", &params.id],
            "fetching the host asset",
        )
        .await
    }

    /// Create a host asset by IP address or hostname. Not idempotent.
    #[tool(
        name = "openvas_create_asset_host",
        annotations(
            title = "Create host asset",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_asset_host(
        &self,
        Parameters(params): Parameters<CreateHostParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("value", params.value)
            .set_opt("comment", params.comment);
        create_resource(self.gateway(), "hosts", body, "creating the host asset").await
    }

    /// Update a host asset's comment. Idempotent (PUT).
    #[tool(
        name = "openvas_update_asset_host",
        annotations(
            title = "Update host asset",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_asset_host(
        &self,
        Parameters(params): Parameters<UpdateCommentParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new().set_opt("comment", params.comment);
        update_resource(
            self.gateway(),
            &["hosts", &params.id],
            body,
            "updating the host asset",
        )
        .await
    }

    /// Delete a host asset (no trashcan).
    #[tool(
        name = "openvas_delete_asset_host",
        annotations(
            title = "Delete host asset",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_asset_host(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource_plain(
            self.gateway(),
            "hosts",
            &params.id,
            "deleting the host asset",
        )
        .await
    }

    /// List operating-system assets detected by scans with host counts and
    /// severities.
    #[tool(
        name = "openvas_list_operating_systems",
        annotations(title = "List operating systems", read_only_hint = true)
    )]
    pub async fn list_operating_systems(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "operating-systems",
            "operatingSystems",
            OS_ROW_KEYS,
            &params,
            "listing operating systems",
        )
        .await
    }

    /// Get one operating-system asset by UUID, including its hosts.
    #[tool(
        name = "openvas_get_operating_system",
        annotations(title = "Get operating system", read_only_hint = true)
    )]
    pub async fn get_operating_system(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["operating-systems", &params.id],
            "fetching the operating system",
        )
        .await
    }

    /// Update an operating-system asset's comment. Idempotent (PUT).
    #[tool(
        name = "openvas_update_operating_system",
        annotations(
            title = "Update operating system",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_operating_system(
        &self,
        Parameters(params): Parameters<UpdateCommentParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new().set_opt("comment", params.comment);
        update_resource(
            self.gateway(),
            &["operating-systems", &params.id],
            body,
            "updating the operating system",
        )
        .await
    }

    /// Delete an operating-system asset (no trashcan).
    #[tool(
        name = "openvas_delete_operating_system",
        annotations(
            title = "Delete operating system",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_operating_system(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource_plain(
            self.gateway(),
            "operating-systems",
            &params.id,
            "deleting the operating system",
        )
        .await
    }

    /// List assets of one type (`host`, `os` or `tls_certificate`) through
    /// the generic asset catalog.
    #[tool(
        name = "openvas_list_assets",
        annotations(title = "List generic assets", read_only_hint = true)
    )]
    pub async fn list_assets(
        &self,
        Parameters(params): Parameters<ListAssetsParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut query = params.list.to_query();
        query.push(("type", params.asset_type));
        list_summarized_query(
            self.gateway(),
            &["assets"],
            &query,
            "assets",
            GENERIC_ROW_KEYS,
            "listing assets",
        )
        .await
    }

    /// Get one generic asset by UUID and type.
    #[tool(
        name = "openvas_get_asset",
        annotations(title = "Get generic asset", read_only_hint = true)
    )]
    pub async fn get_asset(
        &self,
        Parameters(params): Parameters<AssetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough_query(
            self.gateway(),
            &["assets", &params.id],
            &[("type", params.asset_type)],
            "fetching the asset",
        )
        .await
    }

    /// Update a generic asset's comment (type required for the read-back).
    /// Idempotent (PUT).
    #[tool(
        name = "openvas_update_asset",
        annotations(
            title = "Update generic asset",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_asset(
        &self,
        Parameters(params): Parameters<UpdateAssetParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new().set_opt("comment", params.comment).into_value();
        match self
            .gateway()
            .put_json_query::<serde_json::Value>(
                &["assets", &params.id],
                &[("type", params.asset_type)],
                &body,
            )
            .await
        {
            Ok(updated) => json_result(&updated),
            Err(err) => Ok(gateway_tool_error("updating the asset", &err)),
        }
    }

    /// Delete a generic asset of any type (no trashcan).
    #[tool(
        name = "openvas_delete_asset",
        annotations(
            title = "Delete generic asset",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_asset(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource_plain(self.gateway(), "assets", &params.id, "deleting the asset").await
    }
}

/// Arguments for `openvas_create_asset_host`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateHostParams {
    /// IP address or hostname of the host
    pub value: String,
    /// Optional comment
    pub comment: Option<String>,
}

/// Arguments for comment-only updates (host and OS assets).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateCommentParams {
    /// Resource UUID
    pub id: String,
    /// New comment
    pub comment: Option<String>,
}

/// Arguments for `openvas_list_assets`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListAssetsParams {
    /// Asset type: host, os or tls_certificate
    #[serde(rename = "type")]
    pub asset_type: String,
    #[serde(flatten)]
    pub list: ListParams,
}

/// Arguments for `openvas_get_asset`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AssetByIdParams {
    /// Asset UUID
    pub id: String,
    /// Asset type: host, os or tls_certificate
    #[serde(rename = "type")]
    pub asset_type: String,
}

/// Arguments for `openvas_update_asset`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateAssetParams {
    /// Asset UUID
    pub id: String,
    /// Asset type: host, os or tls_certificate
    #[serde(rename = "type")]
    pub asset_type: String,
    /// New comment
    pub comment: Option<String>,
}
