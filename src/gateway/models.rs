//! Serde DTOs mirroring the gateway's `openapi.yaml` (source of truth:
//! `rust-gvm-api` `spec/rest-api/`). Keep field names in lockstep with the
//! spec; the contract tests deserialize the documented examples.
//!
//! Only the shapes this server inspects are typed; everything else is
//! passed through as `serde_json::Value` so new gateway fields never break
//! a tool.

use serde::{Deserialize, Serialize};

/// `GET /health` — 200 response (unversioned root path).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Liveness state (`ok`).
    pub status: String,
}

/// `GET /ready` — 200/503 response (unversioned root path).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessStatus {
    /// `ready` or `notReady`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `GET /api/v1/version` — 200 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    /// REST API contract version, aligned with the gateway package version.
    pub api_version: String,
    /// GMP protocol version reported by the proxied gvmd.
    pub gmp_version: String,
}

/// `POST /api/v1/session` — 201 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreated {
    /// Opaque bearer token for the new session.
    pub session_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gmp_version: Option<String>,
}

/// Minimal reference to a related resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `201 Created` envelope returned by all create and clone endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceCreated {
    pub id: String,
}

/// A scan target as served by `GET /api/v1/targets[/{id}]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_hosts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alive_test: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_list: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_lookup_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_lookup_unify: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_credential: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smb_credential: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub esxi_credential: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snmp_credential: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_use: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writable: Option<bool>,
}

/// `GET /api/v1/targets` — 200 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetList {
    pub data: Vec<Target>,
    pub pagination: Pagination,
}

/// A scan task (or audit — same shape) as served by
/// `GET /api/v1/tasks[/{id}]` and `GET /api/v1/audits[/{id}]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Lifecycle status as reported by gvmd (`New`, `Running`, `Done`, …).
    /// Unknown future gvmd values are preserved.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_group: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oci_image_target: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_application_target: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan_config: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scanner: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alerts: Option<Vec<ResourceRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alterable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosts_ordering: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observers: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_periods: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_report: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_report: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_use: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writable: Option<bool>,
}

/// `GET /api/v1/tasks` — 200 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskList {
    pub data: Vec<Task>,
    pub pagination: Pagination,
}

/// `POST /api/v1/tasks|audits/{id}/start|resume` — 200 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAction {
    /// UUID of the report created by the start/resume action.
    pub report_id: String,
}

/// Shared pagination envelope used by all gateway list endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    pub page: u32,
    pub per_page: u32,
    /// Total number of resources matching the query.
    pub total: u64,
    pub total_pages: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_info_matches_spec_example() {
        let json = r#"{"apiVersion": "0.5.0", "gmpVersion": "22.8"}"#;
        let parsed: VersionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.api_version, "0.5.0");
        assert_eq!(parsed.gmp_version, "22.8");
    }

    #[test]
    fn readiness_matches_spec_shape() {
        let parsed: ReadinessStatus =
            serde_json::from_str(r#"{"status": "notReady", "reason": "gvmd down"}"#).unwrap();
        assert_eq!(parsed.status, "notReady");
        assert_eq!(parsed.reason.as_deref(), Some("gvmd down"));
    }

    #[test]
    fn target_matches_spec_shape() {
        let json = r#"{
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "name": "webservers",
            "hosts": ["192.168.1.0/24"],
            "excludeHosts": ["192.168.1.1"],
            "aliveTest": "ICMP Ping",
            "portList": {"id": "pl-1", "name": "All TCP"},
            "inUse": false,
            "writable": true
        }"#;
        let parsed: Target = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.hosts, vec!["192.168.1.0/24"]);
        assert_eq!(parsed.port_list.unwrap().name.as_deref(), Some("All TCP"));
    }

    #[test]
    fn task_matches_spec_shape_including_variants() {
        let json = r#"{
            "id": "t-1",
            "name": "weekly scan",
            "status": "Running",
            "progress": 42,
            "target": {"id": "tg-1", "name": "webservers"},
            "scanConfig": {"id": "sc-1", "name": "Full and fast"},
            "lastReport": {"id": "r-1"},
            "reportCount": 3,
            "usageType": "scan",
            "trend": "same",
            "observers": {"users": ["alice"], "groups": [], "roles": []},
            "webApplicationTarget": {"id": "wat-1", "name": "shop"}
        }"#;
        let parsed: Task = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.status, "Running");
        assert_eq!(parsed.progress, Some(42));
        assert_eq!(parsed.report_count, Some(3));
        assert_eq!(parsed.usage_type.as_deref(), Some("scan"));
        assert_eq!(
            parsed.web_application_target.unwrap().name.as_deref(),
            Some("shop")
        );
    }

    #[test]
    fn pagination_matches_spec_example() {
        let json = r#"{"page": 1, "perPage": 25, "total": 142, "totalPages": 6}"#;
        let parsed: Pagination = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.per_page, 25);
        assert_eq!(parsed.total, 142);
    }
}
