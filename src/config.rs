//! CLI definition and runtime configuration.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Parser, ValueEnum};
use secrecy::SecretString;
use url::Url;

use crate::mcp::toolset::ToolsetSelection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Transport {
    Stdio,
    StreamableHttp,
}

#[derive(Debug, Parser)]
#[command(
    name = "gvm-mcp",
    version,
    about = "MCP server for Greenbone Vulnerability Management via the GVM REST gateway"
)]
pub struct Cli {
    /// Base URL of the rust-gvm-api REST gateway (origin, without /api/v1)
    #[arg(long, env = "GVM_GATEWAY_URL", default_value = "http://127.0.0.1:8080")]
    pub gateway_url: Url,

    /// gvmd username the server forwards to the gateway when the caller sends
    /// no credentials (stdio, or an HTTP caller without an Authorization
    /// header). Optional: HTTP callers may instead authenticate as themselves.
    #[arg(long, env = "GVM_USERNAME")]
    pub username: Option<String>,

    /// gvmd password (prefer --password-file / GVM_PASSWORD_FILE in production)
    #[arg(long, env = "GVM_PASSWORD", hide_env_values = true)]
    pub password: Option<String>,

    /// File containing the gvmd password (e.g. a mounted secret); takes
    /// precedence over --password / GVM_PASSWORD
    #[arg(long, env = "GVM_PASSWORD_FILE")]
    pub password_file: Option<PathBuf>,

    /// MCP transport
    #[arg(long, value_enum, env = "MCP_TRANSPORT", default_value_t = Transport::Stdio)]
    pub transport: Transport,

    /// Bind address for the streamable-http transport
    #[arg(long, env = "MCP_BIND_ADDR", default_value = "127.0.0.1:8000")]
    pub bind_addr: std::net::SocketAddr,

    /// Host headers accepted by the streamable-http transport (DNS-rebinding
    /// guard). Use "*" to disable the check behind a trusted reverse proxy.
    #[arg(
        long,
        env = "MCP_ALLOWED_HOSTS",
        value_delimiter = ',',
        default_value = "localhost,127.0.0.1,::1"
    )]
    pub allowed_hosts: Vec<String>,

    /// Streamable-http hardening: reject tool calls from HTTP callers that
    /// send no Authorization header instead of serving them with the
    /// configured fallback identity. Has no effect on stdio.
    #[arg(long, env = "MCP_REQUIRE_CALLER_AUTH", default_value_t = false)]
    pub require_caller_auth: bool,

    /// Comma-separated toolsets to expose ("default", "all", or names from
    /// --list-toolsets). Identity is always opt-in.
    #[arg(long, env = "GVM_TOOLSETS", value_delimiter = ',')]
    pub toolsets: Vec<String>,

    /// Expose only read (non-mutating) tools
    #[arg(long, env = "GVM_READ_ONLY", default_value_t = false)]
    pub read_only: bool,

    /// HTTP timeout towards the gateway, in seconds
    #[arg(long, env = "GVM_HTTP_TIMEOUT", default_value_t = 30)]
    pub timeout_secs: u64,

    /// Log level when RUST_LOG is unset (error|warn|info|debug|trace)
    #[arg(long, env = "GVM_LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// List available toolsets and exit
    #[arg(long, default_value_t = false)]
    pub list_toolsets: bool,
}

/// Validated runtime configuration derived from [`Cli`].
#[derive(Debug, Clone)]
pub struct Config {
    pub gateway_url: Url,
    pub username: Option<String>,
    pub password: Option<SecretString>,
    pub transport: Transport,
    pub bind_addr: std::net::SocketAddr,
    pub allowed_hosts: Vec<String>,
    pub require_caller_auth: bool,
    pub toolsets: ToolsetSelection,
    pub read_only: bool,
    pub timeout: Duration,
}

