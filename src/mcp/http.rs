//! Streamable-HTTP transport: serves the MCP server at `/mcp`.
//!
//! Simple request/response tool calls answer as `application/json`, falling
//! back to SSE only when a handler streams. Each HTTP request's
//! `Authorization` header is forwarded to the gateway by the server's
//! `call_tool` (see `gateway::auth`); callers without one are served with the
//! configured fallback identity unless `--require-caller-auth` is set.

use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig;

use super::server::GvmMcpServer;

/// Serve `server` on `listener` until `shutdown` resolves.
///
/// `allowed_hosts` guards against DNS-rebinding: requests whose `Host`
/// header is not listed are rejected. The single entry `"*"` disables the
/// check (only sensible behind a reverse proxy that validates Host itself).
pub async fn serve(
    server: GvmMcpServer,
    listener: tokio::net::TcpListener,
    allowed_hosts: &[String],
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let mut config = StreamableHttpServerConfig::default().with_json_response(true);
    if allowed_hosts.iter().any(|host| host == "*") {
        config = config.disable_allowed_hosts();
    } else if !allowed_hosts.is_empty() {
        config = config.with_allowed_hosts(allowed_hosts.iter().cloned());
    }

    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        LocalSessionManager::default().into(),
        config,
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}
