//! Typed gateway errors: RFC 9457 problem+json and transport failures.

use serde::{Deserialize, Serialize};

/// RFC 9457 Problem Detail as served by the gateway on every error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemDetail {
    #[serde(rename = "type", default)]
    pub type_uri: String,
    /// Stable machine-readable problem identity (e.g. `not_found`).
    pub code: String,
    pub title: String,
    pub status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

impl std::fmt::Display for ProblemDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}, HTTP {})", self.title, self.code, self.status)?;
        if let Some(detail) = &self.detail {
            write!(f, ": {detail}")?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// The gateway could not be reached, timed out, or the connection failed.
    #[error("gateway unreachable: {0}")]
    Transport(#[from] reqwest::Error),

    /// The gateway answered with an RFC 9457 problem+json error.
    #[error("gateway error: {0}")]
    Api(Box<ProblemDetail>),

    /// The gateway answered with an error status but no problem+json body.
    #[error("gateway returned unexpected HTTP {status}: {body}")]
    UnexpectedStatus { status: u16, body: String },

    /// A success response could not be decoded into the expected DTO.
    #[error("failed to decode gateway response from {endpoint}: {source}")]
    Decode {
        endpoint: String,
        #[source]
        source: reqwest::Error,
    },

    /// A downloaded artifact exceeds the inline size limit.
    #[error("artifact exceeds the {limit} byte inline limit")]
    ArtifactTooLarge { limit: usize, size: Option<u64> },
}

impl GatewayError {
    /// HTTP status of the gateway's answer, if it answered at all.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api(problem) => Some(problem.status),
            Self::UnexpectedStatus { status, .. } => Some(*status),
            Self::Transport(err) => err.status().map(|s| s.as_u16()),
            Self::Decode { .. } | Self::ArtifactTooLarge { .. } => None,
        }
    }

    /// Whether the gateway rejected the identity used for the request.
    pub fn is_unauthorized(&self) -> bool {
        self.status() == Some(401)
    }

    /// Whether the gateway throttled the request (HTTP 429), for either of
    /// its two reasons: the per-user session cap or the request budget.
    pub fn is_rate_limited(&self) -> bool {
        self.status() == Some(429)
    }

    /// Whether a 429 is the gateway's per-user session cap, which clears as
    /// soon as another request of the same user finishes, as opposed to the
    /// per-window request budget, which only clears after `retry after`.
    pub fn is_session_limit(&self) -> bool {
        match self {
            Self::Api(problem) if problem.status == 429 => problem
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("session limit")),
            _ => false,
        }
    }
}

/// Convert a non-success gateway response into a typed error, consuming it.
pub async fn error_from_response(response: reqwest::Response) -> GatewayError {
    let status = response.status().as_u16();
    let is_problem = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("application/problem+json"));

    let body = match response.text().await {
        Ok(body) => body,
        Err(err) => return GatewayError::Transport(err),
    };
    error_from_body(status, is_problem, body)
}

/// Typed error for an already-read error body.
pub fn error_from_body(status: u16, is_problem: bool, body: String) -> GatewayError {
    if is_problem && let Ok(problem) = serde_json::from_str::<ProblemDetail>(&body) {
        return GatewayError::Api(Box::new(problem));
    }

    let mut body = body;
    body.truncate(500);
    GatewayError::UnexpectedStatus { status, body }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problem_detail_display_includes_code_status_detail() {
        let problem = ProblemDetail {
            type_uri: "https://gvm-gateway.greenbone.net/errors/not-found".into(),
            code: "not_found".into(),
            title: "Resource Not Found".into(),
            status: 404,
            detail: Some("Target 'x' not found.".into()),
            instance: None,
        };
        let text = problem.to_string();
        assert!(text.contains("not_found"));
        assert!(text.contains("404"));
        assert!(text.contains("Target 'x' not found."));
    }

    #[test]
    fn session_limit_is_told_apart_from_the_request_budget() {
        let limit = |detail: &str| {
            GatewayError::Api(Box::new(ProblemDetail {
                type_uri: String::new(),
                code: "too_many_requests".into(),
                title: "Too Many Requests".into(),
                status: 429,
                detail: Some(detail.into()),
                instance: None,
            }))
        };
        let sessions = limit("per-user session limit exceeded");
        assert!(sessions.is_rate_limited());
        assert!(sessions.is_session_limit());
        let budget = limit("rate limit exceeded; retry after 56 seconds");
        assert!(budget.is_rate_limited());
        assert!(!budget.is_session_limit());
        assert!(!error_from_body(503, false, "down".into()).is_rate_limited());
    }

    #[test]
    fn unauthorized_detection() {
        let err = GatewayError::Api(Box::new(ProblemDetail {
            type_uri: String::new(),
            code: "session_expired".into(),
            title: "Unauthorized".into(),
            status: 401,
            detail: None,
            instance: None,
        }));
        assert!(err.is_unauthorized());

        let err = GatewayError::UnexpectedStatus {
            status: 500,
            body: "boom".into(),
        };
        assert!(!err.is_unauthorized());
        assert!(
            GatewayError::ArtifactTooLarge {
                limit: 1,
                size: None
            }
            .status()
            .is_none()
        );
    }
}
