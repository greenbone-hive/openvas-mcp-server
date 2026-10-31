//! Mock-gateway tests for report drill-down pages and the async export-job
//! lifecycle (create → poll → download/cancel), including the base64
//! envelope and inline-size cap for binary artifacts.

mod support;

use gvm_mcp::mcp::GvmMcpServer;
use gvm_mcp::mcp::tools::common::{GetByIdParams, ListParams};
use gvm_mcp::mcp::tools::reports::{ExportReportParams, ReportPageParams};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use support::{EXPECTED_BASIC, config_for, problem_response};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn json_of(result: &CallToolResult) -> serde_json::Value {
    serde_json::from_str(&text_of(result)).expect("tool output must be JSON")
}

async fn server_with_login() -> (MockServer, GvmMcpServer) {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_for(&server)).unwrap();
    (server, mcp)
}

/// The gateway binds export jobs to the creating session, so a Basic caller
/// gets a job-bound session: `POST /session` with the caller's Basic
/// credential answers a bearer token the export then runs on.
async fn mount_export_session(server: &MockServer, token: &str) {
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .and(header("authorization", EXPECTED_BASIC))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "sessionToken": token, "expiresIn": 300, "gmpVersion": "22.8"
        })))
        .mount(server)
        .await;
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn result_list_body() -> serde_json::Value {
    serde_json::json!({
        "data": [{
            "id": "r-1", "name": "OpenSSH < 8.0", "host": "10.0.0.5",
            "port": "22/tcp", "severity": 7.5, "threat": "High",
            "description": "long text dropped from rows"
        }],
        "pagination": {"page": 1, "perPage": 25, "total": 1, "totalPages": 1}
    })
}

#[tokio::test]
async fn report_drilldown_pages_hit_nested_paths_with_filters() {
    let (server, mcp) = server_with_login().await;
    for page in ["results", "vulnerabilities", "errors", "closed-cves"] {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/reports/rep-1/{page}")))
            .and(query_param("filter", "severity>7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(result_list_body()))
            .expect(1)
            .mount(&server)
            .await;
    }

    let params = || ReportPageParams {
        id: "rep-1".into(),
        list: ListParams {
            filter: Some("severity>7".into()),
            filter_id: None,
            page: None,
            per_page: None,
        },
    };

    let results = mcp.get_report_results(Parameters(params())).await.unwrap();
    assert_eq!(json_of(&results)["results"][0]["host"], "10.0.0.5");
    assert!(json_of(&results)["results"][0].get("description").is_none());

    let vulns = mcp
        .get_report_vulnerabilities(Parameters(params()))
        .await
        .unwrap();
    assert_eq!(json_of(&vulns)["vulnerabilities"][0]["threat"], "High");

    let errors = mcp.get_report_errors(Parameters(params())).await.unwrap();
    assert_eq!(json_of(&errors)["errors"][0]["id"], "r-1");

    let cves = mcp
        .get_report_closed_cves(Parameters(params()))
        .await
        .unwrap();
    assert_eq!(json_of(&cves)["closedCves"][0]["id"], "r-1");
}

#[tokio::test]
async fn report_tls_certificates_summarize() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/reports/rep-1/tls-certificates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "id": "tls-1", "host": "10.0.0.5", "port": 443,
                "subject": "CN=web01", "issuer": "CN=corp-ca",
                "notBefore": "2026-01-01T00:00:00Z", "notAfter": "2027-01-01T00:00:00Z",
                "fingerprintSha256": "ab:cd"
            }],
            "pagination": {"page": 1, "perPage": 25, "total": 1, "totalPages": 1}
        })))
        .mount(&server)
        .await;

    let result = mcp
        .get_report_tls_certificates(Parameters(ReportPageParams {
            id: "rep-1".into(),
            list: ListParams::default(),
        }))
        .await
        .unwrap();
    let json = json_of(&result);
    assert_eq!(json["tlsCertificates"][0]["subject"], "CN=web01");
    // notBefore is part of the gateway row schema and is kept.
    assert_eq!(
        json["tlsCertificates"][0]["notBefore"],
        "2026-01-01T00:00:00Z"
    );
}

