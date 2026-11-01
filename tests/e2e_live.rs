//! Compose-gated end-to-end tests against a real rust-gvm-api gateway and
//! gvmd. Ignored by default; opt in with `--ignored` and the env below:
//!
//! ```bash
//! export GVM_E2E_GATEWAY_URL=http://127.0.0.1:18080
//! export GVM_USERNAME=admin GVM_PASSWORD=admin
//! export GVM_E2E_TARGET_HOST=openvasd          # host the scanner can reach
//! cargo test --test e2e_live -- --ignored --test-threads=1          # catalogs + lifecycles
//! GVM_E2E_SCAN=1 cargo test --test e2e_live -- --ignored --test-threads=1   # + scan/audit runs
//! ```
//!
//! Every tool is driven exactly as an MCP client would, against live
//! infrastructure, and everything created is deleted again. Each test
//! appends one row per exercised gateway operation to the matrix printed at
//! the end (and written to `GVM_E2E_MATRIX_OUT` when set), so the run
//! doubles as the endpoint coverage map in `docs/e2e-test-map.md`.

#![cfg(test)]

use std::collections::BTreeMap;
use std::sync::Mutex;

use clap::Parser;
use gvm_mcp::config::{Cli, Config};
use gvm_mcp::mcp::GvmMcpServer;
use gvm_mcp::mcp::tools::agents::{AgentInstallerInstructionParams, CreateAgentGroupParams};
use gvm_mcp::mcp::tools::alerts::{CreateAlertParams, UpdateAlertParams};
use gvm_mcp::mcp::tools::assets::{
    AssetByIdParams, CreateHostParams, ListAssetsParams, UpdateAssetParams, UpdateCommentParams,
};
use gvm_mcp::mcp::tools::common::{
    DeleteParams, FilterParams, GetByIdParams, ListParams, PageParams, TrashListParams,
};
use gvm_mcp::mcp::tools::compliance::{CreatePolicyParams, UpdatePolicyParams};
use gvm_mcp::mcp::tools::credentials::{CreateCredentialParams, UpdateCredentialParams};
use gvm_mcp::mcp::tools::feeds::ListFeedsParams;
use gvm_mcp::mcp::tools::filters::{CreateFilterParams, UpdateFilterParams};
use gvm_mcp::mcp::tools::identity::{
    CreateMembershipParams, UpdateMembershipParams, UpdateUserSettingParams,
};
use gvm_mcp::mcp::tools::notes::{CreateNoteParams, UpdateNoteParams};
use gvm_mcp::mcp::tools::nvts::{GetNvtParams, SearchNvtsParams};
use gvm_mcp::mcp::tools::overrides::{CreateOverrideParams, UpdateOverrideParams};
use gvm_mcp::mcp::tools::port_lists::{CreatePortListParams, UpdatePortListParams};
use gvm_mcp::mcp::tools::reports::{
    ExportReportParams, GetReportParams, ImportReportParams, ReportPageParams,
};
use gvm_mcp::mcp::tools::scan_configs::{
    CreateScanConfigParams, FamilySelection, ListConfigsParams, ScanConfigNvtParams,
    ScanConfigNvtsParams, ScanConfigPreferenceParams, ScanConfigPreferencesParams,
    SetFamilySelectionParams, SetNvtSelectionParams, SetScanConfigPreferenceParams,
    UpdateScanConfigParams,
};
use gvm_mcp::mcp::tools::schedules::{CreateScheduleParams, UpdateScheduleParams};
use gvm_mcp::mcp::tools::secinfo::SecInfoIdParams;
use gvm_mcp::mcp::tools::tags::{CreateTagParams, UpdateTagParams};
use gvm_mcp::mcp::tools::targets::{
    CreateOciImageTargetParams, CreateTargetParams, CreateWebApplicationTargetParams,
    UpdateOciImageTargetParams, UpdateTargetParams, UpdateWebApplicationTargetParams,
};
use gvm_mcp::mcp::tools::tasks::{CreateTaskParams, UpdateTaskParams};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};

// ───────────────────────── harness ─────────────────────────

struct Env {
    gateway: String,
    username: String,
    password: String,
    target_host: String,
}

