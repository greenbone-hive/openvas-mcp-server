//! Filters toolset: saved GMP filters with CRUD and clone.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, clone_resource, create_resource,
    delete_resource, get_passthrough, list_summarized, update_resource,
};

const ROW_KEYS: &[&str] = &["id", "name", "type", "term", "inUse"];

/// Arguments for `openvas_create_filter`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateFilterParams {
    /// Filter name
    pub name: String,
    /// Optional comment
    pub comment: Option<String>,
    /// GMP filter term, e.g. `severity>7 and host=10.0.0.5`
    pub term: Option<String>,
    /// Resource type the filter applies to, e.g. "result", "task", "report"
    #[serde(rename = "type")]
    pub filter_type: Option<String>,
}

/// Arguments for `openvas_update_filter`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateFilterParams {
    /// UUID of the filter to update
    pub id: String,
    pub comment: Option<String>,
    /// Replacement GMP filter term
    pub term: Option<String>,
    /// Replacement resource type
    #[serde(rename = "type")]
    pub filter_type: Option<String>,
}

#[tool_router(router = filters_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List saved filters (reusable GMP filter expressions).
    #[tool(
        name = "openvas_list_filters",
        annotations(title = "List filters", read_only_hint = true)
    )]
    pub async fn list_filters(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "filters",
            "filters",
            ROW_KEYS,
            &params,
            "listing filters",
        )
        .await
    }

    /// Get one saved filter by UUID.
    #[tool(
        name = "openvas_get_filter",
        annotations(title = "Get filter", read_only_hint = true)
    )]
    pub async fn get_filter(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["filters", &params.id],
            "fetching the filter",
        )
        .await
    }

    /// Create a saved filter. Not idempotent.
    #[tool(
        name = "openvas_create_filter",
        annotations(
            title = "Create filter",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_filter(
        &self,
        Parameters(params): Parameters<CreateFilterParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("term", params.term)
            .set_opt("type", params.filter_type);
        create_resource(self.gateway(), "filters", body, "creating the filter").await
    }

    /// Update a saved filter's term, type or comment. Idempotent (PUT).
    #[tool(
        name = "openvas_update_filter",
        annotations(
            title = "Update filter",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_filter(
        &self,
        Parameters(params): Parameters<UpdateFilterParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("comment", params.comment)
            .set_opt("term", params.term)
            .set_opt("type", params.filter_type);
        update_resource(
            self.gateway(),
            &["filters", &params.id],
            body,
            "updating the filter",
        )
        .await
    }

    /// Delete a saved filter (to the trashcan by default; `ultimate` deletes
    /// permanently). Fails with 409 if an alert still uses it.
    #[tool(
        name = "openvas_delete_filter",
        annotations(
            title = "Delete filter",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_filter(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(self.gateway(), "filters", &params, "deleting the filter").await
    }

    /// Clone a saved filter. Returns the UUID of the copy.
    #[tool(
        name = "openvas_clone_filter",
        annotations(
            title = "Clone filter",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn clone_filter(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        clone_resource(self.gateway(), "filters", &params.id, "cloning the filter").await
    }
}
