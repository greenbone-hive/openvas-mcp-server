//! Mock-gateway tests for the phase-3 write surface: exact request bodies
//! (camelCase, unset optionals omitted), PUT passthrough, deletes and the
//! error surface.

mod support;

use gvm_mcp::mcp::GvmMcpServer;
use gvm_mcp::mcp::tools::alerts::CreateAlertParams;
use gvm_mcp::mcp::tools::common::{DeleteParams, GetByIdParams};
use gvm_mcp::mcp::tools::credentials::CreateCredentialParams;
use gvm_mcp::mcp::tools::notes::CreateNoteParams;
use gvm_mcp::mcp::tools::overrides::CreateOverrideParams;
use gvm_mcp::mcp::tools::targets::{CreateTargetParams, UpdateTargetParams};
use gvm_mcp::mcp::tools::tasks::UpdateTaskParams;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use support::{config_for, problem_response};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn json_of(result: &CallToolResult) -> serde_json::Value {
    let text = result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(&text).expect("tool output must be JSON")
}

async fn server_with_login() -> (MockServer, GvmMcpServer) {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_for(&server)).unwrap();
    (server, mcp)
}

#[tokio::test]
async fn create_target_sends_exact_camel_case_body() {
    let (server, mcp) = server_with_login().await;
    // Exact body match: unset optionals must be omitted entirely.
    Mock::given(method("POST"))
        .and(path("/api/v1/targets"))
        .and(body_json(serde_json::json!({
            "name": "webservers",
            "hosts": ["10.0.0.0/24"],
            "portListId": "pl-1",
            "sshCredentialId": "c-1"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": "tg-new"})))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .create_target(Parameters(CreateTargetParams {
            name: "webservers".into(),
            hosts: vec!["10.0.0.0/24".into()],
            comment: None,
            exclude_hosts: None,
            alive_test: None,
            port_list_id: Some("pl-1".into()),
            reverse_lookup_only: None,
            reverse_lookup_unify: None,
            ssh_credential_id: Some("c-1".into()),
            smb_credential_id: None,
            esxi_credential_id: None,
            snmp_credential_id: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["id"], "tg-new");
}

#[tokio::test]
async fn update_target_puts_partial_body_and_returns_updated_resource() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/targets/tg-1"))
        .and(body_json(serde_json::json!({"name": "renamed"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "tg-1", "name": "renamed", "hosts": ["10.0.0.0/24"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .update_target(Parameters(UpdateTargetParams {
            id: "tg-1".into(),
            name: Some("renamed".into()),
            comment: None,
            hosts: None,
            exclude_hosts: None,
            alive_test: None,
            port_list_id: None,
            reverse_lookup_only: None,
            reverse_lookup_unify: None,
            ssh_credential_id: None,
            smb_credential_id: None,
            esxi_credential_id: None,
            snmp_credential_id: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["name"], "renamed");
}

#[tokio::test]
async fn delete_target_supports_ultimate() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/targets/tg-1"))
        .and(query_param("ultimate", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .delete_target(Parameters(DeleteParams {
            id: "tg-1".into(),
            ultimate: Some(true),
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["deleted"], true);
}

#[tokio::test]
async fn update_task_puts_camel_case_bindings() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/tasks/t-1"))
        .and(body_json(serde_json::json!({
            "scanConfigId": "sc-2",
            "scheduleId": "sch-1"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t-1", "name": "weekly", "status": "New"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .update_task(Parameters(UpdateTaskParams {
            id: "t-1".into(),
            name: None,
            comment: None,
            target_id: None,
            scan_config_id: Some("sc-2".into()),
            scanner_id: None,
            schedule_id: Some("sch-1".into()),
            alert_ids: None,
            alterable: None,
            observers: None,
            schedule_periods: None,
            preferences: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["id"], "t-1");
}

#[tokio::test]
async fn create_credential_maps_type_key() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/credentials"))
        .and(body_json(serde_json::json!({
            "name": "root-ssh",
            "type": "up",
            "login": "root",
            "password": "hunter2"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": "c-new"})))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .create_credential(Parameters(CreateCredentialParams {
            name: "root-ssh".into(),
            credential_type: "up".into(),
            comment: None,
            login: Some("root".into()),
            password: Some("hunter2".into()),
            private_key: None,
            certificate: None,
            community: None,
            auth_algorithm: None,
            privacy_algorithm: None,
            privacy_password: None,
            credential_store_id: None,
            vault_id: None,
            host_identifier: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["id"], "c-new");
}

#[tokio::test]
async fn create_alert_sends_data_maps() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/alerts"))
        .and(body_json(serde_json::json!({
            "name": "mail on done",
            "event": "Task run status changed",
            "condition": "Always",
            "method": "Email",
            "eventData": {"status": "Done"},
            "methodData": {"to_address": "sec@example.com"}
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": "a-new"})))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .create_alert(Parameters(CreateAlertParams {
            name: "mail on done".into(),
            event: "Task run status changed".into(),
            condition: "Always".into(),
            method: "Email".into(),
            comment: None,
            event_data: Some([("status".to_string(), "Done".to_string())].into()),
            condition_data: None,
            method_data: Some([("to_address".to_string(), "sec@example.com".to_string())].into()),
            filter_id: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["id"], "a-new");
}

#[tokio::test]
async fn create_note_and_override_send_nvt_oid() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/notes"))
        .and(body_json(serde_json::json!({
            "nvtOid": "1.3.6.1.4.1.25623.1.0.100315",
            "text": "accepted risk",
            "hosts": ["10.0.0.5"]
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": "n-new"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/overrides"))
        .and(body_json(serde_json::json!({
            "nvtOid": "1.3.6.1.4.1.25623.1.0.100315",
            "text": "false positive",
            "newSeverity": "0.0"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": "o-new"})))
        .expect(1)
        .mount(&server)
        .await;

    let note = mcp
        .create_note(Parameters(CreateNoteParams {
            nvt_oid: "1.3.6.1.4.1.25623.1.0.100315".into(),
            text: Some("accepted risk".into()),
            hosts: Some(vec!["10.0.0.5".into()]),
            port: None,
            severity: None,
            task_id: None,
            result_id: None,
            active: None,
            orphan: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&note)["id"], "n-new");

    let overridden = mcp
        .create_override(Parameters(CreateOverrideParams {
            nvt_oid: "1.3.6.1.4.1.25623.1.0.100315".into(),
            text: Some("false positive".into()),
            hosts: None,
            port: None,
            severity: None,
            new_severity: Some("0.0".into()),
            task_id: None,
            result_id: None,
            active: None,
            orphan: None,
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&overridden)["id"], "o-new");
}

#[tokio::test]
async fn delete_report_hits_reports_endpoint_without_ultimate() {
    let (server, mcp) = server_with_login().await;
    // The gateway rejects `ultimate` on report deletion with 400, so the
    // tool must never send a query string here.
    Mock::given(method("DELETE"))
        .and(path("/api/v1/reports/rep-1"))
        .and(wiremock::matchers::query_param_is_missing("ultimate"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .delete_report(Parameters(GetByIdParams { id: "rep-1".into() }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["deleted"], true);
}

#[tokio::test]
async fn write_conflict_is_a_legible_tool_error() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/targets/tg-1"))
        .respond_with(problem_response(409, "conflict", "Target In Use"))
        .mount(&server)
        .await;

    let result = mcp
        .delete_target(Parameters(DeleteParams {
            id: "tg-1".into(),
            ultimate: None,
        }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}
