//! Tasks toolset: read surface plus the full task lifecycle, every task
//! variant the gateway accepts (classic, agent group, OCI image, web
//! application, import) and clone.

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::{Deserialize, Serialize};

use crate::gateway::models::{Pagination, ResourceCreated, Task, TaskAction, TaskList};
use crate::mcp::error::gateway_tool_error;
use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, clone_resource, json_result, update_resource,
};

/// Summarized list row: compact on purpose (LLM token budget). Full details
/// come from `openvas_get_task`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskRow {
    id: String,
    name: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_report_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    report_count: Option<i64>,
}

#[derive(Debug, Serialize)]
struct TaskListResult {
    tasks: Vec<TaskRow>,
    pagination: Pagination,
}

pub(crate) fn summarize(task: Task) -> TaskRow {
    TaskRow {
        id: task.id,
        name: task.name,
        status: task.status,
        progress: task.progress,
        target: task
            .target
            .or(task.web_application_target)
            .or(task.oci_image_target)
            .or(task.agent_group)
            .and_then(|t| t.name),
        usage_type: task.usage_type,
        last_report_id: task.last_report.map(|r| r.id),
        report_count: task.report_count,
    }
}

/// Arguments for `openvas_create_task`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateTaskParams {
    /// Task name
    pub name: String,
    /// Optional comment
    pub comment: Option<String>,
    /// Task variant: "classic" (default; host target + scan config),
    /// "agentGroup", "ociImage", "webApplication" or "import" (a container
    /// task for openvas_import_report that accepts only name/comment).
    /// Non-classic variants are inferred from their selector when omitted.
    #[serde(rename = "type")]
    pub task_type: Option<String>,
    /// UUID of the scan target (classic tasks)
    pub target_id: Option<String>,
    /// UUID of the agent group (agentGroup tasks)
    pub agent_group_id: Option<String>,
    /// UUID of the OCI image target (ociImage tasks)
    pub oci_image_target_id: Option<String>,
    /// UUID of the web application target (webApplication tasks)
    pub web_application_target_id: Option<String>,
    /// UUID of the scan config (required for classic tasks, rejected for
    /// other variants)
    pub scan_config_id: Option<String>,
    /// UUID of the scanner (required for every non-import task)
    pub scanner_id: Option<String>,
    /// Optional schedule UUID
    pub schedule_id: Option<String>,
    /// Optional alert UUIDs to attach
    pub alert_ids: Option<Vec<String>>,
    /// Whether the task stays modifiable after the first run
    pub alterable: Option<bool>,
    /// Login names of users allowed to observe the task
    pub observers: Option<Vec<String>>,
    /// Number of schedule periods to run (0 = unlimited)
    pub schedule_periods: Option<i32>,
    /// Key-value scan preferences, e.g. {"max_checks": "4"}
    pub preferences: Option<BTreeMap<String, String>>,
}

/// Arguments for `openvas_update_task`: all fields optional, omitted fields
/// stay unchanged.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateTaskParams {
    /// UUID of the task to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    /// Replacement target UUID
    pub target_id: Option<String>,
    /// Replacement scan config UUID
    pub scan_config_id: Option<String>,
    /// Replacement scanner UUID
    pub scanner_id: Option<String>,
    /// Replacement schedule UUID
    pub schedule_id: Option<String>,
    /// Replacement alert UUIDs
    pub alert_ids: Option<Vec<String>>,
    pub alterable: Option<bool>,
    /// Replacement observer login names
    pub observers: Option<Vec<String>>,
    pub schedule_periods: Option<i32>,
    /// Key-value scan preferences
    pub preferences: Option<BTreeMap<String, String>>,
}

/// Build the shared create body for tasks and audits.
pub(crate) fn create_body(params: CreateTaskParams) -> Body {
    Body::new()
        .set("name", params.name)
        .set_opt("comment", params.comment)
        .set_opt("type", params.task_type)
        .set_opt("targetId", params.target_id)
        .set_opt("agentGroupId", params.agent_group_id)
        .set_opt("ociImageTargetId", params.oci_image_target_id)
        .set_opt("webApplicationTargetId", params.web_application_target_id)
        .set_opt("scanConfigId", params.scan_config_id)
        .set_opt("scannerId", params.scanner_id)
        .set_opt("scheduleId", params.schedule_id)
        .set_opt("alertIds", params.alert_ids)
        .set_opt("alterable", params.alterable)
        .set_opt("observers", params.observers)
        .set_opt("schedulePeriods", params.schedule_periods)
        .set_opt_json("preferences", params.preferences)
}