#[tokio::test]
async fn export_report_defaults_to_json_format() {
    let (server, mcp) = server_with_login().await;
    mount_export_session(&server, "sess-json").await;
    Mock::given(method("POST"))
        .and(path("/api/v1/reports/rep-1/exports"))
        .and(header("authorization", bearer("sess-json").as_str()))
        .and(body_json(serde_json::json!({"format": "json"})))
        .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
            "id": "job-1", "kind": "report_export", "status": "queued",
            "createdAt": "2026-08-13T10:00:00Z",
            "report": {"id": "rep-1"}, "format": "json"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .export_report(Parameters(ExportReportParams {
            id: "rep-1".into(),
            report_format_id: None,
            report_config_id: None,
            filter: None,
            filter_id: None,
        }))
        .await
        .unwrap();
    let json = json_of(&result);
    assert_eq!(json["id"], "job-1");
    assert_eq!(json["status"], "queued");
}

#[tokio::test]
async fn export_report_with_gvmd_format_and_filter() {
    let (server, mcp) = server_with_login().await;
    mount_export_session(&server, "sess-pdf").await;
    Mock::given(method("POST"))
        .and(path("/api/v1/reports/rep-1/exports"))
        .and(header("authorization", bearer("sess-pdf").as_str()))
        .and(body_json(serde_json::json!({
            "reportFormatId": "rf-pdf",
            "filter": "severity>7"
        })))
        .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
            "id": "job-2", "kind": "report_export", "status": "queued",
            "createdAt": "2026-08-13T10:00:00Z",
            "report": {"id": "rep-1"}, "format": "gvmd_report_format"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .export_report(Parameters(ExportReportParams {
            id: "rep-1".into(),
            report_format_id: Some("rf-pdf".into()),
            report_config_id: None,
            filter: Some("severity>7".into()),
            filter_id: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["id"], "job-2");
}

#[tokio::test]
async fn job_poll_and_cancel() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "job-1", "kind": "report_export", "status": "succeeded",
            "createdAt": "2026-08-13T10:00:00Z",
            "resultLocation": "/api/v1/jobs/job-1/result",
            "result": {"contentType": "application/pdf", "filename": "report.pdf", "size": 12345}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/jobs/job-2"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;

    let job = mcp
        .get_job(Parameters(GetByIdParams { id: "job-1".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&job)["status"], "succeeded");

    let cancelled = mcp
        .cancel_job(Parameters(GetByIdParams { id: "job-2".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&cancelled)["cancelled"], true);
}

#[tokio::test]
async fn download_json_result_is_returned_inline() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-1/result"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            serde_json::json!({"report": {"id": "rep-1"}, "results": []}).to_string(),
            "application/json",
        ))
        .mount(&server)
        .await;

    let result = mcp
        .download_job_result(Parameters(GetByIdParams { id: "job-1".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["report"]["id"], "rep-1");
}

#[tokio::test]
async fn download_binary_result_is_base64_enveloped() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-1/result"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(b"%PDF-1.7 fake".to_vec(), "application/pdf")
                .insert_header("Content-Disposition", "attachment; filename=\"report.pdf\""),
        )
        .mount(&server)
        .await;

    let result = mcp
        .download_job_result(Parameters(GetByIdParams { id: "job-1".into() }))
        .await
        .unwrap();
    let json = json_of(&result);
    assert_eq!(json["contentType"], "application/pdf");
    assert_eq!(json["filename"], "report.pdf");
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(json["base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(decoded, b"%PDF-1.7 fake");
}

#[tokio::test]
async fn oversized_binary_result_is_refused_legibly() {
    let (server, mcp) = server_with_login().await;
    let big = vec![0u8; 3 * 1024 * 1024 + 1];
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-1/result"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(big, "application/pdf"))
        .mount(&server)
        .await;

    let result = mcp
        .download_job_result(Parameters(GetByIdParams { id: "job-1".into() }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(text_of(&result).contains("narrower filter"));
}

#[tokio::test]
async fn pending_job_result_conflict_is_legible() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-1/result"))
        .respond_with(problem_response(
            409,
            "job_not_complete",
            "Job Not Complete",
        ))
        .mount(&server)
        .await;

    let result = mcp
        .download_job_result(Parameters(GetByIdParams { id: "job-1".into() }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(text_of(&result).contains("Job Not Complete"));
}

// ───── job-bound export sessions ─────

fn queued_job(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "kind": "report_export", "status": "queued",
        "createdAt": "2026-10-31T10:00:00Z", "report": {"id": "rep-1"}, "format": "json"
    })
}

#[tokio::test]
async fn export_session_serves_poll_and_download_then_closes() {
    let (server, mcp) = server_with_login().await;
    mount_export_session(&server, "sess-1").await;
    Mock::given(method("POST"))
        .and(path("/api/v1/reports/rep-1/exports"))
        .and(header("authorization", "Bearer sess-1"))
        .respond_with(ResponseTemplate::new(202).set_body_json(queued_job("job-1")))
        .expect(1)
        .mount(&server)
        .await;
    // Poll and download must ride on the job's session, not on the Basic
    // identity (which would open and immediately tear down a new session).
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-1"))
        .and(header("authorization", "Bearer sess-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "job-1", "kind": "report_export", "status": "succeeded",
            "createdAt": "2026-10-31T10:00:00Z", "report": {"id": "rep-1"}, "format": "json"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-1/result"))
        .and(header("authorization", "Bearer sess-1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            serde_json::json!({"report": {"id": "rep-1"}, "results": []}).to_string(),
            "application/json",
        ))
        .expect(1)
        .mount(&server)
        .await;
    // The session is closed exactly once, after the download.
    Mock::given(method("DELETE"))
        .and(path("/api/v1/session"))
        .and(header("authorization", "Bearer sess-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let job = mcp
        .export_report(Parameters(ExportReportParams {
            id: "rep-1".into(),
            report_format_id: None,
            report_config_id: None,
            filter: None,
            filter_id: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&job)["id"], "job-1");

    let polled = mcp
        .get_job(Parameters(GetByIdParams { id: "job-1".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&polled)["status"], "succeeded");

    let downloaded = mcp
        .download_job_result(Parameters(GetByIdParams { id: "job-1".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&downloaded)["report"]["id"], "rep-1");
    server.verify().await;
}

#[tokio::test]
async fn cancelling_a_tracked_job_closes_its_session() {
    let (server, mcp) = server_with_login().await;
    mount_export_session(&server, "sess-2").await;
    Mock::given(method("POST"))
        .and(path("/api/v1/reports/rep-1/exports"))
        .and(header("authorization", "Bearer sess-2"))
        .respond_with(ResponseTemplate::new(202).set_body_json(queued_job("job-2")))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/jobs/job-2"))
        .and(header("authorization", "Bearer sess-2"))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/session"))
        .and(header("authorization", "Bearer sess-2"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    mcp.export_report(Parameters(ExportReportParams {
        id: "rep-1".into(),
        report_format_id: None,
        report_config_id: None,
        filter: None,
        filter_id: None,
    }))
    .await
    .unwrap();
    let cancelled = mcp
        .cancel_job(Parameters(GetByIdParams { id: "job-2".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&cancelled)["cancelled"], true);
    server.verify().await;
}

#[tokio::test]
async fn terminal_failed_job_closes_its_session() {
    let (server, mcp) = server_with_login().await;
    mount_export_session(&server, "sess-3").await;
    Mock::given(method("POST"))
        .and(path("/api/v1/reports/rep-1/exports"))
        .and(header("authorization", "Bearer sess-3"))
        .respond_with(ResponseTemplate::new(202).set_body_json(queued_job("job-3")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-3"))
        .and(header("authorization", "Bearer sess-3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "job-3", "kind": "report_export", "status": "failed",
            "createdAt": "2026-10-31T10:00:00Z", "report": {"id": "rep-1"}, "format": "json",
            "error": {"type": "", "code": "export_failed", "title": "Export failed", "status": 502}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/session"))
        .and(header("authorization", "Bearer sess-3"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    mcp.export_report(Parameters(ExportReportParams {
        id: "rep-1".into(),
        report_format_id: None,
        report_config_id: None,
        filter: None,
        filter_id: None,
    }))
    .await
    .unwrap();
    let polled = mcp
        .get_job(Parameters(GetByIdParams { id: "job-3".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&polled)["status"], "failed");
    server.verify().await;
}

#[tokio::test]
async fn export_failure_closes_the_fresh_session() {
    let (server, mcp) = server_with_login().await;
    mount_export_session(&server, "sess-4").await;
    Mock::given(method("POST"))
        .and(path("/api/v1/reports/missing/exports"))
        .respond_with(problem_response(404, "not_found", "Report Not Found"))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/session"))
        .and(header("authorization", "Bearer sess-4"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .export_report(Parameters(ExportReportParams {
            id: "missing".into(),
            report_format_id: None,
            report_config_id: None,
            filter: None,
            filter_id: None,
        }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(text_of(&result).contains("Report Not Found"));
    server.verify().await;
}

#[tokio::test]
async fn forwarded_bearer_callers_use_their_own_session() {
    let (server, mcp) = server_with_login().await;
    // No session is ever created for a Bearer caller.
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/reports/rep-1/exports"))
        .and(header("authorization", "Bearer caller-token"))
        .respond_with(ResponseTemplate::new(202).set_body_json(queued_job("job-5")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-5"))
        .and(header("authorization", "Bearer caller-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(queued_job("job-5")))
        .expect(1)
        .mount(&server)
        .await;

    let scoped =
        gvm_mcp::gateway::auth::CALLER_AUTH.scope(Some("Bearer caller-token".to_string()), async {
            let job = mcp
                .export_report(Parameters(ExportReportParams {
                    id: "rep-1".into(),
                    report_format_id: None,
                    report_config_id: None,
                    filter: None,
                    filter_id: None,
                }))
                .await
                .unwrap();
            assert_eq!(json_of(&job)["id"], "job-5");
            mcp.get_job(Parameters(GetByIdParams { id: "job-5".into() }))
                .await
                .unwrap()
        });
    let polled = scoped.await;
    assert_eq!(json_of(&polled)["status"], "queued");
    server.verify().await;
}

#[tokio::test]
async fn another_identity_cannot_borrow_a_tracked_export_session() {
    let (server, mcp) = server_with_login().await;
    mount_export_session(&server, "sess-6").await;
    Mock::given(method("POST"))
        .and(path("/api/v1/reports/rep-1/exports"))
        .and(header("authorization", "Bearer sess-6"))
        .respond_with(ResponseTemplate::new(202).set_body_json(queued_job("job-6")))
        .mount(&server)
        .await;
    // A different caller polling the same job id must present their own
    // identity; the gateway (not this server) decides what they may see.
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-6"))
        .and(header("authorization", "Basic b3RoZXI6b3RoZXI="))
        .respond_with(problem_response(404, "not_found", "Job Not Found"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-6"))
        .and(header("authorization", "Bearer sess-6"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    mcp.export_report(Parameters(ExportReportParams {
        id: "rep-1".into(),
        report_format_id: None,
        report_config_id: None,
        filter: None,
        filter_id: None,
    }))
    .await
    .unwrap();
    let other = gvm_mcp::gateway::auth::CALLER_AUTH
        .scope(Some("Basic b3RoZXI6b3RoZXI=".to_string()), async {
            mcp.get_job(Parameters(GetByIdParams { id: "job-6".into() }))
                .await
                .unwrap()
        })
        .await;
    assert_eq!(other.is_error, Some(true));
    server.verify().await;
}

#[tokio::test]
async fn untracked_job_ids_pass_through_with_the_caller_identity() {
    // Jobs created elsewhere (or before a restart) are still addressable:
    // the tools fall back to plain pass-through with the current identity.
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-7"))
        .and(header("authorization", EXPECTED_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(queued_job("job-7")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/session"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;
    let polled = mcp
        .get_job(Parameters(GetByIdParams { id: "job-7".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&polled)["id"], "job-7");
    server.verify().await;
}
