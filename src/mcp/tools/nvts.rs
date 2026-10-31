//! NVTs toolset: read surface over vulnerability tests and their families.
//! The gateway's NVT catalog uses typed selectors (config, family, sort)
//! rather than GMP filter expressions.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::{PageParams, get_passthrough, list_summarized_query};

const ROW_KEYS: &[&str] = &[
    "oid",
    "name",
    "family",
    "severity",
    "cvssBase",
    "solutionType",
];

/// Arguments for `openvas_get_nvt` (NVTs are addressed by OID, not UUID).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetNvtParams {
    /// NVT OID, e.g. `1.3.6.1.4.1.25623.1.0.100315`
    pub oid: String,
}

/// Arguments for `openvas_search_nvts`: the gateway's typed NVT selectors.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SearchNvtsParams {
    #[serde(flatten)]
    pub page: PageParams,
    /// Restrict to the NVTs selected by this scan config
    pub config_id: Option<String>,
    /// Resolve preference values from this scan config
    pub preferences_config_id: Option<String>,
    /// Restrict to one NVT family, e.g. "Web Servers"
    pub family: Option<String>,
    /// Include preference definitions and values
    pub include_preferences: Option<bool>,
    /// Include each NVT's preference count
    pub include_preference_count: Option<bool>,
    /// Include the special timeout preference (requires config_id)
    pub include_timeout: Option<bool>,
    /// Backend sort direction: ascending or descending
    pub sort_order: Option<String>,
    /// Backend NVT field used for sorting, e.g. "name" or "cvss_base"
    pub sort_field: Option<String>,
}

impl SearchNvtsParams {
    pub fn to_query(&self) -> Vec<(&'static str, String)> {
        let mut query = self.page.to_query();
        let mut push_str = |key: &'static str, value: &Option<String>| {
            if let Some(value) = value {
                query.push((key, value.clone()));
            }
        };
        push_str("configId", &self.config_id);
        push_str("preferencesConfigId", &self.preferences_config_id);
        push_str("family", &self.family);
        push_str("sortOrder", &self.sort_order);
        push_str("sortField", &self.sort_field);
        let mut push_bool = |key: &'static str, value: Option<bool>| {
            if let Some(value) = value {
                query.push((key, value.to_string()));
            }
        };
        push_bool("includePreferences", self.include_preferences);
        push_bool("includePreferenceCount", self.include_preference_count);
        push_bool("includeTimeout", self.include_timeout);
        query
    }
}

#[tool_router(router = nvts_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// Browse the NVT (vulnerability test) catalog with typed selectors:
    /// by family, by scan config, with preferences, sorted. For free-text
    /// search across findings use openvas_list_results with a filter.
    #[tool(
        name = "openvas_search_nvts",
        annotations(title = "Search NVTs", read_only_hint = true)
    )]
    pub async fn search_nvts(
        &self,
        Parameters(params): Parameters<SearchNvtsParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized_query(
            self.gateway(),
            &["nvts"],
            &params.to_query(),
            "nvts",
            ROW_KEYS,
            "searching NVTs",
        )
        .await
    }

    /// Get one NVT by OID, including tags and solution details.
    #[tool(
        name = "openvas_get_nvt",
        annotations(title = "Get NVT", read_only_hint = true)
    )]
    pub async fn get_nvt(
        &self,
        Parameters(params): Parameters<GetNvtParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(self.gateway(), &["nvts", &params.oid], "fetching the NVT").await
    }

    /// List NVT families with their test counts. Pagination only.
    #[tool(
        name = "openvas_list_nvt_families",
        annotations(title = "List NVT families", read_only_hint = true)
    )]
    pub async fn list_nvt_families(
        &self,
        Parameters(params): Parameters<PageParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized_query(
            self.gateway(),
            &["nvt-families"],
            &params.to_query(),
            "families",
            &["name", "maxNvtCount"],
            "listing NVT families",
        )
        .await
    }
}
