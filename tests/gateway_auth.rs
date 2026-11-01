//! Mock-gateway tests for identity forwarding in per-request mode: the client
//! sends the configured fallback `Basic` credential on every call, surfaces
//! the gateway's typed errors, and distinguishes transport failures. The
//! pooled-session default is covered by `tests/gateway_sessions.rs`.

mod support;

use gvm_mcp::config::Config;
use gvm_mcp::gateway::{GatewayClient, GatewayError};
use support::{EXPECTED_BASIC, config_with_args, problem_response};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn config_for(server: &MockServer) -> Config {
    config_with_args(server, &["--gateway-auth-mode", "per-request"])
}

#[tokio::test]
async fn per_request_mode_sends_fallback_basic_and_never_creates_a_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", EXPECTED_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [],
            "pagination": {"page": 1, "perPage": 25, "total": 0, "totalPages": 0}
        })))
        .expect(2)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    for _ in 0..2 {
        let value: serde_json::Value = client.get_json(&["targets"]).await.unwrap();
        assert_eq!(value["pagination"]["total"], 0);
    }
    assert_eq!(
        client.pooled_identities(),
        1,
        "the in-flight gate still tracks the identity"
    );
}

#[tokio::test]
async fn problem_json_maps_to_typed_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets/missing"))
        .respond_with(problem_response(404, "not_found", "Resource Not Found"))
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&["targets", "missing"])
        .await
        .unwrap_err();
    match err {
        GatewayError::Api(problem) => {
            assert_eq!(problem.code, "not_found");
            assert_eq!(problem.status, 404);
        }
        other => panic!("expected Api error, got: {other:?}"),
    }
}

#[tokio::test]
async fn rejected_identity_surfaces_as_401() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(problem_response(401, "unauthorized", "Unauthorized"))
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(401));
    assert!(err.is_unauthorized());
}

#[tokio::test]
async fn non_problem_error_maps_to_unexpected_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(ResponseTemplate::new(500).set_body_string("plain text crash"))
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap_err();
    match err {
        GatewayError::UnexpectedStatus { status, body } => {
            assert_eq!(status, 500);
            assert!(body.contains("plain text crash"));
        }
        other => panic!("expected UnexpectedStatus, got: {other:?}"),
    }
}

#[tokio::test]
async fn unreachable_gateway_maps_to_transport_error() {
    let config = support::config_for_dead_port();
    let client = GatewayClient::new(&config).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap_err();
    assert!(matches!(err, GatewayError::Transport(_)), "got: {err:?}");
}
