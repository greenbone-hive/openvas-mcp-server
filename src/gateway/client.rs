//! Gateway HTTP client: request plumbing, identity forwarding and the
//! pooled-session / in-flight controls that keep the server inside the
//! gateway's limits.
//!
//! The identity of every authorized request is resolved per call by
//! [`super::auth`]: the inbound caller's `Authorization` header when present,
//! otherwise a `Basic` header built from the configured gvmd credentials. How
//! a `Basic` identity reaches the gateway depends on the configured
//! [`GatewayAuthMode`]:
//!
//! - `session` (default): one gateway session per identity, created on first
//!   use and reused as a bearer token (see [`super::sessions`]); the gateway
//!   logs the user into gvmd once per session instead of once per call. A
//!   rejected (expired) session is re-created once and the call retried. If
//!   the gateway cannot create a session the call falls back to request-scoped
//!   `Basic` and session creation pauses for a short while.
//! - `per-request`: the `Basic` header is sent on every call.
//!
//! Forwarded bearer tokens pass through unchanged. In both modes at most
//! `--gateway-max-in-flight` requests per identity are in flight at once,
//! which keeps parallel tool calls below the gateway's per-user session cap.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use tokio::sync::OwnedSemaphorePermit;
use url::Url;

use crate::config::{Config, GatewayAuthMode};

use super::auth::{basic_auth, current_authorization};
use super::error::{GatewayError, error_from_response};
use super::models::{HealthStatus, ReadinessStatus, SessionCreated, VersionInfo};
use super::sessions::{IdentityKey, PooledSession, SUSPEND_AFTER_FAILURE, SessionPool, Slot};

/// Path prefix of the versioned API surface. Liveness/readiness probes live
/// at the unversioned root.
const API_PREFIX: [&str; 2] = ["api", "v1"];

/// Attempts for a request the gateway answered with its per-user session
/// cap (the cap clears as soon as another request of the user completes).
const SESSION_LIMIT_ATTEMPTS: u32 = 3;

/// Base delay between those attempts (doubles each time).
const SESSION_LIMIT_BACKOFF: Duration = Duration::from_millis(50);

/// Time budget for closing pooled sessions at shutdown.
const SHUTDOWN_CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

/// Longest pause between two sweeps of the idle-session reaper.
const REAPER_MAX_INTERVAL: Duration = Duration::from_secs(15);

/// A downloaded binary artifact (report export, agent support bundle).
#[derive(Debug, Clone)]
pub struct Artifact {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    pub filename: Option<String>,
}

/// A response together with the in-flight permit of its identity; the
/// permit is released when the response (and its body) has been consumed.
struct Sent {
    response: reqwest::Response,
    _permit: Option<OwnedSemaphorePermit>,
}

#[derive(Clone)]
pub struct GatewayClient {
    http: reqwest::Client,
    base_url: Url,
    /// `Authorization` used when the caller forwards none (stdio, or an HTTP
    /// caller that sent no credentials). Built once from configured gvmd
    /// credentials; `None` when none are configured. Kept as a secret so it
    /// can never leak through `Debug` output or logs.
    fallback_auth: Option<SecretString>,
    auth_mode: GatewayAuthMode,
    /// Per-identity pooled sessions and in-flight gates.
    pool: Arc<SessionPool>,
    /// Whether the idle-session reaper task has been started (it is spawned
    /// lazily, from the first pooled session, so construction needs no
    /// runtime).
    reaper_started: Arc<AtomicBool>,
}

impl std::fmt::Debug for GatewayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayClient")
            .field("base_url", &self.base_url.as_str())
            .field(
                "fallback_auth",
                &self.fallback_auth.as_ref().map(|_| "<redacted>"),
            )
            .field("auth_mode", &self.auth_mode)
            .field("pool", &self.pool)
            .finish_non_exhaustive()
    }
}

impl GatewayClient {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let http = Self::http_client(config.timeout, &config.gateway_ca_certs)?;
        let fallback_auth = Self::fallback_auth(config);