impl Config {
    pub fn from_cli(cli: Cli) -> anyhow::Result<Self> {
        if !matches!(cli.gateway_url.scheme(), "http" | "https") {
            bail!(
                "gateway URL must use http or https, got '{}'",
                cli.gateway_url
            );
        }
        if cli.gateway_url.host_str().is_none() {
            bail!("gateway URL has no host: '{}'", cli.gateway_url);
        }
        // Credentials embedded in the URL would end up in logs and error
        // messages; the dedicated flags keep them in SecretString.
        if !cli.gateway_url.username().is_empty() || cli.gateway_url.password().is_some() {
            bail!(
                "gateway URL must not embed credentials; use --username/--password \
                 (GVM_USERNAME/GVM_PASSWORD) instead"
            );
        }

        // Credentials are optional: gvm-mcp forwards the caller's identity to
        // the gateway and only falls back to these when the caller sends none.
        // When absent and no caller header is present, gateway calls go out
        // unauthenticated and are rejected with 401.
        let username = cli.username.filter(|u| !u.is_empty());

        let password = match &cli.password_file {
            Some(path) => {
                let raw = std::fs::read_to_string(path).with_context(|| {
                    format!("failed to read password file '{}'", path.display())
                })?;
                let trimmed = raw.trim_end_matches(['\r', '\n']);
                if trimmed.is_empty() {
                    bail!("password file '{}' is empty", path.display());
                }
                Some(trimmed.to_owned())
            }
            None => cli.password.filter(|p| !p.is_empty()),
        };

        if cli.timeout_secs == 0 {
            bail!("--timeout-secs must be greater than 0");
        }

        // A username without a password (or vice versa) cannot form a fallback
        // credential and is almost always a misconfiguration.
        if username.is_some() != password.is_some() {
            bail!(
                "gvmd username and password must be set together (or both omitted \
                 to rely on caller-forwarded credentials)"
            );
        }

        let toolsets = ToolsetSelection::parse(&cli.toolsets)?;

        Ok(Self {
            gateway_url: cli.gateway_url,
            username,
            password: password.map(SecretString::from),
            transport: cli.transport,
            bind_addr: cli.bind_addr,
            allowed_hosts: cli.allowed_hosts,
            require_caller_auth: cli.require_caller_auth,
            toolsets,
            read_only: cli.read_only,
            timeout: Duration::from_secs(cli.timeout_secs),
        })
    }

    /// Whether a fallback gvmd identity is configured.
    pub fn has_fallback_credentials(&self) -> bool {
        self.username.is_some() && self.password.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cli() -> Cli {
        Cli::parse_from([
            "gvm-mcp",
            "--gateway-url",
            "http://gateway.example:8080",
            "--username",
            "admin",
            "--password",
            "s3cret",
        ])
    }

    #[test]
    fn builds_config_from_flags() {
        let config = Config::from_cli(base_cli()).unwrap();
        assert_eq!(config.username.as_deref(), Some("admin"));
        assert_eq!(config.gateway_url.as_str(), "http://gateway.example:8080/");
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert!(!config.read_only);
        assert!(!config.require_caller_auth);
        assert!(config.has_fallback_credentials());
    }

    #[test]
    fn credentials_are_optional_when_both_omitted() {
        let mut cli = base_cli();
        cli.username = None;
        cli.password = None;
        let config = Config::from_cli(cli).unwrap();
        assert!(config.username.is_none());
        assert!(config.password.is_none());
        assert!(!config.has_fallback_credentials());
    }

    #[test]
    fn rejects_username_without_password() {
        let mut cli = base_cli();
        cli.password = None;
        let err = Config::from_cli(cli).unwrap_err();
        assert!(err.to_string().contains("together"));
    }

    #[test]
    fn rejects_password_without_username() {
        let mut cli = base_cli();
        cli.username = None;
        let err = Config::from_cli(cli).unwrap_err();
        assert!(err.to_string().contains("together"));
    }

    #[test]
    fn rejects_non_http_scheme() {
        let mut cli = base_cli();
        cli.gateway_url = Url::parse("unix:/run/gvmd.sock").unwrap();
        let err = Config::from_cli(cli).unwrap_err();
        assert!(err.to_string().contains("http"));
    }

    #[test]
    fn rejects_credentials_embedded_in_gateway_url() {
        let mut cli = base_cli();
        cli.gateway_url = Url::parse("http://admin:secret@gateway.example:8080").unwrap();
        let err = Config::from_cli(cli).unwrap_err();
        assert!(err.to_string().contains("must not embed credentials"));
        // The rejection message must not echo the secret.
        assert!(!err.to_string().contains("secret@"));
    }

    #[test]
    fn require_caller_auth_flag_is_parsed() {
        let cli = Cli::parse_from([
            "gvm-mcp",
            "--transport",
            "streamable-http",
            "--require-caller-auth",
        ]);
        let config = Config::from_cli(cli).unwrap();
        assert!(config.require_caller_auth);
        assert_eq!(config.transport, Transport::StreamableHttp);
    }

    #[test]
    fn password_file_takes_precedence() {
        let dir = std::env::temp_dir().join("gvm-mcp-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("password");
        std::fs::write(&path, "from-file\n").unwrap();

        let mut cli = base_cli();
        cli.password_file = Some(path.clone());
        let config = Config::from_cli(cli).unwrap();
        use secrecy::ExposeSecret;
        assert_eq!(
            config.password.as_ref().map(|p| p.expose_secret()),
            Some("from-file")
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn empty_password_file_is_rejected() {
        let dir = std::env::temp_dir().join("gvm-mcp-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty-password");
        std::fs::write(&path, "\n").unwrap();

        let mut cli = base_cli();
        cli.password_file = Some(path.clone());
        let err = Config::from_cli(cli).unwrap_err();
        assert!(err.to_string().contains("empty"));
        std::fs::remove_file(path).ok();
    }
}