/// Build the shared update body for tasks and audits.
pub(crate) fn update_body(params: &UpdateTaskParams) -> Body {
    Body::new()
        .set_opt("name", params.name.clone())
        .set_opt("comment", params.comment.clone())
        .set_opt("targetId", params.target_id.clone())
        .set_opt("scanConfigId", params.scan_config_id.clone())
        .set_opt("scannerId", params.scanner_id.clone())
        .set_opt("scheduleId", params.schedule_id.clone())
        .set_opt("alertIds", params.alert_ids.clone())
        .set_opt("alterable", params.alterable)
        .set_opt("observers", params.observers.clone())
        .set_opt("schedulePeriods", params.schedule_periods)
        .set_opt_json("preferences", params.preferences.clone())
}

#[tool_router(router = tasks_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List scan tasks with status and progress. Returns summarized rows
    /// plus pagination; use openvas_get_task for full details of one task.
    #[tool(
        name = "openvas_list_tasks",
        annotations(title = "List tasks", read_only_hint = true)
    )]
    pub async fn list_tasks(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        let list: TaskList = match self
            .gateway()
            .get_json_query(&["tasks"], &params.to_query())
            .await
        {
            Ok(list) => list,
            Err(err) => return Ok(gateway_tool_error("listing tasks", &err)),
        };

        json_result(&TaskListResult {
            tasks: list.data.into_iter().map(summarize).collect(),
            pagination: list.pagination,
        })
    }

    /// Get one scan task by UUID, including scan config, scanner, schedule
    /// and report references.
    #[tool(
        name = "openvas_get_task",
        annotations(title = "Get task", read_only_hint = true)
    )]
    pub async fn get_task(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .get_json::<Task>(&["tasks", &params.id])
            .await
        {
            Ok(task) => json_result(&task),
            Err(err) => Ok(gateway_tool_error("fetching the task", &err)),
        }
    }

    /// Create a scan task. Classic tasks bind a target, scan config and
    /// scanner; other variants use their own selector (agent group, OCI
    /// image target, web application target); `type: "import"` creates a
    /// container task for report imports. Returns the new task's UUID; the
    /// task is not started automatically.
    #[tool(
        name = "openvas_create_task",
        annotations(
            title = "Create task",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_task(
        &self,
        Parameters(params): Parameters<CreateTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .post_json::<ResourceCreated>(&["tasks"], &create_body(params).into_value())
            .await
        {
            Ok(created) => json_result(&created),
            Err(err) => Ok(gateway_tool_error("creating the task", &err)),
        }
    }

    /// Start a scan task. Returns the UUID of the report the run writes to.
    #[tool(
        name = "openvas_start_task",
        annotations(title = "Start task", read_only_hint = false, destructive_hint = false)
    )]
    pub async fn start_task(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .post_action::<TaskAction>(&["tasks", &params.id, "start"])
            .await
        {
            Ok(action) => json_result(&action),
            Err(err) => Ok(gateway_tool_error("starting the task", &err)),
        }
    }

    /// Stop a running scan task.
    #[tool(
        name = "openvas_stop_task",
        annotations(title = "Stop task", read_only_hint = false, destructive_hint = false)
    )]
    pub async fn stop_task(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .post_action_empty(&["tasks", &params.id, "stop"])
            .await
        {
            Ok(()) => json_result(&serde_json::json!({ "stopped": true, "id": params.id })),
            Err(err) => Ok(gateway_tool_error("stopping the task", &err)),
        }
    }

    /// Resume a stopped scan task. Returns the UUID of the report the
    /// resumed run writes to.
    #[tool(
        name = "openvas_resume_task",
        annotations(
            title = "Resume task",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn resume_task(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .post_action::<TaskAction>(&["tasks", &params.id, "resume"])
            .await
        {
            Ok(action) => json_result(&action),
            Err(err) => Ok(gateway_tool_error("resuming the task", &err)),
        }
    }

    /// Delete a scan task (to the trashcan by default; `ultimate` deletes
    /// permanently).
    #[tool(
        name = "openvas_delete_task",
        annotations(title = "Delete task", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn delete_task(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .gateway()
            .delete(&["tasks", &params.id], &params.to_query())
            .await
        {
            Ok(()) => json_result(&serde_json::json!({ "deleted": true, "id": params.id })),
            Err(err) => Ok(gateway_tool_error("deleting the task", &err)),
        }
    }

    /// Update a scan task's bindings or metadata. Idempotent (PUT): omitted
    /// fields stay unchanged; fails with 409 while the task is running.
    #[tool(
        name = "openvas_update_task",
        annotations(title = "Update task", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn update_task(
        &self,
        Parameters(params): Parameters<UpdateTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        update_resource(
            self.gateway(),
            &["tasks", &params.id],
            update_body(&params),
            "updating the task",
        )
        .await
    }

    /// Clone a scan task (configuration only, no reports). Returns the UUID
    /// of the copy.
    #[tool(
        name = "openvas_clone_task",
        annotations(title = "Clone task", read_only_hint = false, destructive_hint = false)
    )]
    pub async fn clone_task(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        clone_resource(self.gateway(), "tasks", &params.id, "cloning the task").await
    }
}
