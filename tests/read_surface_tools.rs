//! Mock-gateway tests for the phase-2 read surface: every list tool is
//! exercised against a spec-shaped fixture and must return exactly its
//! summarized keys plus pagination; gets pass the gateway JSON through.

mod support;

use gvm_mcp::mcp::GvmMcpServer;
use gvm_mcp::mcp::tools::common::PageParams;
use gvm_mcp::mcp::tools::common::{GetByIdParams, ListParams};
use gvm_mcp::mcp::tools::feeds::ListFeedsParams;
use gvm_mcp::mcp::tools::nvts::{GetNvtParams, SearchNvtsParams};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use support::{config_for, problem_response};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn json_of(result: &CallToolResult) -> serde_json::Value {
    let text = result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(&text).expect("tool output must be JSON")
}

async fn server_with_login() -> (MockServer, GvmMcpServer) {
    let server = MockServer::start().await;
    let mcp = GvmMcpServer::new(config_for(&server)).unwrap();
    (server, mcp)
}

fn pagination() -> serde_json::Value {
    serde_json::json!({"page": 1, "perPage": 25, "total": 1, "totalPages": 1})
}

/// Mount a list endpoint fixture returning one spec-shaped row.
async fn mount_list(server: &MockServer, resource: &str, row: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/{resource}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [row],
            "pagination": pagination()
        })))
        .mount(server)
        .await;
}

/// Assert the summarized row keeps `kept` keys and drops `dropped`.
fn assert_row(result: &CallToolResult, out_key: &str, kept: &[&str], dropped: &[&str]) {
    let json = json_of(result);
    let row = &json[out_key][0];
    for key in kept {
        assert!(!row[*key].is_null(), "expected key '{key}' in {row}");
    }
    for key in dropped {
        assert!(
            row.get(*key).is_none(),
            "key '{key}' must be summarized away, got {row}"
        );
    }
    assert_eq!(json["pagination"]["total"], 1, "pagination must survive");
}

#[tokio::test]
async fn scan_configs_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "scan-configs",
        serde_json::json!({
            "id": "sc-1", "name": "Full and fast", "comment": "default",
            "familyCount": 60, "nvtCount": 90000, "type": "0",
            "inUse": true, "writable": false
        }),
    )
    .await;
    let result = mcp
        .list_scan_configs(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(
        &result,
        "scanConfigs",
        &["id", "name", "familyCount", "nvtCount"],
        &["writable"],
    );
}

#[tokio::test]
async fn scanners_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "scanners",
        serde_json::json!({
            "id": "s-1", "name": "OpenVAS Default", "host": "/run/ospd.sock",
            "port": 0, "type": "OpenVAS Scanner"
        }),
    )
    .await;
    let result = mcp
        .list_scanners(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(&result, "scanners", &["id", "name", "host", "type"], &[]);
}

#[tokio::test]
async fn schedules_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "schedules",
        serde_json::json!({
            "id": "sch-1", "name": "nightly", "icalendar": "BEGIN:VCALENDAR...",
            "timezone": "UTC", "firstRun": "2026-08-10T02:00:00Z",
            "nextRun": "2026-08-11T02:00:00Z", "inUse": false, "writable": true
        }),
    )
    .await;
    let result = mcp
        .list_schedules(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(
        &result,
        "schedules",
        &["id", "name", "timezone", "nextRun"],
        &["icalendar"],
    );
}

#[tokio::test]
async fn credential_stores_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/credential-stores"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "id": "default", "name": "gvmd", "provider": "gvmd",
                "default": true, "writable": true
            }]
        })))
        .mount(&server)
        .await;

    let result = mcp.list_credential_stores().await.unwrap();
    let json = json_of(&result);
    assert_eq!(json["credentialStores"][0]["provider"], "gvmd");
    assert_eq!(json["credentialStores"][0]["default"], true);
    // CredentialStoreList has no pagination envelope per the spec.
    assert!(json.get("pagination").is_none());
}

