use anyhow::Context;
use clap::Parser;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

use gvm_mcp::config::{Cli, Config, Transport};
use gvm_mcp::mcp::GvmMcpServer;
use gvm_mcp::mcp::toolset::Toolset;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.list_toolsets {
        for toolset in Toolset::ALL {
            println!("{:<18} {}", toolset.name(), toolset.describe());
        }
        return Ok(());
    }

    // The stdio transport owns stdout for the MCP protocol; logs go to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(cli.log_level.clone())),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let config = Config::from_cli(cli)?;
    tracing::info!(
        gateway = %config.gateway_url,
        read_only = config.read_only,
        toolsets = %config.toolsets,
        fallback_identity = config.has_fallback_credentials(),
        "starting gvm-mcp"
    );

    match config.transport {
        Transport::Stdio => {
            let server = GvmMcpServer::new(config)?;
            let service = server
                .serve(stdio())
                .await
                .context("failed to start MCP server on stdio")?;
            service.waiting().await?;
        }
        Transport::StreamableHttp => {
            let bind_addr = config.bind_addr;
            let allowed_hosts = config.allowed_hosts.clone();
            if config.has_fallback_credentials()
                && !config.require_caller_auth
                && !bind_addr.ip().is_loopback()
            {
                tracing::warn!(
                    %bind_addr,
                    "fallback gvmd credentials are configured and the HTTP endpoint is not \
                     loopback-only: every unauthenticated HTTP caller will act as that gvmd \
                     user. Set --require-caller-auth (MCP_REQUIRE_CALLER_AUTH=true) or put an \
                     authenticating reverse proxy in front."
                );
            }
            let server = GvmMcpServer::new(config)?;
            let listener = tokio::net::TcpListener::bind(bind_addr)
                .await
                .with_context(|| format!("failed to bind {bind_addr}"))?;
            tracing::info!(%bind_addr, "serving MCP over streamable HTTP at /mcp");
            gvm_mcp::mcp::http::serve(server, listener, &allowed_hosts, async {
                let _ = tokio::signal::ctrl_c().await;
                tracing::info!("shutting down");
            })
            .await?;
        }
    }

    Ok(())
}