        Ok(Self {
            http,
            base_url: config.gateway_url.clone(),
            fallback_auth,
            auth_mode: config.gateway_auth_mode,
            pool: Arc::new(SessionPool::new(config.gateway_max_in_flight)),
            reaper_started: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Close pooled sessions after `idle_release` without use instead of the
    /// default (see `sessions::IDLE_RELEASE`). Must be called before the
    /// first request.
    pub fn with_idle_release(mut self, idle_release: Duration) -> Self {
        let max_in_flight = self.pool.max_in_flight();
        self.pool = Arc::new(SessionPool::new(max_in_flight).with_idle_release(idle_release));
        self
    }

    /// Build the HTTP client. Redirects are never followed: the gateway
    /// serves every endpoint directly, and following one would hand the
    /// caller's credentials to whatever a misconfigured proxy points at.
    /// TLS is rustls with the built-in roots plus any configured private CA,
    /// TLS 1.2 or newer; connections are kept alive and reused because the
    /// gateway usually lives on another machine.
    fn http_client(
        timeout: Duration,
        extra_roots: &[reqwest::Certificate],
    ) -> anyhow::Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("gvm-mcp/", env!("CARGO_PKG_VERSION")))
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(30));
        for root in extra_roots {
            builder = builder.add_root_certificate(root.clone());
        }
        Ok(builder.build()?)
    }

    /// The configured authentication mode (diagnostics).
    pub fn auth_mode(&self) -> GatewayAuthMode {
        self.auth_mode
    }

    /// Number of identities with pooled state (diagnostics and tests).
    pub fn pooled_identities(&self) -> usize {
        self.pool.len()
    }

    /// Close every pooled session at the gateway (best effort, bounded in
    /// time). Called at shutdown so sessions do not linger until the
    /// gateway's idle timeout.
    pub async fn close_pooled_sessions(&self) {
        let tokens = self.pool.drain();
        if tokens.is_empty() {
            return;
        }
        let total = tokens.len();
        let closing = async {
            let mut closed = 0usize;
            for token in tokens {
                match self.delete_session_raw(&token).await {
                    Ok(()) => closed += 1,
                    Err(err) => {
                        tracing::debug!(error = %err, "closing a pooled gateway session failed")
                    }
                }
            }
            closed
        };
        match tokio::time::timeout(SHUTDOWN_CLOSE_TIMEOUT, closing).await {
            Ok(closed) => tracing::debug!(closed, total, "closed pooled gateway sessions"),
            Err(_) => tracing::debug!(total, "closing pooled gateway sessions timed out"),
        }
    }

    fn fallback_auth(config: &Config) -> Option<SecretString> {
        match (&config.username, &config.password) {
            (Some(user), Some(pass)) => Some(SecretString::from(basic_auth(user, pass))),
            _ => None,
        }
    }

    /// Whether a fallback identity is configured (used for startup
    /// diagnostics; never exposes the credential itself).
    pub fn has_fallback_auth(&self) -> bool {
        self.fallback_auth.is_some()
    }

    /// Test seam: build a client with an explicit fallback `Basic` credential.
    #[cfg(test)]
    pub fn with_basic_fallback(
        gateway_url: Url,
        username: &str,
        password: SecretString,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            http: Self::http_client(Duration::from_secs(5), &[])?,
            base_url: gateway_url,
            fallback_auth: Some(SecretString::from(basic_auth(username, &password))),
            auth_mode: GatewayAuthMode::PerRequest,
            pool: Arc::new(SessionPool::new(8)),
            reaper_started: Arc::new(AtomicBool::new(false)),
        })
    }

    /// URL under `/api/v1`.
    pub fn api_url(&self, segments: &[&str]) -> Url {
        let mut all = API_PREFIX.to_vec();
        all.extend_from_slice(segments);
        join_url(&self.base_url, &all)
    }

    /// URL at the unversioned root (health/readiness probes).
    pub fn root_url(&self, segments: &[&str]) -> Url {
        join_url(&self.base_url, segments)
    }

    /// `GET /health` — liveness, unauthenticated.
    pub async fn health(&self) -> Result<HealthStatus, GatewayError> {
        self.get_unauthenticated(self.root_url(&["health"])).await
    }

    /// `GET /ready` — readiness, unauthenticated. The gateway answers 503
    /// with the same body shape while gvmd is unreachable; both are decoded.
    pub async fn ready(&self) -> Result<ReadinessStatus, GatewayError> {
        let url = self.root_url(&["ready"]);
        let response = self.http.get(url.clone()).send().await?;
        let status = response.status();
        if status.is_success() || status.as_u16() == 503 {
            return response
                .json()
                .await
                .map_err(|source| GatewayError::Decode {
                    endpoint: url.to_string(),
                    source,
                });
        }
        Err(error_from_response(response).await)
    }

    /// `GET /api/v1/version` — gvmd version, unauthenticated.
    pub async fn version(&self) -> Result<VersionInfo, GatewayError> {
        self.get_unauthenticated(self.api_url(&["version"])).await
    }

    /// The `Authorization` value the current tool call would send: the
    /// caller's forwarded header, else the configured fallback. Used to open
    /// job-bound gateway sessions on the caller's behalf.
    pub fn current_identity(&self) -> Option<String> {
        let fallback = self.fallback_auth.as_ref().map(|s| s.expose_secret());
        current_authorization(fallback)
    }

    /// `POST /api/v1/session` with an explicit `Basic` credential: opens a
    /// persistent gateway session and returns its bearer token. Used for
    /// asynchronous export jobs, which the gateway binds to the session that
    /// created them (the pooled sessions of session mode are created through
    /// the same endpoint internally).
    pub async fn create_session(&self, authorization: &str) -> Result<String, GatewayError> {
        let url = self.api_url(&["session"]);
        let sent = self
            .send_with(Some(authorization.to_owned()), |http| {
                http.post(url.clone())
            })
            .await?;
        let created: SessionCreated = Self::decode(url, sent).await?;
        Ok(created.session_token)
    }

    /// `DELETE /api/v1/session` for a session this server opened.
    pub async fn delete_session(&self, token: &str) -> Result<(), GatewayError> {
        let url = self.api_url(&["session"]);
        let sent = self
            .send_with(Some(format!("Bearer {token}")), |http| {
                http.delete(url.clone())
            })
            .await?;
        Self::expect_success(sent).await
    }

    /// Session creation for the pool: not gated (the caller already holds
    /// the identity's permit) and never pooled itself.
    async fn create_session_raw(
        &self,
        authorization: &str,
    ) -> Result<SessionCreated, GatewayError> {
        let url = self.api_url(&["session"]);
        let response = self
            .send_once(authorization, &|http: &reqwest::Client| {
                http.post(url.clone())
            })
            .await?;
        let sent = Sent {
            response,
            _permit: None,
        };
        Self::decode(url, sent).await
    }

    /// Session deletion outside the gate (evictions, shutdown).
    async fn delete_session_raw(&self, token: &str) -> Result<(), GatewayError> {
        delete_session_at(&self.http, &self.base_url, token).await
    }

    /// Start the idle-session reaper once. It holds only a weak reference to
    /// the pool, so it ends when the last client handle is dropped.
    fn ensure_reaper(&self) {
        if self
            .reaper_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let pool = Arc::downgrade(&self.pool);
        let http = self.http.clone();
        let base_url = self.base_url.clone();
        let interval =
            (self.pool.idle_release() / 2).clamp(Duration::from_millis(10), REAPER_MAX_INTERVAL);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(pool) = pool.upgrade() else { break };
                for token in pool.reap_idle() {
                    if let Err(err) = delete_session_at(&http, &base_url, &token).await {
                        tracing::debug!(error = %err, "closing an idle gateway session failed");
                    } else {
                        tracing::debug!("closed an idle pooled gateway session");
                    }
                }
            }
        });
    }

    /// Close a session the pool no longer uses without blocking the caller.
    fn close_session_in_background(&self, token: Option<String>) {
        let Some(token) = token else { return };
        let client = self.clone();
        tokio::spawn(async move {
            if let Err(err) = client.delete_session_raw(&token).await {
                tracing::debug!(error = %err, "closing a replaced gateway session failed");
            }
        });
    }

    /// Authorized GET under `/api/v1`, decoded as JSON.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        segments: &[&str],
    ) -> Result<T, GatewayError> {
        self.get_json_query(segments, &[]).await
    }

    /// Authorized GET under `/api/v1` with query parameters, decoded as JSON.
    pub async fn get_json_query<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_authorized(|http| http.get(url.clone()).query(query))
            .await?;
        Self::decode(url, sent).await
    }

    /// Authorized POST under `/api/v1` with a JSON body, decoded as JSON.
    pub async fn post_json<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        body: &impl serde::Serialize,
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_authorized(|http| http.post(url.clone()).json(body))
            .await?;
        Self::decode(url, sent).await
    }

    /// Authorized POST under `/api/v1` with a raw (non-JSON) body and query
    /// parameters, decoded as JSON. Used for the XML report import.
    pub async fn post_raw<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
        content_type: &'static str,
        body: Vec<u8>,
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_authorized(|http| {
                http.post(url.clone())
                    .query(query)
                    .header(CONTENT_TYPE, content_type)
                    .body(body.clone())
            })
            .await?;
        Self::decode(url, sent).await
    }

    /// Authorized PUT under `/api/v1` with a JSON body, decoded as JSON.
    pub async fn put_json<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        body: &impl serde::Serialize,
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_authorized(|http| http.put(url.clone()).json(body))
            .await?;
        Self::decode(url, sent).await
    }

    /// Authorized PUT under `/api/v1` with query parameters and a JSON body,
    /// decoded as JSON (generic assets require a `type` selector).
    pub async fn put_json_query<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
        body: &impl serde::Serialize,
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_authorized(|http| http.put(url.clone()).query(query).json(body))
            .await?;
        Self::decode(url, sent).await
    }

    /// Authorized PUT under `/api/v1` with a JSON body where success carries
    /// no body (204; e.g. scan-config selection and preference writes).
    pub async fn put_json_empty(
        &self,
        segments: &[&str],
        body: &impl serde::Serialize,
    ) -> Result<(), GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_authorized(|http| http.put(url.clone()).json(body))
            .await?;
        Self::expect_success(sent).await
    }

    /// Authorized bodyless POST under `/api/v1`, decoded as JSON.
    pub async fn post_action<T: DeserializeOwned>(
        &self,
        segments: &[&str],
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let sent = self.send_authorized(|http| http.post(url.clone())).await?;
        Self::decode(url, sent).await
    }

    /// Authorized bodyless POST under `/api/v1` where the response body (if
    /// any) carries no information (e.g. task stop, agent sync).
    pub async fn post_action_empty(&self, segments: &[&str]) -> Result<(), GatewayError> {
        let url = self.api_url(segments);
        let sent = self.send_authorized(|http| http.post(url.clone())).await?;
        Self::expect_success(sent).await
    }

    /// Authorized DELETE under `/api/v1` (expects 2xx, usually 204).
    pub async fn delete(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
    ) -> Result<(), GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_authorized(|http| http.delete(url.clone()).query(query))
            .await?;
        Self::expect_success(sent).await
    }

    /// Authorized GET under `/api/v1` returning raw bytes plus the
    /// `Content-Type` and `Content-Disposition` filename (artifact
    /// downloads: rendered report exports, agent support bundles).
    ///
    /// The body is streamed and capped at `max_bytes`: anything larger is
    /// rejected with [`GatewayError::ArtifactTooLarge`] before it is held
    /// in memory, so a huge artifact cannot exhaust the server.
    pub async fn get_bytes(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
        max_bytes: usize,
    ) -> Result<Artifact, GatewayError> {
        self.get_bytes_with(None, segments, query, max_bytes).await
    }

    /// [`Self::get_bytes`] with an explicit `Authorization` (job-bound
    /// export sessions).
    pub async fn get_bytes_as(
        &self,
        authorization: &str,
        segments: &[&str],
        query: &[(&str, String)],
        max_bytes: usize,
    ) -> Result<Artifact, GatewayError> {
        self.get_bytes_with(Some(authorization.to_owned()), segments, query, max_bytes)
            .await
    }

    /// Authorized GET with an explicit `Authorization`, decoded as JSON.
    pub async fn get_json_as<T: DeserializeOwned>(
        &self,
        authorization: &str,
        segments: &[&str],
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_with(Some(authorization.to_owned()), |http| http.get(url.clone()))
            .await?;
        Self::decode(url, sent).await
    }

    /// Authorized POST with an explicit `Authorization`, decoded as JSON.
    pub async fn post_json_as<T: DeserializeOwned>(
        &self,
        authorization: &str,
        segments: &[&str],
        body: &impl serde::Serialize,
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_with(Some(authorization.to_owned()), |http| {
                http.post(url.clone()).json(body)
            })
            .await?;
        Self::decode(url, sent).await
    }

    /// Authorized DELETE with an explicit `Authorization` (expects 2xx).
    pub async fn delete_as(
        &self,
        authorization: &str,
        segments: &[&str],
    ) -> Result<(), GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_with(Some(authorization.to_owned()), |http| {
                http.delete(url.clone())
            })
            .await?;
        Self::expect_success(sent).await
    }

    async fn get_bytes_with(
        &self,
        authorization: Option<String>,
        segments: &[&str],
        query: &[(&str, String)],
        max_bytes: usize,
    ) -> Result<Artifact, GatewayError> {
        let url = self.api_url(segments);
        let sent = self
            .send_with(authorization, |http| http.get(url.clone()).query(query))
            .await?;
        // Keep the permit until the body has been streamed.
        let Sent {
            mut response,
            _permit,
        } = sent;
        if !response.status().is_success() {
            return Err(error_from_response(response).await);
        }

        if let Some(declared) = response.content_length()
            && declared > max_bytes as u64
        {
            return Err(GatewayError::ArtifactTooLarge {
                limit: max_bytes,
                size: Some(declared),
            });
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let filename = response
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_disposition_filename);

        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > max_bytes {
                return Err(GatewayError::ArtifactTooLarge {
                    limit: max_bytes,
                    size: None,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Artifact {
            bytes,
            content_type,
            filename,
        })
    }

    async fn get_unauthenticated<T: DeserializeOwned>(&self, url: Url) -> Result<T, GatewayError> {
        let response = self.http.get(url.clone()).send().await?;
        Self::decode(
            url,
            Sent {
                response,
                _permit: None,
            },
        )
        .await
    }

    /// Send with the identity for the current request: the caller's forwarded
    /// `Authorization` if present, otherwise the configured fallback. If
    /// neither is available the request goes out unauthenticated and the
    /// gateway answers `401`. `build` constructs the request minus
    /// authorization.
    async fn send_authorized<F>(&self, build: F) -> Result<Sent, GatewayError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        self.send_with(None, build).await
    }

    /// Send with an explicit `Authorization` when given, otherwise with the
    /// identity for the current request. Explicit authorizations (export
    /// job sessions) and forwarded bearer tokens are sent as they are; a
    /// `Basic` identity goes through the session pool in session mode. Every
    /// identity's requests pass its in-flight gate.
    async fn send_with<F>(
        &self,
        authorization: Option<String>,
        build: F,
    ) -> Result<Sent, GatewayError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        let explicit = authorization.is_some();
        let Some(identity) = authorization.or_else(|| self.current_identity()) else {
            let response = build(&self.http).send().await?;
            return Ok(Sent {
                response,
                _permit: None,
            });
        };

        let (slot, evicted) = self.pool.slot(IdentityKey::of(&identity));
        self.close_session_in_background(evicted);
        let permit = slot
            .gate()
            .acquire_owned()
            .await
            .expect("in-flight gate is never closed");

        let pooled = !explicit
            && self.auth_mode == GatewayAuthMode::Session
            && identity.starts_with("Basic ");
        let response = if pooled {
            self.send_pooled(&slot, &identity, &build).await?
        } else {
            self.send_once(&identity, &build).await?
        };
        Ok(Sent {
            response,
            _permit: Some(permit),
        })
    }

    /// Session-mode send for a `Basic` identity: bearer token from the pool,
    /// one re-authentication when the gateway rejects the session, request-
    /// scoped `Basic` when no session can be obtained.
    async fn send_pooled<F>(
        &self,
        slot: &Slot,
        identity: &str,
        build: &F,
    ) -> Result<reqwest::Response, GatewayError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        let Some(token) = self.pooled_token(slot, identity).await? else {
            return self.send_once(identity, build).await;
        };
        let response = self.send_once(&format!("Bearer {token}"), build).await?;
        match response.status().as_u16() {
            // The gateway no longer accepts the session (idle expiry, restart,
            // eviction). Forget it and retry once with a fresh one; a second
            // 401 is then a genuine credential problem and propagates.
            401 => {
                drop(response);
                if slot.invalidate(&token) {
                    tracing::debug!("pooled gateway session was rejected; re-authenticating");
                }
            }
            // The gateway keeps a session "active" even after its gvmd
            // connection died (gvmd restart, or a command the connected gvmd
            // does not support, which makes gvmd drop the connection); every
            // further command on it then fails the same way. Drop and close
            // the poisoned session so the next call logs in afresh, and
            // retry now when the request cannot have side effects.
            502..=504 => {
                let idempotent = build(&self.http)
                    .build()
                    .map(|request| {
                        matches!(
                            *request.method(),
                            reqwest::Method::GET
                                | reqwest::Method::HEAD
                                | reqwest::Method::PUT
                                | reqwest::Method::DELETE
                        )
                    })
                    .unwrap_or(false);
                if slot.invalidate(&token) {
                    tracing::debug!(
                        status = response.status().as_u16(),
                        "pooled gateway session lost its gvmd connection; closing it"
                    );
                    self.close_session_in_background(Some(token));
                }
                if !idempotent {
                    return Ok(response);
                }
                drop(response);
            }
            _ => return Ok(response),
        }
        match self.pooled_token(slot, identity).await? {
            Some(token) => self.send_once(&format!("Bearer {token}"), build).await,
            None => self.send_once(identity, build).await,
        }
    }

    /// The pooled bearer token for `identity`, creating the session when
    /// needed. `Ok(None)` means "use request-scoped `Basic` for this call":
    /// the gateway refused to create a session for a reason other than the
    /// credential itself, and creation is paused for a while. A `401` from
    /// session creation is returned as the error it is.
    async fn pooled_token(
        &self,
        slot: &Slot,
        identity: &str,
    ) -> Result<Option<String>, GatewayError> {
        let (token, stale) = slot.current();
        self.close_session_in_background(stale);
        if let Some(token) = token {
            return Ok(Some(token));
        }

        let _creating = slot.creating().await;
        // Another call may have created the session while we waited.
        let (token, stale) = slot.current();
        self.close_session_in_background(stale);
        if let Some(token) = token {
            return Ok(Some(token));
        }
        if slot.is_suspended() {
            return Ok(None);
        }

        match self.create_session_raw(identity).await {
            Ok(created) => {
                let token = created.session_token.clone();
                slot.install(PooledSession::new(
                    created.session_token,
                    created.expires_in,
                ));
                self.ensure_reaper();
                tracing::debug!(
                    expires_in = ?created.expires_in,
                    "opened a pooled gateway session"
                );
                Ok(Some(token))
            }
            // Wrong credentials, or the gateway's per-user session cap / request
            // budget (already retried by `send_once`): request-scoped Basic would
            // get the very same answer, so report it instead of doubling the load.
            Err(err) if err.is_unauthorized() || err.is_rate_limited() => Err(err),
            Err(err) => {
                slot.suspend(SUSPEND_AFTER_FAILURE);
                tracing::warn!(
                    error = %err,
                    "gateway session creation failed; falling back to request-scoped \
                     Basic authentication for {}s",
                    SUSPEND_AFTER_FAILURE.as_secs()
                );
                Ok(None)
            }
        }
    }

    /// One request with the given `Authorization`. A `429` caused by the
    /// gateway's per-user session cap is retried briefly, because it clears
    /// as soon as another request of the same user finishes; the per-window
    /// request budget is not retried (its `retry after` is tens of seconds)
    /// and is returned as a typed error.
    async fn send_once<F>(
        &self,
        authorization: &str,
        build: &F,
    ) -> Result<reqwest::Response, GatewayError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        for attempt in 0..SESSION_LIMIT_ATTEMPTS {
            let response = build(&self.http)
                .header(AUTHORIZATION, authorization)
                .send()
                .await?;
            if response.status().as_u16() != 429 {
                return Ok(response);
            }
            let err = error_from_response(response).await;
            if !err.is_session_limit() || attempt + 1 == SESSION_LIMIT_ATTEMPTS {
                return Err(err);
            }
            tokio::time::sleep(SESSION_LIMIT_BACKOFF * 2u32.pow(attempt)).await;
        }
        unreachable!("the retry loop returns on its last attempt")
    }

    async fn expect_success(sent: Sent) -> Result<(), GatewayError> {
        let Sent { response, _permit } = sent;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(error_from_response(response).await)
        }
    }

    async fn decode<T: DeserializeOwned>(url: Url, sent: Sent) -> Result<T, GatewayError> {
        let Sent { response, _permit } = sent;
        if !response.status().is_success() {
            return Err(error_from_response(response).await);
        }
        response
            .json()
            .await
            .map_err(|source| GatewayError::Decode {
                endpoint: url.to_string(),
                source,
            })
    }
}

