//! Mock-gateway tests for the targets and tasks toolsets: request shapes
//! (paths, query params, JSON bodies), summarized list output and legible
//! error surfaces.

mod support;

use gvm_mcp::mcp::GvmMcpServer;
use gvm_mcp::mcp::tools::common::{DeleteParams, GetByIdParams, ListParams};
use gvm_mcp::mcp::tools::tasks::CreateTaskParams;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use support::{EXPECTED_BASIC, config_for, problem_response};
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
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

fn pagination() -> serde_json::Value {
    serde_json::json!({"page": 1, "perPage": 25, "total": 1, "totalPages": 1})
}

#[tokio::test]
async fn list_targets_passes_filters_and_summarizes_rows() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(query_param("filter", "name~web"))
        .and(query_param("page", "2"))
        .and(query_param("perPage", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "id": "tg-1",
                "name": "webservers",
                "hosts": ["10.0.0.0/24"],
                "portList": {"id": "pl-1", "name": "All TCP"},
                "sshCredential": {"id": "c-1", "name": "root-key"},
                "inUse": true,
                "writable": true
            }],
            "pagination": pagination()
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .list_targets(Parameters(ListParams {
            filter: Some("name~web".into()),
            filter_id: None,
            page: Some(2),
            per_page: Some(10),
        }))
        .await
        .unwrap();

    let json = json_of(&result);
    assert_eq!(json["targets"][0]["id"], "tg-1");
    assert_eq!(json["targets"][0]["portList"], "All TCP");
    // Summarized rows must stay compact: no credential objects in list output.
    assert!(json["targets"][0].get("sshCredential").is_none());
    assert_eq!(json["pagination"]["total"], 1);
}

#[tokio::test]
async fn get_target_returns_full_details() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets/tg-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "tg-1",
            "name": "webservers",
            "hosts": ["10.0.0.0/24"],
            "aliveTest": "ICMP Ping",
            "sshCredential": {"id": "c-1", "name": "root-key"}
        })))
        .mount(&server)
        .await;

    let result = mcp
        .get_target(Parameters(GetByIdParams { id: "tg-1".into() }))
        .await
        .unwrap();
    let json = json_of(&result);
    assert_eq!(json["aliveTest"], "ICMP Ping");
    assert_eq!(json["sshCredential"]["name"], "root-key");
}

#[tokio::test]
async fn get_target_404_is_a_legible_tool_error() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets/missing"))
        .respond_with(problem_response(404, "not_found", "Resource Not Found"))
        .mount(&server)
        .await;

    let result = mcp
        .get_target(Parameters(GetByIdParams {
            id: "missing".into(),
        }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = text_of(&result);
    assert!(text.contains("Not found"), "got: {text}");
    assert!(text.contains("not_found"), "got: {text}");
}

#[tokio::test]
async fn list_tasks_summarizes_status_and_reports() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/tasks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "id": "t-1",
                "name": "weekly",
                "status": "Running",
                "progress": 42,
                "target": {"id": "tg-1", "name": "webservers"},
                "scanConfig": {"id": "sc-1", "name": "Full and fast"},
                "lastReport": {"id": "r-9"},
                "reportCount": 3
            }],
            "pagination": pagination()
        })))
        .mount(&server)
        .await;

    let result = mcp
        .list_tasks(Parameters(ListParams::default()))
        .await
        .unwrap();
    let json = json_of(&result);
    let row = &json["tasks"][0];
    assert_eq!(row["status"], "Running");
    assert_eq!(row["progress"], 42);
    assert_eq!(row["target"], "webservers");
    assert_eq!(row["lastReportId"], "r-9");
}

#[tokio::test]
async fn create_task_sends_camel_case_body() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/tasks"))
        .and(body_partial_json(serde_json::json!({
            "name": "scan web",
            "targetId": "tg-1",
            "scanConfigId": "sc-1",
            "scannerId": "s-1"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": "t-new"})))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .create_task(Parameters(CreateTaskParams {
            name: "scan web".into(),
            comment: None,
            task_type: None,
            target_id: Some("tg-1".into()),
            agent_group_id: None,
            oci_image_target_id: None,
            web_application_target_id: None,
            scan_config_id: Some("sc-1".into()),
            scanner_id: Some("s-1".into()),
            schedule_id: None,
            alert_ids: None,
            alterable: None,
            observers: None,
            schedule_periods: None,
            preferences: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["id"], "t-new");
}

#[tokio::test]
async fn start_and_resume_return_report_id() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/tasks/t-1/start"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"reportId": "r-1"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/tasks/t-1/resume"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"reportId": "r-2"})),
        )
        .mount(&server)
        .await;

    let started = mcp
        .start_task(Parameters(GetByIdParams { id: "t-1".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&started)["reportId"], "r-1");

    let resumed = mcp
        .resume_task(Parameters(GetByIdParams { id: "t-1".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&resumed)["reportId"], "r-2");
}

#[tokio::test]
async fn start_conflict_is_a_legible_tool_error() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/tasks/t-1/start"))
        .respond_with(problem_response(409, "conflict", "Task Already Running"))
        .mount(&server)
        .await;

    let result = mcp
        .start_task(Parameters(GetByIdParams { id: "t-1".into() }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(text_of(&result).contains("Task Already Running"));
}

#[tokio::test]
async fn stop_task_accepts_empty_success_body() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/tasks/t-1/stop"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let result = mcp
        .stop_task(Parameters(GetByIdParams { id: "t-1".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["stopped"], true);
}

#[tokio::test]
async fn delete_task_defaults_to_trashcan_and_supports_ultimate() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/tasks/t-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/tasks/t-2"))
        .and(query_param("ultimate", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let trashed = mcp
        .delete_task(Parameters(DeleteParams {
            id: "t-1".into(),
            ultimate: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&trashed)["deleted"], true);

    let purged = mcp
        .delete_task(Parameters(DeleteParams {
            id: "t-2".into(),
            ultimate: Some(true),
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&purged)["deleted"], true);
}

#[tokio::test]
async fn write_tool_forwards_fallback_basic_credential() {
    let server = MockServer::start().await;
    // The gateway only answers when the request carries the configured
    // fallback Basic credential — proving gvm-mcp forwards an identity per
    // request rather than logging in and reusing a session token.
    Mock::given(method("POST"))
        .and(path("/api/v1/tasks/t-1/start"))
        .and(header("authorization", EXPECTED_BASIC))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"reportId": "r-1"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mcp = GvmMcpServer::new(config_for(&server)).unwrap();
    let result = mcp
        .start_task(Parameters(GetByIdParams { id: "t-1".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["reportId"], "r-1");
}
