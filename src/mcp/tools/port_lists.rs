//! Port-lists toolset: read surface plus create/update/delete.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, create_resource, delete_resource,
    get_passthrough, list_summarized, update_resource,
};

const ROW_KEYS: &[&str] = &["id", "name", "portCount", "tcpCount", "udpCount", "inUse"];

#[tool_router(router = port_lists_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List port lists targets can scan.
    #[tool(
        name = "openvas_list_port_lists",
        annotations(title = "List port lists", read_only_hint = true)
    )]
    pub async fn list_port_lists(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "port-lists",
            "portLists",
            ROW_KEYS,
            &params,
            "listing port lists",
        )
        .await
    }

    /// Get one port list by UUID, including its port ranges.
    #[tool(
        name = "openvas_get_port_list",
        annotations(title = "Get port list", read_only_hint = true)
    )]
    pub async fn get_port_list(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["port-lists", &params.id],
            "fetching the port list",
        )
        .await
    }

    /// Create a port list from a range expression like
    /// "T:1-1000,U:53,T:8080". Not idempotent.
    #[tool(
        name = "openvas_create_port_list",
        annotations(
            title = "Create port list",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_port_list(
        &self,
        Parameters(params): Parameters<CreatePortListParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("portRange", params.port_range);
        create_resource(self.gateway(), "port-lists", body, "creating the port list").await
    }

    /// Update a port list's name, comment or ranges. Idempotent (PUT).
    #[tool(
        name = "openvas_update_port_list",
        annotations(
            title = "Update port list",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_port_list(
        &self,
        Parameters(params): Parameters<UpdatePortListParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("portRange", params.port_range);
        update_resource(
            self.gateway(),
            &["port-lists", &params.id],
            body,
            "updating the port list",
        )
        .await
    }

    /// Delete a port list (to the trashcan by default; `ultimate` deletes
    /// permanently). Fails with 409 if a target still uses it.
    #[tool(
        name = "openvas_delete_port_list",
        annotations(
            title = "Delete port list",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_port_list(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(
            self.gateway(),
            "port-lists",
            &params,
            "deleting the port list",
        )
        .await
    }
}

/// Arguments for `openvas_create_port_list`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreatePortListParams {
    /// Port list name
    pub name: String,
    /// Optional comment
    pub comment: Option<String>,
    /// Port range expression, e.g. "T:1-1000,U:53"
    pub port_range: Option<String>,
}

/// Arguments for `openvas_update_port_list`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdatePortListParams {
    /// UUID of the port list to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    /// Replacement port range expression, e.g. "T:1-1000,U:53"
    pub port_range: Option<String>,
}