/// `DELETE /api/v1/session` for `token`, usable from the reaper task.
async fn delete_session_at(
    http: &reqwest::Client,
    base_url: &Url,
    token: &str,
) -> Result<(), GatewayError> {
    let url = join_url(base_url, &["api", "v1", "session"]);
    let response = http
        .delete(url)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .send()
        .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(error_from_response(response).await)
    }
}

/// Extract the plain `filename=` parameter of a `Content-Disposition` value.
/// Only the token is kept (quotes stripped, path separators rejected) — the
/// name is reported to the caller, never used to touch a filesystem.
fn parse_content_disposition_filename(value: &str) -> Option<String> {
    value
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("filename="))
        .map(|raw| raw.trim_matches('"').trim().to_owned())
        .filter(|name| !name.is_empty() && !name.contains(['/', '\\']))
}

/// Append path segments to a base URL, preserving any base path prefix
/// (e.g. a gateway mounted behind a reverse proxy at `/gvm`).
fn join_url(base: &Url, segments: &[&str]) -> Url {
    let mut url = base.clone();
    {
        let mut path = url
            .path_segments_mut()
            .expect("gateway URL validated as http(s) with a host");
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use base64::Engine;

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
    fn join_url_appends_segments() {
        let base = Url::parse("http://gw:8080").unwrap();
        assert_eq!(
            join_url(&base, &["api", "v1", "targets"]).as_str(),
            "http://gw:8080/api/v1/targets"
        );
    }

    #[test]
    fn join_url_preserves_base_path_prefix() {
        let base = Url::parse("http://proxy/gvm/").unwrap();
        assert_eq!(
            join_url(&base, &["health"]).as_str(),
            "http://proxy/gvm/health"
        );
    }

    #[test]
    fn join_url_percent_encodes_untrusted_segments() {
        // A caller-supplied id must stay one path segment: no traversal into
        // a sibling route, no query injection.
        let base = Url::parse("http://gw:8080").unwrap();
        let url = join_url(&base, &["api", "v1", "targets", "../session?x=1"]);
        assert_eq!(
            url.as_str(),
            "http://gw:8080/api/v1/targets/..%2Fsession%3Fx=1"
        );
    }

    #[test]
    fn secinfo_ids_stay_one_path_segment() {
        // CPE names and advisory ids contain `/`; they must be sent as one
        // percent-encoded segment so the gateway's `{id}` route receives them.
        let base = Url::parse("http://gw:8080").unwrap();
        let url = join_url(&base, &["api", "v1", "cpes", "cpe:/a:openbsd:openssh"]);
        eprintln!("encoded cpe url: {url}");
        assert_eq!(
            url.as_str(),
            "http://gw:8080/api/v1/cpes/cpe:%2Fa:openbsd:openssh"
        );
    }

    #[test]
    fn content_disposition_filename_is_sanitized() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"report.pdf\""),
            Some("report.pdf".to_owned())
        );
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=../etc/passwd"),
            None
        );
        assert_eq!(parse_content_disposition_filename("inline"), None);
    }

    #[test]
    fn debug_output_never_contains_the_fallback_credential() {
        // The password is derived at run time (no literal secret in the
        // source); the encoded header is computed the same way the client
        // builds it so the assertion covers both plain and encoded forms.
        let password = runtime_secret();
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(format!("admin:{password}").as_bytes());
        let client = GatewayClient::with_basic_fallback(
            Url::parse("http://gw:8080").unwrap(),
            "admin",
            SecretString::from(password.clone()),
        )
        .unwrap();
        let rendered = format!("{client:?}");
        assert!(rendered.contains("<redacted>"), "got: {rendered}");
        assert!(!rendered.contains(&encoded), "got: {rendered}");
        assert!(!rendered.contains(&password), "got: {rendered}");
    }
}