#[tokio::test]
async fn credentials_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "credentials",
        serde_json::json!({
            "id": "c-1", "name": "root-ssh", "type": "up",
            "login": "root", "inUse": true, "writable": true
        }),
    )
    .await;
    let result = mcp
        .list_credentials(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(
        &result,
        "credentials",
        &["id", "name", "type", "login"],
        &["writable"],
    );
}

#[tokio::test]
async fn alerts_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "alerts",
        serde_json::json!({
            "id": "a-1", "name": "mail on done", "event": "Task run status changed",
            "condition": "Always", "method": "Email",
            "eventData": {"status": "Done"}, "methodData": {"to_address": "x@y"},
            "inUse": false
        }),
    )
    .await;
    let result = mcp
        .list_alerts(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(
        &result,
        "alerts",
        &["id", "name", "event", "method"],
        &["eventData", "methodData"],
    );
}

#[tokio::test]
async fn port_lists_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "port-lists",
        serde_json::json!({
            "id": "pl-1", "name": "All TCP", "portCount": 65535,
            "tcpCount": 65535, "udpCount": 0, "inUse": true
        }),
    )
    .await;
    let result = mcp
        .list_port_lists(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(&result, "portLists", &["id", "name", "portCount"], &[]);
}

#[tokio::test]
async fn results_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "results",
        serde_json::json!({
            "id": "r-1", "name": "OpenSSH < 8.0", "host": "10.0.0.5",
            "port": "22/tcp", "severity": 7.5, "threat": "High",
            "description": "very long finding text...",
            "nvt": {"oid": "1.3.6.1", "name": "OpenSSH check"},
            "occurrences": 2
        }),
    )
    .await;
    let result = mcp
        .list_results(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(
        &result,
        "results",
        &["id", "host", "severity", "threat"],
        &["description", "nvt"],
    );
}

#[tokio::test]
async fn reports_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "reports",
        serde_json::json!({
            "id": "rep-1", "task": {"id": "t-1", "name": "weekly"},
            "scanStart": "2026-08-09T00:00:00Z", "scanEnd": "2026-08-09T01:00:00Z",
            "severity": 9.8,
            "resultCount": {"total": 40, "high": 3, "medium": 10, "low": 20, "log": 7, "debug": 0, "falsePositive": 0},
            "results": [{"id": "r-1"}]
        }),
    )
    .await;
    let result = mcp
        .list_reports(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(
        &result,
        "reports",
        &["id", "task", "severity", "resultCount"],
        &["results"],
    );
}

#[tokio::test]
async fn asset_hosts_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "hosts",
        serde_json::json!({
            "id": "h-1", "name": "10.0.0.5", "ip": "10.0.0.5",
            "hostname": "web01", "severity": 7.5, "os": "Linux",
            "creationTime": "2026-08-01T00:00:00Z"
        }),
    )
    .await;
    let result = mcp
        .list_asset_hosts(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(
        &result,
        "hosts",
        &["id", "ip", "hostname", "severity", "os"],
        &["creationTime"],
    );
}

#[tokio::test]
async fn report_formats_list_summarizes() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "report-formats",
        serde_json::json!({
            "id": "rf-1", "name": "PDF", "contentType": "application/pdf",
            "extension": "pdf", "active": true, "predefined": true,
            "summary": "Portable Document Format"
        }),
    )
    .await;
    let result = mcp
        .list_report_formats(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(
        &result,
        "reportFormats",
        &["id", "name", "contentType", "extension"],
        &["summary"],
    );
}

