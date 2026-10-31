//! Compliance toolset: audits (usage-type `audit` tasks) and policies
//! (usage-type `policy` scan configs). Same shapes as tasks/scan configs,
//! scoped by the gateway to compliance resources.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::gateway::models::{ResourceCreated, Task, TaskAction, TaskList};
use crate::mcp::error::gateway_tool_error;
use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, GetByIdParams, ListParams, create_resource, delete_resource_plain, get_passthrough,
    json_result, list_summarized, update_resource,
};
use super::tasks::{CreateTaskParams, UpdateTaskParams, create_body, summarize, update_body};

const POLICY_ROW_KEYS: &[&str] = &[
    "id",
    "name",
    "comment",
    "familyCount",
    "nvtCount",
    "usageType",
    "inUse",
];

/// Arguments for `openvas_create_policy`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreatePolicyParams {
    /// Policy name
    pub name: String,
    /// UUID of the active policy to copy from (see openvas_list_policies)
    pub base_policy_id: String,
    /// Optional comment
    pub comment: Option<String>,
}

/// Arguments for `openvas_update_policy`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdatePolicyParams {
    /// UUID of the policy to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
}

#[tool_router(router = compliance_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List compliance audits with status and progress.
    #[tool(
        name = "openvas_list_audits",
        annotations(title = "List audits", read_only_hint = true)
    )]
    pub async fn list_audits(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        let list: TaskList = match self
            .gateway()
            .get_json_query(&["audits"], &params.to_query())
            .await
        {
            Ok(list) => list,
            Err(err) => return Ok(gateway_tool_error("listing audits", &err)),
        };
        json_result(&serde_json::json!({
            "audits": list.data.into_iter().map(summarize).collect::<Vec<_>>(),
            "pagination": list.pagination,
        }))
    }

    /// Get one compliance audit by UUID.
    #[tool(
        name = "openvas_get_audit",
        annotations(title = "Get audit", read_only_hint = true)
    )]
    pub async fn get_audit(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .get_json::<Task>(&["audits", &params.id])
            .await
        {
            Ok(audit) => json_result(&audit),
            Err(err) => Ok(gateway_tool_error("fetching the audit", &err)),
        }
    }

    /// Create a compliance audit binding a target, a policy (as
    /// `scan_config_id`) and a scanner. Returns the new audit's UUID.
    #[tool(
        name = "openvas_create_audit",
        annotations(
            title = "Create audit",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_audit(
        &self,
        Parameters(params): Parameters<CreateTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .post_json::<ResourceCreated>(&["audits"], &create_body(params).into_value())
            .await
        {
            Ok(created) => json_result(&created),
            Err(err) => Ok(gateway_tool_error("creating the audit", &err)),
        }
    }

    /// Update a compliance audit. Idempotent (PUT): omitted fields stay
    /// unchanged.
    #[tool(
        name = "openvas_update_audit",
        annotations(
            title = "Update audit",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_audit(
        &self,
        Parameters(params): Parameters<UpdateTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        update_resource(
            self.gateway(),
            &["audits", &params.id],
            update_body(&params),
            "updating the audit",
        )
        .await
    }

    /// Delete a compliance audit.
    #[tool(
        name = "openvas_delete_audit",
        annotations(
            title = "Delete audit",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_audit(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource_plain(self.gateway(), "audits", &params.id, "deleting the audit").await
    }

    /// Start a compliance audit. Returns the UUID of the report it writes to.
    #[tool(
        name = "openvas_start_audit",
        annotations(
            title = "Start audit",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn start_audit(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .post_action::<TaskAction>(&["audits", &params.id, "start"])
            .await
        {
            Ok(action) => json_result(&action),
            Err(err) => Ok(gateway_tool_error("starting the audit", &err)),
        }
    }

    /// Stop a running compliance audit.
    #[tool(
        name = "openvas_stop_audit",
        annotations(title = "Stop audit", read_only_hint = false, destructive_hint = false)
    )]
    pub async fn stop_audit(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .post_action_empty(&["audits", &params.id, "stop"])
            .await
        {
            Ok(()) => json_result(&serde_json::json!({ "stopped": true, "id": params.id })),
            Err(err) => Ok(gateway_tool_error("stopping the audit", &err)),
        }
    }

    /// Resume a stopped compliance audit. Returns the report UUID.
    #[tool(
        name = "openvas_resume_audit",
        annotations(
            title = "Resume audit",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn resume_audit(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .post_action::<TaskAction>(&["audits", &params.id, "resume"])
            .await
        {
            Ok(action) => json_result(&action),
            Err(err) => Ok(gateway_tool_error("resuming the audit", &err)),
        }
    }

    /// List compliance policies (audit scan configurations).
    #[tool(
        name = "openvas_list_policies",
        annotations(title = "List policies", read_only_hint = true)
    )]
    pub async fn list_policies(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "policies",
            "policies",
            POLICY_ROW_KEYS,
            &params,
            "listing policies",
        )
        .await
    }

    /// Get one compliance policy by UUID.
    #[tool(
        name = "openvas_get_policy",
        annotations(title = "Get policy", read_only_hint = true)
    )]
    pub async fn get_policy(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["policies", &params.id],
            "fetching the policy",
        )
        .await
    }

    /// Create a compliance policy as a copy of an existing one. Not
    /// idempotent.
    #[tool(
        name = "openvas_create_policy",
        annotations(
            title = "Create policy",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_policy(
        &self,
        Parameters(params): Parameters<CreatePolicyParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set("basePolicyId", params.base_policy_id)
            .set_opt("comment", params.comment);
        create_resource(self.gateway(), "policies", body, "creating the policy").await
    }

    /// Rename or re-comment a compliance policy. Idempotent (PUT).
    #[tool(
        name = "openvas_update_policy",
        annotations(
            title = "Update policy",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_policy(
        &self,
        Parameters(params): Parameters<UpdatePolicyParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment);
        update_resource(
            self.gateway(),
            &["policies", &params.id],
            body,
            "updating the policy",
        )
        .await
    }

    /// Delete a compliance policy. Fails with 409 if an audit still uses it.
    #[tool(
        name = "openvas_delete_policy",
        annotations(
            title = "Delete policy",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_policy(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource_plain(
            self.gateway(),
            "policies",
            &params.id,
            "deleting the policy",
        )
        .await
    }
}
