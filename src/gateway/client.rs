//! Gateway HTTP client: request plumbing and per-request identity forwarding.
//!
//! The client holds no session. Every authorized request carries an
//! `Authorization` resolved per call by [`super::auth`]: the inbound caller's
//! header when present, otherwise a `Basic` header built from configured
//! gvmd credentials. The gateway (backed by gvmd) authorizes each request.

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use url::Url;

use crate::config::Config;

use super::auth::{basic_auth, current_authorization};
use super::error::{GatewayError, error_from_response};
use super::models::{HealthStatus, ReadinessStatus, SessionCreated, VersionInfo};

/// Path prefix of the versioned API surface. Liveness/readiness probes live
/// at the unversioned root.
const API_PREFIX: [&str; 2] = ["api", "v1"];

/// A downloaded binary artifact (report export, agent support bundle).
#[derive(Debug, Clone)]
pub struct Artifact {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    pub filename: Option<String>,
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
}

impl std::fmt::Debug for GatewayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayClient")
            .field("base_url", &self.base_url.as_str())
            .field(
                "fallback_auth",
                &self.fallback_auth.as_ref().map(|_| "<redacted>"),
            )
            .finish_non_exhaustive()
    }
}

impl GatewayClient {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let http = Self::http_client(config.timeout)?;
        let fallback_auth = Self::fallback_auth(config);

