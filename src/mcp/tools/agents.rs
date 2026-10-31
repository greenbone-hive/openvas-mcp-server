//! Agents toolset: sensor agents, agent groups, agent-control scan-config
//! defaults and installer instructions. Requires a gvmd build with the
//! agents feature enabled; otherwise the gateway answers with a problem
//! (surfaced as a tool error).

use base64::Engine;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::error::gateway_tool_error;
use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, TrashListParams, clone_resource,
    create_resource, delete_resource, delete_resource_plain, get_passthrough,
    get_passthrough_query, json_result, list_summarized, list_summarized_query, update_resource,
    update_resource_empty,
};
use super::reports::MAX_INLINE_ARTIFACT_BYTES;

const AGENT_ROW_KEYS: &[&str] = &[
    "id",
    "name",
    "status",
    "version",
    "authorized",
    "updateToLatest",
    "lastContactTime",
    "scanner",
];
const GROUP_ROW_KEYS: &[&str] = &["id", "name", "schedulerCronTime", "agents", "inUse"];

/// Arguments for `openvas_update_agent`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateAgentParams {
    /// Agent UUID
    pub id: String,
    /// Authorize (true) or revoke (false) the agent
    pub authorized: Option<bool>,
    /// Request an update to the latest agent version
    pub update_to_latest: Option<bool>,
    pub comment: Option<String>,
    /// Agent configuration update object (agentControl.retry,
    /// agentScriptExecutor, heartbeat), as in the gateway's AgentConfigUpdate
    pub config: Option<serde_json::Value>,
}

/// Arguments for `openvas_get_agent_support_bundle`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AgentSupportBundleParams {
    /// Agent UUID
    pub id: String,
    /// Limit the bundle to the most recent number of days
    pub days: Option<u32>,
    /// Request backend encryption of the bundle (omit for the default)
    pub encryption: Option<bool>,
}

/// Arguments for `openvas_update_agent_control_scan_config`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateAgentControlScanConfigParams {
    /// Agent-control scan config UUID
    pub id: String,
    /// Default agent configuration (AgentConfigUpdate object)
    pub agent_defaults: Option<serde_json::Value>,
    /// Update agents to the latest version by default
    pub update_to_latest: Option<bool>,
}

/// Arguments for `openvas_get_agent_installer_instruction`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AgentInstallerInstructionParams {
    /// Scanner UUID the agent will report to
    pub scanner_id: String,
    /// Absolute origin URL used in the generated instructions
    pub origin_url: String,
    /// Instruction language: en or de
    pub language: Option<String>,
}

/// Arguments for `openvas_create_agent_group`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateAgentGroupParams {
    /// Group name
    pub name: String,
    /// Scheduler cron expression for the group
    pub scheduler_cron_time: String,
    pub comment: Option<String>,
    /// UUIDs of member agents
    pub agent_ids: Option<Vec<String>>,
}

/// Arguments for `openvas_update_agent_group`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateAgentGroupParams {
    /// Group UUID
    pub id: String,
    /// Scheduler cron expression (required by the gateway on every update)
    pub scheduler_cron_time: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    /// Replacement member agent UUIDs
    pub agent_ids: Option<Vec<String>>,
}

