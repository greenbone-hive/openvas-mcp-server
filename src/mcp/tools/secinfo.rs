//! SecInfo toolset: CVE, CPE, CERT-Bund and DFN-CERT catalogs (feed
//! data; empty until the SCAP/CERT feeds are synced).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::server::GvmMcpServer;

use super::common::{ListParams, get_passthrough, list_summarized};

const ROW_KEYS: &[&str] = &["id", "name"];

/// Arguments for SecInfo get tools (identifiers are CVE/CPE/advisory ids,
/// not UUIDs).
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SecInfoIdParams {
    /// SecInfo identifier, e.g. `CVE-2024-3094`, a CPE name or an advisory id
    pub id: String,
}

#[tool_router(router = secinfo_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List CVEs from the SCAP feed, filterable with GMP expressions.
    #[tool(
        name = "openvas_list_cves",
        annotations(title = "List CVEs", read_only_hint = true)
    )]
    pub async fn list_cves(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "cves",
            "cves",
            ROW_KEYS,
            &params,
            "listing CVEs",
        )
        .await
    }

    /// Get one CVE by identifier (e.g. `CVE-2024-3094`).
    #[tool(
        name = "openvas_get_cve",
        annotations(title = "Get CVE", read_only_hint = true)
    )]
    pub async fn get_cve(
        &self,
        Parameters(params): Parameters<SecInfoIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(self.gateway(), &["cves", &params.id], "fetching the CVE").await
    }

    /// List CPEs (platform identifiers) from the SCAP feed.
    #[tool(
        name = "openvas_list_cpes",
        annotations(title = "List CPEs", read_only_hint = true)
    )]
    pub async fn list_cpes(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "cpes",
            "cpes",
            ROW_KEYS,
            &params,
            "listing CPEs",
        )
        .await
    }

    /// Get one CPE by name.
    #[tool(
        name = "openvas_get_cpe",
        annotations(title = "Get CPE", read_only_hint = true)
    )]
    pub async fn get_cpe(
        &self,
        Parameters(params): Parameters<SecInfoIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(self.gateway(), &["cpes", &params.id], "fetching the CPE").await
    }

    /// List CERT-Bund advisories from the CERT feed.
    #[tool(
        name = "openvas_list_cert_bund_advisories",
        annotations(title = "List CERT-Bund advisories", read_only_hint = true)
    )]
    pub async fn list_cert_bund_advisories(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "cert-bund-advisories",
            "certBundAdvisories",
            ROW_KEYS,
            &params,
            "listing CERT-Bund advisories",
        )
        .await
    }

    /// Get one CERT-Bund advisory by identifier.
    #[tool(
        name = "openvas_get_cert_bund_advisory",
        annotations(title = "Get CERT-Bund advisory", read_only_hint = true)
    )]
    pub async fn get_cert_bund_advisory(
        &self,
        Parameters(params): Parameters<SecInfoIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["cert-bund-advisories", &params.id],
            "fetching the CERT-Bund advisory",
        )
        .await
    }

    /// List DFN-CERT advisories from the CERT feed.
    #[tool(
        name = "openvas_list_dfn_cert_advisories",
        annotations(title = "List DFN-CERT advisories", read_only_hint = true)
    )]
    pub async fn list_dfn_cert_advisories(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "dfn-cert-advisories",
            "dfnCertAdvisories",
            ROW_KEYS,
            &params,
            "listing DFN-CERT advisories",
        )
        .await
    }

    /// Get one DFN-CERT advisory by identifier.
    #[tool(
        name = "openvas_get_dfn_cert_advisory",
        annotations(title = "Get DFN-CERT advisory", read_only_hint = true)
    )]
    pub async fn get_dfn_cert_advisory(
        &self,
        Parameters(params): Parameters<SecInfoIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["dfn-cert-advisories", &params.id],
            "fetching the DFN-CERT advisory",
        )
        .await
    }
}
