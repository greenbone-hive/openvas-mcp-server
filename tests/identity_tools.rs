//! Mock-gateway tests for the opt-in identity toolset.

mod support;

use gvm_mcp::mcp::GvmMcpServer;
use gvm_mcp::mcp::tools::common::{FilterParams, GetByIdParams, ListParams};
use gvm_mcp::mcp::tools::identity::{CreateUserParams, UpdateUserSettingParams};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use support::config_with_args;
use wiremock::matchers::{body_json, method, path};
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

async fn identity_server() -> (MockServer, GvmMcpServer) {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_with_args(
        &server,
        &["--toolsets", "default,identity"],
    ))
    .unwrap();
    (server, mcp)
}

#[tokio::test]
async fn list_users_summarizes_roles_and_groups() {
    let (server, mcp) = identity_server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "id": "u-1", "name": "admin", "writable": true, "inUse": false,
                "roles": [{"id": "r-1", "name": "Admin"}],
                "groups": [],
                "hostsAllow": false,
                "creationTime": "2026-01-01T00:00:00Z"
            }],
            "pagination": {"page": 1, "perPage": 25, "total": 1, "totalPages": 1}
        })))
        .mount(&server)
        .await;

    let result = mcp
        .list_users(Parameters(ListParams::default()))
        .await
        .unwrap();
    let json = json_of(&result);
    assert_eq!(json["users"][0]["name"], "admin");
    assert_eq!(json["users"][0]["roles"][0]["name"], "Admin");
    assert!(json["users"][0].get("creationTime").is_none());
}

#[tokio::test]
async fn create_user_sends_roles_and_auth_type() {
    let (server, mcp) = identity_server().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/users"))
        .and(body_json(serde_json::json!({
            "name": "auditor",
            "password": "s3cret!",
            "roles": ["r-observer"],
            "authenticationType": "file"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"id": "u-new"})))
        .expect(1)
        .mount(&server)
        .await;

    let result = mcp
        .create_user(Parameters(CreateUserParams {
            name: "auditor".into(),
            comment: None,
            password: Some("s3cret!".into()),
            hosts: None,
            roles: Some(vec!["r-observer".into()]),
            authentication_type: Some("file".into()),
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&result)["id"], "u-new");
}

#[tokio::test]
async fn user_settings_list_and_update() {
    let (server, mcp) = identity_server().await;
    // UserSettingList has no pagination per the spec.
    Mock::given(method("GET"))
        .and(path("/api/v1/user-settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": "s-1", "name": "Rows Per Page", "value": "50"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/user-settings/s-1"))
        .and(body_json(serde_json::json!({"value": "100"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "s-1", "name": "Rows Per Page", "value": "100"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let list = mcp
        .list_user_settings(Parameters(FilterParams::default()))
        .await
        .unwrap();
    let json = json_of(&list);
    assert_eq!(json["settings"][0]["value"], "50");
    assert!(json.get("pagination").is_none());

    let updated = mcp
        .update_user_setting(Parameters(UpdateUserSettingParams {
            id: "s-1".into(),
            value: "100".into(),
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&updated)["value"], "100");
}

#[tokio::test]
async fn get_permission_passthrough() {
    let (server, mcp) = identity_server().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/permissions/p-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "p-1", "name": "get_tasks", "writable": true, "inUse": false,
            "subjectType": "role",
            "subject": {"id": "r-1", "name": "Observers"},
            "resourceType": "task",
            "resource": {"id": "t-1", "name": "weekly"}
        })))
        .mount(&server)
        .await;

    let result = mcp
        .get_permission(Parameters(GetByIdParams { id: "p-1".into() }))
        .await
        .unwrap();
    let json = json_of(&result);
    assert_eq!(json["subject"]["name"], "Observers");
    assert_eq!(json["resource"]["id"], "t-1");
}
