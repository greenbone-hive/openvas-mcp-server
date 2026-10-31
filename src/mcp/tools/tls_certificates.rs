//! TLS-certificates toolset: global certificate assets discovered by scans.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData as McpError, tool, tool_router};

use crate::mcp::server::GvmMcpServer;

use super::common::{GetByIdParams, ListParams, get_passthrough, list_summarized};

const ROW_KEYS: &[&str] = &[
    "id",
    "name",
    "subjectDn",
    "issuerDn",
    "activationTime",
    "expirationTime",
    "sha256Fingerprint",
    "valid",
];

#[tool_router(router = tls_certificates_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List TLS certificates discovered across all scans (subject, issuer,
    /// validity window, fingerprint).
    #[tool(
        name = "openvas_list_tls_certificates",
        annotations(title = "List TLS certificates", read_only_hint = true)
    )]
    pub async fn list_tls_certificates(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "tls-certificates",
            "tlsCertificates",
            ROW_KEYS,
            &params,
            "listing TLS certificates",
        )
        .await
    }

    /// Get one TLS certificate by UUID, including the PEM certificate.
    #[tool(
        name = "openvas_get_tls_certificate",
        annotations(title = "Get TLS certificate", read_only_hint = true)
    )]
    pub async fn get_tls_certificate(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        get_passthrough(
            self.gateway(),
            &["tls-certificates", &params.id],
            "fetching the TLS certificate",
        )
        .await
    }
}
