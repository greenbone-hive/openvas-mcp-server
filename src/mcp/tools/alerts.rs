//! Alerts toolset: read surface plus create/update/delete.

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, DeleteParams, GetByIdParams, ListParams, create_resource, delete_resource,
    get_passthrough, list_summarized, update_resource,
};

fn data_map(map: Option<BTreeMap<String, String>>) -> Option<serde_json::Value> {
    map.map(|m| serde_json::to_value(m).expect("string map serializes"))
}

const ROW_KEYS: &[&str] = &["id", "name", "event", "condition", "method", "inUse"];

#[tool_router(router = alerts_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List alerts (notifications triggered by task events).
    #[tool(
        name = "openvas_list_alerts",
        annotations(title = "List alerts", read_only_hint = true)
    )]
    pub async fn list_alerts(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "alerts",
            "alerts",
            ROW_KEYS,
            &params,
            "listing alerts",
        )
        .await
    }

    /// Get one alert by UUID, including event/condition/method data.
    #[tool(
        name = "openvas_get_alert",
        annotations(title = "Get alert", read_only_hint = true)
    )]
    pub async fn get_alert(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["alerts", &params.id],
            "fetching the alert",
        )
        .await
    }

    /// Create an alert binding an event, condition and method (e.g. email
    /// on task_run_status_changed with event_data {"status": "Done"}). The
    /// event/condition/method values are the gateway's snake_case
    /// identifiers. Not idempotent.
    #[tool(
        name = "openvas_create_alert",
        annotations(
            title = "Create alert",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn create_alert(
        &self,
        Parameters(params): Parameters<CreateAlertParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set("name", params.name)
            .set("event", params.event)
            .set("condition", params.condition)
            .set("method", params.method)
            .set_opt("comment", params.comment)
            .set_opt("eventData", data_map(params.event_data))
            .set_opt("conditionData", data_map(params.condition_data))
            .set_opt("methodData", data_map(params.method_data))
            .set_opt("filterId", params.filter_id);
        create_resource(self.gateway(), "alerts", body, "creating the alert").await
    }

    /// Update an alert. Idempotent (PUT): omitted fields stay unchanged.
    #[tool(
        name = "openvas_update_alert",
        annotations(
            title = "Update alert",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn update_alert(
        &self,
        Parameters(params): Parameters<UpdateAlertParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = Body::new()
            .set_opt("name", params.name)
            .set_opt("comment", params.comment)
            .set_opt("event", params.event)
            .set_opt("condition", params.condition)
            .set_opt("method", params.method)
            .set_opt("eventData", data_map(params.event_data))
            .set_opt("conditionData", data_map(params.condition_data))
            .set_opt("methodData", data_map(params.method_data))
            .set_opt("filterId", params.filter_id);
        update_resource(
            self.gateway(),
            &["alerts", &params.id],
            body,
            "updating the alert",
        )
        .await
    }

    /// Delete an alert (to the trashcan by default; `ultimate` deletes
    /// permanently). Fails with 409 if a task still uses it.
    #[tool(
        name = "openvas_delete_alert",
        annotations(
            title = "Delete alert",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_alert(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource(self.gateway(), "alerts", &params, "deleting the alert").await
    }
}

/// Arguments for `openvas_create_alert`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateAlertParams {
    /// Alert name
    pub name: String,
    /// Trigger event: task_run_status_changed, updated_secinfo or
    /// new_secinfo
    pub event: String,
    /// Trigger condition: always, filter_count_at_least,
    /// filter_count_changed, severity_at_least or severity_changed
    pub condition: String,
    /// Delivery method: email, http_get, scp, send_email, smb, snmp,
    /// sourcefire_connector, start_task, syslog, tippingpoint, verinice_ce,
    /// verinice_net or alemba
    pub method: String,
    /// Optional comment
    pub comment: Option<String>,
    /// Event parameters, e.g. {"status": "Done"}
    pub event_data: Option<BTreeMap<String, String>>,
    /// Condition parameters, e.g. {"severity": "7.0"}
    pub condition_data: Option<BTreeMap<String, String>>,
    /// Method parameters, e.g. {"to_address": "sec@example.com"}
    pub method_data: Option<BTreeMap<String, String>>,
    /// UUID of a filter restricting which results the alert reports
    pub filter_id: Option<String>,
}

/// Arguments for `openvas_update_alert`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateAlertParams {
    /// UUID of the alert to update
    pub id: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    /// Same values as openvas_create_alert
    pub event: Option<String>,
    /// Same values as openvas_create_alert
    pub condition: Option<String>,
    /// Same values as openvas_create_alert
    pub method: Option<String>,
    pub event_data: Option<BTreeMap<String, String>>,
    pub condition_data: Option<BTreeMap<String, String>>,
    pub method_data: Option<BTreeMap<String, String>>,
    pub filter_id: Option<String>,
}