        Ok(Self {
            http,
            base_url: config.gateway_url.clone(),
            fallback_auth,
        })
    }

    /// Build the HTTP client. Redirects are never followed: the gateway
    /// serves every endpoint directly, and following one would hand the
    /// caller's credentials to whatever a misconfigured proxy points at.
    fn http_client(timeout: std::time::Duration) -> anyhow::Result<reqwest::Client> {
        Ok(reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("gvm-mcp/", env!("CARGO_PKG_VERSION")))
            .build()?)
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
            http: Self::http_client(std::time::Duration::from_secs(5))?,
            base_url: gateway_url,
            fallback_auth: Some(SecretString::from(basic_auth(username, &password))),
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
    /// persistent gateway session and returns its bearer token. Only used
    /// for asynchronous export jobs, which the gateway binds to the session
    /// that created them.
    pub async fn create_session(&self, authorization: &str) -> Result<String, GatewayError> {
        let url = self.api_url(&["session"]);
        let response = self
            .send_with(Some(authorization.to_owned()), |http| {
                http.post(url.clone())
            })
            .await?;
        let created: SessionCreated = Self::decode(url, response).await?;
        Ok(created.session_token)
    }

    /// `DELETE /api/v1/session` for a session this server opened.
    pub async fn delete_session(&self, token: &str) -> Result<(), GatewayError> {
        let url = self.api_url(&["session"]);
        let response = self
            .send_with(Some(format!("Bearer {token}")), |http| {
                http.delete(url.clone())
            })
            .await?;
        Self::expect_success(response).await
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
        let response = self
            .send_authorized(|http| http.get(url.clone()).query(query))
            .await?;
        Self::decode(url, response).await
    }

    /// Authorized POST under `/api/v1` with a JSON body, decoded as JSON.
    pub async fn post_json<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        body: &impl serde::Serialize,
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let response = self
            .send_authorized(|http| http.post(url.clone()).json(body))
            .await?;
        Self::decode(url, response).await
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
        let response = self
            .send_authorized(|http| {
                http.post(url.clone())
                    .query(query)
                    .header(CONTENT_TYPE, content_type)
                    .body(body.clone())
            })
            .await?;
        Self::decode(url, response).await
    }

    /// Authorized PUT under `/api/v1` with a JSON body, decoded as JSON.
    pub async fn put_json<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        body: &impl serde::Serialize,
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let response = self
            .send_authorized(|http| http.put(url.clone()).json(body))
            .await?;
        Self::decode(url, response).await
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
        let response = self
            .send_authorized(|http| http.put(url.clone()).query(query).json(body))
            .await?;
        Self::decode(url, response).await
    }

    /// Authorized PUT under `/api/v1` with a JSON body where success carries
    /// no body (204; e.g. scan-config selection and preference writes).
    pub async fn put_json_empty(
        &self,
        segments: &[&str],
        body: &impl serde::Serialize,
    ) -> Result<(), GatewayError> {
        let url = self.api_url(segments);
        let response = self
            .send_authorized(|http| http.put(url.clone()).json(body))
            .await?;
        Self::expect_success(response).await
    }

    /// Authorized bodyless POST under `/api/v1`, decoded as JSON.
    pub async fn post_action<T: DeserializeOwned>(
        &self,
        segments: &[&str],
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let response = self.send_authorized(|http| http.post(url.clone())).await?;
        Self::decode(url, response).await
    }

    /// Authorized bodyless POST under `/api/v1` where the response body (if
    /// any) carries no information (e.g. task stop, agent sync).
    pub async fn post_action_empty(&self, segments: &[&str]) -> Result<(), GatewayError> {
        let url = self.api_url(segments);
        let response = self.send_authorized(|http| http.post(url.clone())).await?;
        Self::expect_success(response).await
    }

    /// Authorized DELETE under `/api/v1` (expects 2xx, usually 204).
    pub async fn delete(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
    ) -> Result<(), GatewayError> {
        let url = self.api_url(segments);
        let response = self
            .send_authorized(|http| http.delete(url.clone()).query(query))
            .await?;
        Self::expect_success(response).await
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
        let response = self
            .send_with(Some(authorization.to_owned()), |http| http.get(url.clone()))
            .await?;
        Self::decode(url, response).await
    }

    /// Authorized POST with an explicit `Authorization`, decoded as JSON.
    pub async fn post_json_as<T: DeserializeOwned>(
        &self,
        authorization: &str,
        segments: &[&str],
        body: &impl serde::Serialize,
    ) -> Result<T, GatewayError> {
        let url = self.api_url(segments);
        let response = self
            .send_with(Some(authorization.to_owned()), |http| {
                http.post(url.clone()).json(body)
            })
            .await?;
        Self::decode(url, response).await
    }

    /// Authorized DELETE with an explicit `Authorization` (expects 2xx).
    pub async fn delete_as(
        &self,
        authorization: &str,
        segments: &[&str],
    ) -> Result<(), GatewayError> {
        let url = self.api_url(segments);
        let response = self
            .send_with(Some(authorization.to_owned()), |http| {
                http.delete(url.clone())
            })
            .await?;
        Self::expect_success(response).await
    }

    async fn get_bytes_with(
        &self,
        authorization: Option<String>,
        segments: &[&str],
        query: &[(&str, String)],
        max_bytes: usize,
    ) -> Result<Artifact, GatewayError> {
        let url = self.api_url(segments);
        let mut response = self
            .send_with(authorization, |http| http.get(url.clone()).query(query))
            .await?;
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
        Self::decode(url, response).await
    }

    /// Send with the identity for the current request: the caller's forwarded
    /// `Authorization` if present, otherwise the configured fallback. If
    /// neither is available the request goes out unauthenticated and the
    /// gateway answers `401`. `build` constructs the request minus
    /// authorization.
    async fn send_authorized<F>(&self, build: F) -> Result<reqwest::Response, GatewayError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        self.send_with(None, build).await
    }

    /// Send with an explicit `Authorization` when given, otherwise with the
    /// identity for the current request.
    async fn send_with<F>(
        &self,
        authorization: Option<String>,
        build: F,
    ) -> Result<reqwest::Response, GatewayError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        let mut request = build(&self.http);
        let resolved = authorization.or_else(|| self.current_identity());
        if let Some(authorization) = resolved {
            request = request.header(AUTHORIZATION, authorization);
        }
        Ok(request.send().await?)
    }

    async fn expect_success(response: reqwest::Response) -> Result<(), GatewayError> {
        if response.status().is_success() {
            Ok(())
        } else {
            Err(error_from_response(response).await)
        }
    }

    async fn decode<T: DeserializeOwned>(
        url: Url,
        response: reqwest::Response,
    ) -> Result<T, GatewayError> {
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
        let password = format!("pw-{}-{}", std::process::id(), line!());
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
