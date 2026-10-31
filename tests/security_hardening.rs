//! Security-hardening tests: the gateway client never follows redirects
//! (so credentials cannot be bounced to a third host), artifact downloads
//! are bounded before they are buffered, and `--require-caller-auth`
//! refuses HTTP callers that send no identity instead of serving them with
//! the fallback account.

mod support;

use gvm_mcp::gateway::{GatewayClient, GatewayError};
use gvm_mcp::mcp::GvmMcpServer;
use gvm_mcp::mcp::tools::common::GetByIdParams;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ContentBlock;
use support::{config_for, config_with_args};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
async fn redirects_are_not_followed() {
    let server = MockServer::start().await;
    // A redirect to a different origin must surface as an error, never as a
    // second request carrying the Authorization header.
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("location", "http://evil.example/steal"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let err = client
        .get_json::<serde_json::Value>(&["targets"])
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(302), "got: {err:?}");
}

#[tokio::test]
async fn oversized_artifact_is_refused_before_buffering() {
    let server = MockServer::start().await;
    let big = vec![b'x'; 64 * 1024];
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-1/result"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(big)
                .insert_header("content-type", "application/pdf"),
        )
        .mount(&server)
        .await;

    let client = GatewayClient::new(&config_for(&server)).unwrap();
    let err = client
        .get_bytes(&["jobs", "job-1", "result"], &[], 16 * 1024)
        .await
        .unwrap_err();
    assert!(
        matches!(err, GatewayError::ArtifactTooLarge { limit: 16384, .. }),
        "got: {err:?}"
    );

    // Through the tool the refusal is a legible error, including for JSON
    // exports (which were previously returned inline without any bound).
    let mcp = GvmMcpServer::new(config_for(&server)).unwrap();
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/job-2/result"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![b'{'; 4 * 1024 * 1024])
                .insert_header("content-type", "application/json"),
        )
        .mount(&server)
        .await;
    let result = mcp
        .download_job_result(Parameters(GetByIdParams { id: "job-2".into() }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    assert!(
        text_of(&result).contains("inline limit"),
        "{}",
        text_of(&result)
    );
}

/// Drive the streamable-HTTP server with raw JSON-RPC and return the
/// `tools/call` result for `openvas_list_targets`, with or without an
/// inbound Authorization header.
async fn call_list_targets_over_http(
    extra_args: &[&str],
    authorization: Option<&str>,
) -> serde_json::Value {
    let gateway = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/targets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [], "pagination": {"page": 1, "perPage": 25, "total": 0, "totalPages": 0}
        })))
        .mount(&gateway)
        .await;
    let mut args = vec!["--transport", "streamable-http"];
    args.extend_from_slice(extra_args);
    let mcp = GvmMcpServer::new(config_with_args(&gateway, &args)).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        gvm_mcp::mcp::http::serve(
            mcp,
            listener,
            &["127.0.0.1".to_string()],
            std::future::pending(),
        )
        .await
        .unwrap();
    });

    let client = reqwest::Client::new();
    let url = format!("http://{addr}/mcp");
    let with_auth = |req: reqwest::RequestBuilder| match authorization {
        Some(auth) => req.header("Authorization", auth),
        None => req,
    };
    let init = with_auth(
        client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .json(&serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                           "clientInfo": {"name": "t", "version": "0"}}
            })),
    )
    .send()
    .await
    .unwrap();
    let session = init
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let with_session = |req: reqwest::RequestBuilder| match &session {
        Some(s) => req.header("mcp-session-id", s.clone()),
        None => req,
    };
    with_session(with_auth(
        client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .json(&serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
    ))
    .send()
    .await
    .unwrap();
    let call = with_session(with_auth(
        client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .json(&serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "openvas_list_targets", "arguments": {}}
            })),
    ))
    .send()
    .await
    .unwrap();
    let text = call.text().await.unwrap();
    let json_text = text
        .lines()
        .rev()
        .find_map(|line| {
            line.strip_prefix("data: ")
                .filter(|data| !data.trim().is_empty())
        })
        .unwrap_or(&text);
    serde_json::from_str(json_text).unwrap()
}

#[tokio::test]
async fn require_caller_auth_refuses_anonymous_http_callers() {
    let body = call_list_targets_over_http(&["--require-caller-auth"], None).await;
    let result = &body["result"];
    assert_eq!(result["isError"], true, "got: {body}");
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Authentication required"), "got: {text}");
}

#[tokio::test]
async fn require_caller_auth_still_serves_authenticated_http_callers() {
    let body =
        call_list_targets_over_http(&["--require-caller-auth"], Some("Basic YWRtaW46czNjcmV0"))
            .await;
    let result = &body["result"];
    assert_ne!(result["isError"], true, "got: {body}");
}

#[tokio::test]
async fn without_the_flag_anonymous_http_callers_use_the_fallback_identity() {
    // Documented default: the fallback identity serves callers that send no
    // Authorization. The startup warning (main.rs) covers the non-loopback
    // case; this test pins the behavior the warning describes.
    let body = call_list_targets_over_http(&[], None).await;
    assert_ne!(body["result"]["isError"], true, "got: {body}");
}
