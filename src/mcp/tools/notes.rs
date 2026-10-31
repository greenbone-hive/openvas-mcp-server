//! Notes toolset: read surface plus create/update/delete.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, create_resource, delete_resource,
    get_passthrough, list_summarized, update_resource,
};

const ROW_KEYS: &[&str] = &["id", "text", "nvt", "hosts", "port", "severity", "active"];

#[tool_router(router = notes_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List notes attached to scan results.
    #[tool(
        name = "openvas_list_notes",
        annotations(title = "List notes", read_only_hint = true)
    )]
    pub async fn list_notes(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "notes",
            "notes",
            ROW_KEYS,
            &params,
            "listing notes",
        )
        .await
    }

    /// Get one note by UUID.
    #[tool(
        name = "openvas_get_note",
        annotations(title = "Get note", read_only_hint = true)
    )]
    pub async fn get_note(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(self.gateway(), &["notes", &params.id], "fetching the note").await
    }

    /// Create a note on an NVT, optionally scoped to hosts, a port, a task
    /// or a specific result. Not idempotent.
    #[tool(
        name = "openvas_create_note",
        annotations(
            title = "Create note",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_note(
        &self,
        Parameters(params): Parameters<CreateNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("nvtOid", params.nvt_oid)
            .set_opt("text", params.text)
            .set_opt("hosts", params.hosts)
            .set_opt("port", params.port)
            .set_opt("severity", params.severity)
            .set_opt("taskId", params.task_id)
            .set_opt("resultId", params.result_id)
            .set_opt("active", params.active)
            .set_opt("orphan", params.orphan);
        create_resource(self.gateway(), "notes", body, "creating the note").await
    }

    /// Update a note. Idempotent (PUT): omitted fields stay unchanged.
    #[tool(
        name = "openvas_update_note",
        annotations(title = "Update note", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn update_note(
        &self,
        Parameters(params): Parameters<UpdateNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("text", params.text)
            .set_opt("hosts", params.hosts)
            .set_opt("port", params.port)
            .set_opt("severity", params.severity)
            .set_opt("taskId", params.task_id)
            .set_opt("resultId", params.result_id)
            .set_opt("active", params.active)
            .set_opt("orphan", params.orphan);
        update_resource(
            self.gateway(),
            &["notes", &params.id],
            body,
            "updating the note",
        )
        .await
    }

    /// Delete a note (to the trashcan by default; `ultimate` deletes
    /// permanently).
    #[tool(
        name = "openvas_delete_note",
        annotations(title = "Delete note", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn delete_note(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(self.gateway(), "notes", &params, "deleting the note").await
    }
}

/// Arguments for `openvas_create_note`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateNoteParams {
    /// OID of the NVT the note applies to
    pub nvt_oid: String,
    /// Note text
    pub text: Option<String>,
    /// Restrict to these hosts
    pub hosts: Option<Vec<String>>,
    /// Restrict to a port, e.g. "22/tcp"
    pub port: Option<String>,
    /// Restrict to results with this severity (string per GMP, e.g. "7.5")
    pub severity: Option<String>,
    /// Restrict to one task by UUID
    pub task_id: Option<String>,
    /// Attach to one specific result by UUID
    pub result_id: Option<String>,
    /// Whether the note is active
    pub active: Option<bool>,
    /// Keep the note even when its result no longer exists
    pub orphan: Option<bool>,
}

/// Arguments for `openvas_update_note`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateNoteParams {
    /// UUID of the note to update
    pub id: String,
    pub text: Option<String>,
    pub hosts: Option<Vec<String>>,
    pub port: Option<String>,
    pub severity: Option<String>,
    pub task_id: Option<String>,
    pub result_id: Option<String>,
    pub active: Option<bool>,
    pub orphan: Option<bool>,
}
