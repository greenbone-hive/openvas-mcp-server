//! Feeds toolset: feed status reads.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::list_summarized_query;

/// Arguments for `openvas_list_feeds`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct ListFeedsParams {
    /// Return one feed type only: NVT, CERT, SCAP or GVMD_DATA
    #[serde(rename = "type")]
    pub feed_type: Option<String>,
}

#[tool_router(router = feeds_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// Get the status of the feeds (NVT, CERT, SCAP, GVMD_DATA): versions,
    /// whether a sync is in progress, and the feed-access flags.
    #[tool(
        name = "openvas_list_feeds",
        annotations(title = "List feed status", read_only_hint = true)
    )]
    pub async fn list_feeds(
        &self,
        Parameters(params): Parameters<ListFeedsParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = match params.feed_type {
            Some(feed_type) => vec![("type", feed_type)],
            None => Vec::new(),
        };
        list_summarized_query(
            self.gateway(),
            &["feeds"],
            &query,
            "feeds",
            &[
                "type",
                "name",
                "version",
                "status",
                "currentlySyncing",
                "syncTimestamp",
            ],
            "listing feeds",
        )
        .await
    }
}
