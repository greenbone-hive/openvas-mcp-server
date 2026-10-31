//! Vulnerabilities toolset: the global vulnerability listing.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, tool, tool_router};

use crate::mcp::server::GvmMcpServer;

use super::common::{ListParams, list_summarized};

#[tool_router(router = vulnerabilities_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List vulnerabilities observed across all reports (one row per NVT),
    /// filterable with GMP expressions such as `severity>7`.
    #[tool(
        name = "openvas_list_vulnerabilities",
        annotations(title = "List vulnerabilities", read_only_hint = true)
    )]
    pub async fn list_vulnerabilities(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "vulnerabilities",
            "vulnerabilities",
            &["id", "name"],
            &params,
            "listing vulnerabilities",
        )
        .await
    }
}
