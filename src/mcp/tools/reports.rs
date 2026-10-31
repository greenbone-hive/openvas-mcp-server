//! Reports toolset: reads, every drill-down page, XML import and the
//! asynchronous export-job lifecycle.
//!
//! Export jobs are the one stateful workflow: the gateway binds a job to the
//! session that created it and cancels the job when that session ends, and
//! request-scoped `Basic` sessions end with the request. So when the caller
//! authenticates with `Basic` credentials, `openvas_export_report` opens one
//! gateway session on their behalf, runs the job on it, serves the polls and
//! the download through it, and deletes it once the job is downloaded,
//! cancelled or terminal (see `gateway::jobs`). Callers that forward their
//! own gateway bearer token already own a session; it is used as-is.

use base64::Engine;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{ErrorData as McpError, schemars, tool, tool_router};
use serde::Deserialize;

use crate::mcp::error::gateway_tool_error;
use crate::mcp::server::GvmMcpServer;

use super::common::{
    Body, GetByIdParams, ListParams, delete_resource_plain, get_passthrough, get_passthrough_query,
    json_result, list_summarized, list_summarized_at,
};

const ROW_KEYS: &[&str] = &[
    "id",
    "task",
    "scanStart",
    "scanEnd",
    "severity",
    "resultCount",
];

/// Per-page row keys, taken from the gateway's response schemas so each
/// drill-down keeps the fields that identify its rows.
const RESULT_ROW_KEYS: &[&str] = &[
    "id",
    "name",
    "host",
    "port",
    "severity",
    "threat",
    "occurrences",
];
const VULNERABILITY_ROW_KEYS: &[&str] = &[
    "id",
    "nvt",
    "host",
    "hostsCount",
    "port",
    "severity",
    "threat",
    "occurrences",
];
const ERROR_ROW_KEYS: &[&str] = &["id", "name", "nvtName", "host", "port", "description"];
const CLOSED_CVE_ROW_KEYS: &[&str] = &["id", "cve", "nvt", "host", "severity", "threat"];
const NAMED_SEVERITY_ROW_KEYS: &[&str] = &["id", "name", "severity"];
const OS_ROW_KEYS: &[&str] = &["bestOsCpe", "bestOsText", "hostsCount"];
const TLS_ROW_KEYS: &[&str] = &[
    "id",
    "host",
    "port",
    "subject",
    "issuer",
    "notBefore",
    "notAfter",
    "fingerprintSha256",
];

/// Inline artifacts above this size are refused; ask for a narrower filter
/// or page through the results instead. Keeps tool output within an
/// LLM-usable budget and bounds this server's memory per call.
pub const MAX_INLINE_ARTIFACT_BYTES: usize = 3 * 1024 * 1024;

/// Upper bound for an imported XML report (mirrors the gateway's limit).
const MAX_IMPORT_BYTES: usize = 10 * 1024 * 1024;

/// Arguments for report drill-down pages: report UUID plus list filtering.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReportPageParams {
    /// Report UUID
    pub id: String,
    #[serde(flatten)]
    pub list: ListParams,
}

/// Arguments for `openvas_get_report`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetReportParams {
    /// Report UUID
    pub id: String,
    /// Page of embedded results to include (1-indexed)
    pub page: Option<u32>,
    /// Embedded results per page (1-1000)
    pub per_page: Option<u32>,
}

/// Arguments for `openvas_export_report`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExportReportParams {
    /// Report UUID
    pub id: String,
    /// UUID of a gvmd report format (see openvas_list_report_formats).
    /// Omit to request the gateway's native JSON export.
    pub report_format_id: Option<String>,
    /// Optional report configuration UUID (gvmd report formats only)
    pub report_config_id: Option<String>,
    /// Optional GMP filter expression applied during generation
    pub filter: Option<String>,
    /// Optional saved filter UUID applied during generation
    pub filter_id: Option<String>,
}

/// Arguments for `openvas_import_report`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ImportReportParams {
    /// UUID of an existing import task (create one with openvas_create_task
    /// and `type: "import"`)
    pub task_id: String,
    /// One complete GMP XML report envelope (`<report>…</report>`), at most
    /// 10 MiB
    pub report_xml: String,
    /// Whether hosts from the report are added to assets (omit for the
    /// backend default)
    pub in_assets: Option<bool>,
}