#[tokio::test]
async fn filters_tags_notes_overrides_summarize() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "filters",
        serde_json::json!({"id": "f-1", "name": "high sev", "type": "result", "term": "severity>7"}),
    )
    .await;
    mount_list(
        &server,
        "tags",
        serde_json::json!({"id": "tag-1", "name": "env", "value": "prod", "resourceType": "target", "resourceCount": 4, "active": true}),
    )
    .await;
    mount_list(
        &server,
        "notes",
        serde_json::json!({"id": "n-1", "text": "accepted risk", "nvt": {"oid": "1.3"}, "hosts": ["10.0.0.5"], "port": "22/tcp", "severity": 7.5, "active": true, "creationTime": "x"}),
    )
    .await;
    mount_list(
        &server,
        "overrides",
        serde_json::json!({"id": "o-1", "text": "false positive", "nvt": {"oid": "1.3"}, "hosts": ["10.0.0.5"], "severity": 7.5, "newSeverity": 0.0, "active": true, "creationTime": "x"}),
    )
    .await;

    let filters = mcp
        .list_filters(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(&filters, "filters", &["id", "name", "term"], &[]);

    let tags = mcp
        .list_tags(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(&tags, "tags", &["id", "name", "value", "resourceType"], &[]);

    let notes = mcp
        .list_notes(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(&notes, "notes", &["id", "text", "hosts"], &["creationTime"]);

    let overrides = mcp
        .list_overrides(Parameters(ListParams::default()))
        .await
        .unwrap();
    assert_row(
        &overrides,
        "overrides",
        &["id", "text", "newSeverity"],
        &["creationTime"],
    );
}

#[tokio::test]
async fn nvts_search_get_and_families() {
    let (server, mcp) = server_with_login().await;
    mount_list(
        &server,
        "nvts",
        serde_json::json!({
            "oid": "1.3.6.1.4.1.25623.1.0.100315", "name": "OpenSSH check",
            "family": "Gain a shell remotely", "severity": 7.5,
            "cvssBase": "7.5", "solutionType": "VendorFix",
            "tags": "cvss_base_vector=AV:N..."
        }),
    )
    .await;
    mount_list(
        &server,
        "nvt-families",
        serde_json::json!({"name": "Gain a shell remotely", "maxNvtCount": 1234}),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/nvts/1.3.6.1.4.1.25623.1.0.100315"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "oid": "1.3.6.1.4.1.25623.1.0.100315", "name": "OpenSSH check",
            "tags": "full tag string"
        })))
        .mount(&server)
        .await;

    let nvts = mcp
        .search_nvts(Parameters(SearchNvtsParams::default()))
        .await
        .unwrap();
    assert_row(
        &nvts,
        "nvts",
        &["oid", "name", "family", "severity"],
        &["tags"],
    );

    let families = mcp
        .list_nvt_families(Parameters(PageParams::default()))
        .await
        .unwrap();
    assert_row(&families, "families", &["name", "maxNvtCount"], &[]);

    let nvt = mcp
        .get_nvt(Parameters(GetNvtParams {
            oid: "1.3.6.1.4.1.25623.1.0.100315".into(),
        }))
        .await
        .unwrap();
    assert_eq!(json_of(&nvt)["tags"], "full tag string");
}

#[tokio::test]
async fn feeds_list_handles_missing_pagination() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/feeds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"type": "NVT", "name": "Greenbone Community Feed", "version": "202608090510", "currentlySyncing": false}]
        })))
        .mount(&server)
        .await;

    let result = mcp
        .list_feeds(Parameters(ListFeedsParams::default()))
        .await
        .unwrap();
    let json = json_of(&result);
    assert_eq!(json["feeds"][0]["type"], "NVT");
    assert!(json.get("pagination").is_none());
}

#[tokio::test]
async fn get_passthrough_returns_full_gateway_json() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/scan-configs/sc-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "sc-1", "name": "Full and fast",
            "familyCount": 60, "nvtCount": 90000,
            "someFutureField": {"nested": true}
        })))
        .mount(&server)
        .await;

    let result = mcp
        .get_scan_config(Parameters(GetByIdParams { id: "sc-1".into() }))
        .await
        .unwrap();
    let json = json_of(&result);
    assert_eq!(json["someFutureField"]["nested"], true);
}

#[tokio::test]
async fn read_tool_404_is_a_legible_tool_error() {
    let (server, mcp) = server_with_login().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/schedules/missing"))
        .respond_with(problem_response(404, "not_found", "Resource Not Found"))
        .mount(&server)
        .await;

    let result = mcp
        .get_schedule(Parameters(GetByIdParams {
            id: "missing".into(),
        }))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
}
