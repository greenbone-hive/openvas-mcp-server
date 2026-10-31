//! Overrides toolset: read surface plus create/update/delete.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, create_resource, delete_resource,
    get_passthrough, list_summarized, update_resource,
};

const ROW_KEYS: &[&str] = &[
    "id",
    "text",
    "nvt",
    "hosts",
    "severity",
    "newSeverity",
    "active",
];

#[tool_router(router = overrides_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List severity overrides.
    #[tool(
        name = "openvas_list_overrides",
        annotations(title = "List overrides", read_only_hint = true)
    )]
    pub async fn list_overrides(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "overrides",
            "overrides",
            ROW_KEYS,
            &params,
            "listing overrides",
        )
        .await
    }

    /// Get one severity override by UUID.
    #[tool(
        name = "openvas_get_override",
        annotations(title = "Get override", read_only_hint = true)
    )]
    pub async fn get_override(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["overrides", &params.id],
            "fetching the override",
        )
        .await
    }

    /// Create a severity override for an NVT (e.g. mark a finding as false
    /// positive by overriding to 0.0). Not idempotent.
    #[tool(
        name = "openvas_create_override",
        annotations(
            title = "Create override",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_override(
        &self,
        Parameters(params): Parameters<CreateOverrideParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("nvtOid", params.nvt_oid)
            .set_opt("text", params.text)
            .set_opt("hosts", params.hosts)
            .set_opt("port", params.port)
            .set_opt("severity", params.severity)
            .set_opt("newSeverity", params.new_severity)
            .set_opt("taskId", params.task_id)
            .set_opt("resultId", params.result_id)
            .set_opt("active", params.active)
            .set_opt("orphan", params.orphan);
        create_resource(self.gateway(), "overrides", body, "creating the override").await
    }

    /// Update a severity override. Idempotent (PUT): omitted fields stay
    /// unchanged.
    #[tool(
        name = "openvas_update_override",
        annotations(
            title = "Update override",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_override(
        &self,
        Parameters(params): Parameters<UpdateOverrideParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("text", params.text)
            .set_opt("hosts", params.hosts)
            .set_opt("port", params.port)
            .set_opt("severity", params.severity)
            .set_opt("newSeverity", params.new_severity)
            .set_opt("taskId", params.task_id)
            .set_opt("resultId", params.result_id)
            .set_opt("active", params.active)
            .set_opt("orphan", params.orphan);
        update_resource(
            self.gateway(),
            &["overrides", &params.id],
            body,
            "updating the override",
        )
        .await
    }

    /// Delete a severity override (to the trashcan by default; `ultimate`
    /// deletes permanently).
    #[tool(
        name = "openvas_delete_override",
        annotations(
            title = "Delete override",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_override(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(
            self.gateway(),
            "overrides",
            &params,
            "deleting the override",
        )
        .await
    }
}

/// Arguments for `openvas_create_override`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateOverrideParams {
    /// OID of the NVT the override applies to
    pub nvt_oid: String,
    /// Justification text
    pub text: Option<String>,
    /// Restrict to these hosts
    pub hosts: Option<Vec<String>>,
    /// Restrict to a port, e.g. "22/tcp"
    pub port: Option<String>,
    /// Match results with this severity (string per GMP, e.g. "7.5")
    pub severity: Option<String>,
    /// Severity to report instead (string per GMP, e.g. "0.0" for false
    /// positive)
    pub new_severity: Option<String>,
    /// Restrict to one task by UUID
    pub task_id: Option<String>,
    /// Attach to one specific result by UUID
    pub result_id: Option<String>,
    /// Whether the override is active
    pub active: Option<bool>,
    /// Keep the override even when its result no longer exists
    pub orphan: Option<bool>,
}

/// Arguments for `openvas_update_override`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateOverrideParams {
    /// UUID of the override to update
    pub id: String,
    pub text: Option<String>,
    pub hosts: Option<Vec<String>>,
    pub port: Option<String>,
    pub severity: Option<String>,
    pub new_severity: Option<String>,
    pub task_id: Option<String>,
    pub result_id: Option<String>,
    pub active: Option<bool>,
    pub orphan: Option<bool>,
}
