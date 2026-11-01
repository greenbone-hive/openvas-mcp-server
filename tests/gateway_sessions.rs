//! Mock-gateway tests for the pooled-session default: one gateway session per
//! identity, bearer reuse, re-authentication after an expired session,
//! fallback to request-scoped `Basic`, identity isolation, the per-identity
//! in-flight gate and the session-limit retry.

mod support;

use std::time::{Duration, Instant};

use gvm_mcp::gateway::auth::CALLER_AUTH;
use gvm_mcp::gateway::{GatewayClient, GatewayError};
use support::{EXPECTED_BASIC, config_for, config_with_args, problem_response};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn empty_page() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "data": [],
        "pagination": {"page": 1, "perPage": 25, "total": 0, "totalPages": 0}
    }))
}

fn session_created(token: &str) -> ResponseTemplate {
    ResponseTemplate::new(201).set_body_json(serde_json::json!({
        "sessionToken": token, "expiresIn": 300, "gmpVersion": "22.8"
    }))
}

async fn mount_session(server: &MockServer, basic: &str, token: &str) {
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .and(header("authorization", basic))
        .respond_with(session_created(token))
        .mount(server)
        .await;
}

#[tokio::test]
async fn one_login_per_identity_then_bearer_reuse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .and(header("authorization", EXPECTED_BASIC))
        .respond_with(session_created("pooled-1"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer pooled-1"))
        .respond_with(empty_page())
        .expect(3)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    for _ in 0..3 {
        let value: serde_json::Value = client.get_json(&["targets"]).await.unwrap();
        assert_eq!(value["pagination"]["total"], 0);
    }
    assert_eq!(client.pooled_identities(), 1);
}

#[tokio::test]
async fn burst_of_first_calls_performs_a_single_login() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_created("pooled-burst").set_delay(Duration::from_millis(100)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer pooled-burst"))
        .respond_with(empty_page())
        .expect(6)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let calls = (0..6).map(|_| {
        let client = client.clone();
        tokio::spawn(async move { client.get_json::<serde_json::Value>(&["targets"]).await })
    });
    for call in calls {
        call.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn rejected_session_is_recreated_once_and_the_call_retried() {
    let server = MockServer::start().await;
    // First login yields "old", the second "new": the mock for "old" is
    // consumed once, so the next creation falls through to "new".
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_created("old"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_created("new"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer old"))
        .respond_with(problem_response(401, "session_expired", "Unauthorized"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer new"))
        .respond_with(empty_page())
        .expect(1)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let value: serde_json::Value = client.get_json(&["targets"]).await.unwrap();
    assert_eq!(value["pagination"]["total"], 0);
}

fn backend_unavailable() -> ResponseTemplate {
    problem_response(502, "backend_unavailable", "Bad Gateway")
}

#[tokio::test]
async fn a_session_whose_backend_died_is_dropped_and_reads_are_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_created("poisoned"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_created("fresh"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/session"))
        .and(header("authorization", "Bearer poisoned"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer poisoned"))
        .respond_with(backend_unavailable())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer fresh"))
        .respond_with(empty_page())
        .expect(1)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap();
    // Give the background close a moment before wiremock verifies expectations.
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn a_failed_write_on_a_dead_session_is_not_repeated_but_the_session_is_dropped() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_created("poisoned"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_created("fresh"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer poisoned"))
        .respond_with(backend_unavailable())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer fresh"))
        .respond_with(empty_page())
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer fresh"))
        .respond_with(empty_page())
        .expect(1)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let err = client
        .post_json::<serde_json::Value>(&["targets"], &serde_json::json!({"name": "x"}))
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(502));
    // The next call logs in again instead of reusing the dead session.
    client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap();
}

#[tokio::test]
async fn a_second_401_is_a_credential_error_not_a_loop() {
    let server = MockServer::start().await;
    mount_session(&server, EXPECTED_BASIC, "tok").await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(problem_response(401, "unauthorized", "Unauthorized"))
        .expect(2)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap_err();
    assert!(err.is_unauthorized());
}

#[tokio::test]
async fn bad_credentials_fail_at_login_without_a_basic_fallback() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(problem_response(401, "unauthorized", "Unauthorized"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(empty_page())
        .expect(0)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap_err();
    assert!(err.is_unauthorized());
}

fn session_cap() -> ResponseTemplate {
    ResponseTemplate::new(429).set_body_raw(
        serde_json::json!({
            "type": "https://gvm-gateway.greenbone.net/errors/too_many_requests",
            "code": "too_many_requests", "title": "Too Many Requests", "status": 429,
            "detail": "per-user session limit exceeded"
        })
        .to_string(),
        "application/problem+json",
    )
}

#[tokio::test]
async fn transient_session_cap_at_login_is_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_cap())
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_created("after-cap"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer after-cap"))
        .respond_with(empty_page())
        .expect(1)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap();
}

#[tokio::test]
async fn persistent_session_cap_is_reported_without_a_basic_fallback() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(session_cap())
        .expect(3)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(empty_page())
        .expect(0)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap_err();
    assert!(err.is_session_limit(), "{err}");
}

#[tokio::test]
async fn idle_sessions_are_closed_and_reopened_on_demand() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .and(header("authorization", EXPECTED_BASIC))
        .respond_with(session_created("idle-1"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .and(header("authorization", EXPECTED_BASIC))
        .respond_with(session_created("idle-2"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/session"))
        .and(header("authorization", "Bearer idle-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(empty_page())
        .expect(2)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server))
        .unwrap()
        .with_idle_release(Duration::from_millis(100));
    client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap();
}

#[tokio::test]
async fn gateway_without_session_endpoint_degrades_to_basic() {
    let server = MockServer::start().await;
    // Nothing mounted for /session: wiremock answers 404.
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", EXPECTED_BASIC))
        .respond_with(empty_page())
        .expect(2)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    for _ in 0..2 {
        client
            .get_json::<serde_json::Value>(&["targets"])
            .await
            .unwrap();
    }
    assert_eq!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/api/v1/session")
            .count(),
        1
    );
}

#[tokio::test]
async fn identities_never_share_a_session() {
    let server = MockServer::start().await;
    mount_session(&server, EXPECTED_BASIC, "fallback-session").await;
    let other = "Basic b3RoZXI6b3RoZXI=";
    mount_session(&server, other, "other-session").await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer fallback-session"))
        .respond_with(empty_page())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer other-session"))
        .respond_with(empty_page())
        .expect(1)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap();
    CALLER_AUTH
        .scope(Some(other.to_string()), async {
            client
                .get_json::<serde_json::Value>(&["targets"])
                .await
                .unwrap();
        })
        .await;
    assert_eq!(client.pooled_identities(), 2);
}

#[tokio::test]
async fn forwarded_bearer_tokens_pass_through_without_a_login() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/session"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .and(header("authorization", "Bearer caller-token"))
        .respond_with(empty_page())
        .expect(1)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    CALLER_AUTH
        .scope(Some("Bearer caller-token".to_string()), async {
            client
                .get_json::<serde_json::Value>(&["targets"])
                .await
                .unwrap();
        })
        .await;
}

#[tokio::test]
async fn in_flight_gate_queues_parallel_calls_per_identity() {
    let server = MockServer::start().await;
    mount_session(&server, EXPECTED_BASIC, "gated").await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(empty_page().set_delay(Duration::from_millis(150)))
        .expect(6)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_with_args(
        &server,
        &["--gateway-max-in-flight", "2"],
    ))
    .unwrap();
    let started = Instant::now();
    let calls: Vec<_> = (0..6)
        .map(|_| {
            let client = client.clone();
            tokio::spawn(async move { client.get_json::<serde_json::Value>(&["targets"]).await })
        })
        .collect();
    for call in calls {
        call.await.unwrap().unwrap();
    }
    // Six 150 ms calls through a gate of two take at least three rounds.
    assert!(
        started.elapsed() >= Duration::from_millis(400),
        "calls were not gated: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn session_cap_429_is_retried_but_the_request_budget_is_not() {
    let server = MockServer::start().await;
    mount_session(&server, EXPECTED_BASIC, "retry").await;
    let cap = ResponseTemplate::new(429).set_body_raw(
        serde_json::json!({
            "type": "https://gvm-gateway.greenbone.net/errors/too_many_requests",
            "code": "too_many_requests", "title": "Too Many Requests", "status": 429,
            "detail": "per-user session limit exceeded"
        })
        .to_string(),
        "application/problem+json",
    );
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(cap)
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(empty_page())
        .expect(1)
        .mount(&server)
        .await;
    let budget = ResponseTemplate::new(429).set_body_raw(
        serde_json::json!({
            "type": "https://gvm-gateway.greenbone.net/errors/too_many_requests",
            "code": "too_many_requests", "title": "Too Many Requests", "status": 429,
            "detail": "rate limit exceeded; retry after 56 seconds"
        })
        .to_string(),
        "application/problem+json",
    );
    Mock::given(method("GET"))
        .and(path("/api/v1/tasks"))
        .respond_with(budget)
        .expect(1)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap();
    let err = client
        .get_json::<serde_json::Value>(&["tasks"])
        .await
        .unwrap_err();
    match err {
        GatewayError::Api(problem) => {
            assert_eq!(problem.status, 429);
            assert!(problem.detail.unwrap().contains("retry after"));
        }
        other => panic!("expected the budget 429 to surface, got {other:?}"),
    }
}

#[tokio::test]
async fn shutdown_closes_pooled_sessions() {
    let server = MockServer::start().await;
    mount_session(&server, EXPECTED_BASIC, "closing").await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(empty_page())
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/session"))
        .and(header("authorization", "Bearer closing"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap();
    client.close_pooled_sessions().await;
    client.close_pooled_sessions().await; // idempotent
}
