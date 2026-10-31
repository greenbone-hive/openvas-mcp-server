//! System toolset: connectivity, version and timezone tools.

use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{ErrorData as McpError, tool, tool_router};
use serde::Serialize;

use crate::gateway::models::VersionInfo;
use crate::mcp::error::gateway_tool_error;
use crate::mcp::server::GvmMcpServer;

use super::common::{get_passthrough, json_result};

/// Structured payload returned by `openvas_test_connection`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TestConnectionReport {
    gateway_url: String,
    gateway_status: String,
    /// Readiness as reported by `GET /ready` (`ready` / `notReady`).
    gateway_readiness: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway_readiness_reason: Option<String>,
    api_version: String,
    gmp_version: String,
    /// Whether the identity used for this call is accepted by the gateway.
    authenticated: bool,
    read_only: bool,
    toolsets: String,
}

#[tool_router(router = system_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// Verify connectivity to the GVM stack: checks gateway liveness and
    /// readiness, queries the gvmd version and makes one authenticated call
    /// to confirm the forwarded identity is accepted. Use this first if
    /// other tools fail.
    #[tool(
        name = "openvas_test_connection",
        annotations(title = "Test GVM connection", read_only_hint = true)
    )]
    pub async fn test_connection(&self) -> Result<CallToolResult, McpError> {
        let health = match self.gateway().health().await {
            Ok(health) => health,
            Err(err) => {
                return Ok(gateway_tool_error(
                    "checking gateway liveness (GET /health)",
                    &err,
                ));
            }
        };

        let readiness = match self.gateway().ready().await {
            Ok(readiness) => readiness,
            Err(err) => {
                return Ok(gateway_tool_error(
                    "checking gateway readiness (GET /ready)",
                    &err,
                ));
            }
        };

        let version: VersionInfo = match self.gateway().version().await {
            Ok(version) => version,
            Err(err) => {
                return Ok(gateway_tool_error(
                    "querying gvmd version (GET /api/v1/version)",
                    &err,
                ));
            }
        };

        // One authenticated call verifies the identity the gateway will use
        // for real tool calls (the caller's forwarded credentials, or the
        // configured fallback). A 401 here surfaces as "authentication failed".
        if let Err(err) = self
            .gateway()
            .get_json_query::<serde_json::Value>(&["targets"], &[("perPage", "1".to_string())])
            .await
        {
            return Ok(gateway_tool_error(
                "verifying gateway credentials (GET /api/v1/targets)",
                &err,
            ));
        }

        let report = TestConnectionReport {
            gateway_url: self.config().gateway_url.to_string(),
            gateway_status: health.status,
            gateway_readiness: readiness.status,
            gateway_readiness_reason: readiness.reason,
            api_version: version.api_version,
            gmp_version: version.gmp_version,
            authenticated: true,
            read_only: self.config().read_only,
            toolsets: self.config().toolsets.to_string(),
        };

        let text = serde_json::to_string_pretty(&report)
            .map_err(|err| McpError::internal_error(err.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    /// Get the GVM REST API contract version and the GMP protocol version
    /// reported by gvmd.
    #[tool(
        name = "openvas_get_version",
        annotations(title = "Get GVM versions", read_only_hint = true)
    )]
    pub async fn get_version(&self) -> Result<CallToolResult, McpError> {
        match self.gateway().version().await {
            Ok(version) => json_result(&version),
            Err(err) => Ok(gateway_tool_error(
                "querying gvmd version (GET /api/v1/version)",
                &err,
            )),
        }
    }

    /// List the IANA timezones known to the connected gvmd (use these names
    /// for schedules). Backends without timezone discovery answer 501.
    #[tool(
        name = "openvas_list_timezones",
        annotations(title = "List timezones", read_only_hint = true)
    )]
    pub async fn list_timezones(&self) -> Result<CallToolResult, McpError> {
        get_passthrough(self.gateway(), &["timezones"], "listing timezones").await
    }
}