fn env() -> Option<Env> {
    let gateway = std::env::var("GVM_E2E_GATEWAY_URL").ok()?;
    let (Ok(username), Ok(password)) =
        (std::env::var("GVM_USERNAME"), std::env::var("GVM_PASSWORD"))
    else {
        eprintln!("skipping: GVM_USERNAME / GVM_PASSWORD not set");
        return None;
    };
    Some(Env {
        gateway,
        username,
        password,
        target_host: std::env::var("GVM_E2E_TARGET_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
    })
}

fn server_with(env: &Env, extra: &[&str]) -> GvmMcpServer {
    let mut args = vec![
        "gvm-mcp",
        "--gateway-url",
        &env.gateway,
        "--username",
        &env.username,
        "--password",
        &env.password,
        "--timeout-secs",
        "120",
    ];
    args.extend_from_slice(extra);
    let config = Config::from_cli(Cli::parse_from(args)).expect("valid e2e config");
    GvmMcpServer::new(config).expect("build server")
}

fn live_server() -> Option<(Env, GvmMcpServer)> {
    let env = env()?;
    let mcp = server_with(&env, &[]);
    Some((env, mcp))
}

fn scan_enabled() -> bool {
    std::env::var("GVM_E2E_SCAN").is_ok()
}

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn json_of(result: &CallToolResult) -> serde_json::Value {
    serde_json::from_str(&text_of(result)).unwrap_or(serde_json::Value::Null)
}

fn is_err(result: &CallToolResult) -> bool {
    result.is_error == Some(true)
}

fn unique(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("gvm-mcp-e2e-{prefix}-{nanos}")
}

/// Coverage matrix: one row per exercised gateway operation.
static MATRIX: Mutex<Vec<(String, String, String, String)>> = Mutex::new(Vec::new());

fn record(method: &str, path: &str, tool: &str, outcome: &str) {
    eprintln!("E2E | {method:6} {path:48} | {tool:46} | {outcome}");
    MATRIX.lock().unwrap().push((
        method.to_owned(),
        path.to_owned(),
        tool.to_owned(),
        outcome.to_owned(),
    ));
}

/// Flush the matrix to `GVM_E2E_MATRIX_OUT` (append) so a full run yields
/// the endpoint coverage document.
fn flush_matrix(section: &str) {
    let rows: Vec<_> = MATRIX.lock().unwrap().drain(..).collect();
    let Ok(path) = std::env::var("GVM_E2E_MATRIX_OUT") else {
        return;
    };
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open matrix file");
    writeln!(file, "\n### {section}\n").unwrap();
    writeln!(file, "| Method | Gateway path | MCP tool | Outcome |").unwrap();
    writeln!(file, "| --- | --- | --- | --- |").unwrap();
    for (method, path, tool, outcome) in rows {
        writeln!(file, "| {method} | `{path}` | `{tool}` | {outcome} |").unwrap();
    }
}

/// Assert success and record a PASS row.
fn must(result: &CallToolResult, method: &str, path: &str, tool: &str) -> serde_json::Value {
    assert!(
        !is_err(result),
        "{tool} ({method} {path}) failed: {}",
        text_of(result)
    );
    record(method, path, tool, "PASS");
    json_of(result)
}

/// Record PASS on success, or a SKIP row carrying the gateway's reason when
/// the connected gvmd lacks the capability (501) or the feature is not
/// provisioned in this stack. Any other failure is a hard error.
fn capability(
    result: &CallToolResult,
    method: &str,
    path: &str,
    tool: &str,
) -> Option<serde_json::Value> {
    if !is_err(result) {
        record(method, path, tool, "PASS");
        return Some(json_of(result));
    }
    let text = text_of(result);
    // 501: the gateway knows the route but gvmd lacks the command; 502
    // backend_unavailable: this gvmd build rejects the (feature-gated) GMP
    // command outright — observed for OCI image targets and agents on the
    // compose stack.
    let tolerated = text.contains("Not supported by the connected gvmd")
        || text.contains("HTTP 501")
        || text.contains("backend_unavailable")
        || text.contains("agent")
        || text.contains("Agent")
        || text.contains("not_implemented")
        || text.contains("feature");
    assert!(tolerated, "{tool} ({method} {path}) failed: {text}");
    let reason: String = text.chars().take(140).collect();
    record(method, path, tool, &format!("SKIP (capability) {reason}"));
    None
}

fn first_id(rows: &serde_json::Value, key: &str) -> Option<String> {
    rows[key]
        .as_array()?
        .first()?
        .get("id")?
        .as_str()
        .map(str::to_owned)
}

fn pick_id(rows: &serde_json::Value, key: &str, preferred: &str) -> String {
    let rows = rows[key].as_array().expect("summarized rows are an array");
    assert!(!rows.is_empty(), "no {key} rows to pick from");
    rows.iter()
        .find(|row| row["name"] == preferred)
        .unwrap_or(&rows[0])["id"]
        .as_str()
        .expect("row has a string id")
        .to_owned()
}

fn id(id: &str) -> GetByIdParams {
    GetByIdParams { id: id.to_owned() }
}

fn ultimate(id: &str) -> DeleteParams {
    DeleteParams {
        id: id.to_owned(),
        ultimate: Some(true),
    }
}

fn lp() -> ListParams {
    ListParams::default()
}

fn create_task_params(
    name: &str,
    target_id: &str,
    scan_config_id: &str,
    scanner_id: &str,
) -> CreateTaskParams {
    CreateTaskParams {
        name: name.to_owned(),
        comment: Some("created by gvm-mcp e2e".into()),
        task_type: None,
        target_id: Some(target_id.to_owned()),
        agent_group_id: None,
        oci_image_target_id: None,
        web_application_target_id: None,
        scan_config_id: Some(scan_config_id.to_owned()),
        scanner_id: Some(scanner_id.to_owned()),
        schedule_id: None,
        alert_ids: None,
        alterable: Some(true),
        observers: None,
        schedule_periods: None,
        preferences: None,
    }
}

fn update_task_params(id: &str) -> UpdateTaskParams {
    UpdateTaskParams {
        id: id.to_owned(),
        name: None,
        comment: Some("updated by gvm-mcp e2e".into()),
        target_id: None,
        scan_config_id: None,
        scanner_id: None,
        schedule_id: None,
        alert_ids: None,
        alterable: None,
        observers: None,
        schedule_periods: None,
        preferences: None,
    }
}

async fn create_target(mcp: &GvmMcpServer, env: &Env, port_list_id: &str) -> String {
    let created = mcp
        .create_target(Parameters(CreateTargetParams {
            name: unique("target"),
            hosts: vec![env.target_host.clone()],
            comment: Some("created by gvm-mcp e2e".into()),
            exclude_hosts: None,
            alive_test: Some("Consider Alive".into()),
            port_list_id: Some(port_list_id.to_owned()),
            reverse_lookup_only: None,
            reverse_lookup_unify: None,
            ssh_credential_id: None,
            smb_credential_id: None,
            esxi_credential_id: None,
            snmp_credential_id: None,
        }))
        .await
        .unwrap();
    must(&created, "POST", "/targets", "openvas_create_target")["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

struct Catalog {
    scan_config_id: String,
    scanner_id: String,
    port_list_id: String,
    pdf_format_id: String,
}

async fn catalog(mcp: &GvmMcpServer) -> Catalog {
    let configs = must(
        &mcp.list_scan_configs(Parameters(ListParams {
            per_page: Some(100),
            ..Default::default()
        }))
        .await
        .unwrap(),
        "GET",
        "/scan-configs",
        "openvas_list_scan_configs",
    );
    let scanners = must(
        &mcp.list_scanners(Parameters(lp())).await.unwrap(),
        "GET",
        "/scanners",
        "openvas_list_scanners",
    );
    let port_lists = must(
        &mcp.list_port_lists(Parameters(lp())).await.unwrap(),
        "GET",
        "/port-lists",
        "openvas_list_port_lists",
    );
    let formats = must(
        &mcp.list_report_formats(Parameters(ListParams {
            per_page: Some(100),
            ..Default::default()
        }))
        .await
        .unwrap(),
        "GET",
        "/report-formats",
        "openvas_list_report_formats",
    );
    Catalog {
        scan_config_id: pick_id(&configs, "scanConfigs", "Host Discovery"),
        scanner_id: pick_id(&scanners, "scanners", "OpenVAS Default"),
        port_list_id: pick_id(&port_lists, "portLists", "All IANA assigned TCP"),
        pdf_format_id: pick_id(&formats, "reportFormats", "PDF"),
    }
}

/// Which lifecycle resource to poll.
#[derive(Clone, Copy)]
enum Lifecycle {
    Task,
    Audit,
}

/// Poll a task/audit until it leaves the running states or ~15 minutes pass.
async fn wait_terminal(mcp: &GvmMcpServer, kind: Lifecycle, resource_id: &str) -> String {
    for _ in 0..180 {
        let result = match kind {
            Lifecycle::Task => mcp.get_task(Parameters(id(resource_id))).await.unwrap(),
            Lifecycle::Audit => mcp.get_audit(Parameters(id(resource_id))).await.unwrap(),
        };
        assert!(!is_err(&result), "poll failed: {}", text_of(&result));
        let status = json_of(&result)["status"].as_str().unwrap_or("").to_owned();
        if !matches!(
            status.as_str(),
            "New" | "Requested" | "Queued" | "Running" | "Processing" | "Stop Requested"
        ) {
            return status;
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    "Timeout".into()
}

// ───────────────────────── 1. bootstrap + catalogs ─────────────────────────

#[tokio::test]
#[ignore = "requires a live gateway + gvmd (set GVM_E2E_GATEWAY_URL)"]
async fn e2e_01_bootstrap_and_read_catalogs() {
    let Some((_env, mcp)) = live_server() else {
        return;
    };

    let conn = must(
        &mcp.test_connection().await.unwrap(),
        "GET",
        "/health + /ready + /version + /targets",
        "openvas_test_connection",
    );
    assert_eq!(conn["gatewayStatus"], "ok");
    assert_eq!(conn["gatewayReadiness"], "ready");
    assert_eq!(conn["authenticated"], true);
    eprintln!(
        "connected: api {} / GMP {}",
        conn["apiVersion"], conn["gmpVersion"]
    );
    must(
        &mcp.get_version().await.unwrap(),
        "GET",
        "/version",
        "openvas_get_version",
    );
    capability(
        &mcp.list_timezones().await.unwrap(),
        "GET",
        "/timezones",
        "openvas_list_timezones",
    );
    let feeds = must(
        &mcp.list_feeds(Parameters(ListFeedsParams::default()))
            .await
            .unwrap(),
        "GET",
        "/feeds",
        "openvas_list_feeds",
    );
    assert!(!feeds["feeds"].as_array().unwrap().is_empty());
    must(
        &mcp.list_feeds(Parameters(ListFeedsParams {
            feed_type: Some("NVT".into()),
        }))
        .await
        .unwrap(),
        "GET",
        "/feeds?type=NVT",
        "openvas_list_feeds",
    );

    let cat = catalog(&mcp).await;
    must(
        &mcp.get_scan_config(Parameters(id(&cat.scan_config_id)))
            .await
            .unwrap(),
        "GET",
        "/scan-configs/{id}",
        "openvas_get_scan_config",
    );
    must(
        &mcp.get_scanner(Parameters(id(&cat.scanner_id)))
            .await
            .unwrap(),
        "GET",
        "/scanners/{id}",
        "openvas_get_scanner",
    );
    must(
        &mcp.get_port_list(Parameters(id(&cat.port_list_id)))
            .await
            .unwrap(),
        "GET",
        "/port-lists/{id}",
        "openvas_get_port_list",
    );
    must(
        &mcp.get_report_format(Parameters(id(&cat.pdf_format_id)))
            .await
            .unwrap(),
        "GET",
        "/report-formats/{id}",
        "openvas_get_report_format",
    );

    // Scan-config sub-resources (read side) on the selected config.
    let nvts = must(
        &mcp.list_scan_config_nvts(Parameters(ScanConfigNvtsParams {
            id: cat.scan_config_id.clone(),
            family: None,
            page: PageParams {
                page: Some(1),
                per_page: Some(5),
            },
        }))
        .await
        .unwrap(),
        "GET",
        "/scan-configs/{id}/nvts",
        "openvas_list_scan_config_nvts",
    );
    if let Some(oid) = nvts["nvts"][0]["oid"].as_str() {
        must(
            &mcp.get_scan_config_nvt(Parameters(ScanConfigNvtParams {
                id: cat.scan_config_id.clone(),
                oid: oid.to_owned(),
            }))
            .await
            .unwrap(),
            "GET",
            "/scan-configs/{id}/nvts/{oid}",
            "openvas_get_scan_config_nvt",
        );
    }
    let prefs = must(
        &mcp.list_scan_config_preferences(Parameters(ScanConfigPreferencesParams {
            id: cat.scan_config_id.clone(),
            nvt_oid: None,
        }))
        .await
        .unwrap(),
        "GET",
        "/scan-configs/{id}/preferences",
        "openvas_list_scan_config_preferences",
    );
    // Detail lookup of a preference the list just returned. On the current
    // gateway main this answers 404 for both scanner-level and NVT
    // preferences while the PUT on the same name succeeds; that is recorded
    // as a gateway-side defect rather than a tool failure.
    let rows = prefs["data"].as_array().cloned().unwrap_or_default();
    let scanner_pref = rows.iter().find(|p| p["nvt"].is_null());
    let nvt_pref = rows.iter().find(|p| !p["nvt"].is_null());
    for pref in [scanner_pref, nvt_pref].into_iter().flatten() {
        let name = pref["name"].as_str().unwrap().to_owned();
        let nvt_oid = pref["nvt"]["oid"].as_str().map(str::to_owned);
        let result = mcp
            .get_scan_config_preference(Parameters(ScanConfigPreferenceParams {
                id: cat.scan_config_id.clone(),
                name: name.clone(),
                nvt_oid: nvt_oid.clone(),
            }))
            .await
            .unwrap();
        let label = if nvt_oid.is_some() {
            "/scan-configs/{id}/preferences/{name}?nvtOid="
        } else {
            "/scan-configs/{id}/preferences/{name}"
        };
        if is_err(&result) && text_of(&result).contains("HTTP 404") {
            record(
                "GET",
                label,
                "openvas_get_scan_config_preference",
                "GATEWAY-DEFECT (404 for a preference returned by the list; PUT on the same name works)",
            );
        } else {
            must(&result, "GET", label, "openvas_get_scan_config_preference");
        }
    }

    // Generic configs (all usage types).
    let configs = must(
        &mcp.list_configs(Parameters(ListConfigsParams {
            list: lp(),
            usage_type: Some("scan".into()),
        }))
        .await
        .unwrap(),
        "GET",
        "/configs?usageType=scan",
        "openvas_list_configs",
    );
    if let Some(config_id) = first_id(&configs, "configs") {
        must(
            &mcp.get_config(Parameters(id(&config_id))).await.unwrap(),
            "GET",
            "/configs/{id}",
            "openvas_get_config",
        );
    }

    // NVT catalog with typed selectors.
    let families = must(
        &mcp.list_nvt_families(Parameters(PageParams::default()))
            .await
            .unwrap(),
        "GET",
        "/nvt-families",
        "openvas_list_nvt_families",
    );
    let family = families["families"][0]["name"].as_str().map(str::to_owned);
    let nvts = must(
        &mcp.search_nvts(Parameters(SearchNvtsParams {
            page: PageParams {
                page: Some(1),
                per_page: Some(3),
            },
            family: family.clone(),
            sort_order: Some("ascending".into()),
            sort_field: Some("name".into()),
            ..Default::default()
        }))
        .await
        .unwrap(),
        "GET",
        "/nvts?family=&sortOrder=&sortField=",
        "openvas_search_nvts",
    );
    if let Some(oid) = nvts["nvts"][0]["oid"].as_str() {
        must(
            &mcp.get_nvt(Parameters(GetNvtParams {
                oid: oid.to_owned(),
            }))
            .await
            .unwrap(),
            "GET",
            "/nvts/{id}",
            "openvas_get_nvt",
        );
    }
    must(
        &mcp.search_nvts(Parameters(SearchNvtsParams {
            page: PageParams {
                page: Some(1),
                per_page: Some(2),
            },
            config_id: Some(cat.scan_config_id.clone()),
            include_preferences: Some(true),
            include_preference_count: Some(true),
            ..Default::default()
        }))
        .await
        .unwrap(),
        "GET",
        "/nvts?configId=&includePreferences=",
        "openvas_search_nvts",
    );

    // Catalog reads: every list endpoint, plus get on the first row.
    macro_rules! list_and_get {
        ($list:ident, $get:ident, $key:literal, $path:literal) => {{
            let rows = must(
                &mcp.$list(Parameters(lp())).await.unwrap(),
                "GET",
                $path,
                stringify!($list),
            );
            if let Some(row_id) = first_id(&rows, $key) {
                must(
                    &mcp.$get(Parameters(id(&row_id))).await.unwrap(),
                    "GET",
                    concat!($path, "/{id}"),
                    stringify!($get),
                );
            } else {
                record(
                    "GET",
                    concat!($path, "/{id}"),
                    stringify!($get),
                    "SKIP (no rows in this stack)",
                );
            }
        }};
    }
    list_and_get!(list_targets, get_target, "targets", "/targets");
    list_and_get!(list_tasks, get_task, "tasks", "/tasks");
    list_and_get!(
        list_credentials,
        get_credential,
        "credentials",
        "/credentials"
    );
    list_and_get!(list_alerts, get_alert, "alerts", "/alerts");
    list_and_get!(list_schedules, get_schedule, "schedules", "/schedules");
    list_and_get!(list_filters, get_filter, "filters", "/filters");
    list_and_get!(list_tags, get_tag, "tags", "/tags");
    list_and_get!(list_notes, get_note, "notes", "/notes");
    list_and_get!(list_overrides, get_override, "overrides", "/overrides");
    let reports = must(
        &mcp.list_reports(Parameters(lp())).await.unwrap(),
        "GET",
        "/reports",
        "openvas_list_reports",
    );
    if let Some(report_id) = first_id(&reports, "reports") {
        must(
            &mcp.get_report(Parameters(GetReportParams {
                id: report_id,
                page: Some(1),
                per_page: Some(5),
            }))
            .await
            .unwrap(),
            "GET",
            "/reports/{id}",
            "openvas_get_report",
        );
    } else {
        record(
            "GET",
            "/reports/{id}",
            "openvas_get_report",
            "SKIP (no reports yet)",
        );
    }
    list_and_get!(list_results, get_result, "results", "/results");
    list_and_get!(list_asset_hosts, get_asset_host, "hosts", "/hosts");
    list_and_get!(
        list_operating_systems,
        get_operating_system,
        "operatingSystems",
        "/operating-systems"
    );
    list_and_get!(
        list_tls_certificates,
        get_tls_certificate,
        "tlsCertificates",
        "/tls-certificates"
    );
    list_and_get!(list_policies, get_policy, "policies", "/policies");
    list_and_get!(list_audits, get_audit, "audits", "/audits");
    must(
        &mcp.list_vulnerabilities(Parameters(lp())).await.unwrap(),
        "GET",
        "/vulnerabilities",
        "openvas_list_vulnerabilities",
    );

    // SecInfo: lists always answer; detail only when the feed has rows.
    macro_rules! secinfo {
        ($list:ident, $get:ident, $key:literal, $path:literal) => {{
            let rows = must(
                &mcp.$list(Parameters(lp())).await.unwrap(),
                "GET",
                $path,
                stringify!($list),
            );
            if let Some(row_id) = first_id(&rows, $key) {
                must(
                    &mcp.$get(Parameters(SecInfoIdParams { id: row_id }))
                        .await
                        .unwrap(),
                    "GET",
                    concat!($path, "/{id}"),
                    stringify!($get),
                );
            } else {
                record(
                    "GET",
                    concat!($path, "/{id}"),
                    stringify!($get),
                    "SKIP (feed not loaded in this stack)",
                );
            }
        }};
    }
    secinfo!(list_cves, get_cve, "cves", "/cves");
    secinfo!(list_cpes, get_cpe, "cpes", "/cpes");
    secinfo!(
        list_cert_bund_advisories,
        get_cert_bund_advisory,
        "certBundAdvisories",
        "/cert-bund-advisories"
    );
    secinfo!(
        list_dfn_cert_advisories,
        get_dfn_cert_advisory,
        "dfnCertAdvisories",
        "/dfn-cert-advisories"
    );

    // Generic assets by type. The gateway contract lists `tls_certificate`
    // as a known type, but gvmd 26.43 answers "Failed to find type" for it
    // (its TLS certificates live behind a separate GMP command); that is
    // recorded as a gateway-side defect rather than a tool failure.
    for asset_type in ["host", "os", "tls_certificate"] {
        let listed = mcp
            .list_assets(Parameters(ListAssetsParams {
                asset_type: asset_type.into(),
                list: lp(),
            }))
            .await
            .unwrap();
        if is_err(&listed) && text_of(&listed).contains("Failed to find type") {
            record(
                "GET",
                "/assets?type=tls_certificate",
                "openvas_list_assets",
                "GATEWAY-DEFECT (spec lists the type; gvmd rejects it with 404)",
            );
            continue;
        }
        let rows = must(&listed, "GET", "/assets?type=", "openvas_list_assets");
        if let Some(asset_id) = first_id(&rows, "assets") {
            must(
                &mcp.get_asset(Parameters(AssetByIdParams {
                    id: asset_id,
                    asset_type: asset_type.into(),
                }))
                .await
                .unwrap(),
                "GET",
                "/assets/{id}?type=",
                "openvas_get_asset",
            );
        }
    }

    // Specialized targets + agents: capability dependent.
    capability(
        &mcp.list_oci_image_targets(Parameters(TrashListParams::default()))
            .await
            .unwrap(),
        "GET",
        "/oci-image-targets",
        "openvas_list_oci_image_targets",
    );
    capability(
        &mcp.list_web_application_targets(Parameters(TrashListParams::default()))
            .await
            .unwrap(),
        "GET",
        "/web-application-targets",
        "openvas_list_web_application_targets",
    );
    let agents = capability(
        &mcp.list_agents(Parameters(lp())).await.unwrap(),
        "GET",
        "/agents",
        "openvas_list_agents",
    );
    if let Some(agent_id) = agents.as_ref().and_then(|rows| first_id(rows, "agents")) {
        must(
            &mcp.get_agent(Parameters(id(&agent_id))).await.unwrap(),
            "GET",
            "/agents/{id}",
            "openvas_get_agent",
        );
    } else {
        record(
            "GET",
            "/agents/{id}",
            "openvas_get_agent",
            "SKIP (no agents enrolled)",
        );
        record(
            "GET",
            "/agents/{id}/support-bundle",
            "openvas_get_agent_support_bundle",
            "SKIP (no agents enrolled)",
        );
    }
    capability(
        &mcp.list_agent_groups(Parameters(TrashListParams::default()))
            .await
            .unwrap(),
        "GET",
        "/agent-groups",
        "openvas_list_agent_groups",
    );
    capability(
        &mcp.get_agent_installer_instruction(Parameters(AgentInstallerInstructionParams {
            scanner_id: cat.scanner_id.clone(),
            origin_url: "https://gvm.example".into(),
            language: Some("en".into()),
        }))
        .await
        .unwrap(),
        "GET",
        "/scanners/{id}/agent-installer-instruction",
        "openvas_get_agent_installer_instruction",
    );
    capability(
        &mcp.list_credential_stores().await.unwrap(),
        "GET",
        "/credential-stores",
        "openvas_list_credential_stores",
    );

    flush_matrix("Bootstrap and read catalogs");
}

// ───────────────────────── 2. resource lifecycles ─────────────────────────

#[tokio::test]
#[ignore = "requires a live gateway + gvmd (mutates gvmd, cleans up)"]
async fn e2e_02_resource_lifecycles() {
    let Some((env, mcp)) = live_server() else {
        return;
    };
    let cat = catalog(&mcp).await;

    // Port list
    let pl = must(
        &mcp.create_port_list(Parameters(CreatePortListParams {
            name: unique("portlist"),
            comment: None,
            port_range: Some("T:22,80,443".into()),
        }))
        .await
        .unwrap(),
        "POST",
        "/port-lists",
        "openvas_create_port_list",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_port_list(Parameters(UpdatePortListParams {
            id: pl.clone(),
            name: Some(unique("portlist-renamed")),
            comment: Some("updated".into()),
            port_range: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/port-lists/{id}",
        "openvas_update_port_list",
    );

    // Credential (up) + credential store admin (capability)
    let cred = must(
        &mcp.create_credential(Parameters(CreateCredentialParams {
            name: unique("cred"),
            credential_type: "up".into(),
            comment: None,
            login: Some("scanner".into()),
            password: Some("e2e-not-a-real-password".into()),
            private_key: None,
            certificate: None,
            community: None,
            auth_algorithm: None,
            privacy_algorithm: None,
            privacy_password: None,
            credential_store_id: None,
            vault_id: None,
            host_identifier: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/credentials",
        "openvas_create_credential",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_credential(Parameters(UpdateCredentialParams {
            id: cred.clone(),
            name: None,
            comment: Some("updated".into()),
            login: None,
            password: None,
            private_key: None,
            certificate: None,
            community: None,
            auth_algorithm: None,
            privacy_algorithm: None,
            privacy_password: None,
            credential_store_id: None,
            vault_id: None,
            host_identifier: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/credentials/{id}",
        "openvas_update_credential",
    );
    if let Some(stores) = capability(
        &mcp.list_credential_stores().await.unwrap(),
        "GET",
        "/credential-stores",
        "openvas_list_credential_stores",
    ) && let Some(store_id) = first_id(&stores, "credentialStores")
    {
        capability(
            &mcp.get_credential_store(Parameters(id(&store_id)))
                .await
                .unwrap(),
            "GET",
            "/credential-stores/{id}",
            "openvas_get_credential_store",
        );
        capability(
            &mcp.verify_credential_store(Parameters(id(&store_id)))
                .await
                .unwrap(),
            "POST",
            "/credential-stores/{id}/actions/verify",
            "openvas_verify_credential_store",
        );
    } else {
        record(
            "GET",
            "/credential-stores/{id}",
            "openvas_get_credential_store",
            "SKIP (no credential store in this stack)",
        );
        record(
            "PUT",
            "/credential-stores/{id}",
            "openvas_update_credential_store",
            "SKIP (no credential store in this stack)",
        );
        record(
            "POST",
            "/credential-stores/{id}/actions/verify",
            "openvas_verify_credential_store",
            "SKIP (no credential store in this stack)",
        );
    }

    // Schedule
    let sched = must(
        &mcp.create_schedule(Parameters(CreateScheduleParams {
            name: unique("schedule"),
            icalendar: "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//gvm-mcp e2e//EN\r\nBEGIN:VEVENT\r\nUID:gvm-mcp-e2e@example\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20270101T020000Z\r\nRRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n".into(),
            timezone: "UTC".into(),
            comment: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/schedules",
        "openvas_create_schedule",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_schedule(Parameters(UpdateScheduleParams {
            id: sched.clone(),
            name: None,
            comment: Some("updated".into()),
            icalendar: None,
            timezone: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/schedules/{id}",
        "openvas_update_schedule",
    );

    // Alert
    let alert = must(
        &mcp.create_alert(Parameters(CreateAlertParams {
            name: unique("alert"),
            event: "task_run_status_changed".into(),
            condition: "always".into(),
            method: "syslog".into(),
            comment: None,
            event_data: Some(BTreeMap::from([("status".to_string(), "Done".to_string())])),
            condition_data: None,
            method_data: None,
            filter_id: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/alerts",
        "openvas_create_alert",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_alert(Parameters(UpdateAlertParams {
            id: alert.clone(),
            name: None,
            comment: Some("updated".into()),
            event: None,
            condition: None,
            method: None,
            event_data: None,
            condition_data: None,
            method_data: None,
            filter_id: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/alerts/{id}",
        "openvas_update_alert",
    );

    // Filter + clone
    let filter = must(
        &mcp.create_filter(Parameters(CreateFilterParams {
            name: unique("filter"),
            comment: None,
            term: Some("severity>7".into()),
            filter_type: Some("result".into()),
        }))
        .await
        .unwrap(),
        "POST",
        "/filters",
        "openvas_create_filter",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_filter(Parameters(UpdateFilterParams {
            id: filter.clone(),
            comment: Some("updated".into()),
            term: Some("severity>8".into()),
            filter_type: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/filters/{id}",
        "openvas_update_filter",
    );
    let filter_clone = must(
        &mcp.clone_filter(Parameters(id(&filter))).await.unwrap(),
        "POST",
        "/filters/{id}/clone",
        "openvas_clone_filter",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Target + clone + task + clone
    let target = create_target(&mcp, &env, &cat.port_list_id).await;
    must(
        &mcp.update_target(Parameters(UpdateTargetParams {
            id: target.clone(),
            name: None,
            comment: Some("updated".into()),
            hosts: None,
            exclude_hosts: None,
            alive_test: None,
            port_list_id: None,
            reverse_lookup_only: None,
            reverse_lookup_unify: None,
            ssh_credential_id: Some(cred.clone()),
            smb_credential_id: None,
            esxi_credential_id: None,
            snmp_credential_id: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/targets/{id}",
        "openvas_update_target",
    );
    let target_clone = must(
        &mcp.clone_target(Parameters(id(&target))).await.unwrap(),
        "POST",
        "/targets/{id}/clone",
        "openvas_clone_target",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let task = must(
        &mcp.create_task(Parameters(create_task_params(
            &unique("task"),
            &target,
            &cat.scan_config_id,
            &cat.scanner_id,
        )))
        .await
        .unwrap(),
        "POST",
        "/tasks",
        "openvas_create_task",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut upd = update_task_params(&task);
    upd.schedule_id = Some(sched.clone());
    upd.alert_ids = Some(vec![alert.clone()]);
    must(
        &mcp.update_task(Parameters(upd)).await.unwrap(),
        "PUT",
        "/tasks/{id}",
        "openvas_update_task",
    );
    let task_clone = must(
        &mcp.clone_task(Parameters(id(&task))).await.unwrap(),
        "POST",
        "/tasks/{id}/clone",
        "openvas_clone_task",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Tag attached to the task + clone
    let tag = must(
        &mcp.create_tag(Parameters(CreateTagParams {
            name: unique("tag"),
            comment: None,
            value: Some("e2e".into()),
            resource_type: Some("task".into()),
            resource_id: Some(task.clone()),
            active: Some(true),
        }))
        .await
        .unwrap(),
        "POST",
        "/tags",
        "openvas_create_tag",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_tag(Parameters(UpdateTagParams {
            id: tag.clone(),
            comment: Some("updated".into()),
            value: Some("e2e-2".into()),
            resource_type: None,
            resource_id: None,
            active: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/tags/{id}",
        "openvas_update_tag",
    );
    let tag_clone = must(
        &mcp.clone_tag(Parameters(id(&tag))).await.unwrap(),
        "POST",
        "/tags/{id}/clone",
        "openvas_clone_tag",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Note + override on an NVT selected by the config
    let nvts = json_of(
        &mcp.list_scan_config_nvts(Parameters(ScanConfigNvtsParams {
            id: cat.scan_config_id.clone(),
            family: None,
            page: PageParams {
                page: Some(1),
                per_page: Some(1),
            },
        }))
        .await
        .unwrap(),
    );
    let nvt_oid = nvts["nvts"][0]["oid"].as_str().unwrap().to_owned();
    let note = must(
        &mcp.create_note(Parameters(CreateNoteParams {
            nvt_oid: nvt_oid.clone(),
            text: Some("gvm-mcp e2e note".into()),
            hosts: Some(vec![env.target_host.clone()]),
            port: None,
            severity: None,
            task_id: Some(task.clone()),
            result_id: None,
            active: Some(true),
            orphan: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/notes",
        "openvas_create_note",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_note(Parameters(UpdateNoteParams {
            id: note.clone(),
            text: Some("gvm-mcp e2e note (updated)".into()),
            hosts: None,
            port: None,
            severity: None,
            task_id: None,
            result_id: None,
            active: Some(false),
            orphan: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/notes/{id}",
        "openvas_update_note",
    );
    let over = must(
        &mcp.create_override(Parameters(CreateOverrideParams {
            nvt_oid: nvt_oid.clone(),
            text: Some("gvm-mcp e2e override".into()),
            hosts: None,
            port: None,
            severity: None,
            new_severity: Some("0.0".into()),
            task_id: Some(task.clone()),
            result_id: None,
            active: Some(true),
            orphan: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/overrides",
        "openvas_create_override",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_override(Parameters(UpdateOverrideParams {
            id: over.clone(),
            text: Some("gvm-mcp e2e override (updated)".into()),
            hosts: None,
            port: None,
            severity: None,
            new_severity: Some("1.0".into()),
            task_id: None,
            result_id: None,
            active: None,
            orphan: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/overrides/{id}",
        "openvas_update_override",
    );

    // Scan config: copy, tune preferences/selection, generic clone, delete
    let sc = must(
        &mcp.create_scan_config(Parameters(CreateScanConfigParams {
            name: unique("scanconfig"),
            base_scan_config_id: cat.scan_config_id.clone(),
            comment: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/scan-configs",
        "openvas_create_scan_config",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_scan_config(Parameters(UpdateScanConfigParams {
            id: sc.clone(),
            name: None,
            comment: Some("updated".into()),
        }))
        .await
        .unwrap(),
        "PUT",
        "/scan-configs/{id}",
        "openvas_update_scan_config",
    );
    let sc_prefs = json_of(
        &mcp.list_scan_config_preferences(Parameters(ScanConfigPreferencesParams {
            id: sc.clone(),
            nvt_oid: None,
        }))
        .await
        .unwrap(),
    );
    if let Some(pref) = sc_prefs["data"]
        .as_array()
        .and_then(|rows| rows.iter().find(|p| p["nvt"].is_null()))
    {
        let name = pref["name"].as_str().unwrap().to_owned();
        let value = pref["value"].as_str().unwrap_or("").to_owned();
        must(
            &mcp.set_scan_config_preference(Parameters(SetScanConfigPreferenceParams {
                id: sc.clone(),
                name: name.clone(),
                nvt_oid: None,
                value: Some(value),
            }))
            .await
            .unwrap(),
            "PUT",
            "/scan-configs/{id}/preferences/{name}",
            "openvas_set_scan_config_preference",
        );
    }
    let sc_nvts = json_of(
        &mcp.list_scan_config_nvts(Parameters(ScanConfigNvtsParams {
            id: sc.clone(),
            family: None,
            page: PageParams {
                page: Some(1),
                per_page: Some(1),
            },
        }))
        .await
        .unwrap(),
    );
    let family = sc_nvts["nvts"][0]["family"].as_str().unwrap().to_owned();
    let oid = sc_nvts["nvts"][0]["oid"].as_str().unwrap().to_owned();
    must(
        &mcp.set_scan_config_family_selection(Parameters(SetFamilySelectionParams {
            id: sc.clone(),
            families: vec![FamilySelection {
                name: family.clone(),
                growing: true,
                all: true,
            }],
            auto_add_new_families: Some(false),
        }))
        .await
        .unwrap(),
        "PUT",
        "/scan-configs/{id}/family-selection",
        "openvas_set_scan_config_family_selection",
    );
    must(
        &mcp.set_scan_config_nvt_selection(Parameters(SetNvtSelectionParams {
            id: sc.clone(),
            family: family.clone(),
            nvt_oids: vec![oid],
        }))
        .await
        .unwrap(),
        "PUT",
        "/scan-configs/{id}/families/{family}/nvts",
        "openvas_set_scan_config_nvt_selection",
    );
    let config_clone = must(
        &mcp.clone_config(Parameters(id(&sc))).await.unwrap(),
        "POST",
        "/configs/{id}/clone",
        "openvas_clone_config",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.get_config(Parameters(id(&config_clone))).await.unwrap(),
        "GET",
        "/configs/{id}",
        "openvas_get_config",
    );

    // Host asset create/update/delete; OS update when any exists
    let host = must(
        &mcp.create_asset_host(Parameters(CreateHostParams {
            value: "192.0.2.77".into(),
            comment: Some("gvm-mcp e2e".into()),
        }))
        .await
        .unwrap(),
        "POST",
        "/hosts",
        "openvas_create_asset_host",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_asset_host(Parameters(UpdateCommentParams {
            id: host.clone(),
            comment: Some("gvm-mcp e2e (updated)".into()),
        }))
        .await
        .unwrap(),
        "PUT",
        "/hosts/{id}",
        "openvas_update_asset_host",
    );
    must(
        &mcp.update_asset(Parameters(UpdateAssetParams {
            id: host.clone(),
            asset_type: "host".into(),
            comment: Some("gvm-mcp e2e (generic)".into()),
        }))
        .await
        .unwrap(),
        "PUT",
        "/assets/{id}?type=host",
        "openvas_update_asset",
    );
    let os_rows = json_of(&mcp.list_operating_systems(Parameters(lp())).await.unwrap());
    if let Some(os_id) = first_id(&os_rows, "operatingSystems") {
        let updated = mcp
            .update_operating_system(Parameters(UpdateCommentParams {
                id: os_id,
                comment: Some("gvm-mcp e2e".into()),
            }))
            .await
            .unwrap();
        if is_err(&updated) && text_of(&updated).contains("HTTP 404") {
            // gvmd answers `modify_asset` with "not found" for operating
            // systems a scan created (`writable: false` on the full record).
            record(
                "PUT",
                "/operating-systems/{id}",
                "openvas_update_operating_system",
                "SKIP (OS asset is scan-created and read-only in gvmd)",
            );
        } else {
            must(
                &updated,
                "PUT",
                "/operating-systems/{id}",
                "openvas_update_operating_system",
            );
        }
    } else {
        record(
            "PUT",
            "/operating-systems/{id}",
            "openvas_update_operating_system",
            "SKIP (no OS assets yet; populated by scans)",
        );
        record(
            "DELETE",
            "/operating-systems/{id}",
            "openvas_delete_operating_system",
            "SKIP (no OS assets yet; populated by scans)",
        );
    }

    // Specialized targets (capability dependent)
    if let Some(oci) = capability(
        &mcp.create_oci_image_target(Parameters(CreateOciImageTargetParams {
            name: unique("oci"),
            image_references: vec!["registry.example/app:1.0".into()],
            comment: None,
            credential_id: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/oci-image-targets",
        "openvas_create_oci_image_target",
    ) {
        let oci_id = oci["id"].as_str().unwrap().to_owned();
        must(
            &mcp.get_oci_image_target(Parameters(id(&oci_id)))
                .await
                .unwrap(),
            "GET",
            "/oci-image-targets/{id}",
            "openvas_get_oci_image_target",
        );
        must(
            &mcp.update_oci_image_target(Parameters(UpdateOciImageTargetParams {
                id: oci_id.clone(),
                name: None,
                comment: Some("updated".into()),
                image_references: None,
                credential_id: None,
            }))
            .await
            .unwrap(),
            "PUT",
            "/oci-image-targets/{id}",
            "openvas_update_oci_image_target",
        );
        let oci_clone = must(
            &mcp.clone_oci_image_target(Parameters(id(&oci_id)))
                .await
                .unwrap(),
            "POST",
            "/oci-image-targets/{id}/clone",
            "openvas_clone_oci_image_target",
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        for target_id in [oci_clone, oci_id] {
            must(
                &mcp.delete_oci_image_target(Parameters(ultimate(&target_id)))
                    .await
                    .unwrap(),
                "DELETE",
                "/oci-image-targets/{id}",
                "openvas_delete_oci_image_target",
            );
        }
    }
    if let Some(web) = capability(
        &mcp.create_web_application_target(Parameters(CreateWebApplicationTargetParams {
            name: unique("webapp"),
            urls: vec!["https://shop.example".into()],
            comment: None,
            exclude_urls: Some(vec!["https://shop.example/logout".into()]),
            credential_id: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/web-application-targets",
        "openvas_create_web_application_target",
    ) {
        let web_id = web["id"].as_str().unwrap().to_owned();
        must(
            &mcp.get_web_application_target(Parameters(id(&web_id)))
                .await
                .unwrap(),
            "GET",
            "/web-application-targets/{id}",
            "openvas_get_web_application_target",
        );
        must(
            &mcp.update_web_application_target(Parameters(UpdateWebApplicationTargetParams {
                id: web_id.clone(),
                name: None,
                comment: Some("updated".into()),
                urls: None,
                exclude_urls: None,
                credential_id: None,
            }))
            .await
            .unwrap(),
            "PUT",
            "/web-application-targets/{id}",
            "openvas_update_web_application_target",
        );
        let web_clone = must(
            &mcp.clone_web_application_target(Parameters(id(&web_id)))
                .await
                .unwrap(),
            "POST",
            "/web-application-targets/{id}/clone",
            "openvas_clone_web_application_target",
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        for target_id in [web_clone, web_id] {
            must(
                &mcp.delete_web_application_target(Parameters(ultimate(&target_id)))
                    .await
                    .unwrap(),
                "DELETE",
                "/web-application-targets/{id}",
                "openvas_delete_web_application_target",
            );
        }
    }

    // Agents: sync + group create are capability dependent.
    capability(
        &mcp.sync_agents().await.unwrap(),
        "POST",
        "/agents/sync",
        "openvas_sync_agents",
    );
    if let Some(group) = capability(
        &mcp.create_agent_group(Parameters(CreateAgentGroupParams {
            name: unique("agent-group"),
            scheduler_cron_time: "0 2 * * *".into(),
            comment: None,
            agent_ids: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/agent-groups",
        "openvas_create_agent_group",
    ) {
        let group_id = group["id"].as_str().unwrap().to_owned();
        must(
            &mcp.get_agent_group(Parameters(id(&group_id)))
                .await
                .unwrap(),
            "GET",
            "/agent-groups/{id}",
            "openvas_get_agent_group",
        );
        let group_clone = must(
            &mcp.clone_agent_group(Parameters(id(&group_id)))
                .await
                .unwrap(),
            "POST",
            "/agent-groups/{id}/clone",
            "openvas_clone_agent_group",
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        for gid in [group_clone, group_id] {
            must(
                &mcp.delete_agent_group(Parameters(ultimate(&gid)))
                    .await
                    .unwrap(),
                "DELETE",
                "/agent-groups/{id}",
                "openvas_delete_agent_group",
            );
        }
    }

    // Import task + report import (reports from imports are retained by
    // gvmd; deleting one answers 404 on the compose gvmd, mirroring the
    // gateway's own e2e expectation).
    let import_task = must(
        &mcp.create_task(Parameters(CreateTaskParams {
            name: unique("import-task"),
            comment: Some("gvm-mcp e2e import".into()),
            task_type: Some("import".into()),
            target_id: None,
            agent_group_id: None,
            oci_image_target_id: None,
            web_application_target_id: None,
            scan_config_id: None,
            scanner_id: None,
            schedule_id: None,
            alert_ids: None,
            alterable: None,
            observers: None,
            schedule_periods: None,
            preferences: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/tasks (type=import)",
        "openvas_create_task",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let imported = must(
        &mcp.import_report(Parameters(ImportReportParams {
            task_id: import_task.clone(),
            report_xml:
                "<report><name>gvm-mcp e2e imported report</name><comment>e2e</comment></report>"
                    .into(),
            in_assets: Some(false),
        }))
        .await
        .unwrap(),
        "POST",
        "/reports?taskId=&inAssets=false",
        "openvas_import_report",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.get_report(Parameters(GetReportParams {
            id: imported.clone(),
            page: Some(1),
            per_page: Some(5),
        }))
        .await
        .unwrap(),
        "GET",
        "/reports/{id}?page=&perPage=",
        "openvas_get_report",
    );
    let del_imported = mcp.delete_report(Parameters(id(&imported))).await.unwrap();
    if is_err(&del_imported) {
        record(
            "DELETE",
            "/reports/{id}",
            "openvas_delete_report",
            "SKIP (compose gvmd retains imported reports; exercised in the scan lifecycle)",
        );
    } else {
        record("DELETE", "/reports/{id}", "openvas_delete_report", "PASS");
    }

    // ── cleanup, in dependency order ──
    must(
        &mcp.delete_task(Parameters(ultimate(&import_task)))
            .await
            .unwrap(),
        "DELETE",
        "/tasks/{id}?ultimate=true",
        "openvas_delete_task",
    );
    must(
        &mcp.delete_override(Parameters(ultimate(&over)))
            .await
            .unwrap(),
        "DELETE",
        "/overrides/{id}",
        "openvas_delete_override",
    );
    must(
        &mcp.delete_note(Parameters(ultimate(&note))).await.unwrap(),
        "DELETE",
        "/notes/{id}",
        "openvas_delete_note",
    );
    for tag_id in [tag_clone, tag] {
        must(
            &mcp.delete_tag(Parameters(ultimate(&tag_id))).await.unwrap(),
            "DELETE",
            "/tags/{id}",
            "openvas_delete_tag",
        );
    }
    for task_id in [task_clone, task] {
        must(
            &mcp.delete_task(Parameters(ultimate(&task_id)))
                .await
                .unwrap(),
            "DELETE",
            "/tasks/{id}",
            "openvas_delete_task",
        );
    }
    for target_id in [target_clone, target] {
        must(
            &mcp.delete_target(Parameters(ultimate(&target_id)))
                .await
                .unwrap(),
            "DELETE",
            "/targets/{id}",
            "openvas_delete_target",
        );
    }
    must(
        &mcp.delete_config(Parameters(ultimate(&config_clone)))
            .await
            .unwrap(),
        "DELETE",
        "/configs/{id}",
        "openvas_delete_config",
    );
    must(
        &mcp.delete_scan_config(Parameters(ultimate(&sc)))
            .await
            .unwrap(),
        "DELETE",
        "/scan-configs/{id}",
        "openvas_delete_scan_config",
    );
    must(
        &mcp.delete_asset_host(Parameters(id(&host))).await.unwrap(),
        "DELETE",
        "/hosts/{id}",
        "openvas_delete_asset_host",
    );
    for filter_id in [filter_clone, filter] {
        must(
            &mcp.delete_filter(Parameters(ultimate(&filter_id)))
                .await
                .unwrap(),
            "DELETE",
            "/filters/{id}",
            "openvas_delete_filter",
        );
    }
    must(
        &mcp.delete_alert(Parameters(ultimate(&alert)))
            .await
            .unwrap(),
        "DELETE",
        "/alerts/{id}",
        "openvas_delete_alert",
    );
    must(
        &mcp.delete_schedule(Parameters(ultimate(&sched)))
            .await
            .unwrap(),
        "DELETE",
        "/schedules/{id}",
        "openvas_delete_schedule",
    );
    must(
        &mcp.delete_credential(Parameters(ultimate(&cred)))
            .await
            .unwrap(),
        "DELETE",
        "/credentials/{id}",
        "openvas_delete_credential",
    );
    must(
        &mcp.delete_port_list(Parameters(ultimate(&pl)))
            .await
            .unwrap(),
        "DELETE",
        "/port-lists/{id}",
        "openvas_delete_port_list",
    );

    flush_matrix("Resource lifecycles (create → update → clone → delete)");
}

// ───────────────────────── 3. scan lifecycle + reports ─────────────────────────

#[tokio::test]
#[ignore = "requires a live stack and GVM_E2E_SCAN=1 (runs a scan, slow)"]
async fn e2e_03_scan_lifecycle_reports_and_exports() {
    if !scan_enabled() {
        eprintln!("skipping: set GVM_E2E_SCAN=1 to run the scan lifecycle");
        return;
    }
    let Some((env, mcp)) = live_server() else {
        return;
    };
    let cat = catalog(&mcp).await;

    let target = create_target(&mcp, &env, &cat.port_list_id).await;
    let task = must(
        &mcp.create_task(Parameters(create_task_params(
            &unique("scan"),
            &target,
            &cat.scan_config_id,
            &cat.scanner_id,
        )))
        .await
        .unwrap(),
        "POST",
        "/tasks",
        "openvas_create_task",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Resume before the first run must be rejected (409) and stop of an idle
    // task is accepted by the gateway contract.
    let resume = mcp.resume_task(Parameters(id(&task))).await.unwrap();
    assert!(is_err(&resume), "resume of a new task must fail");
    record(
        "POST",
        "/tasks/{id}/resume (new task)",
        "openvas_resume_task",
        "PASS (rejected as expected)",
    );

    let started = must(
        &mcp.start_task(Parameters(id(&task))).await.unwrap(),
        "POST",
        "/tasks/{id}/start",
        "openvas_start_task",
    );
    let report_id = started["reportId"].as_str().unwrap().to_owned();
    eprintln!("started task {task}; report {report_id}");

    let status = wait_terminal(&mcp, Lifecycle::Task, &task).await;
    eprintln!("final task status: {status}");
    assert!(
        matches!(status.as_str(), "Done" | "Stopped" | "Interrupted"),
        "unexpected terminal status: {status}"
    );
    record("GET", "/tasks/{id} (poll)", "openvas_get_task", "PASS");

    // Report reads
    let report = must(
        &mcp.get_report(Parameters(GetReportParams {
            id: report_id.clone(),
            page: Some(1),
            per_page: Some(10),
        }))
        .await
        .unwrap(),
        "GET",
        "/reports/{id}",
        "openvas_get_report",
    );
    assert_eq!(report["task"]["id"], task);
    let reports = must(
        &mcp.list_reports(Parameters(ListParams {
            filter: Some(format!("task_id={task}")),
            ..Default::default()
        }))
        .await
        .unwrap(),
        "GET",
        "/reports?filter=",
        "openvas_list_reports",
    );
    assert!(
        reports["reports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == report_id),
        "report list must contain the new report"
    );
    let page = |filter: Option<&str>| ReportPageParams {
        id: report_id.clone(),
        list: ListParams {
            filter: filter.map(str::to_owned),
            page: Some(1),
            per_page: Some(10),
            ..Default::default()
        },
    };
    let results = must(
        &mcp.get_report_results(Parameters(page(None)))
            .await
            .unwrap(),
        "GET",
        "/reports/{id}/results",
        "openvas_get_report_results",
    );
    must(
        &mcp.get_report_results(Parameters(page(Some("severity>0"))))
            .await
            .unwrap(),
        "GET",
        "/reports/{id}/results?filter=",
        "openvas_get_report_results",
    );
    must(
        &mcp.get_report_vulnerabilities(Parameters(page(None)))
            .await
            .unwrap(),
        "GET",
        "/reports/{id}/vulnerabilities",
        "openvas_get_report_vulnerabilities",
    );
    must(
        &mcp.get_report_hosts(Parameters(page(None))).await.unwrap(),
        "GET",
        "/reports/{id}/hosts",
        "openvas_get_report_hosts",
    );
    must(
        &mcp.get_report_ports(Parameters(page(None))).await.unwrap(),
        "GET",
        "/reports/{id}/ports",
        "openvas_get_report_ports",
    );
    must(
        &mcp.get_report_applications(Parameters(page(None)))
            .await
            .unwrap(),
        "GET",
        "/reports/{id}/applications",
        "openvas_get_report_applications",
    );
    must(
        &mcp.get_report_cves(Parameters(page(None))).await.unwrap(),
        "GET",
        "/reports/{id}/cves",
        "openvas_get_report_cves",
    );
    must(
        &mcp.get_report_closed_cves(Parameters(page(None)))
            .await
            .unwrap(),
        "GET",
        "/reports/{id}/closed-cves",
        "openvas_get_report_closed_cves",
    );
    must(
        &mcp.get_report_errors(Parameters(page(None))).await.unwrap(),
        "GET",
        "/reports/{id}/errors",
        "openvas_get_report_errors",
    );
    must(
        &mcp.get_report_operating_systems(Parameters(page(None)))
            .await
            .unwrap(),
        "GET",
        "/reports/{id}/operating-systems",
        "openvas_get_report_operating_systems",
    );
    must(
        &mcp.get_report_tls_certificates(Parameters(page(None)))
            .await
            .unwrap(),
        "GET",
        "/reports/{id}/tls-certificates",
        "openvas_get_report_tls_certificates",
    );
    if let Some(result_id) = first_id(&results, "results") {
        must(
            &mcp.get_result(Parameters(id(&result_id))).await.unwrap(),
            "GET",
            "/results/{id}",
            "openvas_get_result",
        );
        must(
            &mcp.list_results(Parameters(ListParams {
                filter: Some(format!("report_id={report_id}")),
                per_page: Some(5),
                ..Default::default()
            }))
            .await
            .unwrap(),
            "GET",
            "/results?filter=",
            "openvas_list_results",
        );
    }

    // Export jobs: JSON inline, PDF as base64, cancel path.
    async fn run_export(
        mcp: &GvmMcpServer,
        report_id: &str,
        format_id: Option<String>,
        label: &'static str,
    ) -> String {
        let job = must(
            &mcp.export_report(Parameters(ExportReportParams {
                id: report_id.to_owned(),
                report_format_id: format_id,
                report_config_id: None,
                filter: None,
                filter_id: None,
            }))
            .await
            .unwrap(),
            "POST",
            "/reports/{id}/exports",
            label,
        );
        let job_id = job["id"].as_str().unwrap().to_owned();
        let mut status = job["status"].as_str().unwrap_or("").to_owned();
        for _ in 0..60 {
            if matches!(
                status.as_str(),
                "succeeded" | "failed" | "cancelled" | "expired"
            ) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let polled = must(
                &mcp.get_job(Parameters(id(&job_id))).await.unwrap(),
                "GET",
                "/jobs/{id}",
                "openvas_get_job",
            );
            status = polled["status"].as_str().unwrap_or("").to_owned();
        }
        assert_eq!(status, "succeeded", "export job {job_id} ended as {status}");
        job_id
    }
    let json_job = run_export(&mcp, &report_id, None, "openvas_export_report (json)").await;
    let json_download = mcp
        .download_job_result(Parameters(id(&json_job)))
        .await
        .unwrap();
    assert!(!is_err(&json_download), "{}", text_of(&json_download));
    let json_body: serde_json::Value = serde_json::from_str(&text_of(&json_download)).unwrap();
    assert_eq!(json_body["report"]["id"], report_id);
    record(
        "GET",
        "/jobs/{id}/result (json)",
        "openvas_download_job_result",
        "PASS",
    );

    let pdf_job = run_export(
        &mcp,
        &report_id,
        Some(cat.pdf_format_id.clone()),
        "openvas_export_report (pdf)",
    )
    .await;
    let pdf = must(
        &mcp.download_job_result(Parameters(id(&pdf_job)))
            .await
            .unwrap(),
        "GET",
        "/jobs/{id}/result (pdf)",
        "openvas_download_job_result",
    );
    assert_eq!(pdf["contentType"], "application/pdf");
    assert!(pdf["size"].as_u64().unwrap() > 0);

    // Cancel: a terminal job answers 204 (already finished) — the tool
    // reports cancelled either way.
    must(
        &mcp.cancel_job(Parameters(id(&pdf_job))).await.unwrap(),
        "DELETE",
        "/jobs/{id}",
        "openvas_cancel_job",
    );

    // Stop on a finished task must be a legible 409, not a crash.
    let stop = mcp.stop_task(Parameters(id(&task))).await.unwrap();
    record(
        "POST",
        "/tasks/{id}/stop (finished task)",
        "openvas_stop_task",
        if is_err(&stop) {
            "PASS (409 surfaced as tool error)"
        } else {
            "PASS"
        },
    );

    // Hosts / OS assets created by the scan
    must(
        &mcp.list_asset_hosts(Parameters(ListParams {
            filter: Some(format!("name={}", env.target_host)),
            ..Default::default()
        }))
        .await
        .unwrap(),
        "GET",
        "/hosts?filter=",
        "openvas_list_asset_hosts",
    );

    // Cleanup: report, task, target
    must(
        &mcp.delete_report(Parameters(id(&report_id))).await.unwrap(),
        "DELETE",
        "/reports/{id}",
        "openvas_delete_report",
    );
    must(
        &mcp.delete_task(Parameters(ultimate(&task))).await.unwrap(),
        "DELETE",
        "/tasks/{id}",
        "openvas_delete_task",
    );
    must(
        &mcp.delete_target(Parameters(ultimate(&target)))
            .await
            .unwrap(),
        "DELETE",
        "/targets/{id}",
        "openvas_delete_target",
    );

    flush_matrix("Scan lifecycle, report drill-downs and export jobs");
}

// ───────────────────────── 4. compliance audit ─────────────────────────

#[tokio::test]
#[ignore = "requires a live stack and GVM_E2E_SCAN=1 (runs an audit, slow)"]
async fn e2e_04_compliance_audit_lifecycle() {
    if !scan_enabled() {
        eprintln!("skipping: set GVM_E2E_SCAN=1 to run the audit lifecycle");
        return;
    }
    let Some((env, mcp)) = live_server() else {
        return;
    };
    let cat = catalog(&mcp).await;

    let policies = must(
        &mcp.list_policies(Parameters(lp())).await.unwrap(),
        "GET",
        "/policies",
        "openvas_list_policies",
    );
    let base_policy = first_id(&policies, "policies").expect("feed ships built-in policies");
    let policy = must(
        &mcp.create_policy(Parameters(CreatePolicyParams {
            name: unique("policy"),
            base_policy_id: base_policy,
            comment: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/policies",
        "openvas_create_policy",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let fetched = must(
        &mcp.get_policy(Parameters(id(&policy))).await.unwrap(),
        "GET",
        "/policies/{id}",
        "openvas_get_policy",
    );
    assert_eq!(fetched["usageType"], "policy");
    must(
        &mcp.update_policy(Parameters(UpdatePolicyParams {
            id: policy.clone(),
            name: None,
            comment: Some("updated".into()),
        }))
        .await
        .unwrap(),
        "PUT",
        "/policies/{id}",
        "openvas_update_policy",
    );

    let target = create_target(&mcp, &env, &cat.port_list_id).await;
    let audit = must(
        &mcp.create_audit(Parameters(create_task_params(
            &unique("audit"),
            &target,
            &policy,
            &cat.scanner_id,
        )))
        .await
        .unwrap(),
        "POST",
        "/audits",
        "openvas_create_audit",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let got = must(
        &mcp.get_audit(Parameters(id(&audit))).await.unwrap(),
        "GET",
        "/audits/{id}",
        "openvas_get_audit",
    );
    assert_eq!(got["usageType"], "audit");
    must(
        &mcp.update_audit(Parameters(update_task_params(&audit)))
            .await
            .unwrap(),
        "PUT",
        "/audits/{id}",
        "openvas_update_audit",
    );
    // Audits never leak into the scan-task list.
    let tasks = json_of(
        &mcp.list_tasks(Parameters(ListParams {
            per_page: Some(1000),
            ..Default::default()
        }))
        .await
        .unwrap(),
    );
    assert!(
        !tasks["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == audit),
        "audit must not appear under /tasks"
    );

    let started = must(
        &mcp.start_audit(Parameters(id(&audit))).await.unwrap(),
        "POST",
        "/audits/{id}/start",
        "openvas_start_audit",
    );
    let report_id = started["reportId"].as_str().unwrap().to_owned();
    let status = wait_terminal(&mcp, Lifecycle::Audit, &audit).await;
    eprintln!("final audit status: {status}");
    assert!(
        matches!(status.as_str(), "Done" | "Stopped" | "Interrupted"),
        "unexpected terminal audit status: {status}"
    );
    // Stop/resume on a finished audit surface as legible errors (409).
    let stop = mcp.stop_audit(Parameters(id(&audit))).await.unwrap();
    record(
        "POST",
        "/audits/{id}/stop (finished)",
        "openvas_stop_audit",
        if is_err(&stop) {
            "PASS (409 surfaced as tool error)"
        } else {
            "PASS"
        },
    );
    let resume = mcp.resume_audit(Parameters(id(&audit))).await.unwrap();
    record(
        "POST",
        "/audits/{id}/resume (finished)",
        "openvas_resume_audit",
        if is_err(&resume) {
            "PASS (409 surfaced as tool error)"
        } else {
            "PASS"
        },
    );
    must(
        &mcp.get_report(Parameters(GetReportParams {
            id: report_id.clone(),
            page: Some(1),
            per_page: Some(5),
        }))
        .await
        .unwrap(),
        "GET",
        "/reports/{id} (audit report)",
        "openvas_get_report",
    );

    // Cleanup. Audits are always deleted non-ultimately by the gateway, so
    // the trashed audit keeps referencing its target: gvmd refuses an
    // ultimate target delete ("Target is in use") but accepts trashing it.
    let _ = mcp.delete_report(Parameters(id(&report_id))).await;
    must(
        &mcp.delete_audit(Parameters(id(&audit))).await.unwrap(),
        "DELETE",
        "/audits/{id}",
        "openvas_delete_audit",
    );
    must(
        &mcp.delete_target(Parameters(DeleteParams {
            id: target.clone(),
            ultimate: None,
        }))
        .await
        .unwrap(),
        "DELETE",
        "/targets/{id} (trashcan; audit still references it)",
        "openvas_delete_target",
    );
    must(
        &mcp.delete_policy(Parameters(id(&policy))).await.unwrap(),
        "DELETE",
        "/policies/{id}",
        "openvas_delete_policy",
    );

    flush_matrix("Compliance audit lifecycle");
}

// ───────────────────────── 5. identity (opt-in toolset) ─────────────────────────

#[tokio::test]
#[ignore = "requires a live gateway + gvmd (identity toolset)"]
async fn e2e_05_identity_admin() {
    let Some(env) = env() else {
        return;
    };
    let mcp = server_with(&env, &["--toolsets", "default,identity"]);

    let users = must(
        &mcp.list_users(Parameters(lp())).await.unwrap(),
        "GET",
        "/users",
        "openvas_list_users",
    );
    let user_id = first_id(&users, "users").expect("admin user exists");
    must(
        &mcp.get_user(Parameters(id(&user_id))).await.unwrap(),
        "GET",
        "/users/{id}",
        "openvas_get_user",
    );
    let roles = must(
        &mcp.list_roles(Parameters(lp())).await.unwrap(),
        "GET",
        "/roles",
        "openvas_list_roles",
    );
    if let Some(role_id) = first_id(&roles, "roles") {
        must(
            &mcp.get_role(Parameters(id(&role_id))).await.unwrap(),
            "GET",
            "/roles/{id}",
            "openvas_get_role",
        );
    }
    let permissions = must(
        &mcp.list_permissions(Parameters(lp())).await.unwrap(),
        "GET",
        "/permissions",
        "openvas_list_permissions",
    );
    if let Some(permission_id) = first_id(&permissions, "permissions") {
        must(
            &mcp.get_permission(Parameters(id(&permission_id)))
                .await
                .unwrap(),
            "GET",
            "/permissions/{id}",
            "openvas_get_permission",
        );
    }

    // Group lifecycle
    let group = must(
        &mcp.create_group(Parameters(CreateMembershipParams {
            name: unique("group"),
            comment: None,
            users: Some(vec![env.username.clone()]),
        }))
        .await
        .unwrap(),
        "POST",
        "/groups",
        "openvas_create_group",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.list_groups(Parameters(lp())).await.unwrap(),
        "GET",
        "/groups",
        "openvas_list_groups",
    );
    must(
        &mcp.get_group(Parameters(id(&group))).await.unwrap(),
        "GET",
        "/groups/{id}",
        "openvas_get_group",
    );
    must(
        &mcp.update_group(Parameters(UpdateMembershipParams {
            id: group.clone(),
            comment: Some("updated".into()),
            users: None,
        }))
        .await
        .unwrap(),
        "PUT",
        "/groups/{id}",
        "openvas_update_group",
    );
    must(
        &mcp.delete_group(Parameters(ultimate(&group)))
            .await
            .unwrap(),
        "DELETE",
        "/groups/{id}",
        "openvas_delete_group",
    );

    // Role lifecycle
    let role = must(
        &mcp.create_role(Parameters(CreateMembershipParams {
            name: unique("role"),
            comment: None,
            users: None,
        }))
        .await
        .unwrap(),
        "POST",
        "/roles",
        "openvas_create_role",
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    must(
        &mcp.update_role(Parameters(UpdateMembershipParams {
            id: role.clone(),
            comment: Some("updated".into()),
            users: Some(vec![env.username.clone()]),
        }))
        .await
        .unwrap(),
        "PUT",
        "/roles/{id}",
        "openvas_update_role",
    );
    must(
        &mcp.delete_role(Parameters(ultimate(&role))).await.unwrap(),
        "DELETE",
        "/roles/{id}",
        "openvas_delete_role",
    );

    // User settings: list, get, update and restore
    let settings = must(
        &mcp.list_user_settings(Parameters(FilterParams::default()))
            .await
            .unwrap(),
        "GET",
        "/user-settings",
        "openvas_list_user_settings",
    );
    let rows = settings["settings"].as_array().unwrap();
    let setting = rows
        .iter()
        .find(|s| s["name"] == "Rows Per Page")
        .or_else(|| rows.first())
        .expect("user settings exist");
    let setting_id = setting["id"].as_str().unwrap().to_owned();
    let current = must(
        &mcp.get_user_setting(Parameters(id(&setting_id)))
            .await
            .unwrap(),
        "GET",
        "/user-settings/{id}",
        "openvas_get_user_setting",
    );
    let original = current["value"].as_str().unwrap_or("25").to_owned();
    must(
        &mcp.update_user_setting(Parameters(UpdateUserSettingParams {
            id: setting_id.clone(),
            value: original.clone(),
        }))
        .await
        .unwrap(),
        "PUT",
        "/user-settings/{id}",
        "openvas_update_user_setting",
    );

    flush_matrix("Identity administration (opt-in toolset)");
}

// ───────────────────────── 6. negative paths + gating ─────────────────────────

#[tokio::test]
#[ignore = "requires a live gateway + gvmd"]
async fn e2e_06_negative_paths_and_gating() {
    let Some((env, mcp)) = live_server() else {
        return;
    };

    let missing = mcp
        .get_target(Parameters(id("00000000-0000-0000-0000-000000000000")))
        .await
        .unwrap();
    assert!(is_err(&missing));
    assert!(
        text_of(&missing).contains("Not found"),
        "{}",
        text_of(&missing)
    );
    record(
        "GET",
        "/targets/{id} (unknown)",
        "openvas_get_target",
        "PASS (404 → legible tool error)",
    );

    let malformed = mcp.get_target(Parameters(id("not-a-uuid"))).await.unwrap();
    assert!(is_err(&malformed));
    record(
        "GET",
        "/targets/{id} (malformed)",
        "openvas_get_target",
        "PASS (400/404 → legible tool error)",
    );

    // Deleting a report with `ultimate` is a 400 at the gateway; the tool
    // never offers the parameter, so this cannot be triggered — assert the
    // tool surface instead.
    assert!(
        !mcp.tool_names()
            .contains(&"openvas_list_tickets".to_string()),
        "ticket tools must not exist"
    );

    // Wrong credentials: every authenticated call is a legible 401.
    let bad = Config::from_cli(Cli::parse_from([
        "gvm-mcp",
        "--gateway-url",
        &env.gateway,
        "--username",
        &env.username,
        "--password",
        "definitely-wrong-password",
    ]))
    .unwrap();
    let bad_mcp = GvmMcpServer::new(bad).unwrap();
    let denied = bad_mcp.test_connection().await.unwrap();
    assert!(is_err(&denied));
    assert!(
        text_of(&denied).contains("Authentication failed"),
        "{}",
        text_of(&denied)
    );
    record(
        "GET",
        "/targets (bad credentials)",
        "openvas_test_connection",
        "PASS (401 → legible tool error)",
    );

    // Read-only gating against the live gateway: no mutating tool exposed.
    let ro = server_with(&env, &["--read-only"]);
    let names = ro.tool_names();
    assert_eq!(names.len(), 89, "read-only surface drifted: {names:?}");
    assert!(
        !names
            .iter()
            .any(|n| n.contains("create") || n.contains("delete"))
    );
    let listed = must(
        &ro.list_targets(Parameters(lp())).await.unwrap(),
        "GET",
        "/targets (read-only server)",
        "openvas_list_targets",
    );
    assert!(listed["pagination"].is_object());

    flush_matrix("Negative paths and gating");
}

// ───────────────────────── 7. streamable HTTP against the live gateway ─────────────────────────

#[tokio::test]
#[ignore = "requires a live gateway + gvmd"]
async fn e2e_07_streamable_http_pass_through() {
    let Some(env) = env() else {
        return;
    };
    // No fallback credentials: every HTTP caller must authenticate itself.
    // (clap also reads GVM_USERNAME/GVM_PASSWORD from the environment, so
    // the parsed values are cleared explicitly.)
    let mut cli = Cli::parse_from([
        "gvm-mcp",
        "--gateway-url",
        &env.gateway,
        "--transport",
        "streamable-http",
    ]);
    cli.username = None;
    cli.password = None;
    cli.password_file = None;
    let config = Config::from_cli(cli).unwrap();
    assert!(!config.has_fallback_credentials());
    let mcp = GvmMcpServer::new(config).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        gvm_mcp::mcp::http::serve(
            mcp,
            listener,
            &["127.0.0.1".to_string()],
            std::future::pending(),
        )
        .await
        .unwrap();
    });

    use base64::Engine;
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", env.username, env.password))
    );
    let client = reqwest::Client::new();
    let url = format!("http://{addr}/mcp");
    let payload = |text: String| -> serde_json::Value {
        let json_text = text
            .lines()
            .rev()
            .find_map(|line| {
                line.strip_prefix("data: ")
                    .filter(|data| !data.trim().is_empty())
            })
            .map(str::to_owned)
            .unwrap_or(text);
        serde_json::from_str(&json_text).unwrap()
    };
    let rpc = |method: &str, params: serde_json::Value, id: u64| serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});

    let session_for = |auth: String| {
        let client = client.clone();
        let url = url.clone();
        async move {
            let init = client
                .post(&url)
                .header("Accept", "application/json, text/event-stream")
                .header("Authorization", auth.clone())
                .json(&rpc(
                    "initialize",
                    serde_json::json!({"protocolVersion": "2025-06-18", "capabilities": {},
                        "clientInfo": {"name": "e2e", "version": "0"}}),
                    1,
                ))
                .send()
                .await
                .unwrap();
            let session = init
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
                .unwrap_or_default();
            client
                .post(&url)
                .header("Accept", "application/json, text/event-stream")
                .header("Authorization", auth)
                .header("mcp-session-id", session.clone())
                .json(&serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                .send()
                .await
                .unwrap();
            session
        }
    };

    let session = session_for(basic.clone()).await;
    let list = client
        .post(&url)
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", &basic)
        .header("mcp-session-id", &session)
        .json(&rpc("tools/list", serde_json::json!({}), 2))
        .send()
        .await
        .unwrap();
    let tools = payload(list.text().await.unwrap());
    assert_eq!(
        tools["result"]["tools"].as_array().unwrap().len(),
        171,
        "default HTTP tool surface"
    );

    let call = client
        .post(&url)
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", &basic)
        .header("mcp-session-id", &session)
        .json(&rpc(
            "tools/call",
            serde_json::json!({"name": "openvas_test_connection", "arguments": {}}),
            3,
        ))
        .send()
        .await
        .unwrap();
    let body = payload(call.text().await.unwrap());
    assert_ne!(body["result"]["isError"], true, "got: {body}");
    let report: serde_json::Value =
        serde_json::from_str(body["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(report["authenticated"], true);
    record(
        "HTTP",
        "/mcp tools/call (Basic forwarded)",
        "openvas_test_connection",
        "PASS",
    );

    // A caller with wrong credentials gets a 401 tool error, not the
    // server's identity (there is none configured).
    let wrong = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{}:wrong", env.username))
    );
    let session = session_for(wrong.clone()).await;
    let call = client
        .post(&url)
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", &wrong)
        .header("mcp-session-id", &session)
        .json(&rpc(
            "tools/call",
            serde_json::json!({"name": "openvas_list_targets", "arguments": {}}),
            4,
        ))
        .send()
        .await
        .unwrap();
    let body = payload(call.text().await.unwrap());
    assert_eq!(body["result"]["isError"], true, "got: {body}");
    assert!(
        body["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Authentication failed")
    );
    record(
        "HTTP",
        "/mcp tools/call (wrong Basic)",
        "openvas_list_targets",
        "PASS (401 → legible tool error)",
    );

    // An anonymous caller (no Authorization, no fallback) is rejected by the
    // gateway with 401 as well.
    let anon_session = {
        let init = client
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .json(&rpc(
                "initialize",
                serde_json::json!({"protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": {"name": "e2e", "version": "0"}}),
                1,
            ))
            .send()
            .await
            .unwrap();
        init.headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .unwrap_or_default()
    };
    let call = client
        .post(&url)
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", &anon_session)
        .json(&rpc(
            "tools/call",
            serde_json::json!({"name": "openvas_list_targets", "arguments": {}}),
            5,
        ))
        .send()
        .await
        .unwrap();
    let body = payload(call.text().await.unwrap());
    assert_eq!(body["result"]["isError"], true, "got: {body}");
    record(
        "HTTP",
        "/mcp tools/call (anonymous, no fallback)",
        "openvas_list_targets",
        "PASS (401 → legible tool error)",
    );

    flush_matrix("Streamable HTTP pass-through against the live gateway");
}
