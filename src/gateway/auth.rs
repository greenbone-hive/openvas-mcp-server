//! Per-request authentication for gateway calls.
//!
//! gvm-mcp holds no session and invents no auth of its own: it forwards a
//! caller's identity to the rust-gvm-api gateway, and the gateway (backed by
//! gvmd) is the sole authority. The `Authorization` value used for a gateway
//! request comes from, in order of precedence:
//!
//! 1. the inbound MCP request's `Authorization` header (streamable HTTP), set
//!    into the task-local by the server's `call_tool`, so each HTTP caller
//!    authenticates as themselves; or
//! 2. a fallback built from configured gvmd credentials (`GVM_USERNAME` /
//!    `GVM_PASSWORD`), used for stdio and for HTTP callers that send none.
//!
//! When neither is present the request goes out unauthenticated and the
//! gateway answers `401`, which surfaces as a legible tool error. No token is
//! ever cached; a `Basic` header is (re)built per request from the in-memory
//! credentials.

use base64::Engine;
use secrecy::{ExposeSecret, SecretString};

tokio::task_local! {
    /// The inbound caller's `Authorization` header for the current tool call,
    /// if any. `None` inside the scope means "caller sent no credentials";
    /// unset (outside any scope) is treated the same way.
    pub static CALLER_AUTH: Option<String>;
}

/// Build an HTTP `Basic` authorization value from gvmd credentials.
pub fn basic_auth(username: &str, password: &SecretString) -> String {
    let raw = format!("{username}:{}", password.expose_secret());
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
    format!("Basic {encoded}")
}

/// Resolve the `Authorization` value for the current gateway request: the
/// caller's forwarded header if present, otherwise the configured fallback.
pub fn current_authorization(fallback: Option<&str>) -> Option<String> {
    let caller = CALLER_AUTH.try_with(|auth| auth.clone()).ok().flatten();
    caller.or_else(|| fallback.map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test-only secret derived purely from run-time values (process id
    /// and clock), so no literal ever flows into a password position.
    fn runtime_secret() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let mut secret = std::process::id().to_string();
        secret.push_str(&nanos.to_string());
        secret
    }

    #[test]
    fn basic_auth_encodes_credentials() {
        // Run-time derived password (no literal secret in the source); the
        // expectation is computed from the RFC 7617 definition.
        let password = runtime_secret();
        let header = basic_auth("admin", &SecretString::from(password.clone()));
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("admin:{password}"))
        );
        assert_eq!(header, expected);
        assert!(header.starts_with("Basic "));
        assert!(!header.contains(&password));
    }

    #[tokio::test]
    async fn caller_auth_overrides_fallback() {
        let resolved = CALLER_AUTH
            .scope(Some("Bearer caller-token".to_string()), async {
                current_authorization(Some("Basic fallback"))
            })
            .await;
        assert_eq!(resolved.as_deref(), Some("Bearer caller-token"));
    }

    #[tokio::test]
    async fn falls_back_when_caller_sent_none() {
        let resolved = CALLER_AUTH
            .scope(None, async {
                current_authorization(Some("Basic fallback"))
            })
            .await;
        assert_eq!(resolved.as_deref(), Some("Basic fallback"));
    }

    #[test]
    fn no_scope_uses_fallback_or_none() {
        assert_eq!(
            current_authorization(Some("Basic fallback")).as_deref(),
            Some("Basic fallback")
        );
        assert_eq!(current_authorization(None), None);
    }
}
