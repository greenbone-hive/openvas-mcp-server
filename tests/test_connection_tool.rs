//! Mock-gateway tests for the `openvas_test_connection` tool: health →
//! version → one authenticated call verifying the forwarded identity.

mod support;

use gvm_mcp::mcp::GvmMcpServer;
use rmcp::model::ContentBlock;
use support::{EXPECTED_BASIC, config_for, problem_response};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mount_healthy_gateway(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok"
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/ready"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ready"
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "apiVersion": "0.1.0",
            "gmpVersion": "22.7"
        })))
        .mount(server)
        .await;
    // The auth probe: the request must carry the fallback Basic credential.
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", EXPECTED_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [],
            "pagination": {"page": 1, "perPage": 1, "total": 0, "totalPages": 0}
        })))
        .mount(server)
        .await;
}

fn text_of(result: &rmcp::model::CallToolResult) -> String {
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

#[tokio::test]
async fn reports_full_stack_health_on_success() {
    let server = MockServer::start().await;
    mount_healthy_gateway(&server).await;

    let mcp = GvmMcpServer::new(config_for(&server)).unwrap();
    let result = mcp.test_connection().await.unwrap();

    assert_ne!(result.is_error, Some(true));
    let text = text_of(&result);
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["gatewayStatus"], "ok");
    assert_eq!(report["gatewayReadiness"], "ready");
    assert_eq!(report["gmpVersion"], "22.7");
    assert_eq!(report["authenticated"], true);
}

#[tokio::test]
async fn unreachable_gateway_is_a_legible_tool_error() {
    let config = support::config_for_dead_port();
    let mcp = GvmMcpServer::new(config).unwrap();
    let result = mcp.test_connection().await.unwrap();

    assert_eq!(result.is_error, Some(true));
    let text = text_of(&result);
    assert!(text.contains("gateway unreachable"), "got: {text}");
    assert!(text.contains("GET /health"), "got: {text}");
}

#[tokio::test]
async fn bad_credentials_are_a_legible_tool_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/ready"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ready"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "apiVersion": "0.1.0",
            "gmpVersion": "22.7"
        })))
        .mount(&server)
        .await;
    // The gateway rejects the identity → the auth probe 401s.
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(problem_response(401, "unauthorized", "Unauthorized"))
        .mount(&server)
        .await;

    let mcp = GvmMcpServer::new(config_for(&server)).unwrap();
    let result = mcp.test_connection().await.unwrap();

    assert_eq!(result.is_error, Some(true));
    let text = text_of(&result);
    assert!(text.contains("Authentication failed"), "got: {text}");
    assert!(text.contains("unauthorized"), "got: {text}");
}