#[tool_router(router = reports_router, vis = "pub(crate)")]
impl GvmMcpServer {
    /// List scan reports with severity and result counts.
    #[tool(
        name = "openvas_list_reports",
        annotations(title = "List reports", read_only_hint = true)
    )]
    pub async fn list_reports(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        list_summarized(
            self.gateway(),
            "reports",
            "reports",
            ROW_KEYS,
            &params,
            "listing reports",
        )
        .await
    }

    /// Get one scan report by UUID: task, timing, severity, result counts
    /// and one page of embedded results (`page`/`per_page`).
    #[tool(
        name = "openvas_get_report",
        annotations(title = "Get report", read_only_hint = true)
    )]
    pub async fn get_report(
        &self,
        Parameters(params): Parameters<GetReportParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut query = Vec::new();
        if let Some(page) = params.page {
            query.push(("page", page.to_string()));
        }
        if let Some(per_page) = params.per_page {
            query.push(("perPage", per_page.to_string()));
        }
        get_passthrough_query(
            self.gateway(),
            &["reports", &params.id],
            &query,
            "fetching the report",
        )
        .await
    }

    /// Permanently delete a scan report (reports have no trashcan). The
    /// task itself is not affected.
    #[tool(
        name = "openvas_delete_report",
        annotations(
            title = "Delete report",
            read_only_hint = false,
            destructive_hint = true
        )
    )]
    pub async fn delete_report(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        delete_resource_plain(self.gateway(), "reports", &params.id, "deleting the report").await
    }

    /// Import a GMP XML report into an existing import task. Returns the
    /// new report's UUID.
    #[tool(
        name = "openvas_import_report",
        annotations(
            title = "Import report",
            read_only_hint = false,
            destructive_hint = false
        )
    )]
    pub async fn import_report(
        &self,
        Parameters(params): Parameters<ImportReportParams>,
    ) -> Result<CallToolResult, McpError> {
        let xml = params.report_xml.into_bytes();
        if xml.is_empty() || xml.len() > MAX_IMPORT_BYTES {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "report_xml must be between 1 and {MAX_IMPORT_BYTES} bytes (got {})",
                xml.len()
            ))]));
        }
        let mut query = vec![("taskId", params.task_id)];
        if let Some(in_assets) = params.in_assets {
            query.push(("inAssets", in_assets.to_string()));
        }
        match self
            .gateway()
            .post_raw::<serde_json::Value>(&["reports"], &query, "application/xml", xml)
            .await
        {
            Ok(created) => json_result(&created),
            Err(err) => Ok(gateway_tool_error("importing the report", &err)),
        }
    }

    /// List the individual results (findings) inside one report. Supports
    /// GMP filter expressions like `severity>7`.
    #[tool(
        name = "openvas_get_report_results",
        annotations(title = "Report results", read_only_hint = true)
    )]
    pub async fn get_report_results(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(&params, "results", "results", RESULT_ROW_KEYS)
            .await
    }

    /// List the vulnerabilities page of one report (aggregated findings
    /// per NVT).
    #[tool(
        name = "openvas_get_report_vulnerabilities",
        annotations(title = "Report vulnerabilities", read_only_hint = true)
    )]
    pub async fn get_report_vulnerabilities(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(
            &params,
            "vulnerabilities",
            "vulnerabilities",
            VULNERABILITY_ROW_KEYS,
        )
        .await
    }

    /// List scan errors recorded in one report (NVTs that failed to run).
    #[tool(
        name = "openvas_get_report_errors",
        annotations(title = "Report errors", read_only_hint = true)
    )]
    pub async fn get_report_errors(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(&params, "errors", "errors", ERROR_ROW_KEYS)
            .await
    }

    /// List CVEs that are closed (no longer detected) according to one
    /// report.
    #[tool(
        name = "openvas_get_report_closed_cves",
        annotations(title = "Report closed CVEs", read_only_hint = true)
    )]
    pub async fn get_report_closed_cves(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(&params, "closed-cves", "closedCves", CLOSED_CVE_ROW_KEYS)
            .await
    }

    /// List TLS certificates discovered during the scan behind one report.
    #[tool(
        name = "openvas_get_report_tls_certificates",
        annotations(title = "Report TLS certificates", read_only_hint = true)
    )]
    pub async fn get_report_tls_certificates(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(&params, "tls-certificates", "tlsCertificates", TLS_ROW_KEYS)
            .await
    }

    /// List the hosts covered by one report with their severity.
    #[tool(
        name = "openvas_get_report_hosts",
        annotations(title = "Report hosts", read_only_hint = true)
    )]
    pub async fn get_report_hosts(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(&params, "hosts", "hosts", NAMED_SEVERITY_ROW_KEYS)
            .await
    }

    /// List the ports found open in one report with their severity.
    #[tool(
        name = "openvas_get_report_ports",
        annotations(title = "Report ports", read_only_hint = true)
    )]
    pub async fn get_report_ports(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(&params, "ports", "ports", NAMED_SEVERITY_ROW_KEYS)
            .await
    }

    /// List the applications (CPEs) detected in one report.
    #[tool(
        name = "openvas_get_report_applications",
        annotations(title = "Report applications", read_only_hint = true)
    )]
    pub async fn get_report_applications(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(
            &params,
            "applications",
            "applications",
            NAMED_SEVERITY_ROW_KEYS,
        )
        .await
    }

    /// List the CVEs referenced by the findings of one report.
    #[tool(
        name = "openvas_get_report_cves",
        annotations(title = "Report CVEs", read_only_hint = true)
    )]
    pub async fn get_report_cves(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(&params, "cves", "cves", NAMED_SEVERITY_ROW_KEYS)
            .await
    }

    /// List the operating systems detected in one report.
    #[tool(
        name = "openvas_get_report_operating_systems",
        annotations(title = "Report operating systems", read_only_hint = true)
    )]
    pub async fn get_report_operating_systems(
        &self,
        Parameters(params): Parameters<ReportPageParams>,
    ) -> Result<CallToolResult, McpError> {
        self.report_page(
            &params,
            "operating-systems",
            "operatingSystems",
            OS_ROW_KEYS,
        )
        .await
    }

    /// Start an asynchronous report export. Returns a job (usually
    /// `queued`); poll it with openvas_get_job, then fetch the artifact with
    /// openvas_download_job_result once the status is `succeeded`. Jobs and
    /// artifacts expire ~15 minutes after completion; keep polling at least
    /// every few minutes, since an idle job session is closed by the gateway.
    #[tool(
        name = "openvas_export_report",
        annotations(title = "Export report", read_only_hint = true)
    )]
    pub async fn export_report(
        &self,
        Parameters(params): Parameters<ExportReportParams>,
    ) -> Result<CallToolResult, McpError> {
        let body = match &params.report_format_id {
            Some(format_id) => Body::new()
                .set("reportFormatId", format_id.clone())
                .set_opt("reportConfigId", params.report_config_id),
            None => Body::new().set("format", "json"),
        }
        .set_opt("filter", params.filter)
        .set_opt("filterId", params.filter_id)
        .into_value();
        let segments = ["reports", params.id.as_str(), "exports"];

        // Bearer callers own a persistent gateway session already; anyone
        // else (Basic caller, stdio fallback) needs a job-bound session or
        // the gateway would cancel the job when this request's scoped
        // session ends.
        let identity = self.gateway().current_identity();
        let needs_session = identity
            .as_deref()
            .is_some_and(|auth| auth.starts_with("Basic "));
        if !needs_session {
            return match self
                .gateway()
                .post_json::<serde_json::Value>(&segments, &body)
                .await
            {
                Ok(job) => json_result(&job),
                Err(err) => Ok(gateway_tool_error("starting the report export", &err)),
            };
        }
        let owner = identity.expect("checked above");

        let token = match self.gateway().create_session(&owner).await {
            Ok(token) => token,
            Err(err) => {
                return Ok(gateway_tool_error(
                    "opening a gateway session for the report export",
                    &err,
                ));
            }
        };
        let bearer = format!("Bearer {token}");
        let job = match self
            .gateway()
            .post_json_as::<serde_json::Value>(&bearer, &segments, &body)
            .await
        {
            Ok(job) => job,
            Err(err) => {
                self.close_export_session(&token).await;
                return Ok(gateway_tool_error("starting the report export", &err));
            }
        };
        match job["id"].as_str() {
            Some(job_id) => {
                for stale in self.jobs().track(job_id.to_owned(), token, &owner) {
                    self.close_export_session(&stale).await;
                }
            }
            None => self.close_export_session(&token).await,
        }
        json_result(&job)
    }

    /// Get the status of an asynchronous job (queued, running, succeeded,
    /// failed, cancelling, cancelled or expired).
    #[tool(
        name = "openvas_get_job",
        annotations(title = "Get job status", read_only_hint = true)
    )]
    pub async fn get_job(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some((owner, token)) = self.export_session(&params.id) else {
            return get_passthrough(self.gateway(), &["jobs", &params.id], "fetching the job")
                .await;
        };
        let job = match self
            .gateway()
            .get_json_as::<serde_json::Value>(&format!("Bearer {token}"), &["jobs", &params.id])
            .await
        {
            Ok(job) => job,
            Err(err) => return Ok(gateway_tool_error("fetching the job", &err)),
        };
        // Nothing left to download: release the job-bound session.
        if matches!(
            job["status"].as_str(),
            Some("failed" | "cancelled" | "expired")
        ) {
            self.release_export_session(&params.id, &owner).await;
        }
        json_result(&job)
    }

    /// Cancel an asynchronous job you created.
    #[tool(
        name = "openvas_cancel_job",
        annotations(title = "Cancel job", read_only_hint = false, destructive_hint = true)
    )]
    pub async fn cancel_job(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let outcome = match self.export_session(&params.id) {
            Some((owner, token)) => {
                let outcome = self
                    .gateway()
                    .delete_as(&format!("Bearer {token}"), &["jobs", &params.id])
                    .await;
                self.release_export_session(&params.id, &owner).await;
                outcome
            }
            None => self.gateway().delete(&["jobs", &params.id], &[]).await,
        };
        match outcome {
            Ok(()) => json_result(&serde_json::json!({ "cancelled": true, "id": params.id })),
            Err(err) => Ok(gateway_tool_error("cancelling the job", &err)),
        }
    }

    /// Download the artifact of a succeeded export job. JSON exports are
    /// returned inline; binary formats (PDF, XML, CSV…) are returned as
    /// base64. Both are capped at 3 MB — for larger artifacts, export with
    /// a narrower filter or page through the results instead.
    #[tool(
        name = "openvas_download_job_result",
        annotations(title = "Download job result", read_only_hint = true)
    )]
    pub async fn download_job_result(
        &self,
        Parameters(params): Parameters<GetByIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let segments = ["jobs", params.id.as_str(), "result"];
        let session = self.export_session(&params.id);
        let download = match &session {
            Some((_, token)) => {
                self.gateway()
                    .get_bytes_as(
                        &format!("Bearer {token}"),
                        &segments,
                        &[],
                        MAX_INLINE_ARTIFACT_BYTES,
                    )
                    .await
            }
            None => {
                self.gateway()
                    .get_bytes(&segments, &[], MAX_INLINE_ARTIFACT_BYTES)
                    .await
            }
        };
        let artifact = match download {
            Ok(artifact) => artifact,
            Err(err) => return Ok(gateway_tool_error("downloading the job result", &err)),
        };
        // The artifact is in hand: the job-bound session has done its work.
        if let Some((owner, _)) = session {
            self.release_export_session(&params.id, &owner).await;
        }

        let is_json = artifact
            .content_type
            .as_deref()
            .is_some_and(|ct| ct.starts_with("application/json"));
        if is_json {
            let text = String::from_utf8_lossy(&artifact.bytes).into_owned();
            return Ok(CallToolResult::success(vec![ContentBlock::text(text)]));
        }

        json_result(&serde_json::json!({
            "contentType": artifact.content_type,
            "filename": artifact.filename,
            "size": artifact.bytes.len(),
            "base64": base64::engine::general_purpose::STANDARD.encode(&artifact.bytes),
        }))
    }
}

impl GvmMcpServer {
    /// The job-bound session for `job_id`, if this server created the job
    /// for the identity making the current call.
    fn export_session(&self, job_id: &str) -> Option<(String, String)> {
        let owner = self.gateway().current_identity()?;
        let token = self.jobs().token_for(job_id, &owner)?;
        Some((owner, token))
    }

    /// Forget the job and close its gateway session (best effort: the
    /// gateway also expires idle sessions and terminal jobs on its own).
    async fn release_export_session(&self, job_id: &str, owner: &str) {
        if let Some(token) = self.jobs().untrack(job_id, owner) {
            self.close_export_session(&token).await;
        }
    }

    async fn close_export_session(&self, token: &str) {
        if let Err(err) = self.gateway().delete_session(token).await {
            tracing::debug!(error = %err, "closing export job session failed");
        }
    }

    async fn report_page(
        &self,
        params: &ReportPageParams,
        page: &str,
        out_key: &str,
        keys: &[&str],
    ) -> Result<CallToolResult, McpError> {
        list_summarized_at(
            self.gateway(),
            &["reports", &params.id, page],
            out_key,
            keys,
            &params.list,
            &format!("listing report {page}"),
        )
        .await
    }
}