#[tool_router(router = agents_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List sensor agents with status, version and authorization.
    #[tool(
        name = "openvas_list_agents",
        annotations(title = "List agents", read_only_hint = true)
    )]
    pub async fn list_agents(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "agents",
            "agents",
            AGENT_ROW_KEYS,
            &params,
            "listing agents",
        )
        .await
    }

    /// Get one agent by UUID, including its configuration.
    #[tool(
        name = "openvas_get_agent",
        annotations(title = "Get agent", read_only_hint = true)
    )]
    pub async fn get_agent(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["agents", &params.id],
            "fetching the agent",
        )
        .await
    }

    /// Authorize, update or reconfigure an agent. Idempotent (PUT).
    #[tool(
        name = "openvas_update_agent",
        annotations(
            title = "Update agent",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_agent(
        &self,
        Parameters(params): Parameters<UpdateAgentParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("authorized", params.authorized)
            .set_opt("updateToLatest", params.update_to_latest)
            .set_opt("comment", params.comment)
            .set_opt("config", params.config);
        update_resource(
            self.gateway(),
            &["agents", &params.id],
            body,
            "updating the agent",
        )
        .await
    }

    /// Delete an agent (no trashcan).
    #[tool(
        name = "openvas_delete_agent",
        annotations(
            title = "Delete agent",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_agent(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource_plain(self.gateway(), "agents", &params.id, "deleting the agent").await
    }

    /// Trigger a synchronization of agents with the agent controller.
    #[tool(
        name = "openvas_sync_agents",
        annotations(
            title = "Sync agents",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn sync_agents(&self) -> Result<CallToolResult, McpError> {
        match self.gateway().post_action_empty(&["agents", "sync"]).await {
            Ok(()) => json_result(&serde_json::json!({ "syncStarted": true })),
            Err(err) => Ok(gateway_tool_error("starting the agent sync", &err)),
        }
    }

    /// Download an agent's support bundle (returned base64-encoded, up to
    /// 3 MB).
    #[tool(
        name = "openvas_get_agent_support_bundle",
        annotations(title = "Get agent support bundle", read_only_hint = true)
    )]
    pub async fn get_agent_support_bundle(
        &self,
        Parameters(params): Parameters<AgentSupportBundleParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut query = Vec::new();
        if let Some(days) = params.days {
            query.push(("days", days.to_string()));
        }
        if let Some(encryption) = params.encryption {
            query.push(("encryption", encryption.to_string()));
        }
        match self
            .gateway()
            .get_bytes(
                &["agents", &params.id, "support-bundle"],
                &query,
                MAX_INLINE_ARTIFACT_BYTES,
            )
            .await
        {
            Ok(artifact) => json_result(&serde_json::json!({
                "contentType": artifact.content_type,
                "filename": artifact.filename,
                "size": artifact.bytes.len(),
                "base64": base64::engine::general_purpose::STANDARD.encode(&artifact.bytes),
            })),
            Err(err) => Ok(gateway_tool_error(
                "downloading the agent support bundle",
                &err,
            )),
        }
    }

    /// Change the default agent configuration of an agent-control scan
    /// config. Idempotent (PUT).
    #[tool(
        name = "openvas_update_agent_control_scan_config",
        annotations(
            title = "Update agent-control scan config",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_agent_control_scan_config(
        &self,
        Parameters(params): Parameters<UpdateAgentControlScanConfigParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("agentDefaults", params.agent_defaults)
            .set_opt("updateToLatest", params.update_to_latest);
        update_resource_empty(
            self.gateway(),
            &["agent-control-scan-configs", &params.id],
            body,
            "updating the agent-control scan config",
        )
        .await
    }

    /// Get the installer instructions for enrolling an agent with a scanner.
    #[tool(
        name = "openvas_get_agent_installer_instruction",
        annotations(title = "Get agent installer instruction", read_only_hint = true)
    )]
    pub async fn get_agent_installer_instruction(
        &self,
        Parameters(params): Parameters<AgentInstallerInstructionParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut query = vec![("originUrl", params.origin_url)];
        if let Some(language) = params.language {
            query.push(("language", language));
        }
        get_passthrough_query(
            self.gateway(),
            &[
                "scanners",
                &params.scanner_id,
                "agent-installer-instruction",
            ],
            &query,
            "fetching the agent installer instruction",
        )
        .await
    }

    /// List agent groups. `trash` lists trashed ones.
    #[tool(
        name = "openvas_list_agent_groups",
        annotations(title = "List agent groups", read_only_hint = true)
    )]
    pub async fn list_agent_groups(
        &self,
        Parameters(params): Parameters<TrashListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized_query(
            self.gateway(),
            &["agent-groups"],
            &params.to_query(),
            "agentGroups",
            GROUP_ROW_KEYS,
            "listing agent groups",
        )
        .await
    }

    /// Get one agent group by UUID.
    #[tool(
        name = "openvas_get_agent_group",
        annotations(title = "Get agent group", read_only_hint = true)
    )]
    pub async fn get_agent_group(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["agent-groups", &params.id],
            "fetching the agent group",
        )
        .await
    }

    /// Create an agent group. Not idempotent.
    #[tool(
        name = "openvas_create_agent_group",
        annotations(
            title = "Create agent group",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_agent_group(
        &self,
        Parameters(params): Parameters<CreateAgentGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set("schedulerCronTime", params.scheduler_cron_time)
            .set_opt("comment", params.comment)
            .set_opt("agentIds", params.agent_ids);
        create_resource(
            self.gateway(),
            "agent-groups",
            body,
            "creating the agent group",
        )
        .await
    }

    /// Update an agent group. Idempotent (PUT); the scheduler cron time is
    /// always required by the gateway.
    #[tool(
        name = "openvas_update_agent_group",
        annotations(
            title = "Update agent group",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_agent_group(
        &self,
        Parameters(params): Parameters<UpdateAgentGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("schedulerCronTime", params.scheduler_cron_time)
            .set_opt("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("agentIds", params.agent_ids);
        update_resource(
            self.gateway(),
            &["agent-groups", &params.id],
            body,
            "updating the agent group",
        )
        .await
    }

    /// Delete an agent group (to the trashcan by default; `ultimate` deletes
    /// permanently).
    #[tool(
        name = "openvas_delete_agent_group",
        annotations(
            title = "Delete agent group",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_agent_group(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(
            self.gateway(),
            "agent-groups",
            &params,
            "deleting the agent group",
        )
        .await
    }

    /// Clone an agent group. Returns the UUID of the copy.
    #[tool(
        name = "openvas_clone_agent_group",
        annotations(
            title = "Clone agent group",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn clone_agent_group(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        clone_resource(
            self.gateway(),
            "agent-groups",
            &params.id,
            "cloning the agent group",
        )
        .await
    }
}
