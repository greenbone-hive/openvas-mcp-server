//! Map gateway failures to user-legible MCP tool errors.
//!
//! Gateway/tool failures are reported as tool-level errors
//! (`CallToolResult::error`) so the calling model sees the message and can
//! react; protocol errors (`ErrorData`) are reserved for malformed requests
//! and server bugs. Transport failures, 4xx and 5xx are worded distinctly so
//! the model can tell "gateway down" from "bad request" from "gateway bug".

use rmcp::model::{CallToolResult, ContentBlock};

use crate::gateway::GatewayError;

/// Render a gateway failure during `stage` as a tool-level error result.
pub fn gateway_tool_error(stage: &str, err: &GatewayError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(describe(stage, err))])
}

fn describe(stage: &str, err: &GatewayError) -> String {
    match err {
        GatewayError::Transport(source) => format!(
            "GVM gateway unreachable while {stage}: {source}. \
             Check GVM_GATEWAY_URL and that the rust-gvm-api gateway is running."
        ),
        GatewayError::Api(problem) => {
            let class = match problem.status {
                401 => "Authentication failed",
                403 => "Permission denied",
                404 => "Not found",
                409 => "Conflict",
                413 => "Payload too large",
                415 => "Unsupported media type",
                429 => "Rate limited",
                400..=499 => "Request rejected",
                501 => "Not supported by the connected gvmd",
                502 | 504 => "Gateway cannot reach gvmd",
                _ => "Gateway error",
            };
            format!("{class} while {stage}: {problem}")
        }
        GatewayError::UnexpectedStatus { status, body } => {
            format!("Gateway returned unexpected HTTP {status} while {stage}: {body}")
        }
        GatewayError::Decode { endpoint, source } => format!(
            "Gateway response from {endpoint} could not be decoded while {stage}: {source}. \
             The gateway and this server may be running incompatible versions."
        ),
        GatewayError::ArtifactTooLarge { limit, size } => {
            let observed = size
                .map(|s| format!("{s} bytes"))
                .unwrap_or_else(|| "more than the limit".to_owned());
            format!(
                "Artifact is {observed}, larger than the {limit} byte inline limit while {stage}. \
                 Export with a narrower filter, or use the JSON format and page through \
                 the results instead."
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gateway::ProblemDetail;

    use super::*;

    fn problem(status: u16, code: &str) -> GatewayError {
        GatewayError::Api(Box::new(ProblemDetail {
            type_uri: String::new(),
            code: code.into(),
            title: "Some Problem".into(),
            status,
            detail: Some("details here".into()),
            instance: None,
        }))
    }

    #[test]
    fn api_errors_keep_code_title_detail() {
        let text = describe("listing targets", &problem(404, "not_found"));
        assert!(text.contains("Not found"));
        assert!(text.contains("listing targets"));
        assert!(text.contains("not_found"));
        assert!(text.contains("details here"));
    }

    #[test]
    fn status_classes_are_distinguishable() {
        assert!(describe("x", &problem(401, "unauthorized")).contains("Authentication failed"));
        assert!(describe("x", &problem(403, "forbidden")).contains("Permission denied"));
        assert!(describe("x", &problem(409, "conflict")).contains("Conflict"));
        assert!(describe("x", &problem(429, "rate_limited")).contains("Rate limited"));
        assert!(describe("x", &problem(501, "not_implemented")).contains("Not supported"));
        assert!(describe("x", &problem(502, "bad_gateway")).contains("cannot reach gvmd"));
    }

    #[test]
    fn tool_error_result_is_flagged_as_error() {
        let result = gateway_tool_error("testing", &problem(500, "internal"));
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn too_large_artifact_explains_the_limit() {
        let text = describe(
            "downloading the job result",
            &GatewayError::ArtifactTooLarge {
                limit: 3_145_728,
                size: Some(9_000_000),
            },
        );
        assert!(text.contains("9000000 bytes"));
        assert!(text.contains("3145728"));
    }
}
